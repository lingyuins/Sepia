# magpie 单次 API 请求链路调查（入站 → 中间件 → 出站 → 响应）

> 调查对象：`D:\CodeWorkspace\magpie`（只读，未修改）。忽略 `.git`、`node_modules`。
> 范围：一条客户端请求进入本地网关、经过中间件、进入选路、发往上游、把回复送回客户端的**请求生命周期**。路由**策略**细节（策略库、Seat、weighted、Smart 等）另有专文（`magpie-路由模型调查.md`），本文只说明请求如何进入选路并继续。
> 日期：2026-11-03。行号基于当日工作区快照；结论均标 `文件:行号`，拿不准处标「未确认」。

---

## 1. 概览

magpie 的网关是一个进程内 `net/http` 服务器，默认监听 `127.0.0.1:3425`（`internal/gateway`）。一个 TCP 连接到达后：

- `http.Server.Handler` 是 `s.closesWhenHandedOff(s.Front())`（`gateway.go:787`）。
- `Front()` 是最外层：`socketEntry(dumpFirst(lanGuard(s.Handler())))`（`oneport.go:15-21`，实际一行在 `oneport.go:19`）。
- `Handler()` 是客户端 API 的路由与内层中间件链：
  `socketEntry(s.dumpFirst(s.counted(headerLogged(holdCatalog(corsGuard(callerGuard(readOnlyGuard(mux, withCaller(keyLimited(mux))))))))))`（`gateway.go:895-901`，一行在 `gateway.go:900`）。

也就是说，一个普通 HTTP 请求自外向内依次经过（两次 `socketEntry`/`dumpFirst` 分别来自 `Front` 与 `Handler`）：

```
socketEntry → dumpFirst → lanGuard
  → socketEntry → dumpFirst → counted → headerLogged → holdCatalog
  → corsGuard → callerGuard → readOnlyGuard → withCaller → keyLimited
  → ServeMux(routeMux) → 具体 handler
```

到 handler 后分两段：

1. **入站解析**：`handle()`/`gemini()`/`ollamaChat()` 读取并规整请求体（`requestBody`、`requestModel`），再进入 `serveAgent()`。
2. **服务与选路**：`serveAgent()` 先跑网关侧 JS/插件中间件（`gwmw`），然后 `serve()` 做模型解析（`provider.Resolve`）→ `plan`/`planGroup`（`fallback.go`）→ `weighRouted`（`routing.go`，策略库）→ `affine`（`affinity.go`）→ 候选循环 `attempt()`。
3. **出站**：`attempt()` 决定走 `passthrough`（原样中继）还是 `translate`（协议翻译），最终经 `forward`/`forwardOnce` 用 `p.Do()` 发往上游（或经插件 `plugin.Fetch` → Bun `host.js`）。
4. **响应与收尾**：SSE/整包解析 → 写入客户端 → 汇总 `Usage` → 写 `usage.Record`、路由轨迹 `trace`、`route_events` 与「会话粘性」文件（`noteSticks`）。

关键文件地图：

| 阶段 | 主要文件 |
|---|---|
| 对外监听 / 最外层 | `internal/gateway/gateway.go`（`Serve`、`serve`、`handle`）、`oneport.go`（`Front`/`ClientPath`）、`lan.go`（`lanGuard`）、`allow.go`（允许列表） |
| 中间件链 | `gateway.go:900`（链装配）、`cors.go`、`lan.go`、`readonly.go`、`key_limit.go`、`holdcatalog.go`、`wire_headers.go`、`busy.go`、`goroutines.go`、`remote_magpie.go`、`responses_ws.go` |
| 网关 JS 中间件 | `internal/gateway/middleware.go`（`serveAgent`）、`internal/gwmw/gwmw.go`、`internal/middleware/{run.go,account.go,middleware.go,points.go}`、`plugins/middleware`、`docs/subsystems/gateway-middleware.md` |
| 选路 | `fallback.go`（`plan`/`planGroup`/`planPaused`）、`routing.go`（`weigh`/`weighRouted`）、`affinity.go`（`affine`）、`providers/`（`provider.Resolve`/`GroupFor`） |
| 出站 | `gateway.go`（`attempt`/`forward`/`forwardOnce`/`passthrough`/`translate`）、`continuation.go`（`streamTranslated`）、各协议文件（`chat.go`/`responses.go`/`anthropic.go`/`gemini.go`/`vertex.go`）、`upstream_transport.go`、`netproxy`、`internal/provider/plugins.go` + `internal/plugin/host.go`/`host.js` |
| 响应/收尾 | `sse.go`、`gathered.go`、`trace.go`、`route_events.go`、`affinity.go`（`noteSticks`）、`internal/usage` |

---

## 2. 入站链路

### 2.1 客户端 API 路由表（`gateway.go:906-1026`，`routes()`）

`routes()` 建一个 `routeMux`（包装 `http.ServeMux`，记录 pattern，用于 `readOnlyGuard` 判定，`readonly.go:49-58`）。与本调查相关的注册：

| 方法 + 路径 | handler | 行 |
|---|---|---|
| `POST /v1/chat/completions`、`/chat/completions` | `s.handle(provider.Chat)` | 970-971 |
| `POST /v1/responses`、`/responses` | `s.handle(provider.Responses)` | 972-973 |
| `GET /v1/responses`、`/responses` | `s.responsesOverHTTP`（可能是 Responses-over-WebSocket） | 974-975、1031-1041 |
| `POST /v1/messages`、`/messages` | `keepsSignIn(s.handle(provider.Anthropic))` | 976-977 |
| `POST /v1beta/models/{call...}` | `s.gemini`（路径里带 `:generateContent` 等） | 1021 |
| `GET /v1beta/models` | `s.geminiModels` | 1020 |
| `POST /api/chat` | `s.ollamaChat(false)` | 929 |
| `POST /api/generate` | `s.ollamaChat(true)` | 930 |
| `GET /api/version`、`GET /api/tags`、`POST /api/show` | `s.ollamaVersion`/`s.ollamaTags`/`s.ollamaShow` | 924-926 |
| `POST /v1/systemone` | `s.serveSystemOne` | 978 |
| `.../codex/...`（`CodexPath`） | `s.codexBackend` | 1018 |
| 其余媒体/MCP/统计路由（images、embeddings、videos、mcp…） | 各自 handler（不在本调查范围） | 983-1017 |
| `/`（兜底） | 404 说明可用端点 | 1022-1024 |

`GET /v1/models`、`/v1`、`/` 是模型/信息读取（`models`/`info`），`GET /api/hello` 供 Claude Desktop 探活（`gateway.go:913-921`）。

### 2.2 `handle()`：Chat / Responses / Anthropic 的统一入口（`gateway.go:1790-1886`）

`handle(from provider.Protocol)` 返回闭包，步骤：

1. `s.requestBody(w, r, from)`（`request_bounds.go:38` → `readRequestBody:42`）读入请求体并做边界控制（见 §9.6）。
2. `requestModel(body)`（`gateway.go:8385`）取出 `model` 字段；失败写 400。
3. `noteAsked(r.Context(), model)` 记下「客户端要的模型」。
4. 各协议/客户端特判：
   - Anthropic 的 `namesThread(body)` → 拒绝 Claude Code 的 Message Thread（`gateway.go:1806-1809`）。
   - Responses 的 `codexOwnOffMagpie`（Codex 自家模型在 Codex 自身登录上）→ 转 `s.codexBackend`（`1813-1821`）。
   - Responses + Codex：`spawnAgentModels` 补全 subagent 可选模型（`1825-1827`）。
   - Chat：`thinkingEffort`、`liftMessageTools`、`shapeChatTools` 规整思考强度与工具形状（`1828-1837`）。
   - Anthropic + Claude Code：`provider.SawClient` 记录 Claude Code 头（`1843-1851`）。
   - Responses 的 Codex 标题/描述/摘要/预测/native compaction 分支（`1854-1883`）。
5. `s.serveAgent(w, r, from, body)`（`1884`）——进入真正的服务与选路。

### 2.3 Gemini 入口（`gateway.go:1891-1938`）

`gemini()` 从 `r.PathValue("call")` 拆出 `{model}:{method}`（`:generateContent`/`:streamGenerateContent`/`:countTokens`），`requestBody` 读体，`geminiCamel` 统一字段大小写，`decodeRequest` 校验是完整 JSON 对象，`validateModel`；`countTokens` 走 `geminiCount` 本地估算，其余把 `model` 与 `stream` 塞回 body 后 `serveAgent(w,r,provider.Gemini,...)`（`1937`）。`geminiModels` 列目录（`1957`）。

### 2.4 Ollama 入口（`ollama_chat.go:66-121`）

`ollamaChat(generate bool)` 复用 Chat handler：先 `requestBody(provider.Chat)`，解析 `ollamaRequest`，用 `ollamaChatBody` 造一个 Chat 请求体，克隆 `*http.Request` 把 body 换成 Chat 形状，再用一个 `ollamaWriter`（把回复重塑成 Ollama 的 NDJSON）调用 `s.handle(provider.Chat)`（`ollama_chat.go:67,104-119`）。也就是说 Ollama 路由最终仍落到 §2.2 的 `handle(Chat)` → `serveAgent`。空 prompt 的「加载模型」预热请求单独返回（`85-103`）。

### 2.5 Responses over WebSocket（`responses_ws.go`）

`GET /v1/responses` 若 `wantsSocket(r)`（`responses_ws.go:61-63`，GET + `Upgrade: websocket`）则由 `s.serveResponsesSocket` 服务；普通 GET 返回 405/告知用 POST（`gateway.go:1031-1041`）。WebSocket 的每一「turn」会被再次送入入口 handler（见 `socketEntry`，§3）。仅 HTTP Upgrade 但非 WebSocket 的返回 426。

### 2.6 各协议请求 parse 函数

| 协议 | 入口读取 | parse（读成 IR `*Request`） |
|---|---|---|
| Chat | `handle(Chat)` | `readChat`（`chat.go:83`）、`parseChat`（`chat.go:76`） |
| Responses | `handle(Responses)` | `readResponses`/`readResponsesFor`（`responses.go:325/331`）、`parseResponses`（`responses.go:318`） |
| Anthropic | `handle(Anthropic)` | `readAnthropic`（`anthropic.go:148`）、`parseAnthropic`（`anthropic.go:137`） |
| Gemini | `gemini()` | `parseGemini`（`gemini.go:232`；`geminiCamel` 先归一字段名） |
| Ollama | `ollamaChat()` | 无独立 IR 读取，转成 Chat 体再走 Chat |

统一调度在 `parseRequest(proto, body, facts, keepInstructions)`（`gateway.go:8003`）与 `afterParse`（`8024`：`unsignCalls`/`unreasonCalls`、`LastIsTurn`、required-tool-choice 校验）。

---

## 3. 中间件链（顺序表）

以下按「请求自外向内」的顺序列出。`Front`（真实监听器上）与 `Handler`（也被 Web 页内嵌 handler 直接使用）各套一层 `socketEntry`/`dumpFirst`，因此普通请求会依次遇到两次这两个（对普通请求二者都近乎无操作）。

| # | 中间件 | 位置 | 作用 | 拒绝/终止 |
|---|---|---|---|---|
| 0 | `closesWhenHandedOff` | `gateway.go:787` | 交接给另一 magpie 时关闭本连接 | — |
| 1 | `socketEntry`（Front） | `oneport.go:19`；实现 `responses_ws.go:67-77` | 记录 WebSocket 的原始 ctx/req/入口，供每一 turn 重放入口 | — |
| 2 | `dumpFirst`（Front） | `oneport.go:19`；实现 `goroutines.go:65-73` | 若是 `GET <GoroutinesPath>` 且请求合规，直接吐 goroutine dump（抢在任何锁之前） | — |
| 3 | `lanGuard` | `oneport.go:19`；实现 `lan.go:334-418` | 见下「lanGuard 细节」 | 403/401/429 等 |
| 4 | `socketEntry`（Handler） | `gateway.go:900` | 同上（`socketOriginKey` 已在则跳过） | — |
| 5 | `dumpFirst`（Handler） | `gateway.go:900` | 同上 | — |
| 6 | `counted` | `gateway.go:900`；实现 `busy.go:80-113` | 记在飞请求 `inFlight`、登记 `active` inflight（`/v1/magpie/concurrency`）、`watchUnrouted`、panic 时 `endBroken` | — |
| 7 | `headerLogged` | `wire_headers.go:61-74` | 非 GET/HEAD/OPTIONS 时把**客户端到达时**的头 `Clone()` 存进 ctx（`headerLog`），供 Recent calls/归档查看（读出时脱敏） | — |
| 8 | `holdCatalog` | `holdcatalog.go:18-29` | 请求期间 `provider.Hold()` + `filememo.Hold()`：一次请求内目录/文件只构建/读取一次（否则每个 Resolve 重建全部 provider，见注释 #2502）。WebSocket 请求不持（每 turn 另持） | — |
| 9 | `corsGuard` | `cors.go:44-91` | 见下「CORS 细节」 | 403/401/204（preflight） |
| 10 | `callerGuard` | `lan.go:535-546` | 兜底识别调用者：当 ctx 无 `KeyID`、请求带具名 key（`managedKey`）且 `local(r)||remoteKeyed()` 时调 `identifyCaller`（供内嵌 handler/测试也走同一套 key 校验） | 401/429 |
| 11 | `readOnlyGuard` | `readonly.go:62-78` | 只读 key（`access.Key.ReadOnly`）只允许 `readOnlyRoutes`（`readonly.go:28-44`）里的 GET；其余 403 `gateway_key_read_only` | 403 |
| 12 | `withCaller` | `remote_magpie.go:53-65` | 从 `X-Magpie-Agent`/`X-Magpie-Via`/`X-Magpie-Searching` 读出 caller 记录进 ctx，并从请求头删掉这三个（不转给上游） | — |
| 13 | `keyLimited` | `key_limit.go:36-80` | 网关 key 的预算/限额：POST 且受限 key 先 `reserveFor` 预留预算；超额 429（带 `Retry-After`、`X-Magpie-Limit-Reset`、`X-Should-Retry:false`），插件预算关闭却设限额则 503；读/`/mcp/`/`count_tokens`/`KeysPath` 不计 | 429/503 |
| 14 | `routeMux`（`http.ServeMux`） | `gateway.go:907` | 按 pattern 分发到 §2.1 的 handler | 404 |

### 3.1 `lanGuard`（`lan.go:334-418`）细节

严格顺序：

1. `ownCallback(r)`：Claude Code 的 MCP 回调路径（`POST claudeMCPPath+token`，token 常数时间比对，`lan.go:453-477`）直接放行，不需要 key。
2. `takePagePort(r)`：把 `X-Magpie-Page-Port` 从 header 移入 ctx（`oneport.go:82-97`），使其不再转给 vendor。
3. `fromWSL(r)`：WSL 内 agent 从 Windows 网络地址来，视作本机（`asFromLoopback`）。
4. `refuseNotAllowed(w, r)`：**允许 IP 列表优先于 key**（`allow.go:222-229`）——不在允许列表（`MAGPIE_ALLOW_IPS` 或 Settings）的地址直接 403，与 key 无关；无列表则全放行，本机地址总放行（`allow.go:139-155`）。
5. `remote := !local(r)`；`local()` 判据是「连接来自 loopback + 未经过代理/隧道 + 非 page port」（`lan.go:708-715`）。
6. **DNS rebinding 防护**：本机请求若 `rebound(r)`（浏览器标记 + Host 不是本机名，`lan.go:490-502`）→ 403。
7. `remote && !remoteKeyed()`：网关既未共享也非 `OnNetwork` 时，来自他机的请求 403（page port / proxy 各有专门提示，`357-369`）。
8. 允许情况放行：`remote && corsPreflight && corsAllowed` 的 preflight（`382-385`）、key 页本身（`388-391`）、图片 token（`395-398`）、OIDC 兑换（`402-405`）。
9. `remote || managedKey(r)` → `identifyCaller`（`548-602`）：遍历 `callerKeys`（`Authorization` Bearer / `x-api-key` / `x-goog-api-key` / `?key=`，`lan.go:663-682`）用 `access.Check`；失败时本机放行任意 token，他机 401（并对失败做 `tooManyAuthFails` 限流 429）。成功则把身份写入 ctx，并把请求头里的 key **替换成 magpie 自己的 `Token`**（`lan.go:588-600`），这样上游永远看不到调用者的 key。
10. `remote` 时打 `lanKeyed{}` 标记（`sharedWith` 读它，`lan.go:639-642`）。

### 3.2 CORS（`cors.go`）

- `Origin` 为空（几乎所有 agent/SDK）→ 原样放行（`65`）。
- Origin 在 `Settings.CORSOrigins`（`corsAllowed`，`cors.go:199-208`）时：设 `Access-Control-Allow-Origin`、`Expose-Headers: X-Magpie-Session`；preflight 直接 204（`66-82`）；实际调用**必须带启用的 gateway key**（`keyedCaller`，`96-99`），否则 401（`85-88`）。
- Origin 未列出时：`foreignPage`（`cors.go:118-145`，http(s) 且非本机/同源）→ 403；`Origin: null`（沙箱 iframe/file://）无 key → 403；`localhost` 子域（WebKit 可能走 DNS）无 key → 403（`cors.go:47-64`）。非 http(s) scheme（app://、chrome-extension:// 等桌面/扩展）放行。

### 3.3 关于「两次」`socketEntry`/`dumpFirst`

`Front()` 与 `Handler()` 各自装配了一次。真实监听器只用 `Front`（`gateway.go:787`）；`magpie web` 的页面端口把客户端前缀路径（`ClientPath`，`oneport.go:43-53`）经 `Front()` 转给网关（`gui/web.go:434`），所以同一套守卫在页面端口上也生效。`socketEntry` 已带 `socketOriginKey` 时会跳过重复包装（`responses_ws.go:70`），`dumpFirst` 只拦 `GoroutinesPath`，故重复套不改变普通请求语义——但阅读调用栈时注意层数。

---

## 4. 网关 JS 中间件（`gwmw` + `internal/middleware` 的 moejs）

### 4.1 挂载点：`serveAgent`（`gateway.go` → `internal/gateway/middleware.go:23-79`）

`serveAgent(w, r, from, body)` 的顺序：

1. `withGatewaySession(w, r)`（会话身份）、`agentTurn(r)`。
2. `previousResponse(w,r,from,body)`：Responses 里指向上一条 response 的请求先补全整段会话（`prev_response.go`）。
3. `recordConversation(...)`：会话记录。
4. `s.stoppedToolLoop(...)`：一圈工具调用在此直接回答，不进任何 hook/中间件/provider。
5. `gatewayHooks(w,r,from,body)`（`middleware.go:153-172`）：**异步请求钩子**（OpenCode 等插件，跑在 Bun 插件宿主，`plugin.HasGateway()` 时）；它包在外层，所以看到的是 agent 最终得到的回复。这属于 `docs/subsystems/gateway-request-hooks.md`，与 JS 中间件是两条不同的路径。
6. `agents := gwmw.AgentsWanted(middlewareWanted)`（`middleware.go:41`；`middlewareWanted` 判 `middleware.On()>0`，即 `plugins/middleware` 开着，`146`）。**没有中间件就直接 `s.serve`**（`42-45`）。
7. 有中间件时：`info := gwmw.Info{Protocol, Model, Stream, Path, Agent}`，对每个 `gwmw.Agent`：
   - `run := a.Begin(info)`（`serveAgent` 里的 `Begin`）；
   - `run.Request(body)` → 新 body，或 `*gwmw.Reject`（`writeError` 终止）；
   - `w, done = run.Wrap(w)` 包装响应 writer；
   - 把同时实现 `gwmw.Tried`/`gwmw.Accounts` 的 run 收到 `mwRuns` 放进 ctx（`75-77`）；
   - `defer run.End()` 归还 runtime。
8. 最后 `s.serve(w, r, from, body)`。

### 4.2 五个钩子与挂载

| JS 钩子 | Go 侧 | 挂载点 | 说明 |
|---|---|---|---|
| `onRequest(body, ctx)` | `Run.Request`（`internal/middleware/run.go:143`） | `serveAgent` 内 `run.Request(body)`（`middleware.go:65`） | 每个中间件按序改 agent body；返回对象即新 body，`undefined` 原样；`ctx.reject(status,msg)` 终止 |
| `onProvider(body, ctx)` | `Run.Tried`（`run.go:173`）→ `gwmw.Tried` | `triedOn(ctx,p,model,body)`（`middleware.go:95-105`），在候选循环每次尝试前调用 | 每次 try 从 `onRequest` 留下的 body 重新开始，按 provider 改体；`ctx.provider` 给 id/name/account/model |
| `onAccount(account, ctx)` | `Run.Account`（`internal/middleware/account.go:42`）→ `gwmw.Accounts` | `accountSkipped(ctx,p,model)`（`middleware.go:111-126`），在候选循环顶部、任何 try 动作之前 | `{skip:true}` 跳过该账号（resting/扣费都不算）；`ref` 是账号身份 HMAC（`account.go`）；对 key 的 try 不调用 |
| `onEvent(ev, ctx)` | `Run.Wrap` 内的 event 处理（`run.go:398`） | `run.Wrap(w)` 包装的 `http.ResponseWriter` | 逐条 SSE 事件；对象替换、`null` 丢弃、`undefined` 原样；非对象（`[DONE]`）透传 |
| `onResponse(body, ctx)` | `Run.Wrap` 内的整包处理（`run.go:253`、`382-384`） | 同上 | 整包 JSON 回复缓冲后调用，`ctx.status` 可用；带 `Content-Encoding` 的回复原样通过 |

钩子识别在加载期：`internal/middleware/middleware.go:197-206` 用 `in.hook("onRequest"/"onProvider"/"onAccount"/"onEvent"/"onResponse")` 读导出；`middleware.go:2` 列出模块可导出的五钩子。执行限额：`onRequest`/`onProvider`/`onAccount`/`onResponse` 各 250ms、每个 `onEvent` 50ms、加载 1s；**fail-open**（抛错/超时/返回非法值都保持原 body 或原事件，仅计数），`onAccount` 失败则使用该账号（`docs/subsystems/gateway-middleware.md:44-51`）。runtime 按中间件 `sync.Pool` 复用、模块只编译一次（`jsmod`），同一请求多次钩子共享 `ctx.state`。

> 注意：`serveAgent` 也用于 Codex 后端的请求（`docs/subsystems/gateway-middleware.md:12` 提到 `gateway.go` 与 `codex_backend.go` 均调它）。magpie 自己的请求（标题、分类器、search/vision 替身）直接 `serve`，不过 JS 中间件（`middleware.go:19-22`，以及 `ownAsk`，`middleware.go:140-142`）。中间件看到的是**脱敏前**的 body，脱敏仍在上游生效（`gateway-middleware.md:33`）。

---

## 5. 路由分发（请求如何进入选路并继续）

> 本节只说明请求如何进入选路并继续；具体策略见 `magpie-路由模型调查.md`。`serve()` 从 `gateway.go:1980` 到 ~`4669`（`attempt` 之前）是请求主体。

### 5.1 `serve()` 开头（`gateway.go:1980-2159`）

- `offAsNone(from, body)`（客户端 `off` 归一为 none）、`heicAsJPEG`（HEIC 转 JPEG）、`withWires(r)`（本次请求生效的「上游名」，只读一次）。
- `plain := body`（未脱敏原文，供 `unredactedRoute`）；`w, r, named := withMemberNames(...)`（回复里 provider/model 名回填）。
- `holdBody(r.Context(), plain)`（本次请求的 body 只解析一次，`factsOf` 复用）；`capture := &captureResponseWriter{...}` 包住 writer 以便截取回复。
- `who, agent := callerOf(r), agentOf(r)`；`metadata := requestSessionMetadata(...)`；构造 `Call{Time, From, Model, Agent, Via, Kind, wire, headers}`（`2043`）；`observeRequest`、`upBytes`。
- `ask` 处理：`askedFor := modelOf(body)`、`askedAt`（`:effort` 后缀）、subagent/desktop/science 等**替身**可能改 `asked`（`model move` 记在 Route.Moved）。

### 5.2 解析模型（resolution，`gateway.go:2215`）

`p, model, ok := provider.Resolve(asked)`（`2215`）。失败分支依次处理：禁用模型（`idOff`，403/404）、未知模型（404，`SwitchedOff`/`EmptyGroup`/`decision group` 等特判，`2229-2255`）、决策模型（`p.DecidesModel`，400）。`provider.FindGroup(asked)`（`2265`）判断是否路由组，`g = g.Live()`（手动组规则不生效）。

### 5.3 脱敏、hold、pause（进入选路前的过滤）

- `unredactedRoute(p,isGroup,ms)`：本地/局域网红线模型按原文发送；否则 `redactedHits` 脱敏（失败 fail-closed，`2273-2296`）。
- 组限定账号：`heldByGroup(r,g)`（`2304-2306`）。
- gateway key 限定模型/账号：`keyHolds`/`accountHolds`（`2307-2317`），组要 `groupAllowed`。
- pause 规则：`provider.PausedOut(g,ms,agent,folder,ruleClock())` 把暂停成员从组里拿掉（`2321-2336`）。

### 5.4 生成候选与计划（`gateway.go:2517-2561`）

- 非组：`cands, pl = s.planPaused(p, model, from, pausedFn)`（`2542`）。
- 组：`cands, pl = s.planGroup(g, ms, from)`（`2528`）。
- `planPaused`（`fallback.go:497-556`）对主 provider 走 `add()`：`perKeyBarred` → `weigh(q, cs, m, from)`（`fallback.go:504`）→ 生成 `pl.order`；随后沿 `p.Fallback` 链（含回退组 `planFallbackGroup`/`ResolveRoute`）追加，最后 `restLast`（把 resting 的放到最后）。
- `weigh`（`routing.go:2027`）→ `weighRouted`（`routing.go:2056`）：把每个 candidate 折成 `routes.Seat`（allowance/pace/weight/resting/tokens），交 `routes.Find(p.Routing)` 排序；某策略未被服务则按列出顺序。
- 组计划 `planGroup`（`fallback.go:600`）→ `planLevel`（成员逐个 plan，组内组按自身路由），必要时 `sinkPlanned`。

### 5.5 亲和（`gateway.go:2701`）

`cands, pl, aff, stuck := affine(scope, mode, rotate, spread, inOrder, r.Header, from, body, plain, cands, pl)`（`affinity.go`）。会话把上次应答者提前（auto/session/turn/off/strict），并可能把「临时顶替」还回主力；`aff.Kept`/`aff.Why` 记录原因（进 Route.Affinity）。

### 5.6 选路后的细化

`leadFirst`（sealed task 的 lead 先行，`2713`）、`ruleOver`（规则成员优先，`2719`）、`nestedRules`（组内组规则，`2735`）、图像/视觉成员的筛选（`2760-2796`）、`pinTo`（`X-Magpie-Account` 固定，`2797-2808`）、`roomy`（窗口装不下不问，`2812`）、`behindOverflow`（本会话被拒过长排后，`2817`）、`probeOne`（resting 只试最早回来的一个，`2821`）、`fastPick` 等。

### 5.7 候选循环（`gateway.go:2976-4591`）

`for i := 0; i < len(cands); i++`（`2976`）：

- `s.laneMate(...)`：本 key/账号满而同类有槽位就换到有空位的（`2988-2997`）。
- `quotaHeld`：剩余全耗尽则提前告知（`2998-3005`）。
- `accountSkipped(...)`：JS `onAccount` 跳过（`3006-3028`）。
- `s.lanes.take(...)`：并发车道取槽（`3427`，内部可能 `waitMate` 换到有空位的 key/账号）；失败 `errMoved`（直接去有空位者）或 `errQueueFull`/`errQueueWait`/`errRPM`。
- `s.rpms.wait(...)`：MaxRPM 分钟配额（`3458`）。
- 真正的尝试：`call.Status, call.Error = s.attempt(hw, r.WithContext(ctx), from, c.p, c.model, attemptBody, &call)`（`3480`）。
- 失败后：`passing(...)` 同账号重试、`busy(...)` 计入 patience、`restAfter(...)` 设置 rest、Codex 特有分支、`refusal handover` 等（`3530-4590`）。

`attemptBody` 在每次 try 前可能被 `triedOn(...)`（JS `onProvider`）改写（`3116` 起，`triedOn` 在 `middleware.go:95`）。

---

## 6. 出站链路

### 6.1 `attempt()`：一次 try 的编排（`gateway.go:4688-4885`）

1. 目的协议规整：Responses 的 `joinToolRuns`/`orphanedToolOutputs`/`pairToolItems`（`4694-4698`），Chat 的 `chatCallIDs`（`4699-4704`）。
2. `r = r.WithContext(p.Via(r.Context()))`（`4714`）：**Provider 自己的代理**注入 ctx；`noteMember(w,r,p,model)` 回复命名。
3. Claude 订阅：`p.Account.Agent=="claude"` → `s.serveClaudeSubscription(...)`（`4720-4723`，`claude_subscription.go:5767`），走真身 Claude Code 二进制路径。
4. `p.ResolveAuto(ctx, model)`（Copilot Auto 等）（`4730-4736`）。
5. 决定 **relay（原样中继）还是 translate（翻译）**：`relayable := slices.Contains(s.usable(p,model), from) && ...`（`4741`），后面一串规则（Claude 走 Messages、web_search 由谁做、custom tools、Gemini 非流、DashScope 非流、native compaction 等）最终置 `relay`/`ownAPI`。
6. `relay` → `s.passthrough(...)`（`4843`）；`passthrough` 返回 `done=false` 表示该端不服务此模型（换 `to`）；否则走 `s.translate(w,r,p,from,to[0],model,body,u)`（`4884`），`call.To` 记录。

### 6.2 `passthrough()`（`gateway.go:5468` 起）

- `rewriteModel(body, upstream)`：把模型名换成上游名（`provider.UpstreamNameIn`，`5469-5470`）。
- `gathers(p,proto,body)`：插件 Responses 账号且要整包 → 用 `gathered` 缓冲（`gathered.go:22-24`，`5479-5482`）。
- 按 proto 做「同一 API 但 vendor 挑剔」的整形（Responses `5489-5559`，Chat `5560-...`，Anthropic 类似），绝大多数是让不同 vendor 都吃得下的兼容改写。
- 发送与重试：`res, err := s.forward(ctx,p,proto,path,p.Prepare(body),r.Header)`（`5768`）；随后一连串「被某 vendor 拒 → 换形状再发」的分支（`5772-6118`：prev_response 整段、format/schema、history tools、`refusedOptional`、prefill、root union 等）。
- 结果处理（`6120` 起）：
  - `res.StatusCode >= 400` → `p.Explain` → `writeVendorError`（把 vendor 错误翻成该协议的 client 错误，`6121-6137`）。
  - 否则 `eventStream(res)` 判是否 SSE；`w.WriteHeader(res.StatusCode)`（`6154`）；构造一串 **tidy 过滤器**（`chatTidy`、`textcalls`、`nsTidy`、`reasoningTidy`、`intTidy`、`crTidy`、`phaseTidy`、`idTidy`、`sigFieldTidy`、`toolUseIDs` 等，`6158-6239`）；`sniffer` 收 usage（`6156-6157`）。
  - SSE 时逐事件读出并过 tidy，写回客户端；非 SSE 整包同理（`6420-6494`）。
  - `gather != nil` → `gather.finish(client,p,sse,cut,failed)`（`6500-6511`）；否则判定「上游中途断开/提前结束」并补错误事件（`6530-6565`），返回 `(res.StatusCode, sniff.failed, true)`。

### 6.3 `translate()`（`gateway.go:7709` 起）

- `request, err := parseTranslated(from, to, body)`（`7697`/`7714`）把客户端 API 读成 IR `*Request`；web_search 选 searcher（`7718-7736`）。
- **流式**：`request.Stream=true` 后 `s.streamTranslated(...)`（`7768-7772`，`continuation.go:398`）。`streamTranslated` 用 `newSSEWriter`、`encoder(from,...)`（把上游事件编码成客户端 API 的 SSE），`decoderFor(p, actual, tools)` 解码上游流，逐事件 `cont.emit(enc, ev)` 写到客户端；带 keepalive（`continuation.go:486-493`）、中断重试与 prefill 续写。
- **非流式**：`s.forwardTranslated(...)`（`7773`）→ `readSSE` 把上游流读进 `collector`（`7826-7832`，`u.add` 累加 usage、`replyEnds` 判结束）→ `col.finish()` 得 `Result` → `renderUsage(from, res, request, u)` 生成客户端 API 的整包 → `w.WriteHeader(200); w.Write(out)`（`7895-7901`）。
- 错误/边界：空回复（`emptyReply`，`7888-7891`）、提前结束（`7878-7887`）、vendor 中途拒（`7844-7876`）、`thinkingNeededRefusal` 重试（`7849-7854`）。

### 6.4 `forward()` / `forwardOnce()`（`gateway.go:4955-5360`）

`forward`（`4955-5026`）= `forwardOnce` + 一组「同一请求换条件重发」：

- 超字节上限 `refitOverCap`（`body_cap.go`）、`mantleReroute`、`retryTokenField`、`refuseThinkingBack`。
- 403/400 且 `p.Retries()`：`p.Retry` 最多 3 次，`p.AutoNext` 换模型（`4983-5005`）。
- Anthropic 的「不认识的 beta/shape/边界 thinking」最多 3 次去除后重发（`5010-5024`）。

`forwardOnce`（`5029-5360`）是真正发一次：

- 一大串 per-provider body 整形（`deepseekToolPatterns`、`grokToolPatterns`、`toolOneOfAsAnyOf`、`geminiToolTypes`、`withinCap`、`withoutClaudeSeals`、Anthropic 的 `adaptiveThinking`/`withoutRefusedShapes`/`withoutForeignThinking` 等；`5031-5134`）。
- 组装 URL：`base := p.Base(to)`，Zen free/mantle/Vertex 特判（`5135-5146`），`http.NewRequestWithContext(... base+path ...)`（`5147`），`provider.SetBody(req, body)`（延迟发送到被应答，`5152-5153`）。
- 头：`Content-Type`、`Accept`、`User-Agent`（`userAgentFor`）、Anthropic 的 `anthropic-version`/`anthropic-beta`、Claude Code/Codex 中继的「客户端原样头」、插件会话头 `X-Magpie-Conversation`/`X-Magpie-Session`/`X-Magpie-Subagent`/`X-Magpie-Fallback`、remote-magpie 的 `passOnCaller`（`5154-5285`）。
- `p.Sign(ctx, req, to, body)`（`5286`）——账号/插件签名。
- 发送：`res, err := p.Do(s.client, req)`（`5316`）；之后 `RetryOneAuth`/`RetryZhipuCoding`/`RetryOpenCodeFreeOnZen`、`notAnAPIReply`、`codexDropped`、`heardCodexLimits`、`noteSignIn`（`5317-5359`）。
- Codex 账号会 zstd 压缩 body 再发（`5298-5302`）。

### 6.5 传输层 / HTTP2 / 代理（`upstream_transport.go` + `netproxy`）

`upstreamTransport()`（`upstream_transport.go:34-41`）返回 `drawsApart{any, draws, uploads, slow}`：

- `any: codexSocket{ replayable{ netproxy.Dispatch(vendorTransport(upstreamHeaderWait)) } }` —— 普通请求；`codexSocket` 处理 ChatGPT 后端的 WebSocket/HTTP1；`replayable` 对「复用的 keep-alive 连接已断」的 POST 立即在新连接重发（`158-212`）。
- `draws: replayable{ netproxy.Dispatch(vendorTransport(0)) }` —— 绘图请求（首字节等待由 Settings 的绘图超时给，`drawWithin`，最长 60 分钟）。
- `uploads` —— 大 body（≥64KB 或长度未知）走不 ping 的 `uploadTransport`（`86-90`，`WriteByteTimeout: uploadStall`）。
- `vendorTransport(wait)`（`66-77`）：`Proxy: netproxy.Func`、`ResponseHeaderTimeout: wait`（普通 10 分钟，`upstreamHeaderWait=45`）、`ForceAttemptHTTP2: true`、`HTTP2: netproxy.HTTP2Health()`。
- HTTP2 静默检测：`netproxy.HTTP2Health()`（`netproxy.go:57-59`）= `{SendPingTimeout: 20s, PingTimeout: 10s}`（`PingAfter/PingWait`，`netproxy.go:44-46`）：连接 20s 无帧就 ping，10s 无应答就丢弃，使该连接上的请求失败、组里换下一个（见 §9.4）。
- `drawsApart.RoundTrip`（`110-138`）：大 body 且最近发现上行慢（`slowFor` 内）先走 uploads；否则先 `any`，若因「丢 ping」失败（`lostPing`，`142-144`）且有 `GetBody` 则重发到 uploads。
- 代理：`p.Via(ctx)`（`attempt` 里注入）→ `netproxy.Func`（`netproxy.go:64-79`）选 provider 指定代理或全局代理，本机/局域网地址绕过。

### 6.6 插件 provider 的 fetch（Bun 宿主）

当 provider 是插件账户（`p.IsPlugin()`，`provider/plugins.go:132`），`p.Do` 走不同路：

- `Provider.Do`（`provider/plugins.go:940-953`）→ `p.do`（`961-978`）：若 `p.Account.transport != nil`，调 `p.Account.transport(req)`；该 transport 在 `provider/plugins.go:571` 设为 `func(req) { return pluginFetch(pp, acct.Key, req) }`。
- `pluginFetch`（`provider/plugins.go:656-740`）：读取 body、把 magpie 的 `/v1/...` 路径映射到插件 loader 给的 base URL、拼 `plugin.FetchRequest`（provider/account/model/NPM/URL/Method/Headers/Body/Session），调 `plugin.Fetch`。
- `plugin.Fetch`（`internal/plugin/host.go:1831-1878`）：`netproxy.Held` 检查 → `get(ctx)` 取 Bun 宿主 → `beginStream` + `fetchLine`（编码一行请求，`host.go:1285`）`enqueueLine` 发给 Bun，`headRead` 等回复头，返回一个把响应体流式读回的 `*http.Response`。
- Bun 侧由 `internal/plugin/host.js`（约 136KB）执行 loader 的 `fetch`（或 Bun 自带 fetch）。**也就是说插件 provider 的「出站」绕过了 §6.5 的 Go transport**，magpie 只看得到请求/回复头（`provider.SendsThroughPlugin` 钩子把双向头记入 `headerLog`，`wire_headers.go:115-121`）。
- 插件请求同样会加会话头：`X-Magpie-Conversation`、`X-Magpie-Session`、`X-Magpie-Subagent`、`X-Magpie-Fallback`（`gateway.go:5264-5281`）。

### 6.7 `X-Magpie-*` 头

| 头 | 方向 | 位置/说明 |
|---|---|---|
| `X-Magpie-Session` | 客户端→magpie / 记录 | `gateway.go:8487`；`sessionOf` 读取；也转给插件（`provider/plugins.go:70`） |
| `X-Magpie-Account` | 客户端→magpie（固定账号） | `gateway.go:8494`；`pinTo` 使用；不转上游 |
| `X-Magpie-Agent` / `X-Magpie-Via` / `X-Magpie-Searching` | 上游 magpie→本 magpie | `remote_magpie.go:28-29`、`search_remote.go:27`；`withCaller` 读后删除 |
| `X-Magpie-Provider` / `X-Magpie-Model` / `X-Magpie-Response-Model` | magpie→客户端（回复命名） | `member.go:47-49` |
| `X-Magpie-Limit-Reset` | magpie→客户端（key 限额 429） | `key_limit.go:28` |
| `X-Magpie-Page-Port` | `magpie web` 页端口→网关 | `oneport.go:68` |
| `X-Magpie-Conversation`/`Session`/`Subagent`/`Fallback`/`Work-Dir`/`Work-Git` | magpie→插件 | `provider/plugins.go:36-71` |
| `X-Magpie-Cache-Marks`/`-Dropped`、`X-Magpie-Refused`、`X-Magpie-Resets-At`、`X-Magpie-Answered-There`、`X-Magpie-Key-Limit-There` 等 | 诊断/中继 | `claude_cache.go`、`routing.go:1873-1887`、`fallback.go:1622` 等 |

（回复侧的 `X-Magpie-*` 基本不进上游：`provider/replyheaders.go:38` 明确 `X-Magpie-` 前缀不转发。）

### 6.8 两个账号专属后端

- **Claude 订阅**：`attempt` 里 `serveClaudeSubscription`（`claude_subscription.go:5767`）：Claude Code 必须走真身二进制（OAuth 直连会被内容分类为第三方流量）。它自行构造 Claude 请求、跑 Claude Code 进程、把回复按客户端协议回写（`relay`，`claude_subscription.go:6183`）。
- **Codex 后端**：`/codex` 路径进 `s.codexBackend`（`codex_backend.go:68`），以及 `handle(Responses)` 里 `codexOwnOffMagpie` 也会转入。它同样 `trace.beginFor`、`plan`、尝试，并特别处理 ChatGPT 后端（WebSocket/HTTP1、zstd、compaction）。本调查未逐行展开其内部（见 §11）。

---

## 7. 响应与收尾

### 7.1 SSE 解析与事件

- 读取：`readSSE`/`readSSEAlive`（`sse.go:15-68`）按行切 `event:`/`data:`，`alive` 回调用于保活。
- 中继（passthrough）：`newSniffer(proto, Content-Type)`（`gateway.go:6156`）边转发边统计 usage/结束；tidy 过滤器链把上游事件改写成客户端能读的形状（`6158-6239`）；整包时用 `chatWhole` 等（`6181-6184`）。
- 翻译（translate）：`decoderFor` → `decoder(proto)`（`gateway.go:8108-8138` 起）把上游 SSE 解成 `Event`；`encoder(from,...)`（`8257-8288`）把 `Event` 编成客户端协议的 SSE；`render(proto, res, r)`（`8290-8308`）把 `Result` 渲染成整包 JSON。
- 保活：`relayEvents`（`gateway.go:8235-8255`）在事件间隙按 `keepaliveEvery`/`keepaliveLongest` 发 keepalive；`streamTranslated` 有等价逻辑（`continuation.go:487-493`）。

### 7.2 usage 记录

- 一次请求的 `*Usage` 在 `attempt`/`passthrough`/`translate` 里通过 `u.add(...)` 累加（`7829`、`7896`、`u.add(sniff.usage())` 于 `6157` 的 defer、`streamTranslated` 的 `u.add`）。
- `serve` 末尾（`4595-4668`）：`call.Millis`、`call.Usage.ResponseID`，`finishCapture()`；`s.trace.update(tr, ...)` 写回 Route（`4616-4640`，含 `t.Tokens`/`TTFT`/`Output`/`Reasoning`/`Sent`/`Own`/`Queued`/`Masking` 等）；`call.Route = tr.ID`；`s.record(call)`（Recent calls 环，`gateway.go:608-627`）；`call.To != ""` 时构造 `usage.Record` 并 `appendUsage(r, rec)`（`4644-4663`）。
- `appendUsage`（`lan.go:614-632`）：补 session/subagent/caller key、`rec.Local`、`budget.Append(rec)`（进 `internal/usage` 账本）。
- `decodeOf`：`usage.DecodeOf`（`internal/usage/usage.go:696` 注释说明它与前端 `app.js` 的 `ledDecode` 同源），`trace.go:124` 引用它把「输出 token 里的 reasoning」拆出来（Route.Reasoning）。

### 7.3 路由轨迹与事件流

- `trace.beginFor(r, Route{...})`（`gateway.go:2846`）创建 `Route`（`trace.go:48-...`）；`Tries []Try` 记录每次 try（状态、错误、rest、TTFT、served 等）。
- `route_events.go` 提供 `GET /v1/magpie/route-events/snapshot` 与 `?after=<cursor>` 的 SSE 流（`route_events.go:32-41`），只暴露 `routeActivity` 白名单字段（`id/time/agent/provider/model/attempt/phase/status/ms/ttft`），不含 session/账号/key。
- `GET /v1/magpie/trace`（`gateway.go:939`、`s.gatewayTrace`）给本机（或共享+key）看完整 trace。

### 7.4 会话粘性（`noteSticks`）

`noteSticks()`（`affinity.go:223-237`）把「本会话由谁应答」记到内存并延迟（`sticksFlushAfter`，非测试 2s）落盘；网关结束时 `flushSticks()`（`affinity.go:241`，在 `gateway.go:871` 调）确保交给下一个 magpie/下次启动时能读到。一个请求结束时它被调用（`affinity.go:993`、`1019` 附近，`affine` 的收尾）。

### 7.5 客户端断连时

`serve` 里多处 `case <-r.Context().Done():`（`gateway.go:3682`、`4104`、`4264`、`4363`）→ `canceled(r.Context())`（`side_deadline.go:68-73`）返回 `499 "the agent canceled the request"`（或 `sideTimedOut` 时说明是 magpie 自己等超时），并把 `try.Gone = true`。

---

## 8. 完整链路步骤表（一条 `POST /v1/chat/completions`）

| 步 | 层/动作 | 关键函数 | 位置 |
|---|---|---|---|
| 1 | 连接进入 HTTP server | `closesWhenHandedOff(Front())` | `gateway.go:787` |
| 2 | socket 入口 / goroutine dump | `socketEntry`、`dumpFirst` | `oneport.go:19`、`responses_ws.go:67`、`goroutines.go:65` |
| 3 | 允许列表→page port→rebinding→他机 key→preflight→identifyCaller | `lanGuard` | `lan.go:334-418` |
| 4 | 进入 Handler：再 socketEntry/dumpFirst | `Handler` | `gateway.go:895-901` |
| 5 | 并发计数/在飞登记 | `counted` | `busy.go:80` |
| 6 | 记录客户端头 | `headerLogged` | `wire_headers.go:61` |
| 7 | 持有 catalog/文件 | `holdCatalog` | `holdcatalog.go:18` |
| 8 | CORS/外页判定 | `corsGuard` | `cors.go:44` |
| 9 | 兜底 caller 识别 | `callerGuard` | `lan.go:535` |
| 10 | 只读 key 限制 | `readOnlyGuard` | `readonly.go:62` |
| 11 | 记录 caller、剥离 X-Magpie-Agent/Via | `withCaller` | `remote_magpie.go:53` |
| 12 | key 限额预留 | `keyLimited` | `key_limit.go:36` |
| 13 | 路由到 handler | `routeMux` → `s.handle(provider.Chat)` | `gateway.go:970`、`1790` |
| 14 | 读体 + 边界 | `requestBody`/`readRequestBody` | `request_bounds.go:38/42` |
| 15 | 取模型名 | `requestModel` | `gateway.go:8385` |
| 16 | 规整 Chat body | `thinkingEffort`/`liftMessageTools`/`shapeChatTools` | `gateway.go:1828-1837` |
| 17 | 进 serveAgent：会话/记录/工具循环/插件钩子 | `serveAgent` | `middleware.go:23` |
| 18 | JS 中间件 `onRequest` | `Run.Request` | `middleware.go:65`、`run.go:143` |
| 19 | 进 serve：规整/捕获/命名 | `serve` | `gateway.go:1980` |
| 20 | 模型解析 | `provider.Resolve`、`FindGroup` | `gateway.go:2215/2265` |
| 21 | 脱敏/hold/pause | `redactedHits`、`heldByGroup`、`PausedOut` | `gateway.go:2273-2336` |
| 22 | 生成候选 | `planPaused`/`planGroup` | `gateway.go:2528/2542`、`fallback.go:497/600` |
| 23 | 排序（策略库） | `weigh`/`weighRouted` | `routing.go:2027/2056` |
| 24 | 亲和 | `affine` | `gateway.go:2701` |
| 25 | 选路细化 | `pinTo`/`roomy`/`behindOverflow`/`probeOne` | `gateway.go:2797-2821` |
| 26 | 候选循环：车道/RPM | `lanes.take`/`rpms.wait` | `gateway.go:3427/3458` |
| 27 | 每次 try 前 `onProvider`/`onAccount` | `triedOn`/`accountSkipped` | `middleware.go:95/111` |
| 28 | 一次尝试 | `attempt` | `gateway.go:3480`、`4688` |
| 29 | 决定 relay/translate | `usable`/`OnMessages`/… | `gateway.go:4741-4884` |
| 30 | 原样中继 | `passthrough` | `gateway.go:5468` |
| 31 | 发送 | `forward`/`forwardOnce`/`p.Do` | `gateway.go:4955/5029/5316` |
| 32 | 传输/代理/HTTP2 | `upstreamTransport`/`netproxy` | `upstream_transport.go:34`、`netproxy.go:57` |
| 33 | 或插件 fetch | `pluginFetch`→`plugin.Fetch`→`host.js` | `provider/plugins.go:656`、`plugin/host.go:1831` |
| 34 | 或协议翻译（流） | `translate`→`streamTranslated` | `gateway.go:7709`、`continuation.go:398` |
| 35 | 或协议翻译（整包） | `translate`→`forwardTranslated`→`render` | `gateway.go:7773/7895` |
| 36 | 写回复（中继/SSE/tidy） | `passthrough` 结果段 | `gateway.go:6120-6570` |
| 37 | 回复命名 | `member` 头 | `member.go`、`gateway.go:4716` |
| 38 | usage 汇总写账本 | `usage.Record`+`appendUsage` | `gateway.go:4644-4663`、`lan.go:614` |
| 39 | 路由轨迹/事件流 | `trace.update`/`route_events` | `gateway.go:4616`、`route_events.go` |
| 40 | 会话粘性落盘 | `noteSticks`/`flushSticks` | `affinity.go:223/241` |
| 41 | Recent calls 环 | `s.record` | `gateway.go:608` |

（`Errors` 分支会在第 28/30/31/34 步之间反复，见 §9。）

---

## 9. 异常与超时

### 9.1 Rest（失败冷却）

- 一个 try 失败后 `s.restAfter(c, status, header, body)`（`routing.go:1193`）→ `restAfterMarked`（`1199`）按错误分类定 rest 的 `Why`（`failQuota`/`failCredit`/`failRate`/`failOther`/`failRefused`/`failCut`/`failProxy`/`failOffline` 等）、`By`（`cooldown`/`off`/…）、`Until`（供应商给的 `Retry-After` 或猜测）。
- 特殊：`failCut`（插件中途切断）不 rest、不计数（`1210`）；ZDR 拒绝、离线、OpenCode 裸 400、策略拒绝等各自 `By:"off"`（`1212-1243`）。
- `resting(c)`（`weighted.go:43-52`）/`restOf(id)`（`fallback.go:1378`）判断候选是否在冷却；`restLast` 把冷却的排到最后；`probeOne` 只试最早回来的一个。rest 记录进 `Try.Rest`（`gateway.go:4580-4581`）。

### 9.2 FirstToken（首内容超时换成员）

组/成员可在 `FirstToken` 秒内无首内容就切下一个成员：`firstToken := cmp.Or(c.firstToken, g.FirstToken)`，`isGroup && firstToken>0 && !last && streams` 时设 `hw.firstWait`（`gateway.go:3091-3095`）。`firstToken` 也可来自 `Via[].FirstToken`（`fallback.go:635-636`）。超时后由 `holdWriter`/`firstToken` 逻辑结束该 try，进入下一个候选（循环内 `3648` 附近）。

### 9.3 Patience（组内耐心重排）与同账号重试

- `patience`/`patienceFallback`：组用 `g.Waits()`，单 provider 用 `waitsOf(p.Patience, singlePatience)`（`gateway.go:2927-2937`，`patience.go:40`）。
- 失败分类：`busy(status, body)`（`patience.go:67-76`）= 408/500/502/503/504/529（非保护性 502、非 didNotStream）或非额度/非 token 墙的 429 → `noteBusy` 加入重试本轮（`78-86`）。
- `replan(list, round, waitFrom, fallback)`（`patience.go:119-168`）：暂停 `retryPause` 加倍（上限 `replanLongest=30s`）且不短于成员 `Retry-After`，把仍有 `Patience` 预算的成员按轮重排；`replanRounds=12` 为兜底上限。`pasMseAlive`（`180-202`）在暂停期间给流式客户端发 keepalive。
- 同账号的即时重试由 `passing(...)` 决定（`gateway.go:4092` 等），与 patience 是两套。

### 9.4 HTTP2 静默检测

`netproxy.HTTP2Health()`（`netproxy.go:57-59`）给 vendor transport 设置 `SendPingTimeout=20s`、`PingTimeout=10s`：连接 20s 无帧则 ping，10s 无应答则丢弃该 HTTP/2 连接，其上请求失败，组里转下一个（`netproxy.go:48-56` 注释说明 #2843）。大 body 走 `uploadTransport`（不 ping，改用 `WriteByteTimeout=uploadStall=1min`，`upstream_transport.go:86-90`），因 ping 会排在大 body 的 DATA 之后（#3909）；`drawsApart` 在 `lostPing`（错误含 `http2: client connection lost`，`142-144`）时把该请求重发到 uploads 并记住 `slow` 30 分钟（`110-138`）。

### 9.5 流式中断 / 重试

- 中继（passthrough）：上游流中途断开或提前结束（缺 finish_reason / message_stop / response.completed）时，补一个该协议的错误事件，Chat 情况再补 `data: [DONE]`，返回 `(status, failed, true)`（`gateway.go:6530-6565`）；错误文本由 `cutMidReply`（`6577-6579`："connection lost mid-reply"）或 `errEndedShort` 给出；`streamFailure`（`6583-…`）按协议生成错误事件体。
- 翻译（translate）：`translate` 非流式在 `!ended` 时也判 `errEndedShort`（`7878-7887`）；`streamTranslated` 在循环里对「被切断/被拒」重试与 handover（`continuation.go:495` 起）。
- 客户端写入失败/断连：见 §7.5（499）。
- 请求体重试的底层：`replayable`（`upstream_transport.go:158-212`）处理「新建连接在写完请求前被对端关闭」（`closedBeforeSent`，`230-234`；`unsentAgain` 250ms/1s），最终 `errClosedUnsent`（`239-251`）；这类错误不 rest 任何人（`failProxy`）。

### 9.6 请求体边界与超时

- `readRequestBody`（`request_bounds.go:42-70`）：默认上限 `defaultBodyLimit=512MB`（`28-31`），先 `bodyRoom.take` 等内存余量，再 `readBoundedRequestBody` 读并解码（`zstd`/`gzip`，`decodedBody` `97-120`），读超时 `defaultBodyTimeout=60s`；`idleBody`（`74-88`）每次读到字节就续 socket 读截止。
- `ResponseHeaderTimeout`：普通请求 10 分钟（`upstreamHeaderWait`，`upstream_transport.go:45`），绘图 0（由绘图超时控制）。
- `bodyIdleWait`（`plugin/host.go:1883-1895`）：插件 fetch 的回复两次字节之间最多 30 分钟无数据才放弃。
- 自请求（图像描述、搜索）的超时由 `sideDeadline`（`side_deadline.go:32-55`）限制（整体 + 每次写出后），结束时 `canceled` 报 `499` 或 `sideTimedOut`。

---

## 10. 证据（文件:行号）

| 主题 | 证据 |
|---|---|
| 监听与最外层 | `gateway.go:787`（`Handler: s.closesWhenHandedOff(s.Front())`）、`oneport.go:15-21`（`Front`，`19`） |
| Handler 链装配 | `gateway.go:895-901`（`900` 行链） |
| 路由表 | `gateway.go:906-1026`（chat 970、responses 972、messages 976、beta 1021、ollama 929） |
| `handle` | `gateway.go:1790-1886`（`serveAgent` 1884） |
| `gemini` | `gateway.go:1891-1938` |
| Ollama | `ollama_chat.go:66-121`（复用 `s.handle(provider.Chat)` 67、`ollamaChatBody` 104） |
| Responses over WS | `gateway.go:1031-1041`、`responses_ws.go:61-77` |
| 读体/边界 | `request_bounds.go:28-31`（512MB/60s）、`38-70`、`97-120` |
| 模型解析 | `gateway.go:2215`（`provider.Resolve`）、`2265`（`FindGroup`） |
| plan | `gateway.go:2528`（`planGroup`）、`2542`（`planPaused`）；`fallback.go:490`（`plan`）、`497-556`（`planPaused`）、`600-615`（`planGroup`） |
| weighRouted | `routing.go:2027-2033`（`weigh`）、`2056-2175`（`weighRouted`） |
| affine | `gateway.go:2701`；`affinity.go` |
| 候选循环 | `gateway.go:2976`（循环）、`3427`（lane）、`3458`（rpm）、`3480`（attempt） |
| attempt | `gateway.go:4688-4885`（claude 4720、relay 4843、translate 4884） |
| passthrough | `gateway.go:5468`（gathers 5479、forward 5768、结果段 6120-6570、gather.finish 6500-6511） |
| translate | `gateway.go:7709-7903`；`parseTranslated` 7697；`streamTranslated` `continuation.go:398` |
| forward/forwardOnce | `gateway.go:4955-5026`（forward）、`5029-5360`（forwardOnce，`p.Do` 5316） |
| 传输/HTTP2/代理 | `upstream_transport.go:34-41/66-77/110-144/158-251`；`netproxy.go:44-59/64-79` |
| 插件 fetch | `provider/plugins.go:132/571/656-740/940-978`；`plugin/host.go:1831-1878`、`1285` |
| X-Magpie 头 | `gateway.go:8487/8494`；`member.go:47-49`；`remote_magpie.go:28-29`；`key_limit.go:28`；`oneport.go:68`；`provider/plugins.go:36-71`；`provider/replyheaders.go:38` |
| 账号后端 | `claude_subscription.go:5767`、`6183`；`codex_backend.go:68` |
| SSE/编码 | `sse.go:15-68`；`gateway.go:8235-8255`（relayEvents）、`8257-8288`（encoder）、`8290-8308`（render）、`8108-8138`（decoder） |
| usage/收尾 | `gateway.go:4595-4668`；`lan.go:614-632`；`trace.go:48-...`（Route）、`gateway.go:2846`（beginFor）、`4616-4640`（update）；`route_events.go:32-100` |
| 会话粘性 | `affinity.go:223-260`（noteSticks/flushSticks）、`gateway.go:871`（flushSticks 调用） |
| Rest | `routing.go:1193-1274`；`fallback.go:1378`；`weighted.go:43-52` |
| Patience | `patience.go:40/67-86/119-168/180-202`；`gateway.go:2927-2940` |
| FirstToken | `gateway.go:3091-3095`；`fallback.go:635-636` |
| 断连/超时 | `side_deadline.go:32-73`；`gateway.go:3682/4104/4264/4363` |
| 中间件 JS | `middleware.go:23-79/95-126/146/153-172`；`gwmw.go:29-93/178-251`；`internal/middleware/run.go:143/173/253/398`、`account.go:42`、`middleware.go:197-206`；`docs/subsystems/gateway-middleware.md:22-51` |

---

## 11. 未确认项

1. **`internal/middleware` 里 `onEvent`/`onResponse` 与 `wrapWriter` 的完整字节级细节**（如 held-event 的时点、`Content-Encoding` 通过时的确切分支）只读了 `run.go` 关键行（143/173/253/398），未逐行通读 `run.go` 全部 ~460 行；如需精确到「第几个字节」需再读。
2. **`codex_backend.go`（`s.codexBackend`，`codex_backend.go:68`）内部完整链路**未逐段展开（该文件约 160KB）：本文只确认它是 `/codex` 与部分 Responses 请求的入口，并同样走 trace/plan/attempt；其 ChatGPT 后端（WebSocket、zstd、compaction）细节未在本报告核到行。
3. **`claude_subscription.go`（约 233KB）** 的 `serveClaudeSubscription`（5767）只确认了「在 attempt 中作为 Claude 账号分支」与 `relay`（6183）存在，未逐行追踪 Claude Code 子进程与流式 relay 的每个环节。
4. **`internal/plugin/host.js`（Bun 宿主）内部**只确认 `plugin.Fetch`（`host.go:1831`）通过行协议把请求交给 Bun 执行 loader 的 fetch；`host.js` 里 loader fetch 的具体实现（约 136KB）未逐行读，只按 `host.go` 与 `plugins.go` 的调用契约理解。
5. **`decodeOf`** 在 Go 侧只见 `internal/usage/usage.go:696` 的注释（与 `app.js` 的 `ledDecode` 同源）与 `trace.go:124` 的引用；其具体解码算法未逐行核（属 usage 包内部）。
6. **媒体类路由**（`images`/`embeddings`/`retrieve`/`speak`/`transcribe`/`videos`/`codexCatalog` 等）各自有独立 handler（`draw.go:518`、`embeddings.go:29`、`speech.go:24`、`transcribe.go:27` 等）并复用 `attempt`/`forward`，但不在本次「/v1/chat/completions 及同族」请求生命周期范围内，未展开。
7. **`Front()` 与 `Handler()` 各套一层 `socketEntry`/`dumpFirst`**：本文据 `oneport.go:19` 与 `gateway.go:900` 判定普通请求会各遇到两次；除 `GoroutinesPath` 与 WebSocket 外二者无副作用。若关心极端情况下 `dumpFirst` 对 `GoroutinesPath` 的重复判定，可再核对 `goroutinesRefused`（`goroutines.go` 内）。
