# metapi 单次 API 请求链路调查

> 调查对象：`D:\CodeWorkspace\metapi`（TypeScript / Fastify 的「中转站的中转站」网关）
> 范围：**一个代理请求从入站到响应返回的完整生命周期**（入站 → 中间件 → 路由分发 → 出站 → 响应收尾/异常）。
> 说明：本文只读源码，不做修改；所有结论给 `文件:行号`；无法从源码确认的写「未确认」。
> 忽略 `.git / node_modules / dist / output / tmp`。

---

## 0. 概览

metapi 对外暴露的是一个 **OpenAI / Anthropic / Gemini 兼容的代理网关**：客户端用下游 API Key 调用 `POST /v1/chat/completions`、`/v1/messages`、`/v1/responses`、`/v1beta/models/*` 等入口，网关把请求**解析为统一内部结构**，再按「路由（tokenRoutes）→ 渠道（routeChannels = 账号 + 站点 + 令牌）」选出一条上游通道，**转换成上游站点所需的协议**后发出（HTTP 或 WebSocket），最后把上游响应再转回下游协议、统计用量与计费、落库。

一次请求的主干（函数级）：

```
Fastify(onRequest: proxyAuthMiddleware)
  → routes/proxy/*.ts            路由注册与入口
  → proxy-core/surfaces/*.ts     各协议 surface：解析请求 + 选渠道 + 构造上游请求 + 处理响应（核心编排）
  → proxy-core/channelSelection.ts → services/tokenRouter.ts（selectChannel/selectNextChannel）
  → services/upstreamEndpointRuntime → upstreamRequestBuilder（协议转换、构造上游请求）
  → services/runtimeDispatch → routes/proxy/runtimeExecutor → proxy-core/executors/*
  → proxy-core/orchestration/endpointFlow（fetch + 首字节超时 + 端点降级）
  → services/siteProxy（站点代理 dispatcher）+ undici fetch
  → 响应：proxyStream（流式）/ transformFinalResponse（非流式）
  → recordSurfaceSuccess / failureToolkit（用量、计费、proxyLogStore、tokenRouter.recordSuccess/Failure）
```

**重要更正（相对任务假设）**：
1. **代码里没有 `/proxy` 前缀**。任务所说的「代理入口 `/proxy/*`」实际指 `src/server/routes/proxy/` 这一**路由组**，其真实 HTTP 路径是根路径上的 `/v1/*` 及若干别名（如 `/chat/completions`、`/responses`、`/v1beta/models`、`/gemini/:ver/models`）。见 `src/server/routes/proxy/router.ts:21-32`、`src/server/index.ts:234`。全文以「proxy 路由组」表述。
2. **`rate-limiter-flexible` 只用于 `/api/tokens`、`/api/oauth` 两个管理接口**（`src/server/routes/api/tokens.ts:3,58`、`src/server/routes/api/oauth.ts:2,74`），**不在代理链路上**；代理链路没有全局限流器。另一套自研内存限流 `createRateLimitGuard` 也只挂在部分 `/api/*` 管理路由（见 §2.6）。
3. **`DefaultProxyConductor`（`proxy-core/conductor/`）在当前生产请求路径中没有被实例化**，只有其单元测试引用它（`DefaultProxyConductor.test.ts`）。真实 surface 走的是 `selectProxyChannelForAttempt` + `executeEndpointFlow` 的直连编排（见 §4、§9.4）。conductor 目录更像一层解耦抽象/预留，链路叙述以 surface 实路径为准。

---

## 1. 入站链路

### 1.1 Fastify 实例与全局装配

`src/server/index.ts`：

- `202`：`const app = Fastify(buildFastifyOptions(config));` — 创建实例。
- `204`：`await app.register(cors);` — **全局 CORS**，未传 options，即 `@fastify/cors` 默认行为（反射请求 Origin、允许常见方法与头、处理预检 `OPTIONS`）。**注意**：`/api` 与代理 `/v1` 走的是同一实例的 CORS，因此代理入口也受同一套 CORS 策略。
- `207-211`：全局 `onRequest` 钩子，只对 `request.url.startsWith('/api/') && !isPublicApiRoute(url)` 执行 `authMiddleware`（管理后台鉴权，见 §2.1）。
- `214-231`：注册 `/api/*` 管理路由。
- `234`：`await app.register(proxyRoutes);` — 注册代理路由组（`src/server/routes/proxy/router.ts`）。
- `238-261`：生产环境静态前端 + SPA 兜底；`256` 说明 `/api/` 与 `/v1/` 不参与 SPA 兜底（返回 404）。
- `295`：`await app.listen({ port, host })`。

Fastify 选项 `src/server/config.ts:177-185`：

```ts
buildFastifyOptions(appConfig) => { logger: true, trustProxy: true, bodyLimit: appConfig.requestBodyLimit }
```

- `trustProxy: true`：信任 `X-Forwarded-For`，配合 `extractClientIp` 取真实客户端 IP（`middleware/auth.ts:82-92`）。
- `bodyLimit` 来自 `config.requestBodyLimit`；`config.ts:120` `requestBodyLimit: DEFAULT_REQUEST_BODY_LIMIT`，`config.ts:5` `DEFAULT_REQUEST_BODY_LIMIT = 20 * 1024 * 1024`（**20 MiB，硬编码，不读环境变量/运行设置**）。全库没有 `request_body_limit` 之类的可配置项（grep 无结果）。

### 1.2 代理路由组的注册与鉴权钩子

`src/server/routes/proxy/router.ts`：

- `17-19`：在 `proxyRoutes` 插件内 `app.addHook('onRequest', proxyAuthMiddleware)`。因为 Fastify 的封装语义，这个钩子作用于该插件内后续注册的所有代理路由 —— 即**所有代理入口共享同一下游鉴权**。
- `21-32`：逐个 `app.register(...)` 注册各协议路由（chat/claudeMessages/completions/responses/models/embeddings/search/files/rerank/images/videos/gemini）。

### 1.3 各入口路由与其实现位置

| 入口（真实路径） | 注册位置 | 实际处理函数 |
| --- | --- | --- |
| `POST /v1/chat/completions`、`POST /chat/completions` | `routes/proxy/chat.ts:8-11` | `handleChatSurfaceRequest(req,reply,'openai')`（`proxy-core/surfaces/chatSurface.ts:288`） |
| `POST /v1/messages` | `chat.ts:15-16` | `handleChatSurfaceRequest(...,'claude')` |
| `POST /v1/messages/count_tokens` | `chat.ts:17-18` | `handleClaudeCountTokensSurfaceRequest`（`chatSurface.ts:1350`） |
| `POST /v1/completions` | `completions.ts:32` | 内联实现（legacy completions，直接 fetch 透传） |
| `POST /v1/responses`、`/v1/responses/compact`、别名 `/responses`、`/responses/*` | `responses.ts:28-67` | `handleOpenAiResponsesSurfaceRequest`（`openAiResponsesSurface.ts:255`）；GET 返回 426 要求 WebSocket |
| WebSocket `/v1/responses`（升级） | `responses.ts:26`（`ensureResponsesWebsocketTransport(app)`） | `responsesWebsocket.ts:805-834`（`app.server.on('upgrade', ...)`） |
| `GET /v1/models` | `models.ts:8-20` | `listModelsSurface`（`proxy-core/surfaces/modelsSurface.ts`） |
| `POST /v1/embeddings` | `embeddings.ts:30-31` | 内联实现（直接 fetch 透传） |
| `POST /v1/images/generations`、`/v1/images/edits`、`/v1/images/variations` | `images.ts:32,231,451` | 内联实现；`edits` 走 multipart |
| `POST /v1/videos`、`GET/DELETE /v1/videos/:id` | `videos.ts:40,200,233` | 内联实现；`POST` 支持 multipart |
| `POST /v1/search` | `search.ts:30-31` | 内联实现（`__search` 伪模型） |
| `POST /v1/rerank` | `rerank.ts:6-18` | 延迟 `import` → `handleRerankSurfaceRequest`（`rerankSurface.ts:31`） |
| `POST /v1/files`、`GET /v1/files` | `files.ts:1` → `filesSurface.ts:50,53,91` | 本地文件存储（multipart 上传，不走上游） |
| `GET /v1beta/models`、`/gemini/:ver/models`、`POST /v1beta/models/*`、`/gemini/:ver/models/*`、`/v1internal::generateContent`（Gemini CLI） | `gemini.ts:1` → `geminiSurface.ts:1510-1516` | `geminiProxyRoute` 内部 `handleGenerateContent`（`geminiSurface.ts:497`） |

> 结论：**协议面（surface）分两类**——① `chat / responses / gemini / rerank` 是「结构化编排」（解析→选渠道→协议转换→转发→回译）；② `completions / embeddings / images / videos / search / files` 是「直连透传」（按 model 选渠道后，把 body 换成上游 model 直接 fetch 上游同路径），并用 `fetchWithObservedFirstByte` + `siteProxy` 处理代理与首字节超时。

### 1.4 请求体解析与 multipart

- 默认 `application/json` 由 Fastify 内置 JSON 解析器解析为 `request.body`（业务代码直接读 `request.body.model`）。
- 需要 multipart 的路由（images/videos/files）在各自插件注册时调用 `ensureMultipartBufferParser(app)`（`routes/proxy/multipart.ts:13-22`）：给 `multipart/form-data` 注册 `{ parseAs: 'buffer' }` 的 content-type parser（把原始 Buffer 放进 `request.body`），并用 `__metapiMultipartParserRegistered` 做幂等标记。
- `parseMultipartFormData`（`multipart.ts:28-40`）把 Buffer 包成 `Response(Blob)` 后 `response.formData()` 得到 `FormData`；`cloneFormDataWithOverrides`（`multipart.ts:42-71`）在转发前替换字段（例如把 `model` 改成上游真实模型）。
- **body 大小上限统一由 Fastify `bodyLimit`（20 MiB）约束**，multipart 的 buffer parser 也受该上限（`config.ts:5,120,183`）。

### 1.5 请求模型解析（requestedModel / routeAliases / normalizeModelAlias）

1. **下游 `requestedModel` 来自下游协议的解析器**：
   - chat：`chatSurface.ts:302` `downstreamTransformer.transformRequest(request.body)` → `openAiChatInbound.parse`（`transformers/openai/chat/inbound.ts:7`）→ `parseDownstreamChatRequest(body,'openai')`（`transformers/shared/normalized.ts`）取 `parsed.value.requestedModel`（即 `body.model`）。claude 走 `anthropicMessagesTransformer.transformRequest`。
   - responses：`openAiResponsesTransformer.transformRequest(body)`（`openAiResponsesSurface.ts:277`），`requestedModel = requestEnvelope.model`（`:284`）。
   - gemini：`geminiGenerateContentTransformer.parseProxyRequestPath`（`geminiSurface.ts:519`）/ cli 分支从 `body.model` 取（`:507-511`）。
2. **`normalizeModelAlias`（`services/tokenRouter.ts:1411-1419`）**：把模型名 trim + 小写，并**截掉最后一个 `/` 之前的前缀**（`vendor/model` → `model`）。用于候选通道的「来源模型等价」判断（`channelSupportsRequestedModel` `:1427-1434`、`isModelAliasEquivalent` `:1421-1425`）、路由匹配与 stable_first 轮转键（`:3365-3370`）。
3. **`routeAliases` 指 `tokenRoutes.displayName`**：用于（a）Gemini `GET /v1beta/models` 把路由显示名当可暴露模型别名列出（`geminiSurface.ts:224-231`）；（b）选渠道时「按路由显示名命中」则放宽来源模型检查并用通道自身 sourceModel 作为运行时模型（`tokenRouter.ts:2810-2816` 的 `requestedByDisplayName`）。
4. 下游策略校验：`ensureModelAllowedForDownstreamKey(request, reply, requestedModel)`（`routes/proxy/downstreamPolicy.ts:12-31`）→ `isModelAllowedByPolicyOrAllowedRoutes`（`downstreamApiKeyService.ts:327-342`），不通过则 403。

---

## 2. 中间件链（执行顺序与作用）

代理请求实际经过的中间件/钩子顺序如下（Fastify `onRequest` 先于 body 解析，再是 handler）：

| 顺序 | 阶段 | 位置 | 作用 |
| --- | --- | --- | --- |
| 1 | CORS（全局） | `index.ts:204`（`@fastify/cors`） | 跨域头 / `OPTIONS` 预检；默认策略 |
| 2 | 全局 `onRequest`（仅 `/api/`） | `index.ts:207-211` → `authMiddleware`（`auth.ts:109-126`） | 管理后台鉴权：IP 白名单（`adminIpAllowlist`）+ `Authorization: Bearer <AUTH_TOKEN>`。**不作用于代理 `/v1`** |
| 3 | 代理 `onRequest`（proxy 插件内） | `router.ts:17-19` → `proxyAuthMiddleware`（`auth.ts:128-171`） | 下游 Key 鉴权 + 记录请求计数（详见 §2.2） |
| 4 | body 解析 | Fastify 内置 JSON / `multipart.ts:13-22` | 生成 `request.body`（≤20 MiB） |
| 5 | handler（surface） | 各 `routes/proxy/*` → `proxy-core/surfaces/*` | 业务编排入口 |
| 6 | 站点并发/端点池 | `runWithSiteApiEndpointPool`（`siteApiEndpointService.ts:363-427`） | 站点级并发租约 + API 端点池轮换（在 surface 的 try 块内） |
| 7 | 渠道租约 | `acquireSurfaceChannelLease`（`sharedSurface.ts:162-183`→`proxyChannelCoordinator.acquireChannelLease:306`） | 会话维度的单通道并发限制（sticky 会话才占用） |

> 说明：任务提到的「日志/`proxyLogStore`」不是中间件，而是**响应收尾阶段**统一落库（§6）。限流（`rate-limiter-flexible` / `createRateLimitGuard`）**不在代理链路**（§0 更正 2、§2.6）。

### 2.1 `authMiddleware`（管理后台）

`middleware/auth.ts:109-126`：先做 IP 白名单（`isIpAllowed`，支持精确 IP 与 IPv4 CIDR，`:46-107`），再校验 `Authorization: Bearer config.authToken`。仅 `/api/*` 使用。

### 2.2 `proxyAuthMiddleware`（下游 API Key 鉴权）

`middleware/auth.ts:128-171`：

- 取 token 的来源优先级（`:145-147`）：`Authorization: Bearer` > `x-api-key` > `x-goog-api-key` > query `?key=`。
- 无 token → 401（`:149-152`）。
- `authorizeDownstreamToken(token)`（`services/downstreamApiKeyService.ts:418-495`）：
  - 命中托管 Key（`downstreamApiKeys` 表）→ 校验 `enabled / expiresAt / maxCost / maxRequests`，通过则返回 `source:'managed'` 并带 `policy`（由 Key 行转成 `DownstreamRoutingPolicy`，`:470-476`、`toPolicyFromView:376-385`）。
  - 否则若等于全局 `config.proxyToken` → `source:'global'` + 默认空策略（`:479-487`，`getDefaultGlobalPolicy:414-416`）。
  - 否则 403 `Invalid API key`（`:489-494`）。
- 托管 Key 通过后**立即** `consumeManagedKeyRequest(key.id)`（`auth.ts:160-162` → `downstreamApiKeyService.ts:497-505`，原子 `usedRequests+1`）。
- 鉴权上下文写入 `WeakMap`（`proxyAuthContextByRequest`，`:164-170`），供下游 `getProxyAuthContext / getProxyResourceOwner` 读取（`:173-192`）。

### 2.3 `downstreamClientContext`（客户端识别）

`proxy-core/downstreamClientContext.ts:260-286` `detectDownstreamClientContext({downstreamPath, headers, body})`：

- `detectCliProfile(input)`（`cliProfiles/registry.ts`）识别 CLI 客户端（codex/claude-code/gemini-cli 等），产出 `clientKind/sessionId/traceHint`。
- 再叠加应用指纹（`appFingerprintRegistry`：OpenCode、Cherry Studio 等，`:158-200`）与显式自报（`x-openai-client-user-agent`、`OpenClaw` UA，`:236-258`）。
- 产出用于：**sticky session key**（§5.2）、`proxyLogStore` 的 clientFamily/clientAppId 字段（§6）、日志消息拼装（`proxyLogMessage`）。

### 2.4 `downstreamPolicy.ts`（下游策略适配）

`routes/proxy/downstreamPolicy.ts`：

- `getDownstreamRoutingPolicy(request)`：从鉴权上下文取 `policy`，缺失时返回 `EMPTY_DOWNSTREAM_ROUTING_POLICY`（`:6-10`）。
- `ensureModelAllowedForDownstreamKey`：模型不在策略/允许路由内 → 403 `permission_error`（`:12-31`）。
- `recordDownstreamCostUsage(request, estimatedCost)`：响应后把估算成本累加到托管 Key 的 `usedCost`（`:33-37` → `downstreamApiKeyService.ts:507-517`）。
- 策略类型：`services/downstreamPolicyTypes.ts` 的 `DownstreamRoutingPolicy`（`supportedModels / allowedRouteIds / siteWeightMultipliers / excludedSiteIds / excludedCredentialRefs / denyAllWhenEmpty`）。

### 2.5 CORS

`index.ts:204` `await app.register(cors)`，无自定义 options。代理入口与管理接口共用该实例的 CORS 行为（含 `OPTIONS` 预检）。

### 2.6 限流（不在代理链路）

- `rate-limiter-flexible`（`RateLimiterMemory`）：`routes/api/tokens.ts:58`、`routes/api/oauth.ts:74`（仅这两个管理路由文件）。
- 自研 `createRateLimitGuard`（`middleware/requestRateLimit.ts:55-84`，内存 Map、按 IP 计数）：挂载于 `routes/api/accounts.ts:92,98`、`routes/api/auth.ts:9`、`routes/api/monitor.ts:13-31`、`routes/api/oauth.ts:37-117`、`routes/api/stats.ts:85`。
- **代理 `/v1/*` 没有任何限流中间件**（代码事实；若需限流应在网关外层做）。

---

## 3. 路由分发（请求如何进入选渠道）

对结构化 surface（chat/responses/gemini/rerank），选渠道发生在 surface 的**重试循环**里，每次重试选一条新通道：

```
surface while(retryCount <= maxRetries):
  selectSurfaceChannelForAttempt(...)                 // sharedSurface.ts:105-114
    → selectProxyChannelForAttempt(...)               // channelSelection.ts:84-158
      → tokenRouter.selectChannel / selectNextChannel / selectPreferredChannel
  selected = { channel, account, site, tokenValue, tokenName, actualModel }
  excludeChannelIds.push(selected.channel.id)         // 防止重复选中
  构造上游请求 → 发送 → 处理
```

### 3.1 `channelSelection.ts`（selectProxyChannelForAttempt）

`proxy-core/channelSelection.ts:84-158`：

- 若 `forcedChannelId`（测试器强制通道，需 loopback + `x-metapi-tester-request: 1`，见 `:9-69`）→ 固定通道，不自动切换（`:92-101`）。
- `retryCount === 0 && stickySessionKey` → 先尝试 sticky 首选通道（`tokenRouter.selectPreferredChannel`，`:118-140`），失败可刷新路由后重试一次。
- 否则 `retryCount===0` 用 `tokenRouter.selectChannel(...)`，否则 `tokenRouter.selectNextChannel(requestedModel, excludeChannelIds, policy)`（`:142-150`）。
- 首次选不到且未刷新过 → 触发 `routeRefreshWorkflow.refreshModelsAndRebuildRoutes()` 后重选一次（`:106-116,152-155`）。

### 3.2 `tokenRouter.selectChannel` 及选渠道内部

`services/tokenRouter.ts`：

- `selectChannel`（`1835-1842`）：先 `isModelAllowedByDownstreamPolicy` 判模型是否被下游策略允许，再 `findRoute(requestedModel, policy)` 找匹配路由，最后 `selectFromMatch(...)`。
- `selectNextChannel`（`1859-1870`）/ `selectPreferredChannel`（`1872-1892`）：分别带 `excludeChannelIds` 与 `preferredChannelId`。
- `selectFromMatch`（`2803-2935`）核心：
  1. `resolveMappedModel`（`:1464` 起）做 `modelMapping` 映射（请求模型 → 上游模型）；
  2. 过滤候选：`getCandidateEligibilityReasons`（`3284-3340`）——来源模型是否匹配、通道是否禁用、账号/站点状态、下游策略排除（`resolveDownstreamExclusionReason`：站点权重、排除站点、排除凭据）、是否已尝试（`excludeChannelIds`）、令牌是否可用、**是否冷却中**（`cooldownUntil > now`，`:3335-3337`）；
  3. 按路由策略选：`round_robin`（`:2832-2850`）、`stable_first`（`:2852-2896`，含观察态轮转）、默认按 `priority` 分层 + 加权随机（`:2898-2932`）；
  4. `finalizeSelectedCandidateForDispatch` 生成 `SelectedChannel`（channel + account + site + token 值/名称 + `actualModel`）。
- `SelectedChannel` 结构即 conductor `types.ts:1-8` 的 `SelectedChannelLike`：`{ channel{id,routeId}, site, account, tokenName?, tokenValue?, actualModel? }`。

> 注意：`runtimeExecutor.ts`（`routes/proxy/runtimeExecutor.ts`）是**出站执行器分发**，不是选渠道；选渠道在 `channelSelection.ts`/`tokenRouter.ts`。任务链里写的「runtimeExecutor → channelSelection → tokenRouter」顺序，实际是「surface 先选渠道（channelSelection→tokenRouter），再由 runtimeExecutor 负责真正 GET/POST 上游」。`services/runtimeDispatch.ts:1` 只是把 `runtimeExecutor.dispatchRuntimeRequest` 再导出。

---

## 4. 出站链路（协议转换 → 构造上游请求 → 代理选择 → fetch → 流式）

### 4.1 surface 编排主循环（以 chat 为例）

`proxy-core/surfaces/chatSurface.ts:288-1325` 的 `handleChatSurfaceRequest`：

1. 解析：`transformRequest` → `requestedModel / isStream / upstreamBody / claudeOriginalBody`（`:302-313`）。
2. 前置：web-search 仅搜索模拟（`maybeHandleWebSearchOnlySimulation`，`:314-324`）、模型策略校验（`:325`）、强制通道/tester（`:327-330`）、input files 解析（`:331-342`）。
3. 客户端上下文 / sticky key / debug trace / failure toolkit（`:297-379`）。
4. `while (retryCount <= maxRetries)`（`:404`）：
   - 选渠道 `selectSurfaceChannelForAttempt`（`:408-415`），无渠道 → 503（`:417-430`）。
   - `resolveUpstreamEndpointCandidates(...)`（`:450-468`）确定**端点候选顺序**（chat/messages/responses，依据下游格式、站点平台、文件能力、OAUTH 等，见 `upstreamEndpointDerivation.ts`）。
   - 构建上游请求 `buildUpstreamEndpointRequest`（`:532-556`），其中包含协议转换与端口路径选择。
   - `createSurfaceDispatchRequest`（`sharedSurface.ts:308-331`）生成 `dispatchRequest`，内部走 `dispatchRuntimeRequest` + `withSiteRecordProxyRequestInit`（带站点代理 dispatcher）。
   - `endpointStrategy = downstreamTransformer.compatibility.createEndpointStrategy(...)`（`:562-574`），`tryRecover` 处理 401/403 的 OAuth 刷新（`:575-589`）。
   - `executeEndpointFlow({...})`（`:591-673`）对**端点候选**逐个尝试（跨协议降级），并把每次尝试写 debug trace + 端点运行时记忆。
   - `runWithSiteApiEndpointPool`（`:715-726`）+ `acquireSurfaceChannelLease`（`:676-711`）。
   - 成功分支：`isStream` → 流式回写（`:732-1120`）；否则收集完整响应 → 终态转换 → 回写（`:1122-1220`）。
   - 失败分支（catch，`:1221-1324`）：区分站点并发超时 / 上游端点错误 / service_tier 阻断 / 通用执行错误，必要时 `retryCount++` 重试。

### 4.2 协议转换（transformers + upstreamRequestBuilder）

- **下游 → 统一中间表示**：每个协议有 transformer（`transformers/openai/chat`、`anthropic/messages`、`openai/responses`、`gemini/generate-content`、`canonical/*`）。统一契约见 `transformers/contracts.ts:48-78`（`parseRequest / buildProtocolRequest / normalizeFinal / normalizeStreamEvent / serializeFinal / serializeStreamEvent`）与 `transformers/canonical/envelope.ts:50-77`（`CanonicalRequestEnvelope`）。
- **构造上游请求**：`services/upstreamRequestBuilder.ts` 的 `buildUpstreamEndpointRequest`（`404-800`）：
  - `:495-523` `resolveEndpointPath`：按站点平台决定上游路径（openai→`/v1/chat/completions` `/v1/messages` `/v1/responses`；codex→`/responses`；gemini-cli/antigravity→`/v1internal:...`；claude→`/v1/messages`）。
  - `:584-613` 内部 Gemini（gemini-cli/antigravity）：`buildGeminiGenerateContentRequestFromOpenAi` → `providerProfile.prepareRequest`，`runtime.action = streamGenerateContent|generateContent`。
  - `:615-638` 原生 Gemini 且有 assistant tool-call 历史：转 `buildGeminiGenerateContentRequestFromOpenAi`，走 `/v1beta/models/...:generateContent?key=...`，`executor:'gemini-native'`。
  - `:640-707` messages：claude 直传或 `convertOpenAiBodyToAnthropicMessagesBody` + `sanitizeAnthropicMessagesBody`；claude 平台交 `claudeProviderProfile.prepareRequest`。
  - `:709-781` responses：`convertOpenAiBodyToResponsesBodyViaTransformer` + `sanitizeResponsesBodyForProxyViaTransformer` + `normalizeCodexResponsesBodyForProxy`；codex 平台交 `codexProviderProfile.prepareRequest`。
  - `:783-799` 默认 chat：`{...openaiBody, model, stream}` + payload rules。
  - 全程 `applyPayloadRules`（`payloadRules.ts`）做运营级 payload 改写；`serviceTierPolicy` 在 responses 分支由 surface 先行处理（见 4.4）。
- **Provider profiles**（`proxy-core/providers/`）：`registry.ts:7-16` 按 `sitePlatform` 取 profile（codex/claude/gemini-cli/antigravity）；各 profile 负责最终 path/headers/body/runtime（`claudeProviderProfile.ts:9-44`、`codexProviderProfile.ts:9-46`）。headers 统一由 `headerUtils.ts` 构造（含 beta 头、session id、UA 等）。

### 4.3 代理选择（site / account proxy）

`services/siteProxy.ts`：

- `withSiteRecordProxyRequestInit(site, options, accountProxyUrl)`（`:481-500`）：合并站点自定义头（`mergeHeadersWithSiteCustomHeaders`），选择代理 URL = 账号级代理 > 站点 `proxyUrl` > 系统代理（`resolveProxyUrlForSite:474-479`），然后 `withExplicitProxyRequestInit` 把 `dispatcher` 挂到请求上。
- `getDispatcherByProxyUrl`（`:172-193`）：`http/https` 走 undici `ProxyAgent`，`socks*` 走自建 `createSocksDispatcher`；有 3s 缓存。
- `resolveChannelProxyUrl(site, accountExtraConfig)`（`:502-511`）：账号 `extraConfig` 里的代理优先，否则站点代理。
- `withSiteProxyRequestInit`（`:424-455`）是「按请求 URL 反查站点配置」的另一路径（legacy completions/images 等直连路由在用）。

### 4.4 若干出站策略

- **`firstByteTimeout.ts`**：`fetchWithObservedFirstByte`（`:109-170`）用 `AbortController + 定时器 + Promise.race` 实现「首字节超时」；超时返回一个合成的 `408` `Response`（`buildObservedTimeoutResponse:25-33`），并在 `WeakMap` 记录 `firstByteLatencyMs / timedOutBeforeFirstByte`（`:8`），供日志的 `firstByteLatencyMs` 与端点降级判断（`isObservedFirstByteTimeoutResponse:185-187`）。超时来自 `config.proxyFirstByteTimeoutSec`（`config.ts:122`），surface 里换算成 ms 传入（如 `chatSurface.ts:594`）。
- **`serviceTierPolicy.ts`**：`applyOpenAiServiceTierPolicy`（`:124-163`）按 `OPENAI_SERVICE_TIER_RULES` 对 `service_tier` 做 pass/filter/block；block 时 surface 抛 `service-tier blocked` 并直接返回 400（`chatSurface.ts:511-529,1254-1268`）。
- **`webSearchSimulation.ts`**：`maybeHandleWebSearchOnlySimulation`（`:284`）对「只带 web_search 工具」的请求做**本地模拟搜索**（`__search` 伪模型），不转发上游；在 chat(claude) 与 responses 前置调用（`chatSurface.ts:314-324`、`openAiResponsesSurface.ts:295-304`）。

### 4.5 出站执行器（executors）与 fetch

- `services/runtimeDispatch.ts:1` → `routes/proxy/runtimeExecutor.ts:7-24` `dispatchRuntimeRequest`：按 `input.request.runtime?.executor` 分发到 `codexExecutor / claudeExecutor / geminiCliExecutor / antigravityExecutor`，默认 codex。
- `executors/types.ts`：
  - `performFetch`（`:70-83`）：`buildInit(url, request)` → 合并 `signal` → `undici.fetch(url, init)`。
  - `getRuntimeResponseReader`（`:265-309`）：返回解压后的 `ReadableStreamDefaultReader`，专门处理 `zstd/br/gzip/deflate`（`:85-167,228-263`）。
  - `materializeErrorResponse`（`:311-323`）：把错误响应体读成文本、重建 Response（去 content-encoding）。
- 具体执行器：`codexExecutor.ts:4-8`、`claudeExecutor.ts:4-8`（纯 `performFetch`）；`geminiCliExecutor.ts:39-59`（按 model 重写 body/UA，429 换下一个）；`antigravityExecutor.ts:205-285`（固定 Google base URL 列表、重试 no-capacity、流式聚合）。
- **HTTP/WS**：普通走 undici HTTP（上面）。Codex `responses` 的 WebSocket 传输分两条：
  - 下游侧 WS：`routes/proxy/responsesWebsocket.ts:805-834`（`ws` 的 `WebSocketServer` + `app.server.on('upgrade')`），用 `x-metapi-responses-websocket-transport: 1` 标识（`:26`）。
  - 上游侧 WS：`proxy-core/runtime/codexWebsocketRuntime.ts`（握手头/URL 构造 `:10-14`、终止事件判定 `:36-47`、session 响应 id 续接），会话缓存 `codexSessionResponseStore.ts` / `codexWebsocketSessionStore.ts`。

### 4.6 流式处理

- 上游流读取：`getRuntimeResponseReader(upstream)`（`chatSurface.ts:1035`），gemini-cli 再套 `createGeminiCliStreamReader`（`:1036-1038`）。
- 下游 SSE 输出：先 `reply.hijack()` 再 `reply.raw.write`（`chatSurface.ts:735-744` 的 `startSseResponse`），把 `Content-Type: text/event-stream`、`Cache-Control: no-cache, no-transform` 等写死。
- 事件翻译：`openAiChatTransformer.proxyStream.createSession(...)`（`chatSurface.ts:792-807` → `transformers/openai/chat/proxyStream.ts:36-349`）：
  - `run(reader,response)` 用通用 `createProxyStreamLifecycle`（`transformers/shared/protocolLifecycle.ts:20-88`）逐块 `read → pullSseEvents → handleEvent → 结束 onEof`。
  - `handleEventBlock`（`proxyStream.ts:232-296`）把上游 payload 归一化为 `NormalizedStreamEvent`，再 `serializeStreamEvent` 成下游 SSE 行；`[DONE]` 触发 `finalize`。
  - openai 下游有「**缓冲直到出现有意义输出**」策略（`emitLines/emitRaw`，`:119-167`），避免把空 chunk 先吐给客户端；`proxyEmptyContentFailEnabled` 时空内容判失败（`:169-189`）。
- 流结束后：`recordStreamSuccess` → `recordSurfaceSuccess`（`chatSurface.ts:755-777`）；失败 `failureToolkit.recordStreamFailure`（`:828-840`）。**一旦 SSE 已 hijack 并写出字节，不再切通道重试**（注释 `:1096-1100`）。

---

## 5. 响应与收尾

### 5.1 成功收尾 `recordSurfaceSuccess`

`proxy-core/surfaces/sharedSurface.ts:383-524`：

1. 是否含上游 usage：`upstreamUsagePresent`（`:423-427`）。
2. **用量回退**：`resolveProxyUsageWithSelfLogFallback(...)`（`services/proxyUsageFallbackService.ts:538-593`）——若上游没给 usage，会去站点「self-log」里按模型/令牌/时间/时延匹配（`shouldLookupSelfLog:151`、`findBestSelfLogMatch:470`），回填 token 与 `selfLogBillingMeta`、`usageSource`。
3. **计费**：`resolveProxyLogBilling(...)`（`services/proxyBilling.ts:55-92`）→ `estimateProxyCost` + `buildProxyBillingDetails`（`services/modelPricingService.ts`），必要时用配额估算兜底。
4. `tokenRouter.recordSuccess(channel.id, latencyMs, estimatedCost, modelName)`（`sharedSurface.ts:473-478` → `tokenRouter.ts:2448-2528`）：累加 successCount/totalLatency/totalCost、**清冷却**（`cooldownUntil=null, consecutiveFailCount=0, cooldownLevel=0`，`:2507-2527`），并 `recordSiteRuntimeSuccess`。
5. `recordDownstreamCost?.()`（托管 Key `usedCost`）。
6. `logSuccess(...)` → `writeSurfaceProxyLog`（`:239-306`）→ `insertProxyLog`（`services/proxyLogStore.ts:259`）落 `proxyLogs` 表（含 modelRequested/modelActual/状态/tokens/estimatedCost/billingDetails/clientFamily/retryCount 等）。
7. `recordOauthQuotaHeadersSnapshot`（OAuth 配额快照，`:510-517`）。

### 5.2 流式专用

`recordStreamSuccess`（`chatSurface.ts:755-777`）在流结束后调用同一个 `recordSurfaceSuccess`（`isStream:true`，带 `firstByteLatencyMs`）。

### 5.3 sticky 会话绑定

成功后 `bindSurfaceStickyChannel`（`sharedSurface.ts:136-148` → `proxyChannelCoordinator.bindStickyChannel:227-237`），把「会话 key → channelId」记住（TTL 默认 30min，`config.ts:129`），下次同会话请求优先命中同通道（`channelSelection.ts:118-140`）。失败/并发超时会 `clearSurfaceStickyChannel`。

### 5.4 失败收尾与冷却

`createSurfaceFailureToolkit`（`sharedSurface.ts:526-804`）提供 4 个失败处理：

- `handleUpstreamFailure`（`:589-653`）：`tokenRouter.recordFailure`（带 status/errorText/modelName）→ 写失败日志 → OAuth 配额重置提示 → token 过期告警（`isTokenExpiredError`）→ `shouldRetryProxyRequest` 决定 retry/响应 → 全部失败时 `reportProxyAllFailed`。
- `handleDetectedFailure`（`:655-710`）：针对「上游 200 但内容判定失败」（`proxyFailureJudge`）的收尾。
- `handleExecutionError`（`:712-756`）：网络/执行异常。
- `recordStreamFailure`（`:758-802`）：流处理失败（可带 `runtimeFailureStatus=502`）。

`tokenRouter.recordFailure`（`tokenRouter.ts:2655-2788`）——**失败冷却**：
- 支持 OAuth route-unit 成员级冷却（`:2677-2737`）。
- 否则按通道：`shortWindowLimitCooldownUntil`（429 用量限流短窗）优先；`round_robin` 策略下连续失败达阈值 `ROUND_ROBIN_FAILURE_THRESHOLD=3`（`:99`）后升级 `cooldownLevel` 并取对应冷却秒（`:2752-2760`）；其他策略用 `resolveEffectiveFailureCooldownMs(failCount)` 指数退避（`:2761-2765`，退避实现 `:342-344`）。
- 写 `routeChannels.failCount/lastFailAt/consecutiveFailCount/cooldownLevel/cooldownUntil` 并 `patchCachedChannel`，非短窗限流时 `recordSiteRuntimeFailure`（站点熔断计数）。
- 冷却期内的通道在选渠道时被 `getCandidateEligibilityReasons` 以「冷却中」排除（`tokenRouter.ts:3335-3337`）。

### 5.5 冷却清除

`services/routeCooldownService.ts:115-142` `clearRouteCooldown(routeId)`：展开路由对应通道 → `tokenRouter.clearChannelFailureState(channelIds)`（`tokenRouter.ts:2617-2650`，清零 fail/cooldown 并清站点运行时熔断）→ 清路由决策快照。由管理接口 `routes/api/tokens.ts:739` 调用。

### 5.6 首次字节延迟与日志字段

`firstByteLatencyMs` 来自 `getObservedResponseMeta(upstream)?.firstByteLatencyMs`（`chatSurface.ts:730`），经 `writeSurfaceProxyLog` 写入 `proxyLogs.firstByteLatencyMs`（`sharedSurface.ts:288`）。

---

## 6. 完整链路步骤表（从入口到上游到响应）

以一次 `POST /v1/chat/completions`（结构化编排）为例，逐步列出「谁做什么 / 文件:行号」：

| # | 步骤 | 关键代码 |
| --- | --- | --- |
| 1 | Fastify 接收连接；`trustProxy` 取真实 IP；CORS 处理跨域 | `index.ts:202-204`；`config.ts:181-183` |
| 2 | `/api/` 全局鉴权钩子（对 `/v1` 不生效） | `index.ts:207-211` |
| 3 | 进入 proxy 插件；`proxyAuthMiddleware` 下游鉴权（Bearer/x-api-key/x-goog-api-key/`?key`） | `router.ts:17-19`；`auth.ts:128-171` |
| 4 | `authorizeDownstreamToken` 校验 Key（enabled/expiry/cost/requests）→ 写鉴权上下文；托管 Key 请求数 +1 | `downstreamApiKeyService.ts:418-505` |
| 5 | body 解析（JSON 或 multipart，≤20 MiB） | `config.ts:5,120,183`；`multipart.ts:13-40` |
| 6 | 路由命中 `POST /v1/chat/completions` | `chat.ts:8-9` |
| 7 | `handleChatSurfaceRequest` 入口：识别客户端上下文，`transformRequest` 解析出 `requestedModel/isStream/upstreamBody` | `chatSurface.ts:288-313`；`downstreamClientContext.ts:260-286` |
| 8 | 可选：web-search 仅搜索本地模拟 | `chatSurface.ts:314-324`；`webSearchSimulation.ts:284` |
| 9 | 下游策略校验模型（403 拦截）；取 `downstreamPolicy` | `downstreamPolicy.ts:12-31`；`downstreamApiKeyService.ts:327-342` |
| 10 | 解析 input files；构造 sticky session key / debug trace / failure toolkit | `chatSurface.ts:331-379` |
| 11 | **选渠道**：`selectSurfaceChannelForAttempt` → `selectProxyChannelForAttempt` → `tokenRouter.selectChannel/selectNextChannel`；候选过滤（状态/策略/冷却/token）→ 策略选择 → `SelectedChannel` | `sharedSurface.ts:105-114`；`channelSelection.ts:84-158`；`tokenRouter.ts:1835-1842,2803-2935,3284-3340` |
| 12 | 采集渠道租约（会话并发）+ 站点并发/端点池 | `chatSurface.ts:676-711,715`；`sharedSurface.ts:162-183`；`siteApiEndpointService.ts:363-427` |
| 13 | 计算**上游端点候选**（chat/messages/responses） | `chatSurface.ts:450-468`；`upstreamEndpointDerivation.ts` |
| 14 | 构造上游请求：协议转换 + payload rules + headers + runtime.executor | `chatSurface.ts:532-556`；`upstreamRequestBuilder.ts:404-800`；`providers/*` |
| 15 | responses 分支：service_tier 策略（pass/filter/block） | `chatSurface.ts:511-529`；`serviceTierPolicy.ts:124-163` |
| 16 | 生成 `dispatchRequest`（站点代理 dispatcher + 自定义头） | `sharedSurface.ts:308-331`；`siteProxy.ts:481-511` |
| 17 | `executeEndpointFlow`：逐端点尝试；每次 `fetchWithObservedFirstByte`（首字节超时→合成 408）；失败可跨协议降级/恢复 | `chatSurface.ts:591-673`；`endpointFlow.ts:101-262`；`firstByteTimeout.ts:109-170` |
| 18 | 实际出站：`dispatchRuntimeRequest` 选 executor → `performFetch`（undici fetch，带 dispatcher/signal） | `runtimeExecutor.ts:7-24`；`executors/types.ts:70-83`；`codexExecutor.ts`/`antigravityExecutor.ts` 等 |
| 19 | HTTP 走 undici；或 Codex 走上游 WebSocket（`codexWebsocketRuntime`） | `executors/types.ts:79`；`runtime/codexWebsocketRuntime.ts` |
| 20 | 拿到 `upstream` Response；读首字节延迟元数据 | `chatSurface.ts:728-730` |
| 21a | **流式**：hijack + SSE 头；`getRuntimeResponseReader`（解压）→ `proxyStream` 会话逐事件归一化/序列化写回；更新 usage | `chatSurface.ts:732-1120,1035-1057`；`executors/types.ts:265-309`；`transformers/openai/chat/proxyStream.ts:36-349`；`protocolLifecycle.ts:20-88` |
| 21b | **非流式**：读全文（必要时聚合 Responses SSE）→ `transformFinalResponse` → `serializeFinalResponse` | `chatSurface.ts:1122-1220` |
| 22 | 成功收尾：`resolveProxyUsageWithSelfLogFallback` → `resolveProxyLogBilling` → `tokenRouter.recordSuccess`（清冷却） → 托管 Key 计费 → `insertProxyLog` 落库 → OAuth 配额快照 | `sharedSurface.ts:383-524`；`proxyUsageFallbackService.ts:538-593`；`proxyBilling.ts:55-92`；`tokenRouter.ts:2448-2528`；`proxyLogStore.ts:259` |
| 23 | 绑定 sticky 会话；`reply.send` / `reply.raw.write`+`end` | `chatSurface.ts:1215-1220` / `:1115-1119`；`sharedSurface.ts:136-148` |
| 24 | 失败分支：`failureToolkit.handle*` → `tokenRouter.recordFailure`（设冷却/退避）→ 日志 → `shouldRetryProxyRequest` 决定重试或返回错误；重试则回到 #11 | `chatSurface.ts:1221-1324`；`sharedSurface.ts:526-804`；`tokenRouter.ts:2655-2788`；`proxyRetryPolicy.ts:87-96` |

> 直连透传类（completions/embeddings/images/videos/search）链路更短：#1–#10 类似，然后 `selectProxyChannelForAttempt` → `runWithSiteApiEndpointPool` → `fetchWithObservedFirstByte` + `withSiteRecordProxyRequestInit` 直接请求上游同路径 → 读 `response.text()` → `detectProxyFailure`/`parseProxyUsage` → `insertProxyLog`；无协议转换与 transformer 流。

---

## 7. 异常与重试

### 7.1 两层重试循环

1. **通道级重试（surface 外层 while）**：`retryCount` 从 0 到 `maxRetries`，`maxRetries = getProxyMaxChannelRetries()`（`services/proxyChannelRetry.ts:8-10`）= `proxyMaxChannelAttempts - 1`，默认 `PROXY_MAX_CHANNEL_ATTEMPTS=3`（`config.ts:127`）→ **最多 3 次通道尝试**。每次失败后 `excludeChannelIds` 排除已尝试通道再选新通道。`canRetryChannelSelection(retryCount, forcedChannelId)`（`channelSelection.ts:79-82`）在固定通道模式下不允许切换。
2. **端点级降级（endpointFlow 内层 for）**：同一通道下，对 `endpointCandidates`（如 responses→chat，或 messages→chat）逐个尝试（`endpointFlow.ts:115-254`）；`disableCrossProtocolFallback` 或 `shouldAbortRemainingEndpoints` 可提前终止；`shouldDowngrade/onDowngrade` 控制协议降级。

### 7.2 失败判定与是否重试

- `proxyRetryPolicy.ts`：
  - `shouldRetryProxyRequest(status, errText)`（`87-96`）：`>=500 / 408 / 409 / 425 / 429 / 401 / 403` → 重试；模型不支持类文本 → 重试；命中「请求本身非法」模式（invalid request body/validation/malformed/invalid json…）→ 不重试；命中「通道本地错误」模式（unsupported endpoint/invalid api key/rate limit/quota/bad gateway…）→ 重试；`400/404/422` → 不重试。
  - `shouldAbortSameSiteEndpointFallback(status, errText)`（`98-103`）：5xx/408/429 且命中 429/quota/connection reset 等 → 放弃同站点其它端点。
- `proxyFailureJudge.ts` `detectProxyFailure`（`170-203`）：对「上游 200 但内容异常」判定——命中配置错误关键词（`PROXY_ERROR_KEYWORDS`）→ 502；`PROXY_EMPTY_CONTENT_FAIL` 开启且无实际输出且无 completion tokens → 502 `Upstream returned empty content`（内部用 `detectHasUpstreamOutput` 判断是否有真实内容，`:135-168`）。
- `proxyChannelRetry.ts`：`getProxyMaxChannelAttempts/Retries/canRetryProxyChannel`（`3-13`）。

### 7.3 conductor 语义（抽象层，非现网主路径）

`proxy-core/conductor/`：

- `DefaultProxyConductor.execute`（`DefaultProxyConductor.ts:21-127`）：`selectChannel` → 循环 `attempt(ctx)` → 成功 `recordSuccessfulAttempt`；失败 `failureActionOf` → `recordFailedAttempt` → 按 action 分派：`terminal` 终止、`retry_same_channel` 同通道重试、`refresh_auth` 刷新凭据后重试（失败可 failover）、`failover` 换下一个通道（`selectNextChannel`）。
- `retryPolicy.ts:3-21`：`failureActionOf / shouldRetrySameChannel / shouldRefreshAuth / shouldFailover / isTerminalFailure`。
- `streamTermination.ts:1-10` `terminalStreamFailure`：构造 `{action:'terminal', status?, rawErrorText?}`。
- `usageHooks.ts:3-20` `recordSuccessfulAttempt / recordFailedAttempt` → `deps.recordSuccess / recordFailure`。
- **现状**：仅 `DefaultProxyConductor.test.ts` 引用，生产 surface 未使用（见 §0 更正 3）。因此「conductor 的 retryPolicy/usageHooks」目前是**并行/预留实现**，真实链路的成功/失败 Hook 在 `sharedSurface.ts` 的 `recordSurfaceSuccess` / `createSurfaceFailureToolkit`。

### 7.4 流中断/终态

- `protocolLifecycle.ts:20-88`：读循环遇到 `handleEvent` 返回 `true`（如 `[DONE]`/`doneSent`）即 `reader.cancel()` 跳出，最后 `onEof()`（`finalize`）+ `response.end()`。
- `chatSurface.ts:1096-1100` 注释明确：**SSE 已开始向客户端写字节后，不允许再改 HTTP 错误响应或中途切通道**；流级失败只能在流内以 `response.failed/error` 事件表达（`proxyStream.ts:265-272`）。
- 站点并发租约超时不是上游错误，**不降低通道健康度**（gemini 分支显式判断 `!failure.isSiteConcurrencyBusy` 才 `recordFailure`，`geminiSurface.ts:1462-1468`）。

### 7.5 告警

- `reportProxyAllFailed`（`alertService.ts`）：所有通道失败/最终失败时上报（`sharedSurface.ts:638-641,695-698,741-744`）。
- `reportTokenExpired`（`alertService.ts`）：`isTokenExpiredError` 命中时上报（`sharedSurface.ts:624-631`）。

---

## 8. 证据（文件:行号 索引）

**入站与注册**
- `src/server/index.ts:202`（Fastify 实例）、`:204`（CORS）、`:207-211`（/api 鉴权钩子）、`:234`（注册 proxyRoutes）、`:255-260`（SPA 兜底提到 `/v1/`）、`:295`（listen）
- `src/server/config.ts:5`（20MiB）、`:120`（requestBodyLimit）、`:122`（proxyFirstByteTimeoutSec）、`:127`（proxyMaxChannelAttempts=3）、`:129`（sticky TTL）、`:177-185`（buildFastifyOptions）
- `src/server/routes/proxy/router.ts:15-33`
- `src/server/routes/proxy/chat.ts:7-18`；`completions.ts:31-32`；`responses.ts:25-48`；`responsesWebsocket.ts:26,805-834`；`models.ts:8-20`；`embeddings.ts:30-31`；`images.ts:29-32,231,451`；`videos.ts:37-40,200,233`；`search.ts:30-31`；`rerank.ts:5-18`；`files.ts:1`
- `src/server/routes/proxy/multipart.ts:13-22,28-40,42-71`
- `src/server/proxy-core/surfaces/geminiSurface.ts:316,497,1510-1516,224-231`

**中间件**
- `src/server/middleware/auth.ts:109-126`（admin）、`:128-171`（proxy auth）、`:82-107`（IP 提取/白名单）、`:173-192`（上下文/owner）
- `src/server/services/downstreamApiKeyService.ts:327-342`（策略/路由允许）、`:418-495`（授权）、`:497-505`（请求计数）、`:507-517`（成本）
- `src/server/routes/proxy/downstreamPolicy.ts:6-37`
- `src/server/proxy-core/downstreamClientContext.ts:260-286`
- `src/server/middleware/requestRateLimit.ts:55-84`；`src/server/routes/api/tokens.ts:3,58`；`src/server/routes/api/oauth.ts:2,74`

**选渠道**
- `src/server/proxy-core/channelSelection.ts:84-158`
- `src/server/services/tokenRouter.ts:1411-1419`（normalizeModelAlias）、`:1436-1447`（策略允许）、`:1464-1479`（modelMapping）、`:1835-1842`（selectChannel）、`:1859-1892`（next/preferred）、`:2803-2935`（selectFromMatch）、`:3284-3340`（候选资格/冷却）、`:342-344`（冷却退避）
- `src/server/services/proxyChannelRetry.ts:3-13`

**出站/协议转换/传输**
- `src/server/proxy-core/surfaces/chatSurface.ts:288-1325`（主循环）、`:450-468`、`:499-674`、`:676-726`、`:732-1120`（流）、`:1122-1220`（非流）、`:1221-1324`（异常）
- `src/server/proxy-core/surfaces/sharedSurface.ts:105-114,162-183,203-209,239-306,308-331,383-524,526-804`
- `src/server/proxy-core/orchestration/endpointFlow.ts:101-262`；`upstreamRequest.ts:83-101,103-171`
- `src/server/services/upstreamRequestBuilder.ts:404-800`（尤其是 `:495-523,584-613,640-707,709-781,783-799`）
- `src/server/services/upstreamEndpointRuntime.ts:1-10`；`upstreamEndpointDerivation.ts:84-90+`
- `src/server/proxy-core/providers/registry.ts:7-16`；`claudeProviderProfile.ts:9-44`；`codexProviderProfile.ts:9-46`
- `src/server/proxy-core/executors/types.ts:70-83,265-309,311-323`；`codexExecutor.ts:4-8`；`claudeExecutor.ts:4-8`；`geminiCliExecutor.ts:39-59`；`antigravityExecutor.ts:205-285`
- `src/server/services/runtimeDispatch.ts:1`；`src/server/routes/proxy/runtimeExecutor.ts:7-24`
- `src/server/proxy-core/firstByteTimeout.ts:109-170,25-33,185-187`
- `src/server/proxy-core/serviceTierPolicy.ts:124-163`；`webSearchSimulation.ts:284`
- `src/server/services/siteProxy.ts:172-193,419-455,474-511`
- `src/server/services/siteApiEndpointService.ts:363-427,52`
- `src/server/services/proxyChannelCoordinator.ts:195-248,306,371`
- `src/server/transformers/openai/chat/index.ts:25-104`；`transformers/contracts.ts:48-78`；`transformers/canonical/envelope.ts:50-77`；`transformers/openai/chat/proxyStream.ts:36-349`；`transformers/shared/protocolLifecycle.ts:20-88`
- `src/server/proxy-core/runtime/codexWebsocketRuntime.ts:10-14,36-47`

**响应/计费/日志/冷却**
- `src/server/services/proxyUsageFallbackService.ts:538-593`；`proxyUsageParser.ts:307,328,337,379`
- `src/server/services/proxyBilling.ts:55-92`
- `src/server/services/proxyLogStore.ts:259-...`
- `src/server/services/proxyFailureJudge.ts:170-203`
- `src/server/services/proxyRetryPolicy.ts:87-103`
- `src/server/services/tokenRouter.ts:2448-2528`（recordSuccess）、`:2655-2788`（recordFailure）、`:2617-2650`（clearChannelFailureState）
- `src/server/services/routeCooldownService.ts:115-142`；`src/server/routes/api/tokens.ts:739`
- `src/server/proxy-core/conductor/DefaultProxyConductor.ts:21-127`；`retryPolicy.ts:3-21`；`usageHooks.ts:3-20`；`streamTermination.ts:1-10`

---

## 9. 未确认 / 注意事项

1. **「/proxy/*」前缀不存在**：真实入站路径为 `/v1/*` 及别名（§0 更正 1）。若外部有反向代理把 `/proxy` 重写到根路径，属部署层，未在源码中体现。
2. **`rate-limiter-flexible` 未用于代理链路**：仅在 `/api/tokens`、`/api/oauth`（§2.6）。代理入口的限流/防滥用需在网关外层或 `proxyAuthMiddleware` 自行扩展；当前源码无此能力。
3. **`request_body_limit` 不是运行期可配项**：body 上限硬编码 20 MiB（`config.ts:5`），不来自 settings/环境变量（grep 无 `request_body_limit`）。若运行期通过设置改了 bodyLimit，源码中未见对应读取点——未确认是否存在其它注入途径。
4. **`DefaultProxyConductor` 未接入生产路径**：仅测试引用（§0 更正 3、§7.3）。其 retry/usage Hook 语义与 surface 实路径**并存但彼此独立**；若后续把 surface 改为经 conductor，重试语义需重新核对（尤其「同通道重试」在 surface 中并不存在，surface 每次必换通道）。
5. **`endpointCandidates` 的具体排序/降级规则**：`upstreamEndpointDerivation.ts` 未逐行展开（仅给出入口 `:84-90`）；不同平台/能力的精确排序未在本报告穷举，标注为「部分未展开」。`endpointFlow` 的降级条件依赖 `endpointStrategy.shouldDowngrade / shouldAbortRemainingEndpoints`（来自各 transformer 的 compatibility），细节分散在 `transformers/*/*/compatibility.ts`，未逐一确认。
6. **`webSearchSimulation` 的完整模拟搜索实现**（`webSearchSimulation.ts:284` 起）只确认了「仅搜索请求会本地模拟、不转发上游」，其内部如何生成搜索结果未在此报告展开。
7. **上游 WebSocket 与 responses compact**：`codexWebsocketRuntime` 的续接/重连细节、`/v1/responses/compact` 的压缩语义在 `openAiResponsesSurface.ts:465,570-582,1247-1256,1322` 有分布，本报告仅点到入口，未逐分支确认。
8. **`proxyLogs` 表字段随迁移变化**：`insertProxyLog` 会按列存在性（`hasProxyLogBillingDetailsColumn / hasProxyLogClientColumns / hasProxyLogStreamTimingColumns`）动态选择字段（`proxyLogStore.ts:280-299`），不同 DB schema 版本落库字段可能不同。
