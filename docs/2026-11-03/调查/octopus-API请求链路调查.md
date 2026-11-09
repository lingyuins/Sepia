# Octopus 单次 API 请求链路调查（入站 → 中间件 → 出站 → 响应）

- 调查对象：`D:\CodeWorkspace\octopus`
- 调查方式：只读 Go 源码（未修改被调查仓库，未运行会改变其状态的命令）。
- 调查日期：2026-11-03
- 说明：下文路径均相对于 `D:\CodeWorkspace\octopus`；行号用于定位本次工作区源码，后续改动可能使其漂移。
- 范围说明：**只描述“一个请求在代码里怎么流动”**。模型映射、GroupMode/权重、Key 选择策略、账号池调度策略等“路由策略细节”已有同类文档（`docs/2026-11-03/调查/octopus-路由模型调查.md`），本文只在必要处点到为止。

---

## 1. 概览

Octopus 的 API 请求实际由**两套转发引擎**处理，共用同一套“鉴权中间件 + 分组/渠道/均衡器/健康反馈”基础设施，但请求体处理方式完全不同：

| 引擎 | 入口 | 请求体处理 | 适配器 | 典型端点 |
| --- | --- | --- | --- | --- |
| **LLM Relay** | `relay.Handler`（`internal/relay/relay.go:195`） | inbound adapter 解析为 `InternalLLMRequest` → outbound adapter 按目标协议重新编码 | inbound/outbound transformer 双向转换 + 流式转换 | `/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/embeddings` |
| **Media Relay** | `relay.MediaHandler`（`internal/relay/media_relay.go:71`） | 只抽取 `model` 字段，其余 body 基本原样（JSON 或 multipart）转发 | 不走 transformer，仅做 provider 级 body/path 改写 | `/v1/images/*`、`/v1/audio/*`、`/v1/videos/*`、`/v1/music/*`、`/v1/search`、`/v1/rerank`、`/v1/moderations` |
| **节点隧道** | `POST /node/v1/connect`（`handlers/node.go:167`） | WebSocket 帧，非 HTTP 请求体 | `internal/nodetransport` | 节点（Magpie）反向连接 |

核心数据流（LLM 主链路）：

```text
TCP/HTTP
  └─ gin.Engine（全局中间件）
       └─ Group("/v1") 中间件：APIKeyAuth → DevMockPublicSuccess → RequireJSON
            └─ handlers.chat/response/message/embedding
                 └─ relay.Handler(endpointType, inboundType, c)
                      ├─ parseRequest: readLimitedRequestBody → inbound.Get().TransformRequest → Validate
                      ├─ applyBeforeRoutePlugin（插件 relay.request.before_route）
                      ├─ resolveRelayModel → Group + relayModelIdentity
                      ├─ condition.Evaluate（分组级条件）
                      ├─ reserveRelayQuota（RPM/TPM 预留）
                      ├─ 语义缓存 / singleflight（可选短路）
                      └─ executeRelay
                           └─ routeRound →NewIterator→ routeIter.Next()
                                └─ channel → keyRound（Key 或池账号）
                                     └─ adapterLoop（Chat/Responses/Anthropic…）
                                          └─ relayAttempt.attempt → forward
                                               ├─ prepareInternalRequestForOutbound（rewrite + param_override + thinking）
                                               ├─ outAdapter.TransformRequest（按目标协议编码）
                                               ├─ copyHeaders + applyBeforeUpstreamPlugin
                                               ├─ sendRequest（SSRF + proxy + HTTP client / nodeTransport）
                                               └─ handleForwardResponse → handleStreamResponse / handleResponse
                                                    └─ outAdapter.Transform(Stream|Response) → inAdapter.Transform(Stream|Response) → gin 写回
```

三个贯穿全程的 context（易混，先说明）：

- `clientCtx = c.Request.Context()`：承载客户端连接生命周期，客户端断连即取消。
- `operationCtx`：`newRelayOperationContext()` 基于 `context.Background()` 创建（`internal/relay/context.go:38-43`），可用 `OCTOPUS_RELAY_UPSTREAM_TIMEOUT_SECONDS` 加总超时。**它与客户端断连解耦**，用于“客户端断连后仍继续生成给 stream session 缓冲”。
- 持久化 context：`newRelayPersistenceContext()`（`context.go:45-47`），默认 10s，专门用于写 relay log / 统计，避免客户端 ctx 取消导致日志写不进去。

---

## 2. 入站链路

### 2.1 路由注册机制

路由不是集中在一处 `router.go` 里写死，而是**各 handler 包在 `init()` 里通过 `router.NewGroupRouter` 全局注册**，最后由 `server.Start()` 调 `router.RegisterAll` 统一挂到 gin（`internal/server/server.go:88`、`internal/server/router/router.go:103-126`）。

`RegisterAll` 的处理顺序（`router.go:103-126`）：

1. 校验每个 `Route.Validate()`（必须有 handler、method 合法，`router.go:82-91`）；
2. `engine.Group(router.Path, router.Middlewares...)` 建组（组级中间件）；
3. 每个路由的最终 handler 链 = `route.Middlewares...` + `route.Handlers...`（`router.go:116-121`）；
4. `registerRoute` 按 method 注册到 gin（`router.go:129-155`）。

LLM 端点注册（`internal/server/handlers/relay.go:14-35`）：

| Method | Path | handler | 入参枚举 |
| --- | --- | --- | --- |
| POST | `/v1/chat/completions` | `chat` → `relay.Handler(EndpointTypeChat, InboundTypeOpenAIChat, c)` | OpenAI Chat |
| POST | `/v1/responses` | `response` → `relay.Handler(EndpointTypeResponses, InboundTypeOpenAIResponse, c)` | OpenAI Responses |
| POST | `/v1/messages` | `message` → `relay.Handler(EndpointTypeMessages, InboundTypeAnthropic, c)` | Anthropic Messages |
| POST | `/v1/embeddings` | `embedding` → `relay.Handler(EndpointTypeEmbeddings, InboundTypeOpenAIEmbedding, c)` | OpenAI Embedding |

媒体端点注册（`internal/server/handlers/media.go:12-63`）：

- JSON 组（`APIKeyAuth + DevMockPublicSuccess + RequireJSON`）：`/v1/images/generations`、`/v1/audio/speech`、`/v1/videos/generations`、`/v1/music/generations`、`/v1/search`、`/v1/rerank`、`/v1/moderations`；
- multipart 组（`APIKeyAuth + DevMockPublicSuccess`，**不加 RequireJSON**）：`/v1/images/edits`、`/v1/images/variations`、`/v1/audio/transcriptions`；
- 每个 handler 都只是 `relay.MediaHandler(relay.MediaEndpointXxx, c)`（`media.go:65-103`）。

另有一个仅读端点：`GET /v1/models` 由 `handlers/model.go:132-137` 注册（仅 `APIKeyAuth`）。节点隧道端点：`GET /node/v1/connect`（`handlers/node.go:24`），**没有** `Auth`/`APIKeyAuth`，靠自己校验 Bearer 节点令牌。

`/node/v1/connect` 的建连流程（`handlers/node.go:167-195`）：`node.FeatureEnabled` 检查 → 从 `Authorization: Bearer` 取节点凭据 → `node.Authenticate` 校验 → 逐层 `Unwrap()` 拿到真实 `http.ResponseWriter` → `nodetransport.DefaultManager.Accept(...)` 升级为 WebSocket（`internal/nodetransport/manager.go:78-133`，先读 `hello` 帧协商并发）。

### 2.2 读取 body

LLM 主链路：`parseRequest` 调用 `readLimitedRequestBody(c, getMaxRelayJSONBodyBytes())`（`internal/relay/relay.go:600-606`）。

- 默认上限 64 MiB，可用 `relay.max_json_body_bytes` 覆盖（`internal/relay/request_body.go:14-19`）；multipart 同理 64 MiB（`request_body.go:21-26`）。
- `readLimitedRequestBody` 先把 `c.Request.Body` 包成 `http.MaxBytesReader`，再 `io.ReadAll`（`request_body.go:28-43`）；超限被 `errors.As(*http.MaxBytesError)` 归一为 `errRelayRequestBodyTooLarge`（`request_body.go:45-55`），HTTP 状态映射为 413（`request_body.go:57-62`）。

媒体主链路：`extractModelFromRequest` → JSON 用 `extractModelFromJSON`（同样 `readLimitedRequestBody`），multipart 用 `extractModelFromMultipart`（先 `limitRequestBody` 再 `ParseMultipartForm(32<<20)`）（`internal/relay/media_relay.go:544-589`）；multipart 的完整 body 在真正转发时由 `buildMultipartForwardBody` 从 `c.Request.MultipartForm` 重建（`media_relay.go:792`、`:847`）。

### 2.3 inbound adapter 解析成 `InternalLLMRequest`

`parseRequest`（`relay.go:600-631`）：

1. 读 body；
2. `inbound.Get(inboundType)` 取 adapter 工厂（`internal/transformer/inbound/register.go:22-33`，注册 `ChatInbound` / `ResponseInbound` / `EmbeddingInbound` / `MessagesInbound`）；
3. `inAdapter.TransformRequest(ctx, body)` 转成内部 DTO；
4. 把原始 query、原始路径写入 `internalRequest.Query` / `RawPath`（`relay.go:619-621`）；
5. `internalRequest.Validate()`（`relay.go:623-626`）；
6. `populateRelayRequestSessionFields`（`relay.go:628`）。

各 inbound 的 `TransformRequest`：
- OpenAI Chat：`transformer.Unmarshal` 到一个 `InternalLLMRequest`，并写 `RawAPIFormat = APIFormatOpenAIChatCompletion`（`internal/transformer/inbound/openai/chat.go:17-24`）。
- Anthropic Messages：先解成 `MessageRequest`（`MaxTokens<1` 兜底为 1），再手工转 `InternalLLMRequest`，`RawAPIFormat = APIFormatAnthropicMessage`（`internal/transformer/inbound/anthropic/messages.go:51-68`），并顺手累计输入 token 估算（如 `messages.go:86`、`:99`、`:123`）。
- OpenAI Responses / Embedding：分别在 `inbound/openai/response.go:...`、`embedding.go:...`（按同一 `model.Inbound` 接口实现，`internal/transformer/model/interface.go:8-22`）。

`model.Inbound` / `model.Outbound` 接口定义见 `internal/transformer/model/interface.go:8-33`，文件末尾的注释直接给出了标准四步/五步转换流程（`interface.go:35-52`）。

### 2.4 `Validate()` 与请求会话字段

`InternalLLMRequest.Validate()`（`internal/transformer/model/model.go:269-304`）只做**结构合法性**校验：

- `Model` 不能为空；否则 `"model is required"`；
- `EmbeddingInput != nil` 视为 embedding 请求，`len(Messages) > 0` 视为 chat 请求；两者同时存在报错，都不存在报错；
- embedding 要求 `input` 非空；chat 要求 `messages` 非空；
- chat 请求还会 `fillMissingToolCallIDsFromToolMessages()` 补齐缺失的 tool call id（`model.go:342-391`）。

`populateRelayRequestSessionFields`（`internal/relay/request_session.go:12-49`）额外注入**非协议字段**：

- `req.RawRequest`：原始 body 副本（后续流式会话哈希、插件重编码、raw 透传都依赖它）；
- `req.ConversationID`：优先取 `X-Conversation-ID` 头，其次 query `conversation_id`，再次 body 的 `conversation_id`；最后经 `sanitizeRelayConversationID` 过滤控制字符并限制 128 runes（`request_session.go:46-68`）。
- `req.ResumeFromEventID`：`Last-Event-ID` 头 / `last_event_id` / `resume_from_sequence`（`request_session.go:19-44`）。

这些字段决定后续是否启用“可续传流会话”（`shouldUseRelayStreamSession`，`request_session.go:114-119`）。

### 2.5 媒体端点入站差异

`MediaHandler`（`media_relay.go:71-434`）**不使用 inbound adapter**：

- `getMediaEndpointConfig` 从 `mediaEndpointConfigs` 取该端点的 `UpstreamPath / BinaryResponse / MultipartInput / AudioFormat`（`internal/relay/media_type.go:31-72`）；
- 用 `extractModelFromRequest` 只拿 `model` 和原始 body（`media_relay.go:544-589`）；
- 之后用 `mediaEndpointTypeToGroupEndpointType` 把媒体端点映射成 group 端点类型（`media_relay.go:41-66`），再走 `resolveRelayModel`。

### 2.6 入站限流说明

- API Key 的 RPM/TPM **不是路由级中间件**，而是在 `relay.Handler` 内部 `reserveRelayQuota` 里完成（`internal/relay/quota.go:15-40`，见 4.1）；媒体路径同样（`media_relay.go:138-145`）。
- 真正挂在路由上的限流中间件只有一个：`LoginRateLimit`（`internal/server/middleware/rate_limit.go:43-57`），只用于登录/WebAuthn/bootstrap 等少数路由（`handlers/user.go:25`、`handlers/webauthn.go:30,35`、`handlers/bootstrap.go:26`）。

---

## 3. 中间件链（顺序表）

### 3.1 全局中间件（`internal/server/server.go:38-90`，按 `r.Use` 顺序执行）

| # | 中间件 | 注册位置 | 关键行为 |
| --- | --- | --- | --- |
| 0（前置配置） | `setTrustedProxies` | `server.go:43`、`:125-147` | 决定 `c.ClientIP()` 是否信任 XFF；默认不信任任何代理 |
| 1 | `gin.CustomRecovery`（panic 恢复） | `server.go:44-71` | 捕获 panic，写 `ErrorLog`（panic+堆栈，用 `context.Background()+5s` 避免客户端断连导致写库失败），返回 500，`Abort` |
| 2（仅 debug） | `middleware.Logger()` | `server.go:73-75` | `gin.Logger()`（`middleware/logger.go:5-7`） |
| 3 | `middleware.SecurityHeaders()` | `server.go:76` | 注入 `X-Content-Type-Options`、`X-Frame-Options`、CSP 等（`middleware/security.go:11-21`） |
| 4 | `middleware.Cors()` | `server.go:77` | 按设置里的白名单动态构建 CORS；`"*"` 时禁用凭据（`middleware/cors.go:49-92`） |
| 5 | `middleware.AuditManagementWrite()` | `server.go:78` | 仅对 `/api/v1/` 的写方法且命中白名单路由记录审计日志；`/v1` 推理请求**不**审计（`middleware/audit.go:201-262`） |
| 6 | `middleware.StaticLocal/Embed` | `server.go:79-86` | 仅 GET/HEAD 且路径**不以** `/api`、`/v1`、`/node/` 开头才处理静态文件（`middleware/static.go:22-79`，跳过判断在 `:32-35`） |

> 注：全局中间件在 `router.RegisterAll` **之前** `Use`，所以对 `/v1/*` 同样生效；只是 Static 与 Audit 因路径判断自动放行。

### 3.2 路由组中间件

| 组 | 中间件顺序 | 位置 |
| --- | --- | --- |
| `/v1`（LLM） | `APIKeyAuth` → `DevMockPublicSuccess` → `RequireJSON` | `handlers/relay.go:15-18` |
| `/v1`（媒体 JSON） | `APIKeyAuth` → `DevMockPublicSuccess` → `RequireJSON` | `handlers/media.go:14-17` |
| `/v1`（媒体 multipart） | `APIKeyAuth` → `DevMockPublicSuccess` | `handlers/media.go:48-50` |
| `/v1`（models） | `APIKeyAuth` | `handlers/model.go:132-137` |
| `/node/v1` | 无 | `handlers/node.go:24` |
| `/api/v1/*`（管理） | `Auth` → `RequirePermission(...)` →（写路由）`RequireJSON` | 例：`handlers/route.go:20-23`、`handlers/node.go:35-41` |

### 3.3 `APIKeyAuth` 具体校验什么、注入哪些 context 字段

`internal/server/middleware/auth.go:57-128`：

1. 取 Key：优先 `x-api-key`（`requestType="anthropic"`），否则 `Authorization: Bearer <key>`（`requestType="openai"`）（`auth.go:62-68`）；
2. 无 Key → 401；Key 前缀必须是 `sk-<APP_NAME>-`（`auth.go:76-80`）；
3. `ak.GetByKey` 查库，失败 401；`Enabled=false` 401；`ExpireAt>0 && 已过期` 401（`auth.go:81-96`）；
4. `stats.APIKeyGet` 后检查 **费用上限** `MaxCost`（`auth.go:97-102`，超限返回 429 `insufficient_quota`）和 **Token 上限** `MaxTokens`（`auth.go:103-111`）；
5. `isIPAllowed(c.ClientIP(), apiKeyObj.AllowedIPs)`：IP/CIDR 白名单，空表示允许全部（`auth.go:112-116`、`:133-162`）；
6. 通过后注入 context（`auth.go:117-125`）：
   - `request_type`（anthropic/openai）
   - `supported_models`
   - `allowed_group_categories`
   - `api_key_id`
   - `api_key_name`
   - `rate_limit_rpm`、`rate_limit_tpm`
   - `per_model_quota_json`
   - `excluded_channels`

这些字段被后续 `relay.Handler` / `reserveRelayQuota` / `resolveRelayModel` / `balancer.NewIterator` 直接读取。

### 3.4 其它中间件要点

- `RequireJSON`（`middleware/validate.go:11-35`）：GET/DELETE/OPTIONS 直接放行；`ContentLength<=0` 放行；否则要求 `Content-Type` 含 `application/json`，不符返回 415。
- `DevMockPublicSuccess`（`middleware/dev_mock.go:29-43`）：仅当 `conf.IsDevMockSuccess()` 为真时短路，读取请求后按路径直接返回 mock 的 OpenAI/Anthropic 响应（含 SSE），`Abort`。
- `Auth`（管理端，`middleware/auth.go:19-55`）：`Authorization: Bearer <JWT>` → `auth.VerifyJWTToken`（HS256、校验 issuer，`internal/server/auth/auth.go:73-88`）→ 回查用户 → 注入 `user_id`、`username`、`user_role`。
- `RequirePermission`（`middleware/rbac.go:12-27`）：按 `user_role` 做权限点判断。

---

## 4. 路由分发

### 4.1 `relay.Handler` 的完整前置流程（`internal/relay/relay.go:194-364`）

| 步骤 | 代码 | 说明 |
| --- | --- | --- |
| 1 | `InflightInc() / defer InflightDec()`（`:196-197`） | 活跃请求计数 + telemetry（`runtime_stats.go:80-88`） |
| 2 | `parseRequest(...)`（`:199`） | 见 2.3；出错时已写 `resp.Error` 并 return |
| 3 | `internalRequest.Model = TrimSpace(...)`（`:203`） | |
| 4 | `applyBeforeRoutePlugin`（`:204-221`） | 插件 `relay.request.before_route` 可改写请求；若改动则重建 inAdapter 并从新 `RawRequest` 重新初始化有状态 adapter |
| 5 | `apiKeyAllowsModel(supported_models, Model)`（`:222-225`） | Key 支持模型精确匹配（`relay.go:165-175`） |
| 6 | `operationCtx, cancel := newRelayOperationContext()`（`:233-234`） | |
| 7 | `resolveRelayModel(operationCtx, endpointType, requestModel)`（`:235-239`） | 见 4.2；未命中 404 |
| 8 | `apiKeyAllowsGroupCategory(...)`（`:240-243`） | Key 的分类白名单 |
| 9 | `condition.Evaluate(group.Condition, ...)`（`:244-247`） | 见 4.3；不匹配/解析失败 → 404 |
| 10 | `reserveRelayQuota(c, requestModel)`（`:249-254`） | RPM/TPM 预留；`defer settleRelayQuota` |
| 11 | 可续传流会话（`:255-288`） | `resolveRelayStreamSessionIdentity` → `acquireRelayStreamSessionForResume`；`created=false` 直接 `serveRelayStreamSession` 重放，不再选路 |
| 12 | `balancer.HasCandidates`（`:290-294`） | 只按“排除渠道”判断是否有候选 |
| 13 | `rewriteConversationRequestByProvider`（`:297`） | openai/deepseek/mimo 剔除非标准 reasoning 字段（`relay.go:1544-1590`） |
| 14 | `NewRelayMetrics(...)`（`:299-301`） | |
| 15 | 构造请求级 `relayRequest`（`:304-320`） | 带 `c / clientCtx / operationCtx / inAdapter / metrics / quotaUsage / group / streamSession / retryCache` |
| 16 | 语义缓存 / singleflight（`:322-360`） | `maybeServeSemanticCacheHit`；命中直接返回；非流式安全请求可 `serveSharedRelay` 复用并发请求结果 |
| 17 | `executeRelay(...)`（`:362`） | 进入选路 + 重试主循环 |

### 4.2 `resolveRelayModel` 与 `GroupGetEnabledMap*`

`resolveRelayModel`（`internal/relay/model_identity.go:28-55`）：

- 记录 `identity.client = requested`；
- `zen/` 前缀走特例（`resolveRequestedUpstreamModel`，`relay.go:38-55`），否则 `modelmapping.ResolveGlobal` 得到映射后的 `route`；
- `group.GroupGetEnabledMapByEndpoint(endpoint, identity.route, ctx)` 找分组（`internal/op/group/group.go:657-678`）：按 `conversationEndpointLookupOrder` 逐端点类型先精确名、再 `MatchRegex`，最后回退 `EndpointTypeAll`；
- 映射命中但分组未命中时，回退用“原始客户端模型名”再查一次（`model_identity.go:38-45`）；
- 最后可应用分组级映射 `modelmapping.ResolveGroup`（`:49-53`）。

`relayModelIdentity.candidate(item)` 决定真正发给上游的模型名（`model_identity.go:57-67`）。

### 4.3 `condition.Evaluate`

`internal/relay/condition/evaluator.go:30-47`：空条件直接 true；否则反序列化为 `[]ConditionRule` 并逐条 AND；任一 `evaluateRule` 为 false 即 false。`routingConditionContext` 把 model、api_key_id/name、请求头、Content-Type、UTC 小时塞进 `condition.RequestContext`（`internal/relay/condition_context.go:11-20`）。**它是“选中 Group 后的过滤”**，不是在多个 Group 间择优。

### 4.4 `balancer.NewIterator` 之后怎么继续

`executeRelay`（`relay.go:1610-2035`）的三层循环结构：

```text
for routeRound := 1..maxRouteRetries            // 路由轮次（relay.go:1645）
  ├─ applyRouteFilterPlugin（插件 relay.route.filter，可剔除候选，:1666-1674）
  ├─ routeIter = balancer.NewIterator(group, apiKeyID, stickyKey, excludedChannels)   // :1675-1676
  └─ for routeIter.Next()                       // 遍历候选（:1678）
       ├─ 跳过：channel 不存在/禁用/模型空/出站类型不支持/embedding 或 chat 能力不符/zen 偏好不符（:1693-1727）
       ├─ req.internalRequest.Model = identity.candidate(item)   // :1729
       └─ for keyRound := 1..maxKeyRetriesPerRoute   // 同渠道换 Key/账号（:1733）
            ├─ 池模式：poolscheduler.SelectAccountLease + pool.CredentialsForRelay（:1754-1790）
            ├─ inline：GetChannelKeyWithCooldown / PrepareCandidateForRetry（:1797-1801）
            ├─ failure hint / SkipCircuitBreak 跳过（不消耗 keyRound，`keyRound--`）（:1815-1827）
            └─ for adapterIndex, attemptType := range attemptTypes   // adapter 回退链（:1834）
                 ├─ ra := &relayAttempt{... firstTokenTimeOutSec, attemptTimeOutSec, tryIndex, pool* ...}
                 ├─ result = ra.attempt()                        // :1859（真正转发）
                 ├─ 成功 break；否则按 shouldTryAdapterFallback 决定是否试下一 adapter（:1860-1872）
                 └─ 失败后按 result.Decision.Scope 决定同渠道换 Key / 换候选 / 终止（:1958-2008）
```

`balancer.NewIterator` 本身（`internal/relay/balancer/iterator.go:51-107`）：按 GroupMode 生成候选顺序（round_robin/weighted 用 `groupRotationCandidates`，其余用 `GetBalancer(mode).Candidates`）→ 剔除 API Key 渠道黑名单 → 一次性渠道稳定前置 → 有效粘性渠道移到首位。**这里只排“渠道+模型”候选，不选 Key。**

`HasCandidates` 只检查排除集合（`internal/relay/balancer/group_rotation.go:21-28`）；`groupRotationCandidates` 负责轮询偏移与权重平滑序列（`group_rotation.go:30-61`）。

---

## 5. 出站链路

### 5.1 `attempt` → `forward`

`relayAttempt.attempt`（`internal/relay/relay.go:398-598`）：

1. 清空/重置 `metrics.FirstTokenTime`、`InternalResponse`、`Stats`；`defer` 里做响应收集、`quotaUsage` 累加、`PhysicalUsage` 累加（`:403-414`）；
2. `iter.StartAttempt(...)` 生成一次尝试记录（`:415`）；
3. `resetInboundAdapter()`（`:378-395`）：同一请求第 2 次及以后的 adapter 尝试要重建 inbound adapter，并从 `RawRequest` 重新初始化；
4. `forward()` 完成真正的出站与响应处理（`:419-422`）；
5. 分支出客户端断连 / 响应关键词拦截 / 节点执行不确定 / 插件失败 / 空输出重试（`:432-481`），其余进入 `ClassifyRelayError` 分类（`:487`）；
6. 成功后 `collectResponse`、`collectAndStoreStreamResponse`、Key 成本记账、`RecordSuccess` / `RecordAutoSuccess/Latency/TTFT` / `RecordKeyAvailability` / `RecordKeySpeed`、`SetStickyModel`（`:502-545`）；失败则 `RecordKeyCooldown` / `RecordKeyAvailability` / `RecordKeyUsage` 并返回带 `Decision` 的结果（`:548-597`）。

`forward`（`relay.go:634-718`）步骤见 5.2–5.6。

### 5.2 请求体如何按目标协议重编码

在 `forward` 内（`relay.go:646-677`）：

1. 若 `channel.Type` 的 `attemptType` 不是 `passthrough`/`raw`，调 `prepareInternalRequestForOutbound(channel, internalRequest, group)`（`relay.go:656`，实现在 `internal/relay/rewrite.go:12-52`）：
   - `rewrite.Resolve(channel.Type, channel.RequestRewrite)` 解析渠道级请求改写配置（`internal/transformer/rewrite/config.go:27-85`）；
   - `rewrite.Apply(...)` 对消息做系统消息合并、文本块拍平、prompt 正则替换、tool 消息字符串化等（`internal/transformer/rewrite/apply.go:10-29`、`:62-103`）；
   - `applyParamOverride(channel, target)`：把渠道 `param_override` JSON 里的 `max_tokens / max_completion_tokens / temperature / top_p` **仅当客户端未设置时**补上（`rewrite.go:56-94`）；
   - `transmodel.WithThinkingMode(target, group.ThinkingMode)` 与 `attachRelayGroupEndpointMetadata(...)` 注入思考模式与端点元数据（`rewrite.go:43-50`、`:96-110`）。
2. `baseURL := channel.GetNormalizedBaseUrl()`；池账号时被 `ra.poolBaseURL` 覆盖（`relay.go:664-667`）；
3. `ra.outAdapter.TransformRequest(ctx, requestForOutbound, baseURL, ra.usedKey.ChannelKey)`（`relay.go:668-677`）。以 OpenAI Chat 出站为例：克隆请求 → `SanitizeRequestForOpenAICompat`（DeepSeek/MiMo reasoning 兼容、reasoning effort 归一、消息清洗）→ developer 角色转 system → 规范化消息 → 流式补 `stream_options.include_usage` → 序列化（`extra_body` 拍平到顶层）→ 新建 HTTP 请求、设 `Content-Type/Accept/Authorization/api-key` → `BuildOpenAIUpstreamURL(baseURL, "/v1/chat/completions")`（`internal/transformer/outbound/openai/chat.go:17-73`、`:92-169`、`:433-458`）。
4. `passthrough` / `raw` 时 **跳过** rewrite 与 param_override，原样转发客户端 body；`raw` 还会保留原始路径（`outbound/register.go:31-32`、`:140-153`，`passthrough.go`）。

出站 adapter 由 `ResolveAttemptTypes` 决定尝试顺序（`internal/transformer/outbound/register.go:140-176`），工厂注册在 `:91-110`（`chat/response/embedding/anthropic/gemini/volcengine/mimo/cloudflare/passthrough/codex/raw`）。

### 5.3 头复制、插件钩子

- `copyHeaders`（`relay.go:964-985`）：从客户端请求头复制，跳过 `hopByHopHeaders`（`type.go:188-216`，含 `authorization`、`x-api-key`、`cookie`、`host`、`content-length`、`x-forwarded-*` 等）→ 叠加渠道 `CustomHeader` → 叠加 `effectiveRewrite.ExtraHeaders`（如 Codex 头）。
- `applyPoolCredentialHeaders`（`relay.go:741-766`）：cookie 池改 `Cookie` 头；openai-oauth 且非 codex 时手动设 `Authorization: Bearer {access_token}` + `chatgpt-account-id`；并按账号 `extra.header_overrides` 叠加自定义头（黑名单见 `:769-779`）。
- 插件钩子调用点（`internal/relay/plugin_bridge.go`）：
  - `relay.request.before_route`：`applyBeforeRoutePlugin`，在选路前、可改请求并重编码（`plugin_bridge.go:23-71`）；
  - `relay.route.filter`：`applyRouteFilterPlugin`，每轮路由前可剔除渠道（`plugin_bridge.go:117-156`，调用在 `relay.go:1666`）；
  - `relay.request.before_upstream`：`applyBeforeUpstreamPlugin`，**在请求头复制完、`sendRequest` 之前**（`plugin_bridge.go:164-208`，调用在 `relay.go:685-687`）；只能改 `x-octopus-*`、`content-type`、`accept` 头；
  - `relay.response.before_send`：`applyBeforeSendPlugin`，**非流式响应在 inAdapter 转完后、`c.Data` 写出前**（`plugin_bridge.go:222-259`，调用在 `relay.go:1488`）。

### 5.4 BaseURL / HTTP client / 代理 / SSRF

`sendRequest`（`relay.go:987-1034`）：

1. **节点渠道**：`channel.NodeID != 0` 时禁止池代理，直接 `helper.ChannelHttpClient(channel).Do(req)`（`relay.go:989-998`）；`ChannelHttpClient` 对 `NodeID != 0` 返回 `nodetransport.Client(nodeID, timeout)`（`internal/helper/channel.go:45-53`）。
2. **普通渠道**：
   - 先 `xurl.AssertSafeRequestWithPin(req)` 做 SSRF 校验并把安全 IP 钉进 context，防 DNS rebinding（`relay.go:999-1007`）；
   - HTTP client 选择：号池账号级代理 `helper.PoolAccountHttpClient(poolProxyConfigID)` 优先，未配置回退 `helper.ChannelHttpClient(channel)`（`relay.go:1012-1025`）；
   - `ChannelHttpClient` 按 `ProxyMode`（direct/system/pool）返回系统代理/自定义代理客户端（`helper/channel.go:40-91`、`resolveChannelProxy` `:114-152`）。

### 5.5 节点隧道（`internal/nodetransport`）

- 建立：`POST /node/v1/connect` → `Accept` 升级 WebSocket，协商 `hello.MaxConcurrency`，注册 `serverSession`（`manager.go:78-133`）。
- 客户端：`manager.Client(nodeID, timeout)` 返回 `http.Client{Transport: &nodeTransport{...}}`（`manager.go:190-195`）。
- 出站：`nodeTransport.RoundTrip`（`manager.go:381-394`）设置超时并 `roundTrip`；`roundTrip` 校验目标必须是 `https://node.octopus.invalid/v1` 且方法/路径在白名单（`GET /v1/models`、`POST /v1/{chat/completions,responses,messages,embeddings}`）（`protocol.go:97-120`），过滤请求头（`protocol.go:121-136`），把请求编码为帧发给节点，按流回传响应。
- `IsExecutionUncertain(err)`（`protocol.go:66-70`）表示“节点是否执行了不确定”，重试会重复执行，因此这类错误被 `ClassifyRelayError` 直接判 `ScopeAbortAll` 且不计渠道失败（`type.go:420-422`）。

### 5.6 媒体单独路径

`MediaHandler`（`media_relay.go:71-434`）不做 transformer：

- 解析 model/body（5.2 之外的 `extractModelFromRequest`）；
- `resolveRelayModel` + 条件 + `reserveRelayQuota` + `HasCandidates`；
- 自己的 `routeRound → NewIterator → keyRound` 循环（`:170-420`），渠道 Key 选择用 `GetChannelKeyWithCooldown` / `GetChannelKeyExcludingWithCooldown`（`:262-266`），池账号用 `mediaPoolAccount`（`media_pool.go:26-50`）；
- `forwardMediaWithPoolHeaders → forwardMediaRequest`（`media_pool.go:52-76`）：
  - `group.AttemptTimeOut` 可派生超时 ctx；流式且 `FirstTokenTimeOut>0` 时用 `mediaFirstByteTimeout` 监控首字节（`media_relay.go:604-619`）；
  - JSON：`replaceModelInJSON` → provider 级 `rewriteImage/Video/AudioSpeech/MusicRequestByProvider` → `buildMediaUpstreamURL`（或 `EndpointProvider=="raw"` 用原始路径）→ `mergeSafeMediaQuery`（过滤敏感 query，`media_query.go:9-63`）→ 建请求 → `copyMediaForwardHeaders` → SSRF 校验 → `helper.ChannelHttpClient` → `httpClient.Do`（`media_relay.go:639-756`）；
  - multipart：`buildMultipartForwardBody` 重建 body，其余类似（`media_relay.go:759-834`）；
  - 节点渠道明确不支持媒体（返回 400，`media_relay.go:651-653`、`:770-772`）。
- 响应：二进制 `handleBinaryResponse`（`io.Copy`）、SSE `handleSSEResponse`（按行刷出）、JSON `handleJSONResponse`（≤2MiB 记录 usage 后写出，大响应再 `io.Copy`）（`media_relay.go:1179-1258`）；所有响应体读取都外包 `wrapMediaResponseBody(..., mediaBodyIdleTimeout)` 做空闲超时（`media_relay.go:736-737`、`:822-823`，默认 600s，`：1017-1034`）。

---

## 6. 响应与收尾

### 6.1 非流式

`handleResponse`（`relay.go:1447-1495`）：

1. `ra.outAdapter.TransformResponse(ctx, response)` 把上游响应转成 `InternalLLMResponse`（OpenAI 错误体则返回 `*model.ResponseError`，`outbound/openai/chat.go:359-371`）；`metrics.SetInternalResponse` 记录 token/成本（`metrics.go:70-99`）；
2. 输出关键词过滤 `applyResponseFilter`，命中则用 200 + `content_filter` 错误体返回并返回 `errResponseFilterBlocked`（`relay.go:1456-1471`）；
3. `applyReasoningExhaustedHeader`（`relay.go:1473`、`:1497-1517`）；
4. 空输出检测 `isEmptyOutputResponse` → `errEmptyOutput` 触发换 Key 重试（`relay.go:1477-1480`、`type.go:154-172`）；
5. `ra.inAdapter.TransformResponse(...)` 把内部响应转回客户端协议（`relay.go:1482-1486`）；
6. `applyBeforeSendPlugin`（`relay.go:1488-1491`）；
7. `ra.c.Data(http.StatusOK, "application/json", inResponse)` 写出；`storeCanonicalCacheResponse` 存语义缓存（`relay.go:1492-1493`）。

### 6.2 流式

`handleStreamResponse`（`relay.go:1036-1391`）：

- 校验上游 `Content-Type` 必须是 `text/event-stream`，否则报非 SSE 错误（`:1055-1058`）；
- 设置 SSE 响应头（`Content-Type: text/event-stream`、`Cache-Control: no-cache`、`Connection: keep-alive`、`X-Accel-Buffering: no`、可选 `X-Conversation-ID`）（`:1060-1067`）；
- 起一个 goroutine 用 `sse.Read` 逐事件读上游，`results` channel 回传（`:1102-1134`）；
- `firstTokenTimer` 控制首 token 超时（`:1136-1146`、触发在 `:1261-1267`）；`heartbeatTicker` 在有输出后发 `: ping`（`:1150-1151`、`:1268-1276`）；有 stream session 时还有 `clientGoneTicker`（`:1155-1161`、`:1237-1260`）；
- `writeStreamData`：有 stream session 时 `AddPayload` 分序号写回，无 session 直接写客户端（`:1163-1190`）；
- 每个上游事件：`transformStreamData`（`outAdapter.TransformStream` → 关键词过滤 → `inAdapter.TransformStream`，`:1424-1445`）→ 记录 `FinishReason` / `StreamFinished`/`[DONE]` → 首 token 时间 → reasoning buffer 策略 → `writeStreamData`（`:1297-1389`）；
- `finalizeStream`（`:1194-1224`）：空输出且 buffer 策略且无可见内容 → `errEmptyOutput`（可重试）；否则 flush reasoning buffer、`inAdapter.TransformStream({Object:"[DONE]"})` 写终止、`streamSession.Finish(nil)`；
- 流中途错误且已写客户端时，`defer` 会 `writeSSEErrorEvent` 并 `streamSession.Finish(retErr)`（`:1042-1053`）。

### 6.3 usage 统计、metrics、日志

- `RelayMetrics`（`internal/relay/metrics.go:35-53`）在 `relay.Handler` 创建（`:301`）；
- `SetInternalResponse`（`metrics.go:70-99`）从 `Usage` 提取 Prompt/Completion token，按 `price.EffectiveLLMPrice` 计算 Input/Output 成本（区分 Anthropic cache 计费）；
- `Save(success, err, attempts)`（`metrics.go:101-181`）：算总时延、直方图桶、FTUT、延迟分位；更新全局/小时/日/API Key 统计；`recordDailyDimensions`（渠道/模型/Key/渠道-模型）；写日志 `saveLog`；
- `saveLog`（`metrics.go:290-408`）：构造 `model.RelayLog`（请求模型、API Key、ClientIP、端点、渠道、实际模型、耗时、BillingWindow、attempts、FTUT、token、成本、reasoning、可选请求/响应大字段）→ `relaylog.RelayLogAdd` → `relaylog.RelayLogAttemptsAdd` 落每次尝试明细（`:399-407`）。`attempts` 会被 `capAttemptsForLog` 截断到 256 条（`metrics.go:281-288`、`:30`）。
- `Save` 调用点：成功（`relay.go:1880`）、客户端断连（`:1947`）、`ScopeNone`/`ScopeAbortAll` 终态（`:1961`、`:1968`）、`default`（`:2005`）、全部耗尽（`:2025`）。
- 媒体：`recordMediaRelayLog`（`media_relay.go:437-...`）用同一 `RelayLog` 模型，usage 从 JSON body 解析（`media_relay.go:1291-1350`）。
- telemetry/runtime：`InflightInc/Dec`、`RecordRequest`（`runtime_stats.go:80-88`、`:122-128`），`metrics.Save` 末端 `telemetry.Global().RecordRequest`（`metrics.go:179`），并 `publishRelayCompleted` 发插件事件（`plugin_bridge.go:261-270`）。

### 6.4 粘性与亲和记录

- 读粘性：`balancer.NewIterator` 里 `GetSticky(apiKeyID, requestModel, stickyTTL)`，命中则把粘性渠道移到候选首位（`iterator.go:82-99`）；`stickyRoutingKey(group, clientModel)` 把 group ID、端点、模型、各 Generation 拼成 key（`model_identity.go:14-19`）。
- 写粘性：成功时 `balancer.SetStickyModel(apiKeyID, stickyRoutingKey(...), actualModel, channelID, keyID)`（`relay.go:539`；媒体在 `media_relay.go:333`）。
- 失败提示/冷却：`recordFailureHint`（`retry_cache.go:190-207`）、`RecordKeyCooldown`（按 `channelID,keyID,model` 维度）、`balancer.RecordKeyAvailability` 等（`relay.go:496-499`、`:1955-1957`）。

---

## 7. 完整链路步骤表（从 TCP 到响应返回）

> 以 `POST /v1/chat/completions` 流式请求、inline Key 渠道为例；括号内为差异点。

| # | 层次 | 动作 / 函数 | 证据 |
| --- | --- | --- | --- |
| 1 | TCP/HTTP | `net.Listen("tcp", addr)`，`http.Server{Handler: gin.Engine}`，设置 `ReadHeaderTimeout/ReadTimeout/IdleTimeout/MaxHeaderBytes`（**不设 WriteTimeout**，因 SSE 需长写） | `server.go:92-113` |
| 2 | gin 全局 | `gin.CustomRecovery` →（debug）`Logger` → `SecurityHeaders` → `Cors` → `AuditManagementWrite` → `Static` | `server.go:44-86` |
| 3 | 路由组 | `APIKeyAuth` 校验并注入 context | `handlers/relay.go:16`、`middleware/auth.go:57-128` |
| 4 | 路由组 | `DevMockPublicSuccess`（仅 dev mock 模式短路） | `middleware/dev_mock.go:29-43` |
| 5 | 路由组 | `RequireJSON` 校验 `Content-Type` | `middleware/validate.go:11-35` |
| 6 | handler | `chat(c)` → `relay.Handler(EndpointTypeChat, InboundTypeOpenAIChat, c)` | `handlers/relay.go:37-39` |
| 7 | relay | `InflightInc` | `relay.go:196` |
| 8 | relay | `parseRequest`：`readLimitedRequestBody`（≤64MiB） | `relay.go:600-606`、`request_body.go:28-43` |
| 9 | relay | `inbound.Get(InboundTypeOpenAIChat).TransformRequest` → `InternalLLMRequest`，写 `RawAPIFormat` | `relay.go:608-617`、`inbound/register.go:22-33`、`inbound/openai/chat.go:17-24` |
| 10 | relay | 写回 `Query`、`RawPath`，`Validate()`，`populateRelayRequestSessionFields` | `relay.go:619-630` |
| 11 | relay | `applyBeforeRoutePlugin`（插件可改请求并重编码） | `relay.go:204-221`、`plugin_bridge.go:23-71` |
| 12 | relay | `apiKeyAllowsModel` | `relay.go:222-225` |
| 13 | relay | `newRelayOperationContext`（独立于 clientCtx） | `relay.go:233-234`、`context.go:38-43` |
| 14 | relay | `resolveRelayModel` → `GroupGetEnabledMapByEndpoint`（精确名→正则→All 回退） | `model_identity.go:28-55`、`op/group/group.go:657-678` |
| 15 | relay | `apiKeyAllowsGroupCategory` + `condition.Evaluate` | `relay.go:240-247`、`condition/evaluator.go:30-47` |
| 16 | relay | `reserveRelayQuota`（RPM/TPM） | `relay.go:249-254`、`quota.go:15-40` |
| 17 | relay | 可续传流会话（有 `ConversationID`）：`acquireRelayStreamSessionForResume`；`created=false` → `serveRelayStreamSession` 重放返回 | `relay.go:255-288`、`stream_session.go:177-227`、`:687-778` |
| 18 | relay | `balancer.HasCandidates` + `rewriteConversationRequestByProvider` | `relay.go:290-297` |
| 19 | relay | `NewRelayMetrics` + 构造 `relayRequest` | `relay.go:299-320`、`metrics.go:55-64` |
| 20 | relay | 语义缓存 / singleflight 短路（可选） | `relay.go:322-360`、`semantic_cache.go:83-132`、`shared_response.go:33-99` |
| 21 | relay | `executeRelay` → routeRound → `applyRouteFilterPlugin` → `balancer.NewIterator` | `relay.go:1610-1676`、`balancer/iterator.go:51-107` |
| 22 | relay | `routeIter.Next()` → `ch.Get(channelID)` → 各种能力/类型检查 → `req.internalRequest.Model = candidate(item)` | `relay.go:1678-1729` |
| 23 | relay | keyRound：池模式 `SelectAccountLease`/`CredentialsForRelay`；inline `GetChannelKeyWithCooldown` / `PrepareCandidateForRetry`；failure hint / 熔断跳过 | `relay.go:1733-1827` |
| 24 | relay | adapter 循环：构造 `relayAttempt`（带 `firstTokenTimeOutSec`、`attemptTimeOutSec`、pool* 字段） | `relay.go:1834-1858` |
| 25 | relay | `attempt()`：`iter.StartAttempt` → `resetInboundAdapter` → `forward()` | `relay.go:398-422` |
| 26 | relay | `prepareInternalRequestForOutbound`：渠道 rewrite → `param_override` → ThinkingMode/端点元数据 | `relay.go:646-661`、`rewrite.go:12-94` |
| 27 | outbound | `outAdapter.TransformRequest(...)` 按目标协议编码并构建 `*http.Request`（`Authorization`/`api-key`、上游 URL） | `relay.go:664-677`、`outbound/openai/chat.go:17-73` |
| 28 | relay | `applyPoolCredentialHeaders` → `copyHeaders`（跳 hop-by-hop）→ `applyBeforeUpstreamPlugin` | `relay.go:680-687`、`:741-766`、`:964-985`、`plugin_bridge.go:164-208` |
| 29 | transport | `sendRequest`：节点渠道 → `nodetransport.Client`；普通渠道 → SSRF `AssertSafeRequestWithPin` → 池代理/渠道代理 HTTP client → `Do` | `relay.go:987-1034`、`helper/channel.go:40-91`、`nodetransport/manager.go:190-195`、`:381-434` |
| 30 | relay | `handleForwardResponse`：非 2xx 读 ≤64KiB 错误体、存前 2KiB 证据并返回 `upstream error: <code>: <body>` | `relay.go:828-853` |
| 31 | relay | 流式：`handleStreamResponse` 设 SSE 头、起 SSE reader、首 token 计时、heartbeat、逐事件 `outAdapter.TransformStream → 过滤 → inAdapter.TransformStream → 写回` | `relay.go:1036-1391`、`:1424-1445` |
| 32 | relay | 非流式：`handleResponse`：`outAdapter.TransformResponse` → 过滤/空输出检测 → `inAdapter.TransformResponse` → `applyBeforeSendPlugin` → `c.Data` | `relay.go:1447-1495` |
| 33 | relay | 流结束：`finalizeStream` 写 `[DONE]`、`streamSession.Finish(nil)` | `relay.go:1194-1224` |
| 34 | relay | 成功记账：`collectResponse`、Key 成本、渠道/模型统计、熔断/Auto/可用度/TPS、`SetStickyModel` | `relay.go:502-545` |
| 35 | relay | `metrics.Save(true, ...)`：更新全局/日/维度统计并写 `RelayLog` + attempts | `relay.go:1880`、`metrics.go:101-181`、`:290-408` |
| 36 | HTTP | gin 将已 flush 的 SSE 数据经 `net/http` 写回客户端 | `handleStreamResponse` 各 `c.Writer.Write/Flush` |

（媒体请求在第 6 步走 `MediaHandler`，跳过 8–10、24–27、31–33 的 transformer/SSE 复刻逻辑，改为直接 `httpClient.Do` + 二进制/SSE/JSON 回写，见 5.6。）

---

## 8. 异常与超时

### 8.1 客户端断连

- **流式**：`clientDone = ra.clientCtx.Done()`（`relay.go:1078`）。无 stream session 时直接 `return errClientDisconnected`（`relay.go:1231-1235`）；有 stream session 时 `markClientDisconnected()` 并继续读上游以支持重连重放（`relay.go:1081-1093`、`:1236`），宽限 30s 后由 `clientGoneTicker` 强制结束（`stream_session.go:71-82`、`relay.go:1237-1260`）。
- **写失败**：`writeStreamData` 写客户端报错即 `markClientDisconnected` 并返回 `errClientDisconnected`（`relay.go:1184-1189`）。
- **失败记账豁免**：客户端断连产生的 `Decision.SkipFailureAccounting = true`（`relay.go:432-440`），`shouldRecordChannelFailure` 因此不记熔断/Auto（`type.go:370-373`、`relay.go:1935-1938`）；`executeRelay` 遇到 `errClientDisconnected` 立即 `metrics.Save(false, ...)` 并返回（`relay.go:1946-1949`）。
- **媒体**：媒体写失败会把 status 置 0；`markClientCancelIfGone(&decision, c.Request.Context(), fwdErr)` 显式豁免（`media_relay.go:303-311`）。

### 8.2 超时层次

| 超时 | 配置 | 作用点 | 行为 |
| --- | --- | --- | --- |
| 服务端读 | `server.read_header_timeout` / `read_timeout` / `idle_timeout` | `httpSrv`（`server.go:96-98`） | 防 Slowloris；**无 WriteTimeout** |
| 整个 relay 操作 | `OCTOPUS_RELAY_UPSTREAM_TIMEOUT_SECONDS` | `newRelayOperationContext`（`context.go:25-43`） | 未设置则无 deadline（仅可 cancel） |
| 单次尝试 | `group.AttemptTimeOut` | `forward` 派生 `context.WithTimeout`（`relay.go:640-644`） | 超时错误被 `isTimeoutError` 判为 `ScopeNextChannel`（`type.go:553-561`） |
| 首 token | `group.FirstTokenTimeOut` | `handleStreamResponse` 的 `firstTokenTimer`（`relay.go:1136-1146`、`:1261-1267`） | 关闭上游 body，返回 `first token timeout`，换渠道 |
| 节点传输 | `nodeTransport.timeout`（默认 5 分钟） | `nodetransport/manager.go:381-394` | 超时/断连可能 `ExecutionUncertain`，不重试 |
| 媒体响应体空闲 | `OCTOPUS_RELAY_MEDIA_BODY_IDLE_TIMEOUT_SECONDS`（默认 600s） | `wrapMediaResponseBody`（`media_relay.go:1052-1177`） | 两次读到字节间超时即失败，避免 body 永久挂起 |
| 日志持久化 | `OCTOPUS_RELAY_PERSISTENCE_TIMEOUT_SECONDS`（默认 10s） | `newRelayPersistenceContext`（`context.go:45-47`） | 写日志/统计独立超时 |

### 8.3 重试循环结构（routeRound → keyRound → adapter）

- `maxKeyRetriesPerRoute = getMaxAttemptsPerCandidate()` = `relay_retry_count + 1`（默认 3→4）（`type.go:57-69`）；
- `maxRouteRetries = getMaxRouteRetries()`（默认 2）（`type.go:71-77`）；
- `maxTotalAttempts = getMaxTotalAttempts()`（默认 64，统计“决策记录总数”而非仅真实转发）（`type.go:87-95`、`relay.go:1621-1631`）；
- `ratelimitCooldown`（默认 300s）（`type.go:79-85`）。
- 每轮结束把 `routeIter.Attempts()` 累加进 `allAttempts`；上一轮完全没有真实转发则快速失败（`relay.go:1660-1664`）；达到总上限 `goto exhausted`（`relay.go:1679-1682`、`:2018-2034`）。
- 早退检查点：`isClientDisconnected(req.clientCtx)` 与 `req.operationCtx.Err()` 在每层循环开头检查（`relay.go:1647-1655`、`:1683-1691`、`:1738-1746`）。

### 8.4 错误分类与转移

`ClassifyRelayError(statusCode, err, written)`（`internal/relay/type.go:419-453`）：

- 节点执行不确定 / 插件失败 → `ScopeAbortAll` 且 `SkipFailureAccounting`（`:420-425`）；
- `err==nil && 2xx` → `ScopeNone` 成功（`:427-434`）；
- `written == true`（已写流式响应）→ `ScopeAbortAll`（`:437-444`）；
- HTTP 码：400→`ScopeNone`；401/403/429→`ScopeSameChannel`；404/500/502/503/504→`ScopeNextChannel`；其余→`ScopeNextChannel`（`classifyHTTPError`，`:456-539`）；
- 非 HTTP：超时/网络→`ScopeNextChannel`（`classifyNonHTTPError`，`:542-580`）。
- 特殊分支：`errEmptyOutput` 固定 `ScopeSameChannel`（换 Key，不记熔断）（`relay.go:473-481`）；`errResponseFilterBlocked` 固定 `ScopeAbortAll` 且不记渠道失败（`relay.go:445-453`）。
- `Scope` 语义见 `RetryScope`（`type.go:308-313`），`ScopeNone` 时 `writeClientTerminalError` 会把上游 4xx 错误体尽量原样回给客户端（`relay.go:901-962`、`:1958-1965`）。

### 8.5 其它边界

- body 上限：JSON/multipart 默认 64MiB，超限 413（`request_body.go:14-62`）；非流式上游响应体上限 50MiB（`transformer/body.go:9-37`）；上游错误体读取上限 64KiB（`type.go:44`、`relay.go:833-844`）；SSE 单事件上限 32MiB（`type.go:23-27`）。
- panic：`gin.CustomRecovery`（`server.go:44-71`）；SSE reader goroutine 另有 `recover`（`relay.go:1107-1113`）。
- 媒体 SSE 单行上限：用 `ReadSlice` + `ErrBufferFull` 防超长行（`media_relay.go:1207-1215`）。
- 流会话内存保护：`trimEventsLocked`（`stream_session.go:501-522`）、`enforceSessionLimitLocked`（`:241-329`）、`Finish` 后可关闭 replay（`:587-614`）。

---

## 9. 证据（文件:行号）

- 引擎与中间件顺序：`internal/server/server.go:38-90`、`:92-113`；`internal/server/router/router.go:103-155`
- LLM 路由注册：`internal/server/handlers/relay.go:14-48`；媒体路由注册：`internal/server/handlers/media.go:12-103`；`/v1/models`：`internal/server/handlers/model.go:132-137`
- 节点连接：`internal/server/handlers/node.go:22-32`、`:167-195`
- `APIKeyAuth`：`internal/server/middleware/auth.go:57-128`、`:133-162`；`Auth`：`:19-55`
- `RequireJSON` / `DevMockPublicSuccess` / `LoginRateLimit` / `SecurityHeaders` / `Cors` / `AuditManagementWrite` / `Static` / `RequirePermission`：`internal/server/middleware/{validate.go:11-35,dev_mock.go:29-43,rate_limit.go:43-57,security.go:11-21,cors.go:49-92,audit.go:201-262,static.go:22-79,rbac.go:12-27}`
- `relay.Handler` 主体：`internal/relay/relay.go:194-364`
- `parseRequest` / 读 body：`internal/relay/relay.go:600-631`；`internal/relay/request_body.go:28-62`；`internal/relay/request_session.go:12-119`
- inbound 注册与接口：`internal/transformer/inbound/register.go:22-33`；`internal/transformer/model/interface.go:8-52`
- inbound 实现：`internal/transformer/inbound/openai/chat.go:17-24`；`internal/transformer/inbound/anthropic/messages.go:51-68`
- `Validate`：`internal/transformer/model/model.go:269-304`
- 模型/分组解析：`internal/relay/model_identity.go:14-67`；`internal/op/group/group.go:657-678`
- 条件路由：`internal/relay/condition/evaluator.go:30-47`；`internal/relay/condition_context.go:11-20`
- 配额：`internal/relay/quota.go:15-47`
- 语义缓存 / singleflight：`internal/relay/semantic_cache.go:83-132`；`internal/relay/retry_cache.go:341-366`；`internal/relay/shared_response.go:33-99`；`internal/relay/cache_fingerprint.go:19-108`
- 流会话：`internal/relay/stream_session.go:177-227`、`:459-549`、`:587-614`、`:687-778`
- 选路循环：`internal/relay/relay.go:1610-2035`；Iterator：`internal/relay/balancer/iterator.go:51-107`、`group_rotation.go:21-61`
- 尝试与转发：`internal/relay/relay.go:398-718`；出站准备：`internal/relay/rewrite.go:12-110`；渠道改写：`internal/transformer/rewrite/{apply.go:10-103,config.go:27-85}`
- 出站 adapter：`internal/transformer/outbound/register.go:91-176`；OpenAI Chat：`internal/transformer/outbound/openai/chat.go:17-73`、`:92-169`、`:345-431`、`:433-458`
- 插件钩子：`internal/relay/plugin_bridge.go:23-71`、`:117-156`、`:164-208`、`:222-259`、`:261-270`
- HTTP client / 代理 / SSRF：`internal/relay/relay.go:987-1034`；`internal/helper/channel.go:30-152`
- 节点隧道：`internal/nodetransport/manager.go:78-133`、`:190-195`、`:350-434`；`internal/nodetransport/protocol.go:66-136`
- 响应处理：`internal/relay/relay.go:828-853`（错误体）、`:1036-1391`（流式）、`:1424-1445`（流转换）、`:1447-1495`（非流式）
- 指标/日志/统计：`internal/relay/metrics.go:35-181`、`:281-408`；`internal/relay/runtime_stats.go:80-88`、`:122-128`
- 错误分类：`internal/relay/type.go:308-380`、`:402-614`
- 媒体链路：`internal/relay/media_relay.go:41-434`、`:544-834`、`:1179-1350`；`internal/relay/media_type.go:31-72`；`internal/relay/media_pool.go:26-76`；`internal/relay/media_query.go:9-63`
- 响应体读取上限：`internal/transformer/body.go:9-37`
- 统一错误响应包：`internal/server/resp/resp.go:27-38`

---

## 10. 未确认项

1. **`c.GetString("request_id")` 的写入点**：`relay.go:685/1488/1666` 读取 `request_id` 作为插件 attempt ID 前缀，但当前 Go 主链路未搜到 `c.Set("request_id", ...)`。可能由插件运行时或外层代理注入，**未确认**（不影响主链路行为，取不到时为空串）。
2. **插件运行时语义**：`plugin.Invoke / plugin.Handles / plugin.Publish` 的内部实现（超时、隔离、错误传播）未展开，本文只确认了调用时机与数据形状。见 `internal/plugin/`（未逐行阅读）。
3. **账号池调度器内部**：`poolscheduler.SelectAccountLease / ReportResult / Compute429Reset / SetRateLimitCooldown` 的选号算法与状态机未展开；本文只确认“池模式由它选账号并把账号 BaseURL/Proxy/Platform/Type 注入 attempt”。
4. **`group.Item` 的生成与缓存刷新**：`GroupGetEnabledMapByEndpoint` 的前置条件（`groupCache` 何时重建、`finalizeMatchedGroup` 如何剔除无效项）只按路由文档引用，未在本文逐行核验。
5. **多个 `NewGroupRouter("/v1")` 的注册顺序**：`relay.go`、`media.go`、`model.go` 各自 `init()` 向全局 `registeredRouters` 追加，最终由 gin 按完整路径匹配，**同路径不同子路径不冲突**；但 Go 包 `init()` 执行顺序依赖导入顺序，未逐包验证。
6. **`/v1/messages` 与 `/v1/responses` 的完整字段映射**：只核验了入口与 `RawAPIFormat` 设置，未逐字段核对 `messages.go`（1049 行）与 `response.go`（~49KB）的全部字段转换。
7. **`helper.ChannelHttpClient` 的底层 client 超时/连接池参数**：在 `internal/client/http.go`，本文未展开（只确认 `ProxyMode` 分支与 `nodeTransport` 特例）。
8. **媒体 provider 专属改写**（image/video/audio/music/`mimo` TTS）的具体字节级改写规则未逐条核对，仅确认调用链存在（`media_relay.go:1351-1553`）。
