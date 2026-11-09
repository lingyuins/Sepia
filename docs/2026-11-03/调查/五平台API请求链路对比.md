# 五平台 API 请求链路对比（octopus / magpie / new-api / metapi / sub2api）

> 对象：`D:\CodeWorkspace\{octopus,magpie,new-api,metapi,sub2api}`
> 口径：**单次推理请求**的链路——入站（协议入口/解析）→ 中间件（鉴权/限流/路由/分发）→ 出站（适配器/请求构造/代理/传输）→ 响应与收尾。
> 只读源码；逐平台细节见同目录 `<项目>-API请求链路调查.md`。

---

## 0. 入口与形态速览

| 平台 | 监听/服务 | 推理入口 | 备注 |
|---|---|---|---|
| octopus | gin 单进程 | `/v1/*`（chat/completions、responses、messages、embeddings、媒体）、`/node/v1/connect` | 管理面 `/api/v1` 与推理面 `/v1` 分离 |
| magpie | 标准库 ServeMux（网关端口，默认 127.0.0.1:3425） | `/v1/chat/completions`、`/v1/responses`(含 WS)、`/v1/messages`、`/v1beta/models/{call...}`、`/api/chat`(Ollama) | `Front()` 外层 lanGuard + 内层 Handler 链 |
| new-api | gin 单进程 | `/v1/*`（chat/completions、responses[经 task-plugin-protocol-router]、messages、models…）、`/v1beta/*`、`/pg` | relay 路由独立注册 |
| metapi | Fastify（Node） | **根路径** `/v1/*` 及别名（`/chat/completions`、`/responses`、`/v1beta/models`、`/gemini/:ver/models`…） | 无 `/proxy` 前缀（代理路由由插件 `proxyRoutes` 注册在根） |
| sub2api | gin 单进程 | `/v1/*`（messages、responses、chat/completions、models、images/videos…）、`/v1beta/*`（Gemini 原生）、根路径别名、`/backend-api/codex`、Antigravity 专组 | 按 Group 平台分派 `h.Gateway.*` / `h.OpenAIGateway.*`；`getGroupPlatform` 决定 |

---

## 1. 入站链路对比

| 平台 | 读 body | 协议解析 | 备注 |
|---|---|---|---|
| octopus | `readLimitedRequestBody`（默认 64MiB，超限 413） | `inbound.Get(type).TransformRequest` → `InternalLLMRequest` → `Validate()` → `populateRelayRequestSessionFields` | 媒体端点走 `media_relay` 绕过 transformer |
| magpie | `requestBody`（512MB / 60s 超时） | 各协议 `handle()`/`gemini()`/`ollama` 直接解析成内部请求 | Responses 另有 WebSocket 分支 |
| new-api | `DecompressRequest`+`BodyStorageCleanup`，按 Content-Type | `getModelRequest` 取 model；`GetAndValidateRequest` 解析 DTO；`GenRelayInfo` | 多协议 adaptor 稍后转换 |
| metapi | Fastify body parser（**硬编码 20MiB**），multipart 用 buffer parser | `routeAliases`/`normalizeModelAlias` + surface 解析 | 入口在根路径，无 `/proxy` |
| sub2api | `middleware.RequestBodyLimit`（按配置） | handler 解析 body/model/stream，`InboundEndpointMiddleware` 归一端点 | 按 Group 平台分派两套 handler |

---

## 2. 中间件链对比（执行顺序）

| 平台 | 中间件顺序（外→内） |
|---|---|
| **octopus** | 全局：`CustomRecovery → (debug)Logger → SecurityHeaders → Cors → AuditManagementWrite → Static`；`/v1` 组：`APIKeyAuth → DevMockPublicSuccess → RequireJSON`；限流在 Handler 内 `reserveRelayQuota`（不在路由中间件） |
| **magpie** | `Front = socketEntry(dumpFirst(lanGuard(Handler)))`；`lanGuard`：允许IP→pageport→WSL→DNS rebinding→他机 key→preflight→identifyCaller；`Handler`：`counted → headerLogged → holdCatalog → corsGuard → callerGuard → readOnlyGuard → withCaller → keyLimited → routeMux` |
| **new-api** | 全局：`PanicRecover → RequestId → Version → I18n → Gin Logger`；relay 侧 `CORS → Decompress → BodyStorageCleanup → Stats`；`/v1` 组：`RouteTag → SystemPerformanceCheck → TokenAuth → ModelRequestRateLimit → Distribute`（`GlobalAPIRateLimit/AccessTokenAudit` 不在 relay 链） |
| **metapi** | 全局 `CORS`；`/api/*` 管理鉴权；代理：`proxyAuthMiddleware`（Bearer/x-api-key/x-goog-api-key/?key）→ body 解析 → surface handler → 站点并发池/渠道租约；**限流不在代理链**（仅 `/api/tokens`、`/api/oauth`） |
| **sub2api** | 全局：`Recovery → RequestLogger → SessionBindingContext → Logger → CORS → SecurityHeaders → ServerTiming`；网关 `/v1`：`bodyLimit → clientRequestID → opsErrorLogger → endpointNorm → apiKeyAuth → groupModelAllowlist → compositeTarget → requireGroupAnthropic`（再按 Group 平台分派 `Gateway.*`/`OpenAIGateway.*`） |

**关键差异**：octopus/new-api/sub2api 用 Gin 中间件体系；metapi 用 Fastify hook；magpie 是“两层自研守卫”（lanGuard + 内层链）且**网关中间件还有一层 JS 钩子**（`gwmw`，见下）。鉴权：octopus/magpie/sub2api 用 API Key/网关 key（magpie 还有本机信任+只读 key）；new-api 用 Token；metapi 用下游 API Key。

### 2b. 网关内的“JS/插件中间件”层
- **magpie**：`serveAgent` 在选路前跑 `gwmw` 的 `onRequest`，候选循环里 `onProvider`/`onAccount`，流上 `onEvent`/`onResponse`（moejs，250ms/50ms，fail-open）——**请求/回复可被插件改写**。
- **new-api**：`Distribute` 之前有 `RouteTaskPluginEndpoint/PrepareTaskPluginEndpoint`（在 task-plugin-protocol 路由上）；插件协议接管 OpenAI Responses/视频/图片等端点。
- **octopus**：插件钩子 `before_route`/`route.filter`/`before_upstream`/`response.before_send`（WASM，见出站）。
- **metapi/sub2api**：无通用请求中间件插件（sub2api 插件仅出站传输）。

---

## 3. 路由分发对比（请求如何进入选路）

| 平台 | 分发函数 | 说明 |
|---|---|---|
| octopus | `relay.Handler` → `resolveRelayModel`（模型映射）→ `GroupGetEnabledMap` → `condition.Evaluate` → `balancer.NewIterator` → 选 Channel/Key/Pool | 三层策略：GroupMode / KeySelectionStrategy / AccountPool.Strategy |
| magpie | `serve` → `provider.Resolve`/GroupFor → `plan`/`planGroup` → `weighRouted` → `affine` → 候选循环 | 候选 + Seat 策略（可插拔），亲和调整 |
| new-api | `Distribute` → `service.SelectChannelForRequest` → `SetupContextForSelectedChannel`（首次复用 context 渠道，重试重选） | Ability 优先级层 + 权重；渠道亲和 |
| metapi | `selectProxyChannelForAttempt` → `tokenRouter.selectChannel/selectNextChannel` → 过滤（状态/冷却/模型可用性）→ 按策略选 `SelectedChannel` | routeChannels 的 priority/weight；round_robin/stable_first/weighted |
| sub2api | handler → `GatewayService.SelectAccount*` / OpenAI 调度器（模型路由→粘性→优先级/负载/LRU→并发槽→排队） | 平台分组（anthropic/gemini/openai/composite…）；Redis 调度快照 |

---

## 4. 出站链路对比

| 平台 | 协议转换/适配 | 请求构造 | 传输/代理 | 特殊路径 |
|---|---|---|---|---|
| **octopus** | `prepareInternalRequestForOutbound`（重写+param_override+ThinkingMode）→ `outAdapter.TransformRequest` | BaseURL + 头复制（跳 hop-by-hop）+ 池凭证头 | `sendRequest`：`AssertSafeRequestWithPin`(SSRF) + 渠道/池代理 HTTP client；节点渠道 `nodetransport.Client` | 媒体 `media_relay` 直转发；插件 `before_upstream` |
| **magpie** | `passthrough`（原样中继）或 `translate`（跨协议）→ `forward/forwardOnce` | `p.Do`；provider 插件走 `pluginFetch`→Bun `host.js` | `upstream_transport.go`（HTTP2 ping 20s/10s、大 body 例外）、`netproxy`、`Provider.Via` 代理 | Codex/Claude 专属后端、Responses WS |
| **new-api** | `GetAdaptor(ApiType)` → Helper：`InitChannelMeta → ModelMappedHelper → Convert*Request` | `DoApiRequest` 拼 URL/头 + `ApplyHeaderOverride`；`doRequest` 选 client/代理（`GetHttpClientWithProxySettings`） | 按 Channel 类型/代理 | RelayKit 仅“客户端格式≠渠道原生格式”时转换 |
| **metapi** | `transformers`（openai/anthropic/gemini/canonical）+ `buildUpstreamEndpointRequest`（payload rules + provider profile headers） | 上游请求构造 + 端点派生 | `dispatchRuntimeRequest`→executors→`siteProxy`（undici HTTP 或 Codex WS）；站点/账号代理 | `executeEndpointFlow` 首字节超时+端点降级 |
| **sub2api** | `gateway_forward`/`openai_gateway_forward`（passthrough/compat 转换） | 按 Account 选择协议/凭证/代理/TLS | HTTP/WS/TLS 路径；OpenAI OAuth 命中插件经 `openai_plugin_transport`→`PluginManager.RoundTripOpenAIOAuth`(gRPC 子进程) | 多平台适配（grok/bedrock/vertex/gemini…） |

---

## 5. 响应与收尾对比

| 平台 | 流式处理 | usage/计费 | 观测/落盘 |
|---|---|---|---|
| octopus | `handleStreamResponse`：SSE 头/首 Token 计时/heartbeat/outAdapter.TransformStream→过滤→inAdapter→写回/[DONE] | `RelayMetrics.Save` 算 token/成本、按维度统计 | `RelayLog` + attempts 明细 |
| magpie | SSE sniffer/tidy 链或 decoder/encoder/render 回写 | `usage.Record`/`appendUsage`、`decodeOf` | `trace` + `route_events` + `noteSticks` |
| new-api | `DoResponse`（adaptor）输出统一 usage | `PostTextConsumeQuota` → `SettleBilling`（预扣/补差/退款） | 消费日志 + 渠道统计 |
| metapi | surface 流处理 + `streamTermination` | `usageHooks`/`proxyUsageParser`/`proxyBilling`；self-log 回退 | `proxyLogStore` + `recordSuccess` |
| sub2api | handler 流/非流解析 | `gateway_usage_billing`（用户 ActualCost / 账号 TotalCost×rate） | `usage_log` + `ops` 日志 |

---

## 6. 异常与重试对比

| 平台 | 重试结构 | 错误分类/冷却 | 断连/超时 |
|---|---|---|---|
| octopus | `routeRound → keyRound → adapter` 三层 | `ClassifyRelayError`：None/SameChannel/NextChannel/AbortAll；熔断+Key冷却 | 首Token/Attempt/operation/节点/媒体空闲 六层超时；断连流式继续+30s |
| magpie | 候选循环 + `Rest` + `Patience` | 按原因 Rest 阶梯；HTTP2 静默检测 | FirstToken 换成员；499；流中途补错误事件 |
| new-api | `for retry <= RetryTimes`（默认 **0**） | `DecideRelayRetry`；`AutomaticDisableChannelEnabled` 默认 **false**；状态码区间；504/524 不重试 | 渠道/适配器超时；`SkipRetry` |
| metapi | surface 重试循环（通道级最多 3 次 `proxyMaxChannelAttempts`）+ 端点降级 | `proxyRetryPolicy`/`proxyFailureJudge`/`recordFailure` 冷却（round_robin 阈值3/指数退避） | `firstByteTimeout` 首字节超时；流中断 |
| sub2api | `failover_loop`（同账号重试→排除→换号；已写流停止） | `ratelimit_service`/`overload_cooldown`/`error_passthrough`/模型级隔离 | 并发槽 WaitPlan；429/529 冷却 |

---

## 7. 并排链路（一条 `POST /v1/chat/completions`）

| 阶段 | octopus | magpie | new-api | metapi | sub2api |
|---|---|---|---|---|---|
| 身份/入口 | APIKeyAuth | lanGuard+keyLimited | TokenAuth | proxyAuthMiddleware | apiKeyAuth |
| 解析 | inbound adapter→InternalLLMRequest | handle(from)→内部请求 | getModelRequest+DTO | surface 解析 | handler 解析 |
| 选路 | GroupMode→Iterator→Channel/Key | plan→weighRouted→affine | SelectChannelForRequest→Channel | tokenRouter.selectChannel | SelectAccount→Account |
| 出站 | outAdapter.TransformRequest→sendRequest | passthrough/translate→forward→p.Do | GetAdaptor→Convert→DoApiRequest→doRequest | transformers→buildUpstream→executor→siteProxy | forward→(可选插件 gRPC)→upstream |
| 响应 | TransformStream→metrics | SSE 链→usage.Record→trace | DoResponse→PostTextConsumeQuota→SettleBilling | usageHooks→billing→proxyLog | usage→billing→usage_log |
| 失败 | scope 分类→换 key/渠道/终止 | Rest/FirstToken/Patience | DecideRelayRetry | 冷却+重试+端点降级 | failover_loop |

---

## 8. 关键差异总结（答题版）

1. **入口形态**：octopus/new-api/sub2api 是 Gin 单进程；magpie 是标准库 ServeMux + 两层自研守卫；metapi 是 Fastify 且代理在**根路径**（无 `/proxy`）。
2. **中间件风格**：Gin 中间件（octopus/new-api/sub2api）、Fastify hook（metapi）、自研函数链 + JS 插件中间件（magpie）。
3. **可改写请求/回复的插件层**：magpie（JS `gwmw` 全钩子）与 octopus（WASM 事件钩子）最强；new-api 插件主要在任务/媒体协议端点；metapi/sub2api 无请求级插件（sub2api 插件仅出站传输）。
4. **协议转换**：octopus（inbound/outbound adapter 双向）、new-api（adaptor + RelayKit）、magpie（passthrough vs translate）、metapi（transformers + upstreamRequestBuilder）、sub2api（per-platform forward/passthrough）。
5. **传输**：octopus 有 SSRF+节点隧道；magpie 有 HTTP2 健康检查+插件 Bun fetch；new-api 有代理 client；metapi 用 undici/WS + 站点代理；sub2api 有 HTTP/WS/TLS + gRPC 插件传输。
6. **计费收尾**：new-api 的“预扣→补差/退款”最完整；sub2api 订阅配额；octopus/magpie/metapi 各有 metrics/usage 记账。

---

## 9. 证据
见各项目 `-API请求链路调查.md` 的“证据（文件:行号）”与“未确认项”章节。
