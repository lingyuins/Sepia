# new-api 单个 API 请求链路调查（入站 → 中间件 → 出站 → 响应返回）

> 调查对象：`D:\CodeWorkspace\new-api`（只读源码，未做任何修改）
> 调查范围：一次请求从 HTTP 入站到上游返回、计费收尾的**生命周期**。**不含**路由策略细节（渠道优先级/分组权重/亲和等策略另有文档）。
> 结论均给出 `文件:行号`；无法从代码直接确认的写“未确认”。
> 忽略 `.git` / `node_modules` / `dist`。

---

## 一、概览

一次 `/v1/chat/completions` 之类的请求，完整链路可概括为：

```
HTTP 入站
  → [全局中间件] PanicRecover / RequestId / Version / I18n / Gin Logger(+脱敏)
  → [relay 侧中间件] CORS / Decompress / BodyStorageCleanup / Stats
  → [路由组中间件] RouteTag / SystemPerformanceCheck / TokenAuth / ModelRequestRateLimit
  → middleware.Distribute()：解析 model → SelectChannelForRequest 选渠道 → SetupContextForSelectedChannel 写 context
  → controller.Relay()：GetAndValidateRequest 解析请求 → GenRelayInfo → PrepareRequestBilling 预扣
  → 重试循环 for retry <= common.RetryTimes：
        getChannel（首次复用 Distribute 选出的渠道，重试时重新选）
        → TextHelper/ClaudeHelper/GeminiHelper/… 
             InitChannelMeta → ModelMappedHelper(模型映射) → GetAdaptor → ConvertRequest(协议转换)
             → DoApiRequest(拼 URL/头/代理) → 上游 → DoResponse 解析 usage
        → PostTextConsumeQuota / SettleBilling 结算
  失败：DecideRelayRetry 决定重试/停止；ProcessChannelError 记日志、按策略禁用渠道
  → 返回客户端（失败时按 RelayFormat 输出 OpenAI/Claude 错误体）
```

关键结构：
- 路由与中间件注册：`router/main.go:15-22`、`router/relay-router.go:15-222`。
- 选渠道：`middleware/distributor.go:34`（`Distribute`）→ `service/channel_select.go:303`（`SelectChannelForRequest`）。
- 执行与重试：`controller/relay.go:72`（`Relay`）。
- 适配器：`relay/relay_adaptor.go:50`（`GetAdaptor`）+ `relay/channel/*`。
- 计费：`relay/request_billing.go:25`、`service/billing.go:24/65`、`service/text_quota.go:400`。

> 重要勘误（相对任务清单）：
> - `GlobalAPIRateLimit`、`AccessTokenAudit` **不在 relay 链上**。它们挂在 `/api` 管理接口与 Web 回落路由：`router/api-router.go:21,23`、`router/dashboard.go:14`、`router/main.go:35`、`router/web-router.go:34`。relay 路由用的是 `ModelRequestRateLimit`（模型请求限流）。
> - 任务清单里的 `POST /v1/responses` 并不在 `relay-router.go` 注册，而是在 `router/task-plugin-protocol-router.go:29-36`（`openai_responses.create`）注册，回退到 `controller.Relay(c, types.RelayFormatOpenAIResponses)`。`relay-router.go` 只注册了 `POST /v1/responses/compact` 与 `GET /v1/responses`（WebSocket）。

---

## 二、入站链路

### 2.1 服务启动与全局中间件

`main.go:180-199`：`gin.New()` → `middleware.ConfigureTrustedProxies` → 依次 `server.Use`：
- `middleware.PanicRecover()`（`main.go:185`）：`gin.CustomRecoveryWithWriter(common.ErrorLogWriter, …)`，panic 时输出 500 `new_api_panic`（`middleware/recover.go:33-46`）。
- `middleware.RequestId()`（`main.go:188`）：生成 `NewRequestId`，写入 context 与响应头（`middleware/request-id.go:10-18`）。
- `middleware.Version()`（`main.go:189`）：写 `X-New-Api-Version` 头（`middleware/cors.go:18-25`）。
- `middleware.I18n()`（`main.go:190`）：语言探测（`middleware/i18n.go:13`）。
- `middleware.SetUpLogger(server)`（`main.go:191`）：`redactTaskArtifactAccessQuery()` + `gin.LoggerWithConfig`，按 `route_tag` 打 `[GIN]` 访问日志并做查询串脱敏（`middleware/logger.go:21-67`）。

随后 `router.SetRouter` 按顺序装配：`SetApiRouter → SetDashboardRouter → SetRelayRouter → SetTaskPluginProtocolRouter → SetVideoRouter → SetTaskRouter → SetPluginRouter → SetWebRouter`（`router/main.go:15-22`）。

### 2.2 各入口路径与其“主场”路由

| 入站路径 | 注册位置 | 说明 |
|---|---|---|
| `POST /v1/chat/completions`、`/v1/completions`、`/v1/messages`、`/v1/embeddings`、`/v1/rerank`、`/v1/audio/*`、`/v1/moderations`、`/v1/edits`、`/v1/responses/compact`、`/v1/tools/search|fetch`、`/v1/alpha/search`、`/v1/engines/:model/embeddings`、`/v1/models/*path` | `router/relay-router.go:91-180`（`httpRouter` 组，带 `Distribute`） | 主 relay HTTP 路由 |
| `POST /v1/responses` | `router/task-plugin-protocol-router.go:29-36` | host protocol `openai_responses.create`，回退 `Relay(RelayFormatOpenAIResponses)` |
| `GET /v1/responses`（WebSocket） | `router/relay-router.go:78` | `controller.ResponsesWebSocket`，在 `ModelRequestRateLimit`/`Distribute` 之前挂载 |
| `GET /v1/responses/:response_id` | `router/task-plugin-protocol-router.go:51-52` | `openai_responses.retrieve` |
| `POST /v1/images/generations`、`POST /v1/images/edits` | `router/task-plugin-protocol-router.go:37-44` | host protocol `openai_image.generate/edit`，回退 `Relay(RelayFormatOpenAIImage)` |
| `POST /v1/videos` | `router/task-plugin-protocol-router.go:45-50` | `openai_video.create` |
| `POST /v1/video/generations`、`GET /v1/video/generations/:task_id`、`POST /v1/videos/:video_id/remix` | `router/video-router.go:15-32` | 视频任务 |
| `POST /v1beta/models/*path`、`GET /v1beta/models`、`GET /v1beta/openai/models` | `router/relay-router.go:46-62,193-221` | Gemini 原生/兼容 |
| `POST /pg/chat/completions` | `router/relay-router.go:64-70` | Playground（`UserAuth`+`Distribute`） |
| `GET /v1/models`、`GET /v1/models/:model` | `router/relay-router.go:21-44` | 仅列模型，无选渠道 |
| `/mj/*`、`/:mode/mj/*` | `router/relay-router.go:182-190,224-245` | Midjourney |

`/v1/models`、`/v1beta/models` 这类“列模型”路由只挂了 `RouteTag("relay") + TokenAuth()`（`router/relay-router.go:22-23,47-48,56-57`），不进入选渠道与 relay。

---

## 三、中间件链（顺序表）

### 3.1 relay 侧通用中间件（在 `SetRelayRouter` 内注册，随后路由生效）

`router/relay-router.go:16-19`：
```go
router.Use(middleware.CORS())
router.Use(middleware.DecompressRequestMiddleware())
router.Use(middleware.BodyStorageCleanup())
router.Use(middleware.StatsMiddleware())
```
> 说明：这 4 个用 `engine.Use` 挂载，位置在 `SetApiRouter`/`SetDashboardRouter` 之后、relay 各路由注册之前，因此**实际只对之后注册的 relay 侧路由生效**（Gin 在注册路由时固化处理器链）。

| 顺序 | 中间件 | 文件:行 | 作用 |
|---|---|---|---|
| G1 | `PanicRecover` | `middleware/recover.go:33` | 全局 panic 捕获，500 兜底 |
| G2 | `redactTaskArtifactAccessQuery` + Gin Logger | `middleware/logger.go:21-25` | 访问日志、`route_tag`、凭证脱敏 |
| G3 | `RequestId` | `middleware/request-id.go:10` | 请求 ID |
| G4 | `Version` | `middleware/cors.go:18` | 版本头 |
| G5 | `I18n` | `middleware/i18n.go:13` | 语言 |
| R1 | `CORS` | `middleware/cors.go:9-16` | `AllowAllOrigins`、`AllowCredentials`、`AllowHeaders: *` |
| R2 | `DecompressRequestMiddleware` | `middleware/gzip.go:26-113` | 解 gzip/br/zstd，`http.MaxBytesReader` 限流（`constant.MaxRequestBodyMB`，默认 32MB） |
| R3 | `BodyStorageCleanup` | `middleware/body_cleanup.go:11-24` | `c.Next()` 后 `CleanupBodyStorage` + `CleanupFileSources` |
| R4 | `StatsMiddleware` | `middleware/stats.go:17-31` | 活跃连接计数 |

### 3.2 `/v1` relay 组中间件（`router/relay-router.go:71-192`）

| 顺序 | 中间件 | 文件:行 | 说明 |
|---|---|---|---|
| 1 | `RouteTag("relay")` | `relay-router.go:72` / `middleware/logger.go:14` | 标记日志 tag |
| 2 | `SystemPerformanceCheck()` | `relay-router.go:73` / `middleware/performance.go:14` | 系统过载检测，命中返回 503；`/v1/messages` 用 Claude 错误体，其余 OpenAI 错误体（`performance.go:14-39`） |
| 3 | `TokenAuth()` | `relay-router.go:74` / `middleware/auth.go:528` | 令牌鉴权、注入 context（详见 3.3） |
| 4 | （`GET /responses` WS 在此处直接注册） | `relay-router.go:78` | WebSocket 首帧后才选渠道 |
| 5 | `ModelRequestRateLimit()` | `relay-router.go:80` / `middleware/model-rate-limit.go:173` | 按用户/分组的总请求数与成功请求数限流（Redis 或内存），仅 `setting.ModelRequestRateLimitEnabled` 时生效 |
| 6 | `Distribute()` | `relay-router.go:84`（ws）、`relay-router.go:92`（http） / `middleware/distributor.go:34` | 解析 model、选渠道、写 context |

`/v1beta` 组（`relay-router.go:193-221`）顺序为：两个前置 handler（`:countTokens` 404、`:predictLongRunning` 插件分发）→ `RouteTag` → `SystemPerformanceCheck` → `TokenAuth` → `ModelRequestRateLimit` → `Distribute` → `POST /models/*path`。
`/pg` 组（`relay-router.go:64-70`）顺序：`RouteTag` → `SystemPerformanceCheck` → `UserAuth()` → `Distribute()`。
`/mj` 组（`relay-router.go:182-190,226`）：`RouteTag` → `SystemPerformanceCheck` → （组内）`TokenAuth` → `Distribute`。

### 3.3 `TokenAuth`（`middleware/auth.go:528-647`）校验与注入

1. `applyWebSocketSubprotocolAuthorization`（`:531`）：从 `Sec-WebSocket-Protocol` 的 `openai-insecure-api-key.` 取 key（`:649-671`）。
2. `/v1/messages`、`/v1/models` 支持 `x-api-key` → `Authorization`（`:533-538`）；Gemini 路径支持 `?key=`、`x-goog-api-key` → `Authorization`（`:540-553`）。
3. 规范化 key：去 `Bearer `、去 `sk-`，按 `-` 切分取首段（`:554-571`）。
4. `model.ValidateUserToken(key)` 校验（`:572`）；错误 → 401/500（`:579-589`）。
5. 令牌 IP 白名单校验（`token.GetIpLimits`，`:591-605`）。
6. `model.GetUserCache(token.UserId)` → 用户启用状态校验（`:607-618`），`userCache.WriteContext(c)`（`:620`）。
7. 分组：token 组存在则校验可用组 `service.GetUserUsableGroups` 与 `ratio_setting.ContainsGroupRatio`，最后 `ContextKeyUsingGroup` = token 组或用户组（`:622-639`）。
8. `SetupContextForToken`（`middleware/auth.go:673-719`）注入：`id/token_id/token_key/token_name/token_unlimited_quota/token_quota`、`token_model_limit_enabled/token_model_limit`、`ContextKeyTokenGroup`、`ContextKeyTokenCrossGroupRetry`、`ContextKeyTokenAutoGroups`；管理员的 `sk-<key>-<channelId>` 会添加渠道 pin（`:703-719`）。

### 3.4 `Distribute`（`middleware/distributor.go:34-140`）

- 响应 ≥400 时记录策略终止（`:37-41`）。
- 添加按请求路径的渠道过滤（`:42-47`）。
- `modelRequest, shouldSelectChannel, err := getModelRequest(c)`（`:48`）。
- token 模型白名单校验（`:57-74`，`TokenModelLimitAllows` 见 `:555`）。
- 非 pin 情况下：`/pg/chat/completions` 额外解析并校验 `group`（`:82-97`）。
- 选渠道：`service.SelectChannelForRequest(c, modelRequest.Model, &service.RetryParam{...})`（`:111-118`）；失败 `abortWithOpenAiMessage`（`:119-131`）。
- `ContextKeyRequestStartTime = now`（`:133`）→ `SetupContextForSelectedChannel(c, channel, model)`（`:134`）→ `c.Next()`（`:135`）→ 成功（<400）记录渠道亲和 `RecordChannelAffinity`（`:136-138`）。

`getModelRequest`（`middleware/distributor.go:390-549`）按路径解析模型名：
- `/mj/`（`:396-423`）、`/v1/videos*`、`/v1/video/generations`（`:428-466`）；
- `/v1beta/models/`、`/v1/models/` → `extractModelNameFromGeminiPath`（`:467-474`，函数在 `:713`）；
- 其它 JSON/表单请求从 body 的 `model` 字段（`:475-481`，`getModelFromJSONBody` 在 `:285`，用 ojson 只取 `model/group` 并校验重复键）；
- `realtime` 取 query `model`（`:482-485`）、`moderations` 默认、`embeddings` 取路径参数、`tools/search|fetch` 固定路由名、`images` 默认 `dall-e`、`audio` 默认 `whisper-1/tts-1`、`/pg` 取 body 的 `model/group`（`:486-546`）。

`SetupContextForSelectedChannel`（`middleware/distributor.go:586-708`）把选中的渠道写入 context：`original_model`（`:587`）、`ContextKeyChannelId/Name/Type/CreateTime/Setting/OtherSetting`（`:640-645`）、`param_override`/`header_override`（`:657-663`）、`organization`（`:664-666`）、`auto_ban`（`:667`）、`model_mapping`（`:668`）、`status_code_mapping`（`:669`）、`channel.GetNextEnabledKey()` 取 key 并写 `ContextKeyChannelKey`（`:671-683`）、`ContextKeyChannelBaseUrl`（`:684`）、按渠道类型的 `api_version/region/bot_id/...`（`:689-706`）。

---

## 四、路由分发（Distribute → 选渠道 → Relay）

1. `middleware.Distribute` 调用 `service.SelectChannelForRequest`（`service/channel_select.go:303-397`）：
   - 有渠道 pin 时直接取 pin 渠道并校验状态/过滤（`:305-327`）。
   - 首次尝试（`retry==0`）先看渠道亲和 `GetPreferredChannelByAffinity`（`:332-367`）。
   - 否则 `CacheGetRandomSatisfiedChannel`（`:371`，实现 `:114-208`，含 auto 分组/跨组重试逻辑，最终 `model.GetRandomSatisfiedChannel`）。
   - 最终 `model.ChannelSatisfiesFilters` 复核（`:389-395`）。
2. `SetupContextForSelectedChannel` 把渠道/Key/映射写进 context（见 3.4）。
3. `controller.Relay(c, relayFormat)`（`controller/relay.go:72-240`）开始执行。

> 关键点：`Distribute` 已经选好了“首个渠道”；`controller.Relay` 里 `getChannel`（`controller/relay.go:298-330`）在**首次尝试**（`info.ChannelMeta == nil`）时直接用 context 里的 `channel_id/channel_type/channel_name/auto_ban` 构造渠道（`:299-313`），**重试时**才 `service.CacheGetRandomSatisfiedChannel(retryParam)` 重新选（`:314-329`）。`info.InitChannelMeta(c)` 才会把 context 里的渠道信息物化成 `info.ChannelMeta`（`relay/common/relay_info.go:268-356`）。

---

## 五、出站链路

### 5.1 controller.Relay 主流程（`controller/relay.go:72-240`）

1. 若是 `RelayFormatOpenAIRealtime` 先 `upgrader.Upgrade`（`:83-91`）。
2. `defer` 错误出口：记录终止、按 `relayFormat` 输出 OpenAI/Claude/WS 错误体（`:93-121`）；流式响应（已写 `text/event-stream`）用 `writeRelayStreamError`（`:242-257`）。
3. `helper.GetAndValidateRequest(c, relayFormat)`（`:123`，实现 `relay/helper/valid_request.go:22-61`）；请求体过大 → 413，其它 → 400，且均 `SkipRetry`（`:124-132`）。
4. `relaycommon.GenRelayInfo`（`:134`，实现 `relay/common/relay_info.go:692-754`）。
5. `defer` 里做 perfmetrics 记录与 panic 复原（`:140-152`）。
6. Claude 流式且开启 Ping 时启动 keepalive（`:155-158`）。
7. `relay.PrepareRequestBilling`（`:160`）→ `defer relay.RefundFailedRequestBilling`（`:163-165`）。
8. 重试循环 `for ; retryParam.GetRetry() <= common.RetryTimes; retryParam.IncreaseRetry()`（`:177-233`）：
   - `getChannel`（`:182`）、`AppendUsedChannel`（`:188`）、`PrepareTieredBillingForSelectedGroup`（`:189`）。
   - 从 `common.GetBodyStorage` 恢复请求体到 `c.Request.Body`（`:194-204`，支持重试重放）。
   - 按 `relayFormat` 分派：`WssHelper` / `ClaudeHelper` / `geminiRelayHandler` / `relayHandler`（`:206-215`）。
   - 成功：`MarkRequestPolicySuccess` 并 `return`（`:217-221`）。
   - 失败：`NormalizeViolationFeeError` → `DecideRelayRetry` → `RecordPolicyFailure` → `processChannelError`，`decision.Action != "retry"` 则 break（`:223-232`）。

`relayHandler`（`controller/relay.go:35-60`）按 `RelayMode` 分派到：
`ImageHelper`（图片）、`AudioHelper`（音频）、`RerankHelper`、`EmbeddingHelper`、`ResponsesHelper`、`AlphaSearchHelper`、`DeepSeekToolsHelper`、默认 `TextHelper`（文本/chat）。
`geminiRelayHandler`（`:62-70`）：路径含 `embed` → `GeminiEmbeddingHandler`，否则 `GeminiHelper`。

### 5.2 各 Helper 的共同骨架

以 `TextHelper`（`relay/compatible_handler.go:25-187`）为例：
1. `info.InitChannelMeta(c)`（`:26`）——把 context 中渠道信息构建成 `ChannelMeta`（`relay/common/relay_info.go:268`）。
2. 类型断言 `dto.GeneralOpenAIRequest`，`common.DeepCopy`（`:28-36`）。
3. `helper.ModelMappedHelper`（`:42`，实现 `relay/helper/model_mapped.go:14-69`）——按渠道 `model_mapping` 做（可链式）模型重定向，结果写入 `info.UpstreamModelName` 并 `request.SetModelName`。
4. `helper.ApplyReasoningModelSuffix`（`:46`）。
5. Stream/StreamOptions 规整（`:50-68`）。
6. `adaptor := GetAdaptor(info.ApiType)`（`:70`，实现 `relay/relay_adaptor.go:50-128`，`ApiType`→具体 `channel.Adaptor`），`adaptor.Init(info)`（`:74`）。
7. 特殊分支：`chat/completions` 走 `textRequestViaResponses`（`:76-96`，渠道被配置为“用 Responses 执行 chat”）。
8. 请求体：
   - `PassThroughRequestEnabled` 或 `ChannelSetting.PassThroughBodyEnabled`：原样透传 `common.NewReplayableBodyReader(storage)`（`:100-110`）。
   - 否则：`adaptor.ConvertOpenAIRequest`（`:112`）→ `RemoveDisabledFields`（`:128`）→ `ApplyParamOverrideWithRelayInfo`（`:134-139`）→ `NewOutboundJSONBody`（`:143`）。
9. `adaptor.DoRequest(c, info, requestBody)`（`:153`）→ 若非 200：`service.RelayErrorHandler` + `ResetStatusCode`（`:160-169`）。
10. `adaptor.DoResponse(c, httpResp, info)`（`:171`）→ `usage`。
11. `PostAudioConsumeQuota` 或 `PostTextConsumeQuota`（`:178-185`）。

`ClaudeHelper`（`relay/claude_handler.go:22-157`）、`GeminiHelper`（`relay/gemini_handler.go:22-157`）、`ResponsesHelper`（`relay/responses_handler.go:18-101`）、`ImageHelper`（`relay/image_handler.go:24`）、`AudioHelper`（`relay/audio_handler.go:18`）、`RerankHelper`（`relay/rerank_handler.go:20`）均为同一骨架，只是 `Convert*Request`/`DoResponse` 与计费入口不同。

### 5.3 适配器接口与 ApiType→Adapter

接口 `channel.Adaptor`（`relay/channel/adapter.go:18-35`）：`Init`、`GetRequestURL`、`SetupRequestHeader`、`ConvertOpenAIRequest`、`ConvertClaudeRequest`、`ConvertGeminiRequest`、`ConvertEmbeddingRequest`、`ConvertImageRequest`、`ConvertAudioRequest`、`ConvertRerankRequest`、`ConvertOpenAIResponsesRequest`、`DoRequest`、`DoResponse`、`GetModelList`、`GetChannelName`。

`GetAdaptor(apiType)`（`relay/relay_adaptor.go:50-128`）把 `constant.APIType*` 映射到 `relay/channel/*` 的 `Adaptor`，例如：
- `APITypeOpenAI/OpenRouter/Xinference` → `openai.Adaptor`（`:60-61,100-103`）
- `APITypeAnthropic` → `claude.Adaptor`（`:54-55`）
- `APITypeGemini` → `gemini.Adaptor`（`:58-59`）
- `APITypeAws` → `aws.Adaptor`（`:76-77`）
- `APITypeVertexAi` → `vertex.Adaptor`（`:88-89`）等，共 37 个（`:51-126`）。

`ApiType` 来自 `ChannelType2APIType(channelType)`（在 `InitChannelMeta` 中，`relay/common/relay_info.go:281`）。

### 5.4 各 adaptor 的 URL / Header / ConvertRequest

**OpenAI（`relay/channel/openai/adaptor.go`）**
- `GetRequestURL`（`:114-190`）：Azure 走 `deployments/{model}/{task}?api-version=`（`:126-175`）；`ChannelTypeCustom` 用 `{model}` 替换（`:178-181`）；Claude/Gemini 源格式默认拼 `{base}/v1/chat/completions`（`:183-187`）；其余 `relaycommon.GetFullRequestURL(base, RequestURLPath, channelType)`（`:188`）。
- `SetupRequestHeader`（`:192-251`）：`Authorization: Bearer <ApiKey>`（`:238-240`，若 HeaderOverride 已设 Authorization 则跳过）、Azure 用 `api-key`（`:194-197`）、OpenRouter 补 `HTTP-Referer/X-OpenRouter-Title`（`:242-249`）、Realtime/WS 用 `Sec-WebSocket-Protocol`（`:212-236`）。
- `ConvertOpenAIRequest`（`:253-…`）：OpenRouter 方言、reasoning 处理等。
- `DoRequest`（`:752-762`）：音频/图片表单 → `channel.DoFormRequest`；Realtime → `channel.DoWssRequest`；其余 → `channel.DoApiRequest`。
- `DoResponse`（`:764-798`）：按 `RelayMode` 走 `OpenaiRealtimeHandler / OpenaiTTSHandler / OpenaiSTTHandler / OpenaiImage(Stream)Handler / RerankHandler / OaiResponses(Stream)Handler / OaiStreamHandler / OpenaiHandler`。

**Claude（`relay/channel/claude/adaptor.go`）**
- `GetRequestURL`（`:76-90`）：`{base}/v1/messages`，必要时加 `?beta=true`。
- `SetupRequestHeader`（`:116-126`）：`x-api-key`、`anthropic-version`（默认 `2023-06-01`），透传 `anthropic-beta`（`CommonClaudeHeadersOperation` `:105-114`）。
- `ConvertClaudeRequest`（`:36-61`）：`relayconvert.ApplyClaudeThinkingModel`、默认 `max_tokens`；`ConvertOpenAIRequest`（`:128-137`）与 `ConvertGeminiRequest`（`:25-34`）都通过 `service.ConvertRequest(..., RelayFormatClaude, ...)` 做协议转换。
- `DoResponse`（`:164-174`）：置 `FinalRequestRelayFormat=Claude`；`RelayFormatOpenAIResponses` 流式 → `ClaudeResponsesStreamHandler`；否则 `ClaudeStreamHandler/ClaudeHandler`。

**Gemini（`relay/channel/gemini/adaptor.go`）**
- `GetRequestURL`（`:137-163`）：`{base}/{version}/models/{model}:generateContent`，流式 `streamGenerateContent?alt=sse`，embedding/imagen 特判。
- `SetupRequestHeader`（`:165-169`）：`x-goog-api-key`。
- `ConvertGeminiRequest`（`:27-48`）、`ConvertOpenAIRequest/ConvertClaudeRequest/ConvertOpenAIResponsesRequest` 都走 `service.ConvertRequest(..., RelayFormatGemini, ...)`（`:50-59,171-180,244-254`）。
- `DoResponse`（`:260-297`）：Responses / Gemini 原生（embed、stream、non-stream）/ imagen / embedding 分派。

**AWS（`relay/channel/aws/adaptor.go`）**
- `GetRequestURL`（`:95-111`）：`AwsApiTypeMantle` → `mantleRequestURL`；`AwsKeyTypeApiKey` → `bedrock-runtime.{region}.amazonaws.com/model/{awsModelId}/converse`；否则 AKSK 模式返回空 URL（由 SDK 客户端发）。
- `ConvertClaudeRequest`（`:41-80`）：先走 Claude 转换，再把 URL 图片转 base64（`service.GetBase64Data`）。
- `ConvertOpenAIRequest`（`:121-143`）：Nova 模型转 Nova 请求，否则转 Claude。
- `DoRequest`（`:159-181`）：Mantle → `channel.DoRequest`；ApiKey → `channel.DoApiRequest`；AKSK → `doAwsClientRequest`（AWS SDK SigV4）。
- `DoResponse`（`:183-199`）：Mantle/ApiKey 复用 Claude 的 DoResponse；Nova/普通走 `handleNovaRequest/aws(Stream)Handler`。

### 5.5 实际发送与 HTTP client / 代理

`channel.DoApiRequest`（`relay/channel/api_request.go:323-349`）：
1. `a.GetRequestURL(info)`（`:324`）。
2. `http.NewRequest(method, url, body)`，`ApplyUpstreamBodyMetadata` 设 `ContentLength/GetBody`（`:329-333`，实现 `:31-49`）。
3. `a.SetupRequestHeader(c, &headers, info)`（`:335`）。
4. `ApplyHeaderOverride`（`:341`）——在 adaptor 头之后应用，保证用户 HeaderOverride 最高优先级（`:186-321`，支持 `{api_key}`、`{client_header:x}`、`*`、`re:`/`regex:` 透传规则）。
5. `doRequest(c, req, info)`（`:344`）。

`doRequest`（`relay/channel/api_request.go:457-514`）：
- `service.GetHttpClientWithProxySettings(info.ChannelSetting.Proxy, info.ChannelSetting)` 取（缓存）HTTP client（`:458`，实现 `service/http_client.go:400-423`；代理客户端按 proxy+传输策略缓存，`InitHttpClient` 在 `service/http_client.go:147-152`）。
- `relayClient.CheckRedirect = keepUpstreamRedirectResponse`（不自动跟随 3xx，`:467,453-455`）。
- 流式：`helper.SetEventStreamHeaders(c)` + 可选 ping keepalive（`:477-486`）。
- 记录上游 origin 到 context（`:488`）。
- `relayClient.Do(req)`（`:489`），失败 → `ErrorCodeDoRequestFailed`（`:490-493`）并隐藏上游细节。
- 记录上游 `RequestIdKey`（`:507-509`），关闭 body（`:511-512`）。

`GetFullRequestURL`（`relay/common/relay_utils.go:26-38`）：`baseURL + requestURL`，Cloudflare 网关特判。

### 5.6 RelayKit（协议转换）何时介入

`relaykit/relayconvert` 是协议转换注册表：
- `ConvertRequest(ctx, info, target, request)`（`relaykit/relayconvert/request_registry.go:157-178`）：先 `inferRequestRelayFormat` 推断源格式，源==目标直接返回；否则查 `(from,to)` 路由并执行注册的 converter。
- converter 注册表与 ID（`request_registry.go:77-89`）：如 `openai_chat_completions_to_anthropic_messages`、`anthropic_messages_to_openai_chat_completions`、`openai_responses_to_openai_chat_completions`、`gemini_generate_content_to_openai_chat_completions` 等；`registerBuiltinRequestConverter`（`:91-144`）建立直连/链式路由。
- 响应侧由 `relaykit/relayconvert/response_registry.go` 提供 `ResponseOpenAI2Claude`、`StreamResponseOpenAI2Claude`、`ResponseOpenAI2Gemini` 等（供 `service/convert.go:13-27` 转发）。

业务入口是 `service.ConvertRequest`（`service/request_converter.go:26-32`）→ `relayconvert.ConvertRequest`，并 `info.RecordConversionDiagnostics`。
**何时介入**：当**客户端请求格式（RelayFormat）≠ 渠道原生格式（adaptor 目标）**时，adaptor 的 `Convert*Request` 会调用 `service.ConvertRequest` 做跨协议转换（如 Claude/Gemini/AWS 适配器的 `ConvertOpenAIRequest`）；响应侧在 `DoResponse` 里调用 `relayconvert.Response*` 转回客户端格式。若同协议（如 OpenAI→OpenAI chat），`ConvertOpenAIRequest` 主要做字段规整而不跨协议转换。`info.InitRequestConversionChain/AppendRequestConversion`（`relay/common/relay_info.go:756-798`）记录转换链，用于日志与计费语义判定。

---

## 六、响应与收尾（含计费）

### 6.1 DoResponse 与 usage 抽取

- 各 adaptor `DoResponse` 返回 `usage any`，文本场景为 `*dto.Usage`。
- OpenAI 流式（`relay/channel/openai/relay-openai.go:105-199`）：`helper.StreamScannerHandler` 逐事件解析，末尾用 `handleLastResponse` 取 usage；若上游未带 usage，用 `service.ResponseText2Usage(...)` 估算（`:185-188`），并 `applyUsagePostProcessing`（`:190`）。
- OpenAI 非流式 `OpenaiHandler`（`relay/channel/openai/relay-openai.go:253+`）解析响应体 usage。
- API 错误：`service.RelayErrorHandler`（`service/error.go:87-138`）读 body，`TryToOpenAIError` 或按 message 归一为 `NewAPIError`，保留上游状态码；`ResetStatusCode`（`service/error.go:140-163`）按渠道 `status_code_mapping` 改写状态码。

### 6.2 文本计费与结算

`service.PostTextConsumeQuota`（`service/text_quota.go:400-560`）：
1. `effectiveBillingUsage` + 渠道亲和用量缓存上报（`:401-408`）。
2. `calculateTextQuotaSummary`（`:411`，实现 `:229`）。
3. 分层计费 `TryTieredSettle`，命中则 `composeTieredTextQuota`（`:423-439`）。
4. 无可用 usage → 不扣费并记错误（`:458-460`）；否则 `UpdateUserUsedQuotaAndRequestCount` + `UpdateChannelUsedQuota`（`:462-464`）。
5. `SettleBilling`（`:466`）结算差额。
6. `Generate(Claude/Text)OtherInfo` 组装日志附加信息（`:480-543`），`model.RecordConsumeLog`（`:545-558`）。

`service.SettleBilling`（`service/billing.go:65-109`）：
- 有 `relayInfo.Billing`（`BillingSession`）：`delta = actual - preConsumed`，`session.Settle(actual)`（`:66-90`），并按 `BillingSource` 发额度通知（`:92-99`）。
- 无 session：回退 `PostConsumeQuota(delta)`（`:103-108`）。

预扣：`relay.PrepareRequestBilling`（`relay/request_billing.go:25-68`）：
- 敏感词检查（`setting.ShouldCheckPromptSensitive`，`:26,44-51`）。
- `service.EstimateRequestToken` 估算 token（`:53`），`helper.ModelPriceHelper` 计算 `priceData`（`:59`），免费模型跳过（`:63-66`），`service.PreConsumeBilling`（`:67`）。
- `service.PreConsumeBilling`（`service/billing.go:24-57`）：创建 `BillingSession`（`service/billing_session.go:389`）或增量 `ReserveBeforeSubmit`；负值/额度不足返回 400/403 且 `SkipRetry`。

失败退款：`relay.RefundFailedRequestBilling`（`relay/request_billing.go:72-82`）在 `controller.Relay` 的 defer 中执行：`NormalizeViolationFeeError` → `Billing.Refund(c)` → `ChargeViolationFeeIfNeeded`（已结算的 session 不会重复退款，见 `service/billing_session.go:132-153`）。

`BillingSession` 关键方法（`service/billing_session.go`）：`Settle`（`:47`）、`Refund`（`:88`）、`Reserve`/`ReserveBeforeSubmit`（`:158/163`）、`preConsume`（`:208`）、`NewBillingSession`（`:389`）。

分层计费：`controller/relay.go:189` 每次选定渠道后 `service.PrepareTieredBillingForSelectedGroup`；结算见 `service/tiered_settle.go`（`TryTieredSettle` 等）。

### 6.3 收尾

- `Distribute` 在 `c.Next()` 后 `RecordChannelAffinity`（`middleware/distributor.go:136-138`）。
- `BodyStorageCleanup` 的 `defer` 清理磁盘/内存请求体与下载文件缓存（`middleware/body_cleanup.go:14-21`）。
- `StatsMiddleware` 递减活跃连接（`middleware/stats.go:23-29`）。
- `controller.Relay` 顶部 `defer` 统一输出错误体（`controller/relay.go:93-121`）。

---

## 七、完整链路步骤表（路由 → upstream → 响应）

以 `POST /v1/chat/completions`（OpenAI 渠道）为代表：

| # | 阶段 | 位置 |
|---|---|---|
| 1 | 全局中间件（PanicRecover/RequestId/Version/I18n/Logger） | `main.go:185-191` |
| 2 | relay 侧中间件（CORS/Decompress/BodyStorageCleanup/Stats） | `router/relay-router.go:16-19` |
| 3 | `/v1` 组：RouteTag / SystemPerformanceCheck / TokenAuth | `router/relay-router.go:72-74` |
| 4 | ModelRequestRateLimit | `router/relay-router.go:80` |
| 5 | Distribute → getModelRequest 解析 model | `router/relay-router.go:92` / `middleware/distributor.go:390` |
| 6 | SelectChannelForRequest 选渠道 | `middleware/distributor.go:111` / `service/channel_select.go:303` |
| 7 | SetupContextForSelectedChannel 写 context（channel_id/key/base_url/映射/覆盖） | `middleware/distributor.go:586-708` |
| 8 | controller.Relay 进入 | `router/relay-router.go:105-107` → `controller/relay.go:72` |
| 9 | GetAndValidateRequest 解析/校验请求体 | `controller/relay.go:123` / `relay/helper/valid_request.go:22/338` |
| 10 | GenRelayInfo 生成 RelayInfo | `controller/relay.go:134` / `relay/common/relay_info.go:692` |
| 11 | PrepareRequestBilling 预扣（敏感词/预估 token/定价） | `controller/relay.go:160` / `relay/request_billing.go:25` |
| 12 | 重试循环开始；getChannel（首次复用 Distribute 渠道） | `controller/relay.go:177-182,298-313` |
| 13 | TextHelper：InitChannelMeta → ModelMappedHelper → GetAdaptor → Init | `relay/compatible_handler.go:26,42,70,74` |
| 14 | ConvertOpenAIRequest 请求体转换 + ParamOverride | `relay/compatible_handler.go:112-149` |
| 15 | DoRequest → DoApiRequest：GetRequestURL / SetupRequestHeader / HeaderOverride | `relay/compatible_handler.go:153` / `relay/channel/api_request.go:323-349` |
| 16 | doRequest：取 HTTP client（含代理）→ 发往 upstream | `relay/channel/api_request.go:457-514` |
| 17 | 非 200 → RelayErrorHandler + ResetStatusCode | `relay/compatible_handler.go:160-169` / `service/error.go:87,140` |
| 18 | DoResponse 解析响应与 usage（流式/非流式） | `relay/compatible_handler.go:171` / `relay/channel/openai/adaptor.go:764` |
| 19 | PostTextConsumeQuota：算额度 → SettleBilling → RecordConsumeLog | `relay/compatible_handler.go:184` / `service/text_quota.go:400` / `service/billing.go:65` |
| 20 | 成功 MarkRequestPolicySuccess + return；或失败 DecideRelayRetry/ProcessChannelError | `controller/relay.go:217-232` / `service/relay_error.go:20,63` |
| 21 | 请求结束中间件收尾（affinity/body cleanup/stats） | `middleware/distributor.go:136` / `middleware/body_cleanup.go:14` / `middleware/stats.go:23` |

`/v1/messages`：步骤 12 后走 `ClaudeHelper`（`controller/relay.go:209-210`），adapter 可能是 `claude.Adaptor` 或 `openai/gemini/aws`（跨协议转换）。
`/v1beta/models/*path`：`geminiRelayHandler` → `GeminiHelper`/`GeminiEmbeddingHandler`（`controller/relay.go:62-70,211-212`）。
`POST /v1/responses`：入口链为 `task-plugin-protocol-router.go:30-35`，`Distribute` 后经 `RouteTaskPluginEndpoint`，未命中插件时 `controller.Relay(RelayFormatOpenAIResponses)` → `ResponsesHelper`（`relay/responses_handler.go:18`）。
图片/音频：`ImageHelper`/`AudioHelper`（`relay/image_handler.go:24` / `relay/audio_handler.go:18`），`DoResponse` 对应 `OpenaiImage(Stream)Handler`/`OpenaiTTS/STTHandler`（`relay/channel/openai/adaptor.go:768-779`）。

---

## 八、异常与重试

1. **重试循环**：`for retryParam.GetRetry() <= common.RetryTimes`（`controller/relay.go:177`；任务提交版 `:542`；Responses WS 版 `relay/responses_websocket.go:298`）。
2. **`common.RetryTimes` 默认值 = 0**（`common/constants.go:143`）。运行期由 option `RetryTimes` 覆盖（`model/option.go:656-657`），并进入 `model/request_policy.go` 的策略快照（`:129-131`）。
3. **重试决策 `service.DecideRelayRetry`**（`service/relay_error.go:20-57`），优先级：
   - `err == nil` → stop（`:21-23`）；
   - 渠道亲和失败且严格会话 → stop（`:24-30`）；
   - 渠道约束抑制重试（pin）→ stop（`:31-33`）；
   - `types.IsChannelError` → retry（`:34-36`）；
   - `types.IsSkipRetryError` → stop（`:37-39`）；
   - `retryTimes <= 0`（预算耗尽）→ stop（`:40-42`）；
   - 2xx → stop（`:43-46`）；<100 或 >599 → retry（`:47-49`）；
   - 命中“总是跳过重试”的错误码/状态码 → stop（`:50-52`）；
   - `operation_setting.ShouldRetryByStatusCode(code)` → retry（`:53-55`）；
   - 否则 stop（`:56`）。
4. **状态码重试区间默认**（`setting/operation_setting/status_code_ranges.go:21-29`）：`100-199`、`300-399`、`401-407`、`409-499`、`500-503`、`505-523`、`525-599`。即不重试 2xx、`400`、`408`、`504`、`524`。`alwaysSkipRetryStatusCodes = {504,524}`（`:31-34`），`alwaysSkipRetryCodes = {ErrorCodeBadResponseBody}`（`:36-38`）。
5. **`SkipRetry` 的写入点**：请求体解析/校验失败、读 body 过大、模型映射/定价/转换失败、`invalid api type` 等本地错误普遍带 `types.ErrOptionWithSkipRetry()`（例如 `controller/relay.go:127-130`、`relay/compatible_handler.go:30,35,44,72,114,124,145`），因此不会无谓重试。
6. **`AutomaticDisableChannelEnabled` 默认 `false`**（`common/constants.go:134`）。`service.ProcessChannelError`（`service/relay_error.go:63-99`）在错误时：记错误日志；若 `ShouldDisableChannel(err) && channelError.AutoBan` 则异步 `DisableChannel`（`:69-74`，`service/channel.go:29-48`）；并按需记录消费错误日志（`:76-98`）。`ShouldDisableChannel`（`service/channel.go:60-80`）：渠道错误直接 true；`SkipRetry` 错误 false；命中 `AutomaticDisableStatusCodes`（默认 `401`，`status_code_ranges.go:17`）或关键字 true。
7. **重试时重新选渠道**：首次用 Distribute 结果，重试走 `CacheGetRandomSatisfiedChannel`（`controller/relay.go:314-329`），并把每次渠道记入 `use_channel`（`service/channel_select.go:410-412`），最后打 retry 日志（`controller/relay.go:235-239`）。
8. **流式错误**：已开始 SSE 时用 `writeRelayStreamError` 在流内输出错误事件（`controller/relay.go:99-103,242-257`）。

---

## 九、证据（文件:行号）

**入口/路由**
- `main.go:180-199` 全局中间件与 `SetRouter` 调用。
- `router/main.go:15-22` 路由装配顺序。
- `router/relay-router.go:16-19` relay 侧通用中间件；`:21-62` 模型列表路由；`:64-70` playground；`:71-80` `/v1` 组与 `GET /responses`；`:83-92` ws/http 子组；`:97-180` 各 POST relay 路由；`:182-190,224-245` mj；`:193-221` `/v1beta`。
- `router/task-plugin-protocol-router.go:13-25,27-59` host protocol 路由与 `POST /v1/responses`、图片/视频路由。
- `router/video-router.go:10-33` 视频路由。
- `router/api-router.go:17-23` `/api` 上的 `AccessTokenAudit`/`GlobalAPIRateLimit`（非 relay）。

**中间件**
- `middleware/distributor.go:34-140`（`Distribute`）、`:390-549`（`getModelRequest`）、`:586-708`（`SetupContextForSelectedChannel`）。
- `middleware/auth.go:528-647`（`TokenAuth`）、`:673-719`（`SetupContextForToken`）、`:459-526`（`TokenAuthReadOnly`）。
- `middleware/rate-limit.go:110-159,183-188`（IP/全局限流、`GlobalAPIRateLimit`）、`:219-261`（用户限流）。
- `middleware/model-rate-limit.go:173-231`（`ModelRequestRateLimit`）、`:82-165`（Redis/内存实现）。
- `middleware/cors.go:9-16`（CORS）、`:18-25`（Version）。
- `middleware/recover.go:33-46`（`PanicRecover`）。
- `middleware/logger.go:14-19`（`RouteTag`）、`:21-67`（`SetUpLogger`）。
- `middleware/performance.go:14-39`（`SystemPerformanceCheck`）。
- `middleware/body_cleanup.go:11-24`、`middleware/stats.go:17-31`、`middleware/gzip.go:26-113`、`middleware/request-id.go:10-18`。
- `middleware/audit.go:265-320`（`AccessTokenAudit`，仅 `/api`/Web）。

**选渠道/分发**
- `service/channel_select.go:303-397`（`SelectChannelForRequest`）、`:114-208`（`CacheGetRandomSatisfiedChannel`）、`:410-412`（`AppendUsedChannel`）。

**控制器/执行**
- `controller/relay.go:35-70`（mode 分派）、`:72-240`（`Relay`）、`:242-257`（流式错误）、`:298-330`（`getChannel`）、`:332-334`（`processChannelError`）。
- `relay/helper/valid_request.go:22-61,338-397`（请求解析/校验）。

**RelayInfo/模型映射**
- `relay/common/relay_info.go:268-356`（`InitChannelMeta`）、`:585-669`（`genBaseRelayInfo`）、`:692-798`（`GenRelayInfo`/转换链）。
- `relay/helper/model_mapped.go:14-69`（`ModelMappedHelper`）。

**出站/适配器**
- `relay/relay_adaptor.go:50-128`（`GetAdaptor`）。
- `relay/channel/adapter.go:18-35`（`Adaptor` 接口）。
- `relay/channel/api_request.go:31-49`（`ApplyUpstreamBodyMetadata`）、`:186-321`（HeaderOverride）、`:323-349`（`DoApiRequest`）、`:351-379`（`DoFormRequest`）、`:385-432`（`DoWssRequest`）、`:457-514`（`doRequest`）。
- `relay/common/relay_utils.go:26-38`（`GetFullRequestURL`）。
- `service/http_client.go:147-152`（`InitHttpClient`）、`:400-423`（`GetHttpClientWithProxySettings`）。
- `relay/compatible_handler.go:25-187`（`TextHelper`）。
- `relay/claude_handler.go:22-157`（`ClaudeHelper`）。
- `relay/gemini_handler.go:22-157,159-260`（`GeminiHelper`/`GeminiEmbeddingHandler`）。
- `relay/responses_handler.go:18-111`（`ResponsesHelper`/`ConsumeResponsesQuota`）。
- `relay/image_handler.go:24+`、`relay/audio_handler.go:18-77`、`relay/rerank_handler.go:20+`。
- `relay/channel/openai/adaptor.go:114-190,192-251,752-798`。
- `relay/channel/claude/adaptor.go:36-61,76-126,164-174`。
- `relay/channel/gemini/adaptor.go:27-48,137-169,260-297`。
- `relay/channel/aws/adaptor.go:41-80,95-111,121-143,159-199`。
- `relay/channel/openai/relay-openai.go:105-199`（流式 usage）。

**RelayKit**
- `relaykit/relayconvert/request_registry.go:77-89,91-144,157-178`。
- `service/request_converter.go:26-48`、`service/convert.go:13-27`。

**计费/结算**
- `relay/request_billing.go:25-68,72-82`。
- `service/billing.go:24-57,65-109`。
- `service/billing_session.go:47,88,132-167,208,389`。
- `service/text_quota.go:229-388,400-560`。

**异常/重试**
- `service/relay_error.go:20-57,63-99`。
- `service/error.go:87-163`。
- `service/channel.go:29-80`。
- `common/constants.go:133-135,143`。
- `setting/operation_setting/status_code_ranges.go:17-38,53-85`。
- `model/option.go:656-657`。

---

## 十、未确认项

1. `helper.StreamScannerHandler` / `helper.StartStreamKeepAlive` 的内部实现细节（`relay/helper/stream_scanner.go`、`relay/helper/common.go`）未逐行确认；仅在调用点确认了用途。
2. `helper.ModelPriceHelper`（`relay/helper/price.go`）的定价公式与 `PriceData` 各字段计算细节未展开；本文只确认其被 `PrepareRequestBilling` 调用并产出 `QuotaToPreConsume`。
3. `service.PrepareTieredBillingForSelectedGroup`（`service/tiered_settle.go`）的分层计费快照生成逻辑未逐行确认。
4. `model.GetRandomSatisfiedChannel` / `model.CacheGetChannel` 的渠道缓存与优先级算法属“路由策略细节”，本文按任务要求未展开（仅在 `service/channel_select.go` 调用点确认）。
5. `relaykit/relayconvert` 的具体转换矩阵（各 `(from,to)` 组合的质量等级/链式转换）只确认了注册表与 ID 列表，未逐一核验全部转换函数实现（`internal/` 下大量 `to_*_req/resp.go`）。
6. `doAwsClientRequest`（AWS SigV4 签名）与 `mantleRequestURL` 的细节未逐行确认，仅确认 `aws.Adaptor.DoRequest` 的分支（`relay/channel/aws/adaptor.go:159-181`）。
7. `Reserved`/`Task` 类请求（`/v1/videos`、`/mj/*`）的计费与轮询收尾走 `executeTaskSubmissionWith`（`controller/relay.go:502-761`）与 `service/task_billing.go`，本文只在主链路层面提及，未逐行展开。
8. 路由注册对 `engine.Use` 的“向后生效”语义判断基于 Gin 的处理器链固化行为；若后续 Gin 版本改变该行为，`router/relay-router.go:16-19` 四个中间件的实际作用范围需复核（**未确认**是否有意为之）。
