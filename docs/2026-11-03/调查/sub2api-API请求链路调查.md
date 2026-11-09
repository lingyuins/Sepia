# sub2api 单次 API 请求链路调查报告

> 仓库：`D:\CodeWorkspace\sub2api`（Go(ent) + Vue3 + PostgreSQL/Redis 的「AI API 网关 / 订阅配额分发」）
> 范围：**单个 API 请求的完整生命周期** —— 入站 → 中间件 → 路由分发（选账号）→ 出站（上游协议/传输/凭证/插件）→ 响应返回 → 计费与收尾。
> 明确**不覆盖**路由策略细节（另有文档），只说明「请求如何从 handler 进入选账号并继续」。
> 全部结论基于只读源码；标注 `文件:行号`，行号对应本次检出的代码，若文件后续改动会漂移。凡未能从源码确认的写「未确认」。
> 忽略 `.git / node_modules / dist`。

---

## 1. 概览

sub2api 的网关对外提供三种协议兼容面，内部按「分组（Group）平台」决定请求实际被送往哪一类上游：
**Anthropic（Claude）/ OpenAI（含 Codex、Grok、多协议 API Key 供应商）/ Gemini（含 Antigravity）**。

一次请求的宏观生命周期：

```
HTTP 入口(gin.Engine)
  └─ 全局中间件：Recovery → RequestLogger → SessionBindingContext → Logger → CORS → SecurityHeaders → ServerTiming
       └─ RegisterGatewayRoutes 注册的网关分组
            └─ 分组中间件链：bodyLimit → clientRequestID → opsErrorLogger → endpointNorm
                              → apiKeyAuth → groupModelAllowlist → compositeTarget → requireGroupAnthropic
                 └─ handler（按分组平台分派 Gateway.* / OpenAIGateway.* / GeminiV1Beta*）
                      └─ SelectAccountWithLoadAwareness / SelectAccountWithScheduler（选账号 + 并发槽）
                           └─ Forward / ForwardAsResponses / ForwardAsChatCompletions / ForwardAsAnthropic / GeminiCompat.Forward
                                └─ 取凭证(token) → 解析代理/TLS 指纹 → httpUpstream.DoWithTLS(或插件 gRPC RoundTrip)
                                     └─ 解析上游响应/SSE → ForwardResult（含 usage）
                                          └─ submitUsageRecordTask → RecordUsage → usage_log + 扣费(billing)
```

关键角色：
- **入口/路由**：`internal/server/routes/gateway.go`（`RegisterGatewayRoutes`），全局中间件在 `internal/server/router.go` / `http.go`。
- **Handler**：`GatewayHandler`（`gateway_handler.go` 等，Anthropic/Gemini 系与协议转换）、`OpenAIGatewayHandler`（`openai_gateway_handler.go`，OpenAI/Grok 系）。
- **调度（选账号）**：`service.GatewayService.SelectAccountWithLoadAwareness`（`gateway_scheduling.go`）与 `service.OpenAIGatewayService.SelectAccountWithScheduler*`（`openai_account_scheduler.go`）。
- **出站**：`service.GatewayService.Forward`（`gateway_forward.go`）、`service.OpenAIGatewayService.Forward`（`openai_gateway_forward.go`），底层 `HTTPUpstream.Do/DoWithTLS`（`internal/repository/http_upstream.go`）或插件 `PluginManager.RoundTripOpenAIOAuth`（gRPC 子进程）。
- **计费/收尾**：`service.GatewayService.RecordUsage`（`gateway_usage_billing.go`）、`UsageRecordWorkerPool`、`OpsErrorLoggerMiddleware`。

---

## 2. 入站链路

### 2.1 入口装配

- 服务器构建于 `internal/server/http.go:50`：`r := gin.New()`，首个中间件 `middleware2.Recovery()`（`http.go:51`），随后 `SetupRouter`。
- `internal/server/router.go:58-71` 依次挂全局中间件：
  `RequestLogger()`（:59）→ `SessionBindingContext(cfg)`（:62）→ `Logger()`（:63）→ `CORS(cfg.CORS)`（:64）→ `SecurityHeaders(...)`（:65-70）→ `ServerTiming(...)`（:71）。
- 路由注册入口 `internal/server/router.go:133`：`routes.RegisterGatewayRoutes(r, h, apiKeyAuth, apiKeyService, subscriptionService, opsService, settingService, compositeResolver, cfg)`。

### 2.2 `RegisterGatewayRoutes`（`routes/gateway.go:22`）

在该函数开头预构造中间件与分派闭包（`gateway.go:33-47`）：
- `bodyLimit := middleware.RequestBodyLimit(cfg.Gateway.MaxBodySize)`（:33）
- `textBodyLimit := middleware.RequestBodyLimit(cfg.Gateway.TextMaxBodySize)`（:34）
- `clientRequestID := middleware.ClientRequestID()`（:35）
- `opsErrorLogger := handler.OpsErrorLoggerMiddleware(opsService)`（:36）
- `endpointNorm := handler.InboundEndpointMiddleware()`（:37）
- `compositeTarget := compositeTargetPlatformMiddleware(compositeResolver)`（:38）
- `compositeGeminiTarget := compositeGeminiTargetPlatformMiddleware(compositeResolver)`（:39）
- `requireGroupAnthropic := middleware.RequireGroupAssignment(settingService, middleware.AnthropicErrorWriter)`（:42）
- `requireGroupGoogle := middleware.RequireGroupAssignment(settingService, middleware.GoogleErrorWriter)`（:43）
- `groupModelAllowlist := middleware.GroupModelAllowlist()`（:47）

路由分四组注册：

1. **`/v1`（Claude/OpenAI 兼容）**：`gateway := r.Group("/v1")`（:180），链见 2.3。路由覆盖：
   - `POST /v1/messages`（:192-198）：`isOpenAIResponsesCompatibleGatewayPlatform(c)` 为真走 `h.OpenAIGateway.Messages`，否则 `h.Gateway.Messages`。
   - `POST /v1/systemone`（:200，使用 `textBodyLimit`）。
   - `POST /v1/messages/count_tokens`（:203）→ `countTokensHandler`（:53-62，Grok/OpenAI 走 `OpenAIGateway.CountTokens`，否则 `Gateway.CountTokens`）。
   - `GET /v1/models`（:207，`modelsHandler`，带 `client_version` 走 Codex manifest）、`GET /v1/models/:model`（:209）。
   - `GET /v1/usage`（:210）、`POST /v1/live`（:211）、`GET /v1/live/:call_id`（:212）。
   - `POST /v1/responses`（:214-220，按平台走 `OpenAIGateway.Responses` 或 `Gateway.Responses`）；`POST /v1/responses/*subpath`（:221-227，经 `guardResponsesSubpath`，:159-177）；`GET /v1/responses`（:229-231，WebSocket 入口 `ResponsesWebSocket`）。
   - `POST /v1/chat/completions`（:233-239）。
   - `POST /v1/embeddings`（:240-252，仅 OpenAI 平台）、`/v1/images/*`（:253-267）、`/v1/videos/*`（:270-281，Grok）、`/v1/tts|stt|custom-voices|realtime|web_search|x_search`（:295-334，Grok 专属，非 Grok 平台返回 404）。
2. **`/v1beta`（Gemini 原生兼容）**：`gemini := r.Group("/v1beta")`（:338），链见 2.3。路由：`GET /v1beta/models`、`GET /models/:model`、`POST /models/*modelAction`、`/v1beta/cachedContents`（CRUD，:348-357）。
3. **根路径/别名路由**：`rootRoute` 闭包（:370-372）把同一链挂到不带 `/v1` 前缀的 `/api/v3`、`/v3`、`/v1`、``（空）等多个前缀；`/responses`、`/responses/*subpath`、`/alpha/search`、`/models`、`/messages/count_tokens`、`/chat/completions`、images/videos/voice 等（:373-488）。另有 `codexDirect := r.Group("/backend-api/codex")`（:387-399）。
4. **Antigravity 专用**：`r.GET("/antigravity/models", ...)`（:491）；`antigravityV1 := r.Group("/antigravity/v1")`（:494-508，含 `ForcePlatform`）；`antigravityV1Beta := r.Group("/antigravity/v1beta")`（:510-523）。

### 2.3 分组中间件链（`routes/gateway.go`）

- `/v1` 组（:180-189，顺序即 `Use` 顺序）：
  `bodyLimit`(:181) → `clientRequestID`(:182) → `opsErrorLogger`(:183) → `endpointNorm`(:184) → `gin.HandlerFunc(apiKeyAuth)`(:185)；然后在 `GET /sub2api/billing` 之后 `groupModelAllowlist`(:187) → `compositeTarget`(:188) → `requireGroupAnthropic`(:189)。
- `/v1beta` 组（:339-346）：
  `bodyLimit`(:339) → `clientRequestID`(:340) → `opsErrorLogger`(:341) → `endpointNorm`(:342) → `APIKeyAuthWithSubscriptionGoogle(...)`(:343) → `groupModelAllowlist`(:344) → `compositeGeminiTarget`(:345) → `requireGroupGoogle`(:346)。
- 根路径别名（`rootRoute`，:371）与 `/backend-api/codex`（:388）复用同样的 `bodyLimit, clientRequestID, opsErrorLogger, endpointNorm, apiKeyAuth, groupModelAllowlist, compositeTarget, requireGroupAnthropic`。
- Antigravity 组额外在 `apiKeyAuth` 前插入 `ForcePlatform(PlatformAntigravity)`（:499/:515）。

### 2.4 请求解析（handler）

统一模式：取 apiKey/subject → 读 body（带 `MaxBytesError` 判定 413）→ 解析 model/stream → 设置 ops/prcing 上下文 → 校验 → 进入选账号循环。

- **`GatewayHandler.Messages`（`gateway_handler.go:125`）**
  - 读 body：`readLenientJSONRequestBodyWithPrealloc(c.Request, h.cfg)`（:148），空体 400（:158）。
  - `service.ParseGatewayRequest(bodyRef, domain.PlatformAnthropic)`（:166）得到 `ParsedRequest`（`Model/Stream/MaxTokens/ThinkingEnabled` 等）。
  - `setOpsRequestContext` / `setOpsEndpointContext`（:214-215），`WithGatewayTokenRequestPricing`（:216-217）。
  - 平台优先取 `ForcePlatform`（Antigravity）→ `ResolvedTargetPlatformFromContext`（Composite）→ `apiKey.Group.Platform`（:306-314）。
  - 粘性会话：`GenerateSessionHash`（:298）→ 查 `GetCachedSessionAccountID`（:323）→ `WithPrefetchedStickySession`（:334）。
- **`GatewayHandler.ChatCompletions`（`gateway_handler_chat_completions.go:23`）**：读体（:48）、`gjson` 校验（:66）、取 `model`（:73）、`parseOpenAICompatibleStream`（:88）、渠道映射（:105）；非 OpenAI 平台时走 `GatewayService.ForwardAsChatCompletions`（:317）/`geminiCompat`（:305）/`antigravity`（:315）。
- **`GatewayHandler.Responses`（`gateway_handler_responses.go:23`）**：与会话 hash（:185）；循环内 `SelectAccountWithLoadAwareness`（:194）→ `ForwardAsResponses`（:299）或 Antigravity `ForwardAsResponses`（:297）。
- **`OpenAIGatewayHandler.Responses（openai_gateway_handler.go:389`）**：`setOpenAIClientTransportHTTP`（:395）；读体（:423）；`normalizeOpenAIResponsesCompactRequest`（:441）；`gjson` 校验 model（:466）；`parseOpenAICompatibleStream`（:496）；随后循环 `SelectAccountWithSchedulerForCapability`（:676）→ `h.gatewayService.Forward`（:798）。
- **`OpenAIGatewayHandler.Messages（openai_gateway_handler.go:1153`）**：OpenAI 兼容分组收到 `/v1/messages` 时，必要时 `ForwardAsAnthropic`（:1391）。
- **`GatewayHandler.GeminiV1BetaModels`（`gemini_v1beta_handler.go`）**：模型名从 URL 提取，`SelectAccountWithLoadAwareness`（:596）后转发 Gemini 上游。

### 2.5 body 限制与端点归一

- **`middleware.RequestBodyLimit(maxBytes)`（`request_body_limit.go:10`）**：把 `c.Request.Body` 包成 `http.MaxBytesReader`（:12），超限时下游读取返回 `*http.MaxBytesError`，各 handler 用 `extractMaxBytesError` 转 413。
- **`handler.InboundEndpointMiddleware()`（`handler/endpoint.go:282`）**：取 `c.Request.URL.Path`（空则 `c.FullPath()`），用 `NormalizeInboundEndpoint(path)`（:85-127）归一为规范入站端点常量（`EndpointMessages`/`EndpointResponses`/`EndpointChatCompletions`/`EndpointVideos...` 等，:17-36），写入 gin context 键 `_gateway_inbound_endpoint`（:291）。`DeriveUpstreamEndpoint`（:205-252）按账号平台推导上游端点。

---

## 3. 中间件链（顺序表）

`/v1` 组请求进入后的实际执行顺序（`routes/gateway.go:180-189`）：

| # | 中间件 | 源文件:行 | 作用 |
|---|--------|-----------|------|
| 0 | `Recovery()` / `RequestLogger()` / `SessionBindingContext` / `Logger` / `CORS` / `SecurityHeaders` / `ServerTiming` | `server/http.go:51`、`server/router.go:59-71` | 全局：panic 兜底；注入 X-Request-ID 与 request-scoped logger；解析客户端 IP/UA 到 ctx；CORS；安全响应头（CSP nonce 等）；可选 Server-Timing。 |
| 1 | `RequestBodyLimit(cfg.Gateway.MaxBodySize)`（text 端点用 `TextMaxBodySize`） | `middleware/request_body_limit.go:10` | `http.MaxBytesReader` 限制请求体大小，超限 → handler 返回 413。 |
| 2 | `ClientRequestID()` | `middleware/client_request_id.go:19` | 确保 `client_request_id` 存在（优先复用 ctx 中已有值，否则 `uuid`），回写 `X-Client-Request-ID` 响应头，供 Ops 端到端关联。 |
| 3 | `handler.OpsErrorLoggerMiddleware(opsService)` | `handler/ops_error_logger.go:1077` | 用 capture writer 包裹 `c.Writer` 捕获响应体（`:1079-1090`）；请求结束后 `finalizeCapture`，对 ≥400（含 200 但带内错误）请求解析错误体并落 Ops 错误日志（:1092-1148+）。被 ingress reject 标记的请求跳过（:1094）。 |
| 4 | `handler.InboundEndpointMiddleware()` | `handler/endpoint.go:282` | 归一入站端点写入 ctx（`_gateway_inbound_endpoint`）。 |
| 5 | `apiKeyAuth`（`apiKeyAuthWithSubscription`） | `middleware/api_key_auth.go:34` | 鉴权 + 计费执行：提取 Key（Header/`x-api-key`/`x-goog-api-key`，拒绝 query key）、`GetByKey`、Key/用户/分组状态、IP 白/黑名单、订阅/余额/配额/过期校验；写 `apiKey`/`user`/`group`/`subscription` 到 ctx。详见 3.1。 |
| 6（billing 端点之后） | `GET /sub2api/billing`（仅读倍率，`skipBilling`） | `routes/gateway.go:186`、`api_key_auth.go:172` | 该端点只鉴权不计费执行。 |
| 7 | `GroupModelAllowlist()` | `middleware/group_model_allowlist.go:32` | 分组级模型白名单准入。未绑定分组/未开启白名单直接放行（:35）；Responses WS 入口跳过（:45-49）；读体或从路由参数取候选模型逐一旦校验，未命中返回 404（`local_model_configuration` + `model_not_allowed`，:85-88）。 |
| 8 | `compositeTarget`（`compositeTargetPlatformMiddleware`） | `routes/gateway.go:549` | 仅 Composite 分组；解析模型→目标平台/上游模型（`resolver.Resolve`，:583），写入 `CompositeRouteDecision` 到 ctx（:598），必要时用 `sjson` 改写 body 中上游模型名（:599-605）。跨平台 OAuth 歧义直接 400（:592-596）。 |
| 9 | `requireGroupAnthropic`（`RequireGroupAssignment`+`AnthropicErrorWriter`） | `middleware/middleware.go:143`、:105 | 未分组 Key 若系统不允许未分组调度则 403（Anthropic 错误格式）。`/v1beta`/Antigravity-beta 用 `requireGroupGoogle`（Google 格式，`middleware.go:120`）。 |

补充：
- `middleware.ForcePlatform(platform)`（`middleware/middleware.go:36`）仅 Antigravity 组使用，同时写 `request.Context`（`ctxkey.ForcePlatform`）与 gin ctx（`ContextKeyForcePlatform`）。
- `middleware.AbortWithError(c,status,code,msg)`（`middleware.go:78`）是三协议通用的 JSON 错误出口。

### 3.1 API Key 鉴权与计费执行细节（`middleware/api_key_auth.go`）

`apiKeyAuthWithSubscription`（:34）主要阶段：
1. **提取 Key**：先 `rejectInvalidAuthAbuse`（:37）做无效鉴权滥用限流；Header 过大拦截（:42）；query `key/api_key` 直接 400 弃用（:49-56）；依次尝试 `Authorization: Bearer`（:59-68）→ `x-api-key`（:72）→ `x-goog-api-key`（:82）；全无则 401 `API_KEY_REQUIRED`（:87-95）。
2. **验证 Key**：`apiKeyService.GetByKey`（:100），区分 `ErrAPIKeyNotFound`（401）/`ErrAPIKeyAuthOverloaded`（503）。成功后 `SetOpsFallbackAPIKey`（:119）让 Ops 日志可回退归属。
3. **基础鉴权（始终执行）**：Key 非 active（除 expired/quota_exhausted）拦截（:124-130）；IP 限制（:134-146）；无 user/用户非 active（:149-159）；分组不可用/不可允许（:160-165）。
4. **SimpleMode 早退**（`cfg.RunMode == RunModeSimple`，:176-189）：只写 apiKey/user/role/group 与 `TouchLastUsed` 后 `Next`。
5. **订阅加载**（:191-212）：订阅型分组且非 billing 自省端点时 `GetActiveSubscription`；失败在非 `skipBilling` 情况 403。
6. **计费执行（`!skipBilling`）**（:216-268）：Key 状态/运行时过期/配额；订阅模式 `ValidateAndCheckLimits` + `EnsureWindowMaintenance`（:238-260）；非订阅回退余额 `apiKeyBalanceBelowAuthThreshold`（余额 ≤0，:263）。
7. **写 ctx → Next**（:270-286）：`ContextKeySubscription`/`ContextKeyAPIKey`/`ContextKeyUser`/`ContextKeyUserRole`/`setGroupContext`，非 billing 端点 `TouchLastUsed`。
- `skipBilling` 判定（:172）：`/v1/usage`、`/v1/sub2api/billing`、异步生图任务查询（`isAsyncImageTaskRead`，:336）。
- `RequireGroupAssignment`（`middleware.go:143`）在 `apiKeyAuth` 之后单独执行（见上表第 9 步）。

---

## 4. 路由分发（handler → 选账号 → 继续）

### 4.1 平台判定

`routes/gateway.go:getGroupPlatform(c)`（:536-547）：读 ctx 中 apiKey 的分组平台；若分组是 `PlatformComposite`，用 `service.ResolvedTargetPlatformFromContext`（由 `compositeTarget` 中间件写入 `CompositeRouteDecision`）取解析出的真实目标平台。用于 handler 内部在 `Gateway.*` 与 `OpenAIGateway.*` 之间二选一。

### 4.2 选账号（Anthropic/Gemini 系：`GatewayService`）

- `GatewayService.SelectAccountWithLoadAwareness(ctx, groupID, sessionHash, requestedModel, excludedIDs, metadataUserID, sub2apiUserID)`（`gateway_scheduling.go:100`）：
  - 先 `checkClaudeCodeRestriction`（可能把 groupID 换成 fallback 分组，:115）、`withGroupContext`、`withGatewayProfitControlGate`（:119-120）、`checkChannelPricingRestriction`（:124）。
  - 粘性会话：优先 `prefetchedStickyAccountIDFromContext`，否则 `cache.GetSessionAccountID`（:133-141）。
  - 非负载批处理路径（`concurrencyService == nil || !cfg.LoadBatchEnabled`，:164-212）：循环 `SelectAccountForModelWithExclusions` → `tryAcquireAccountSlot` → `checkAndRegisterSession`，成功即 `newSelectionResult`；拿不到槽则返回 `AccountWaitPlan`（含 MaxConcurrency/Timeout/MaxWaiting）。
  - 负载感知路径（:214+）：`resolvePlatform`（:214）、`listSchedulableAccounts`（:223）、`withWindowCostPrefetch`/`withRPMPrefetch`（:230-231），随后做多层候选过滤、路由/粘性/负载评分（细节属"路由策略"，此处略），最终返回 `AccountSelectionResult`（含 `Account`、`Acquired`、`ReleaseFunc`、`WaitPlan`、`ProfitGate`）。
- 更底层：`SelectAccountForModelWithExclusions`（`gateway_scheduling.go:34`）处理 Composite 分组：`resolveCompositeRouteDecision`（:52）把 `requestedModel` 映射为 `TargetPlatform` + `UpstreamModel`（:59-61）。混合调度见 `selectAccountWithMixedScheduling`（:81）。
- `SelectAccount`（:24）/`SelectAccountForModel`（:29）是薄封装。

### 4.3 选账号（OpenAI/Grok 系：`OpenAIGatewayService`）

- `SelectAccountWithScheduler(...)`（`openai_account_scheduler.go:2100`）、`SelectAccountWithSchedulerForCapability(...)`（:2116，带 `requiredCapability`/`requiredTransport`）、`SelectAccountWithSchedulerForImages(...)`（:2137）均委托 `selectAccountWithScheduler`（:2163）。返回 `(*AccountSelectionResult, OpenAIAccountScheduleDecision, error)`。
- handler 侧：`OpenAIGatewayHandler.Responses` 循环（`openai_gateway_handler.go:667-719`）调用它，按 `OpenAIUpstreamTransportAny` + 请求能力选号；`Messages`/`ChatCompletions`/`Images`/`Embeddings`/`CountTokens` 有各自的 `SelectAccount*` 变体（见 grep 证据）。

### 4.4 Composite 合成路由解析

- `CompositeRouteResolver.Resolve(ctx, groupID, model, endpoint)`（`service/composite_route_resolver.go:97`）：按精确/通配匹配 `CompositeModelRoute`，产出 `CompositeRouteDecision`（含 `Matched`、`TargetPlatform`、`UpstreamModel`、OAuth 歧义标记）。`OAuthAmbiguous()`（:27）/`AmbiguousMessage()`（:32）用于中间件拒绝跨平台 OAuth 歧义。
- 中间件 `compositeTargetPlatformMiddleware`（`routes/gateway.go:549`）在 `apiKeyAuth`+`groupModelAllowlist` 之后、`requireGroupAnthropic` 之前执行解析，命中后把 decision 写入 ctx 并改写 body 中的上游模型名（:583-605），使下游 handler 与调度看到一致的目标平台/模型。
- Gemini 版 `compositeGeminiTargetPlatformMiddleware`（:613）从路由参数 `model`/`modelAction` 取模型解析，未命中则把目标平台兜底设为 `PlatformGemini`（:632-634）。

### 4.5 进入出站

handler 拿到 `Account` 后：
- Anthropic 系：`h.gatewayService.Forward(requestCtx, c, account, attemptParsedReq)`（`gateway_handler.go:922`）；Antigravity 非 APIKey 账号走 `antigravityGatewayService.Forward`（:920）。
- Responses（跨协议）：`h.gatewayService.ForwardAsResponses`（`gateway_handler_responses.go:299`）。
- Chat Completions（跨协议）：`h.gatewayService.ForwardAsChatCompletions`（`gateway_handler_chat_completions.go:317`）。
- OpenAI/Grok：`h.gatewayService.Forward(c.Request.Context(), c, account, attemptBody)`（`openai_gateway_handler.go:798`，即 `OpenAIGatewayService.Forward`）。
- OpenAI 兼容分组收到 `/v1/messages`：`ForwardAsAnthropic`（`openai_gateway_handler.go:1391`）。

---

## 5. 出站链路（协议 / 传输 / 插件 / 凭证 / 代理 / TLS）

### 5.1 `GatewayService.Forward`（Anthropic 主链，`gateway_forward.go:91`）

1. **前置校验/特判**：Bedrock 模型 ID 解析（:99-105）；Anthropic `validateClaude55Request`（:107-112）；`beginUpstreamResponseModelObservation`（:123）；Web Search 模拟短路 `handleWebSearchEmulation`（:126-128）。
2. **Passthrough / Bedrock 分支**：
   - Anthropic API Key 直通：`account.IsAnthropicAPIKeyPassthroughEnabled()` → `forwardAnthropicAPIKeyPassthroughWithInput`（:130-148，实现在 `gateway_anthropic_passthrough.go:56`）。
   - Bedrock：`account.IsBedrock()` → `forwardBedrock`（:150-152，`gateway_bedrock.go`）。
3. **Body 改写**（OAuth mimicry、模型映射、cache 控制、thinking 过滤、空块清理等，:168-384）：非 Claude Code 的第三方客户端在 OAuth 账号上走 `normalizeClaudeOAuthRequestBody`（:245）；`ResolveThinkingProtocol`（`thinking_protocol.go:53`）判定 thinking 契约，`NormalizeChineseLLMThinking` 等对国产 passback-required 上游做补正（:377-384）。
4. **取凭证**：`token, tokenType, err := s.GetAccessToken(ctx, account)`（:332）。
5. **代理与 TLS 指纹**：`proxyURL`（:338-343，自定义 base URL 模式下代理作为查询参数）；`tlsProfile := s.tlsFPProfileService.ResolveTLSProfile(account)`（:346）。
6. **重试循环**（:390-675）：每轮 `buildUpstreamRequest`（:393）→ `s.httpUpstream.DoWithTLS(upstreamReq, proxyURL, account.ID, account.Concurrency, tlsProfile)`（:402）。含 400 签名错误两阶段重试（:413-561）、thinking budget 修正重试（:564-612）、通用可重试状态码指数退避重试（:619-664）。
7. **错误分流**：重试耗尽且应 failover → 返回 `*UpstreamFailoverError`（:682-751）；400 且开启 `FailoverOn400` 也可 failover（:754-798）；其余 ≥400 走 `handleErrorResponse`（:800，`gateway_upstream_response.go:364`）。
8. **成功响应**：`reqStream` → `handleStreamingResponse`（:822，`gateway_upstream_response.go:709`）；非流式 → `handleNonStreamingResponse`（:839，`gateway_upstream_response.go:1397`）；组装 `ForwardResult`（:845-858：RequestID、UpstreamHeaders、Usage、Model/UpstreamModel、Stream、Duration、FirstTokenMs、ClientDisconnect）。

### 5.2 `OpenAIGatewayService.Forward`（OpenAI/Grok 主链，`openai_gateway_forward.go:21`）

- 入口即 `ClearActualOpenAIUpstreamEndpoint`（:23）；`detectCodexClientRestriction`（:43，限制只允许 Codex 官方客户端时 403）；一系列 body 规范化（Lite/legacy ingress/tool schema/reasoning 等，:61-150）。
- Grok 分支：`account.Platform == PlatformGrok` → `forwardGrokResponses`（:158-160）。
- **上游协议判定**：`s.resolveUpstreamProtocolFor(ctx, account, APIProtocolResponses, routingModel)`（:186）→ `upstream_protocol_routing.go:resolveUpstreamProtocol`（:32）：按模型分流 / adaptive / 显式 anthropic / raw chat_completions / 默认 Responses 五级优先级决定发往 `APIProtocolAnthropic`（转原生 Anthropic，`forwardResponsesViaNativeAnthropic`）/`APIProtocolChatCompletions`（`forwardResponsesViaRawChatCompletions`）/Responses（:186-195）。
- **Passthrough 分支**：`account.IsOpenAIPassthroughEnabled()` → `forwardOpenAIPassthrough`（:276，`openai_gateway_passthrough.go`）。
- **WS 分支**：`wsDecision.Transport == OpenAIUpstreamTransportResponsesWebsocketV2` → `forwardOpenAIWSV2`（`openai_gateway_forward.go:786-1008`，`openai_ws_forwarder_v2.go`），带重连/previous_response_id 恢复/encrypted content 恢复。
- **HTTP 分支**：`GetAccessToken`（:779）→ `buildUpstreamRequest`（:1372）：按账号类型选目标 URL（OAuth→`chatgptCodexURL`；SetupToken；APIKey→自定义/`openaiPlatformAPIURL`，:1375-1402）；构造 auth headers（:1417）；Codex 协议设置 `Host=chatgpt.com`、originator、session_id/conversation_id（:1428-1479）；白名单透传客户端头（:1436-1444）。
- 实际发送经 `doOpenAIUpstream`（`openai_plugin_transport.go:11`）——见 5.4。

### 5.3 协议转换（跨协议）

- **`ForwardAsResponses`（`gateway_forward_as_responses.go:31`）**：Responses 请求 → Anthropic Messages（`apicompat.ResponsesRequest` 转换，:55-70）→ 发 Anthropic 上游 → 再转回 Responses。用于 Anthropic 分组接收 OpenAI Responses 客户端。
- **`ForwardAsChatCompletions`（`gateway_forward_as_chat_completions.go:29`）**：Chat Completions → Anthropic → 转回。
- **`ForwardAsAnthropic`**（在 `openai_gateway_service.go` 附近被引用；实现文件待核，见未确认项）：OpenAI 兼容分组接收 `/v1/messages` 时转 Anthropic 上游。
- **`convertResponsesShapedChatBody`（`upstream_protocol_routing.go:122`）**：把发到 `/v1/chat/completions` 的 Responses 形状请求体（Cursor）转为 Chat Completions 请求体。
- **thinking 协议**：`ResolveThinkingProtocol`（`thinking_protocol.go:53`）区分 `AnthropicStrict` 与 `PassbackRequired`，配套 `ShouldPreFilterThinkingBlocks`/`ShouldRectifyThinkingSignatureError`/`ShouldApplyRetryFilters`（:101-116），决定是否在转发前剥离/修正 thinking block 及 400 重试行为。

### 5.4 传输层：`HTTPUpstream`

接口 `service.HTTPUpstream`（`http_upstream_port.go:11`）：`Do`（不带 TLS 指纹）与 `DoWithTLS`（带指纹）。实现于 `internal/repository/http_upstream.go`：
- `Do`（:203）：覆盖 `applyGrokCLIProxyHeaders` → `validateRequestHost`（防 SSRF/非法 Host）→ 按 `(proxyURL, accountID, concurrency, profile)` 取/建隔离客户端 `acquireClientWithProfile`（:214）→ `doUpstreamRequest`（:222）→ 用 `wrapTrackedBody` 包装响应体，在 `Close` 时减 `inFlight` 计数（:234-237）。
- `DoWithTLS`（:246）：`profile == nil` 时退化为 `Do`（:247-249）；纯 HTTP 无 TLS 握手也退化为 `Do`（:252-254）；否则 `acquireClientWithTLS`（:275）用指纹 profile 建带自定义 Dialer 的客户端并发送（:281-296）。
- 代理用于 `proxyURL`（空=直连）；代理选择与回退见 `proxy_service.go` 与 `proxy_fallback.go:ResolveProxyFallbackTarget`（:12，支持 direct / 备用代理链、防环）。

### 5.5 TLS 指纹

- `TLSFingerprintProfileService.ResolveTLSProfile(account)`（`tls_fingerprint_profile_service.go:177`）：未启用→nil（不伪装）；启用且绑定 `profile_id`→`GetProfileByID`（:137，本地缓存热路径）；`id==-1`→随机 Profile（:149）；否则返回内置默认 Profile（`"Built-in Default (Node.js 24.x)"`，:194）。
- profile 数据来自 DB/缓存的 `model.TLSFingerprintProfile`，`ToTLSProfile()` 转运行时结构。

### 5.6 凭证获取与刷新

- **`GatewayService.GetAccessToken`（`gateway_service.go:1258`）**：按账号类型分派——OAuth/SetupToken → `getOAuthToken`（:1288）；APIKey → `GetCredential("api_key")`（:1264）；Bedrock → 返回 `"bedrock"`（:1270）；ServiceAccount(Anthropic) → `claudeTokenProvider.GetAccessToken`（:1278）。`getOAuthToken` 对 Anthropic OAuth 用 `ClaudeTokenProvider`（:1290），Grok OAuth 取 `GetGrokAccessToken`（:1300），其余直接读 `access_token`（:1308）。
- **`ClaudeTokenProvider.GetAccessToken`（`claude_token_provider.go:55`）**：先查缓存（`tokenCache.GetAccessToken`，:70）；临近过期（`claudeTokenRefreshSkew=3min`）时 `refreshAPI.RefreshIfNeeded`（:86）或旧路径加分布式锁刷新（:107-118）；取 `access_token` 后回填缓存（TTL 按过期时间，:121-158）；带 token 版本一致性检查（`CheckTokenVersion`，:128）。
- **`OpenAIGatewayService.GetAccessToken`（`openai_gateway_service.go:1213`）**：Grok 走 `grokTokenProvider`（:1228），其余走 `openaiTokenProvider`（:1242）。
- **后台刷新**：`TokenRefresher` 接口（`token_refresher.go:11`）及 `ClaudeTokenRefresher`（:24，`OAuthService.RefreshAccountToken`）/`OpenAITokenRefresher`（:74）；批量/后台调度由 `token_refresh_service.go`（`TokenRefreshService`）承担（本报告只确认为"后台刷新服务"，具体调度细节未展开）。

### 5.7 OpenAI OAuth 命中插件（gRPC 子进程）

- `doOpenAIUpstream`（`openai_plugin_transport.go:11`）：若注入了 `pluginManager`，调用 `PluginManager.RoundTripOpenAIOAuth(ctx, request, proxyURL, account)`；命中返回 `handled=true` 则用插件响应，否则回落 `s.httpUpstream.Do(...)`（:12-18）。
- `PluginManager.ShouldRouteOpenAIOAuth`（`plugin_manager.go:983`）：仅 `PlatformOpenAI + AccountTypeOAuth` 且命中 rollout 百分比（`stablePluginBucket(account.ID) < rolloutPercent`）时路由到插件。
- `PluginManager.RoundTripOpenAIOAuth`（`plugin_manager.go:947`）：检查运行时可用性/健康/`beginRequest` 后调用 `route.runtime.roundTrip`（:968）。
- `pluginRuntime.roundTrip`（`plugin_runtime.go:274`）：通过 `HCPlugin.ProtocolGRPC` 启动的子进程（`startPluginRuntime`，:38，`exec.CommandContext` + `hcplugin.NewClient`，校验二进制 SHA256），用 `api.Forward(streamCtx)` gRPC 双向流发送 `ForwardRequest{Start/BodyChunk/BodyEnd}`，接收 `Start` 响应头与 `BodyChunk` 组装 `*http.Response`（:279-354）。即：**请求/响应体被流式转发给外部插件子进程，由插件完成真正的 OpenAI OAuth 出站，核心链只处理解析/错误映射/SSE/计费**。

---

## 6. 响应与收尾

### 6.1 上游响应解析

- Anthropic 链：`handleStreamingResponse`（`gateway_upstream_response.go:709`）解析 SSE、`parseSSEUsage`（:1165）/`extractSSEUsagePatch`（:1195）累计 `ClaudeUsage`；`handleNonStreamingResponse`（:1397）解析 JSON usage。流中断时 `partialStreamUsageResult` 保留已观测 usage（`gateway_forward.go:830`）。
- OpenAI 链：`openai_gateway_response_handling.go`（响应处理）、`openai_ws_forwarder_v2.go`（WS）等产出 `OpenAIForwardResult`（含 Usage/FirstTokenMs/BillingModel 等）。

### 6.2 usage 计费与落库

- handler 成功后调用 `h.submitUsageRecordTask(ctx, func(ctx){ h.gatewayService.RecordUsage(ctx, &RecordUsageInput{...}) })`：
  - Anthropic 系在 `gateway_handler.go:602-630`（Gemini 循环）/`:965-993`（Anthropic 循环）；OpenAI 系在 `openai_gateway_handler.go:817+`（`submitResponsesUsage`）。
- `GatewayHandler.submitUsageRecordTask`（`gateway_handler.go:2564`）：`wrapUsageRecordTaskContext` 后提交到 `usageRecordWorkerPool.Submit`（:2570）；池停止时降级为同步执行（带 10s 超时的后台 ctx，:2583-2593）。OpenAI 版在 `openai_gateway_handler.go:3439`。
- `UsageRecordWorkerPool.Submit`（`usage_record_worker_pool.go:155`）：`TrySubmit` 入队；队满按 `overflowPolicy`（sync/sample/drop）处理（:178-193）。
- **`GatewayService.RecordUsage`（`gateway_usage_billing.go:656`）→ `recordUsageCore`（:772）**：
  - `ApplyForwardImageBillingResolution`、`ForceCacheBilling` 把 input→cache_read（:783-788）、cache TTL override（:793-796）。
  - `recordUsageMultipliers`（:798/:909，用户>分组>默认倍率 + 高峰因子）、`recordUsageBillingModel`（:799/:929）、`calculateRecordUsageCost`（:809/:1014）→ 调用 `BillingService.CalculateCostUnified`（`billing_service.go:1558`）/`calculateTokenCost`（:1610）。
  - 订阅 vs 余额计费方式（:832-837），`buildRecordUsageLog`（:841/:1298）构造 `UsageLog`（含请求/上游模型、各 token 分类、成本、倍率、endpoint、SessionID、GroupID 等，:1326-1371）。
  - `applyUsageBilling(ctx, requestID, usageLog, &postUsageBillingParams{...}, s.billingDeps(), s.usageBillingRepo)`（:883）执行扣费；失败时 `ActualCost=0` 并仍尽最大努力写 usage_log（:897-901）。
  - `writeUsageLogBestEffort`（:623）写 `usage_log`。
- `postUsageBilling`（`gateway_usage_billing.go:146`）/`finalizePostUsageBilling`（:395）/`syncBalanceCacheAfterDeduction`（:468）/`notifyBalanceLow`（:499）/`notifyAccountQuota`（:540）：扣减用户余额/订阅额度、同步余额缓存、低余额与账号配额通知。

### 6.3 Ops 日志

- `OpsErrorLoggerMiddleware`（`handler/ops_error_logger.go:1077`）：捕获响应体（capture writer），请求结束 `finalizeCapture`（:1092），对失败（或带内流错误）请求 `parseOpsErrorResponse` 解析错误类型/消息，`classifyOpsErrorLog` 归类 phase/owner/source，`logOpsRecoveredUpstream`/`logOpsStreamError` 记录恢复行与流错误（:1112-1146+）。Ingress reject（鉴权早退等）已在 `apiKeyAuth` 中 `MarkIngressRejected` 标记，此处跳过（:1094）。
- handler 侧另通过 `setOpsRequestContext`/`setOpsEndpointContext`/`setOpsSelectedAccount` 把 model/stream/endpoint/account 写入 ops ctx。

---

## 7. 完整链路步骤表（网关请求 → upstream Account → 响应）

以 `POST /v1/messages`（Anthropic 分组）为例，串起「入站 → 中间件 → 选号 → 出站 → 响应 → 计费」：

| 步 | 阶段 | 位置（文件:行） | 说明 |
|----|------|------------------|------|
| 1 | 引擎/全局中间件 | `server/http.go:50-51`、`server/router.go:58-71` | `gin.New()` + `Recovery`；`RequestLogger/SessionBinding/Logger/CORS/SecurityHeaders/ServerTiming`。 |
| 2 | 路由匹配 | `routes/gateway.go:180,192` | 命中 `/v1` 组 `POST /messages`。 |
| 3 | bodyLimit | `routes/gateway.go:181` → `request_body_limit.go:10` | `MaxBytesReader` 限制 body。 |
| 4 | clientRequestID | `routes/gateway.go:182` → `client_request_id.go:19` | 生成/复用 `client_request_id`。 |
| 5 | opsErrorLogger | `routes/gateway.go:183` → `ops_error_logger.go:1077` | 包裹 writer 捕获响应体。 |
| 6 | endpointNorm | `routes/gateway.go:184` → `endpoint.go:282` | 归一为 `/v1/messages`。 |
| 7 | apiKeyAuth | `routes/gateway.go:185` → `api_key_auth.go:34` | 验 Key/用户/分组/IP/订阅/余额，写 ctx。 |
| 8 | groupModelAllowlist | `routes/gateway.go:187` → `group_model_allowlist.go:32` | 分组模型白名单校验（未开启放行）。 |
| 9 | compositeTarget | `routes/gateway.go:188` → `routes/gateway.go:549` | 非 Composite 分组直接 `Next`；Composite 解析目标平台/改写模型。 |
| 10 | requireGroupAnthropic | `routes/gateway.go:189` → `middleware.go:143` | 未分组 Key 拦截（允许时放行）。 |
| 11 | handler 入口 | `routes/gateway.go:192-198` | `getGroupPlatform` 选 `h.Gateway.Messages`（Anthropic）或 `h.OpenAIGateway.Messages`。 |
| 12 | 请求解析 | `gateway_handler.go:125-314` | 读体→`ParseGatewayRequest`→设 ops/pricing→确定平台→算 session hash→查粘性绑定。 |
| 13 | 并发槽（用户） | `gateway_handler.go:253` | `AcquireUserSlotWithWait` 获取用户并发槽。 |
| 14 | 二次计费校验 | `gateway_handler.go:266`、`:277` | `CheckBillingEligibility` + 在途余额预留 `reserveInflightBalance`。 |
| 15 | 选账号 | `gateway_handler.go:682` → `gateway_scheduling.go:100` | `SelectAccountWithLoadAwareness` 返回 `AccountSelectionResult`（含 Account/槽/等待计划）。 |
| 16 | 账号并发槽 | `gateway_handler.go:756-806` | 已抢槽直接用，否则按 `WaitPlan` 等待/入队；`AcquireAccountSlotWithWaitTimeout`。 |
| 17 | 利润终检 | `gateway_handler.go:808-836` | `GatewayProfitControlVetoLatest` 否决则重选；通过则绑定粘性。 |
| 18 | 转发 | `gateway_handler.go:907-923` | `h.gatewayService.Forward`（Antigravity 用 `antigravityGatewayService.Forward`）。 |
| 19 | 取凭证/代理/TLS | `gateway_forward.go:332,338-346` | `GetAccessToken` + `ResolveTLSProfile`。 |
| 20 | 上游 HTTP | `gateway_forward.go:390-402` → `repository/http_upstream.go:246` | 重试循环内 `DoWithTLS`（或插件 gRPC `RoundTripOpenAIOAuth`）。 |
| 21 | 响应解析 | `gateway_forward.go:820-843` → `gateway_upstream_response.go:709/1397` | 流式/非流式解析 usage，得 `ForwardResult`。 |
| 22 | 错误/失败转移 | `gateway_forward.go:682-800`、`failover_loop.go:193` | 上游错误→`UpstreamFailoverError`→handler 换号/同账号重试/停止。 |
| 23 | usage 提交 | `gateway_handler.go:938-993`、`submitUsageRecordTask`(:2564) | 经 worker 池异步提交 `RecordUsage`。 |
| 24 | 计费落库 | `gateway_usage_billing.go:772` | 计算成本→`applyUsageBilling`→写 `usage_log`、扣余额/额度。 |
| 25 | Ops 收尾 | `ops_error_logger.go:1092+` | 失败请求写 Ops 错误日志；成功带内错误按恢复行记录。 |

> OpenAI 系（`/v1/responses`）步骤 12–18 对应 `openai_gateway_handler.go:389-798`：`SelectAccountWithSchedulerForCapability`(:676) → `Forward`(:798)；出站经 `openai_gateway_forward.go:21` 的 `OpenAIGatewayService.Forward`。

---

## 8. 异常与失败转移

### 8.1 handler 层 failover 循环（`internal/handler/failover_loop.go`）

- `FailoverState`（:125）维护 `SwitchCount/MaxSwitches/FailedAccountIDs/SameAccountRetryCount/LastFailoverErr/ForceCacheBilling/profitVetoedAccountIDs/profitVetoCount`。
- **`HandleFailoverError`（:193）**：客户端已断开→`FailoverCanceled`（:203）；`UpstreamFailoverError.ShouldRetryNextAccount()` 为假→`FailoverExhausted`（:207）；同账号重试（`sameAccountRetryAllowed`，含 `RetryableOnSameAccount`/延迟/deadline，:79-103）→睡眠后 `FailoverContinue`（:220-234）；重试用尽→`TempUnscheduleRetryableError` 临时封禁（:237-239）；加入失败集合，`SwitchCount >= MaxSwitches` 则耗尽（:245-247），否则递增并对 Antigravity 线性延时（:258-264）。
- **`HandleSelectionExhausted`（:276）**：针对单账号分组 503 场景，清空排除集合并退避 2s 后重选；若排除集全为利润否决则直接耗尽（避免活锁，:290-296）。
- **`needForceCacheBilling`（:323）**：粘性会话实际换号或上游标记 `ForceCacheBilling` 时，把 input_tokens 转 cache_read 计费。
- 各 handler 循环在 `Forward` 返回 `*UpstreamFailoverError` 时调用 `fs.HandleFailoverError`；若 `c.Writer.Size()` 在 Forward 前后增加（SSE 已写客户端）则禁止 failover，直接按失败收尾（`gateway_handler.go:521-526`、`:917-918`）。

### 8.2 上游错误分类与限流/冷却

- `GatewayService.shouldRetryUpstreamError`（`gateway_forward.go:36`）/`shouldFailoverUpstreamError`（:47）：决定同账号重试 vs 换号。
- **`RateLimitService`（`ratelimit_service.go`）**：`HandleUpstreamError`（:333）统一处理上游错误（401/403/429/5xx/529 等）→ 标记账号限流/冷却/模型级限流/过期；`ApplyAccountSchedulingThreshold`（:165）/`PreCheckUsage`（:604）在选号时过滤；`UpdateSessionWindow`（:2060）更新订阅窗口；`RecoverAccountState`（:2217）/`ClearTempUnschedulable`（:2258）。
- **过载冷却（overload/529）**：`OverloadCooldownSettings`（`settings_view.go:571`，默认启用 10 分钟，`DefaultOverloadCooldownSettings` :614），在 `RateLimitService` 处理 5xx/529 时套用（`ratelimit_service.go:2021-2036`）；配置键 `SettingKeyOverloadCooldownSettings`（`domain_constants.go:578`）。
- **`temp_unsched.go`**：`TempUnschedState`（:9，含 `UntilUnix/StatusCode/MatchedKeyword/Scope/Model` 等）；`TempUnschedCache` 接口（:24）；`HandleTempUnschedulable`（`ratelimit_service.go:2354`）把账号/模型临时移出可调度池，`TempUnscheduleRetryableError`（failover 用）据此冷却。
- **模型级限流**：`model_rate_limit.go`（模型维度限流快照，选号时过滤不支持/被限流模型）。

### 8.3 错误透传（passthrough）

- `error_passthrough_runtime.go:applyErrorPassthroughRule`（:31）：从 ctx 取绑定的 `ErrorPassthroughService`，`MatchRule(platform, upstreamStatus, responseBody)` 命中后按规则改写 status/errType/errMsg（:49-71），`skip_monitoring` 时标记跳过 Ops（:65-67）。
- handler 在转发前 `service.BindErrorPassthroughService(c, h.errorPassthroughService)`（`gateway_handler.go:245-247`），使 service 层非 failover 错误也能复用规则。
- `error_passthrough_service.go`：规则匹配/CRUD（`MatchRule` 等）。

### 8.4 上游协议路由与失败回退

- `resolveUpstreamProtocol`（`upstream_protocol_routing.go:32`）决定 OpenAI 网关请求的上游协议；`shouldForwardOpenAIResponsesViaRawChatCompletions`（`openai_gateway_forward.go:1347`）等用于判定是否降级到 Chat Completions。
- `gateway_upstream_transport_error.go:handleUpstreamTransportError`（:42）：把传输层错误（连接/TLS/代理等）归类为 ops 事件，并据可重试性决定是否 failover（`openai_upstream_transport_error` 类似）。

### 8.5 并发槽与等待

- `ConcurrencyService`（`concurrency_service.go`）：Redis 有序集合管理账号槽 `concurrency:account:{id}`、用户槽 `concurrency:user:{id}`、等待计数（接口 :23-56）。`AcquireAccountSlot/ReleaseAccountSlot`、`IncrementAccountWaitCount/Decrement`、`GetAccountsLoadBatch` 用于负载感知选号与等待计划。
- handler 用 `ConcurrencyHelper`（`AcquireUserSlotWithWait`/`AcquireAccountSlotWithWaitTimeout`/`IncrementAccountWaitCount`）封装，超时/客户端断开时通过 `wrapReleaseOnDone` 自动释放，避免泄漏。

### 8.6 代理回退

- `proxy_fallback.go:ResolveProxyFallbackTarget`（:12）：过期代理按 `FallbackMode`（direct/proxy 链）改投直连或备用代理，防环（:17-21）。
- `proxy_service.go`：代理 CRUD/`URL()`；`openai_proxy_stream_circuit.go`：代理流熔断（隔离不健康代理，选号时降级为偏好，见 `openai_account_scheduler.go:2156-2162`）。

---

## 9. 证据（文件:行号）

- 路由注册与分组链：`backend/internal/server/routes/gateway.go:22`（`RegisterGatewayRoutes`）、`:33-47`（中间件构造）、`:180-189`（`/v1` 链）、`:338-347`（`/v1beta` 链）、`:370-372`（`rootRoute`）、`:387-399`（codexDirect）、`:494-523`（Antigravity）、`:536-547`（`getGroupPlatform`）、`:549-638`（composite 中间件）、`:673-695`（endpoint 归类）。
- 全局中间件：`backend/internal/server/http.go:50-51`、`backend/internal/server/router.go:58-71,133`。
- 中间件实现：`middleware/request_body_limit.go:10`；`middleware/client_request_id.go:19`；`middleware/recovery.go:19`；`middleware/cors.go:16`；`middleware/security_headers.go:111`；`middleware/request_logger.go:17`；`middleware/server_timing.go:29`；`middleware/middleware.go:36,78,105,120,143`；`middleware/api_key_auth.go:34,100,119,134,149,160,172,176,196,216,270`；`middleware/group_model_allowlist.go:32,45,85,113`。
- 端点归一：`handler/endpoint.go:85,205,282,308,330`。
- handler 入口/解析：`handler/gateway_handler.go:125,148,166,253,266,306,323,682,745,756,808,907,920,922,938,2564`；`handler/gateway_handler_chat_completions.go:23,48,66,88,197,305,317`；`handler/gateway_handler_responses.go:23,185,194,297,299`；`handler/openai_gateway_handler.go:389,423,441,466,496,676,798,1153,1391,3439`；`handler/gemini_v1beta_handler.go:596`。
- 调度：`service/gateway_scheduling.go:24,29,34,52,81,100,164,214`；`service/openai_account_scheduler.go:2100,2116,2137,2163`；`service/composite_route_resolver.go:27,32,97`。
- 出站：`service/gateway_forward.go:36,47,91,130,150,332,338,346,390,402,413,619,682,720,754,800,820,839,845`；`service/gateway_upstream_response.go:364,709,1165,1397`；`service/openai_gateway_forward.go:21,158,186,276,779,786,1347,1372`；`service/openai_gateway_service.go:340,361,1213`；`service/gateway_forward_as_responses.go:31`；`service/gateway_forward_as_chat_completions.go:29`；`service/gateway_anthropic_passthrough.go:56,295,373,848`；`service/thinking_protocol.go:53,101`；`service/upstream_protocol_routing.go:32,122`；`service/gateway_bedrock.go`（Bedrock）。
- 凭证/传输/代理/TLS/插件：`service/gateway_service.go:1258,1288`；`service/claude_token_provider.go:55,86,121`；`service/openai_gateway_service.go:1213`；`service/token_refresher.go:11,24,74`；`repository/http_upstream.go:203,246`；`service/tls_fingerprint_profile_service.go:137,177,194`；`service/proxy_service.go:193`；`service/proxy_fallback.go:12`；`service/openai_plugin_transport.go:11`；`service/plugin_manager.go:947,983`；`service/plugin_runtime.go:38,274`。
- 计费/响应收尾：`service/gateway_usage_billing.go:146,356,395,623,656,772,798,809,841,883,909,1014,1298`；`service/usage_record_worker_pool.go:155`；`service/billing_service.go:1558,1610`；`handler/ops_error_logger.go:1077,1092,1112`。
- 失败转移：`handler/failover_loop.go:125,193,276,323`；`service/ratelimit_service.go:165,333,604,2021,2060,2217,2354`；`service/temp_unsched.go:9,24`；`service/model_rate_limit.go`；`service/error_passthrough_runtime.go:31`；`service/upstream_protocol_routing.go:32`；`service/gateway_upstream_transport_error.go:42`；`service/concurrency_service.go:23`；`service/proxy_fallback.go:12`；`service/openai_proxy_stream_circuit.go`。

---

## 10. 未确认项

1. **`ForwardAsAnthropic` 的具体实现文件与行号**：handler 在 `openai_gateway_handler.go:1391` 调用 `h.gatewayService.ForwardAsAnthropic(...)`，但本次检出的 `grep` 未定位到其 `func (s *GatewayService) ForwardAsAnthropic` 定义（应在 `service` 包内某文件）；未确认其确切位置与协议转换细节。
2. **`OpenAIGatewayHandler.Messages`（`openai_gateway_handler.go:1153`）的完整分支**：仅确认其在 OpenAI 兼容分组下可能走 `ForwardAsAnthropic`（:1391）；其余分支（是否直接对 Responses/Chat 路径复用）未逐一读完。
3. **`token_refresh_service.go`（`TokenRefreshService`）的后台调度细节**（轮询间隔、并发、失败重试策略）：仅确认其为后台刷新服务入口，未展开内部实现。
4. **`ratelimit_service.go`（约 111KB）的完整错误码→动作矩阵**：仅列出主要入口函数（`HandleUpstreamError` 等），未逐条穷举 401/403/429/529/5xx 的全部子分支与关键词匹配表。
5. **`openai_ws_forwarder_v2.go` / `openai_ws_forwarder_ingress.go` 的 WS 双向细节**：仅确认 WSv2 分支存在（`openai_gateway_forward.go:786`）及其重连恢复逻辑，未深入帧协议。
6. **`billing_service.go` 的价格表/渠道定价合并算法**（`getModelPricingAt`/`applyChannelTokenPriceOverrides` 等）：仅确认计费调用的入口与 `calculateTokenCost` 存在，具体定价规则不在本次范围逐条核对。
7. **Grok 专属链路**（`openai_gateway_grok.go`、`grok_media.go`、`grok_audio.go`、`openai_live.go`）的完整出站分支：仅在出站入口看到 Grok 分流（`openai_gateway_forward.go:158`），未逐函数展开。
8. **`antigravity_gateway_service.go` 的 Gateway 出站细节**：仅确认 `antigravityGatewayService.Forward/ForwardAsResponses/ForwardAsChatCompletions` 被调用，未深入其内部实现（`antigravity_gateway_*.go`）。
