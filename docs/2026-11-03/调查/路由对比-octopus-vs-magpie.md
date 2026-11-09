# octopus 与 magpie 路由/页面机制对比

> 对象：`D:\CodeWorkspace\octopus`、`D:\CodeWorkspace\magpie`
> 方式：只读源码核对（行号见各节与文末证据），两仓库的逐页清单见同目录
> `octopus-页面与路由调查.md`、`magpie-页面与路由调查.md`。
> 本文件只做**横向对比**，重点回答“两边都有路由，有什么不同”。

---

## 0. 结论速览

| 维度 | octopus | magpie |
|---|---|---|
| 形态 | Web 管理后台（单进程、单端口） | 桌面应用（Wails v3）+ 本机网关 + `magpie web` 浏览器页 + CLI/TUI，另有一个独立官网站点 |
| 前端技术 | Next.js 16 App Router + React 19，**静态导出（output: export）**，需 pnpm 构建 | **手写 HTML/CSS/JS**（无框架、无打包器），`//go:embed assets` 直接内嵌 |
| 前端真实页面数 | 2 个 Next 路由（`/`、开发用 `/layout-check`） | 单页 index.html + 10 个视图（9 个主视图 + Settings） |
| 页面切换方式 | zustand `activeItem` + `ContentLoader`（keep-alive），**URL 不变** | `show(v)` 切 `#view-<id>` 的 hidden，**URL 用 `?view=` 记录** |
| 后端路由库 | **gin v1.11** + 自研“分组自注册”注册表 | **标准库 `net/http.ServeMux`**（Go 1.22+ method+pattern 语法），无第三方 |
| 路由规模 | 45 个分组、约 325 条 `NewRoute` | gateway 约 85 条 + GUI `/api/*` 约 263 条 |
| 管理 API 前缀 | `/api/v1/*`（JWT + RBAC） | `/api/*`（本机 JSON API，无账号体系） |
| 推理 API 前缀 | `/v1/*`（API Key 鉴权） | `/v1/*` **且** 无前缀别名双注册（如 `/chat/completions`） |
| 额外协议面 | `/node/v1`（节点隧道） | Ollama `/api/chat` 等、Gemini `/v1beta/models/{call...}`、MCP `/mcp/*`、Codex `/backend-api/codex`、`/me` 密钥页 |
| 前端静态托管 | `//go:embed static/out` + `StaticEmbed("/")` + SPA fallback | `//go:embed assets` + `FileServer` + 内容哈希版本化 |
| 部署端口 | 单端口（host:port） | 网关默认 `127.0.0.1:3425`，`magpie web`/app 页可另起端口（oneport 转发网关客户端路由） |

一句话：**octopus 是“一个 Web 后台 + 一套管理/推理 API”；magpie 是“桌面/本机网关 + 本地 GUI API + 多协议推理网关 + 官网站点”的组合体**，两者前端都是“单页 + 客户端切视图”，但 magpie 的 `?view=` 可寻址、且页面还包含独立 HTML 页（密钥页、插件页、官网站点）。

---

## 1. 前端“页面”与页面路由的差异

### 1.1 octopus：单入口 SPA，URL 不反映页面
- `web/src/app/` 下只有 `page.tsx`（→ `<AppContainer/>`）和开发用 `layout-check/page.tsx`；**无动态段、无路由组、无 `route.ts`、无 `middleware.ts`、无 `next/navigation` 调用**。
- 14 个“逻辑页面”（home / hub / channel / pool / plugins / group / model / analytics / log / notification / ops / apikey / setting / user）由 `nav-store.ts` 的 `DEFAULT_NAV_ORDER` 定义、`route/config.tsx` 的 `ROUTES` 注册，靠 `useNavStore().activeItem` + `<ContentLoader/>` 切换。
- `ContentLoader` 是 **keep-alive**：访问过的模块保持挂载，只切 `display:none`/`inert`（`content-loader.tsx:9-17`）。
- **URL 基本只有 `/`**；刷新后当前 tab 由 `localStorage['nav-storage']`（zustand persist）恢复；仅 OAuth 回跳/账号池深链用 query（`/pool?oauth=...&pool_id=N`，`app.tsx:185-218`、`PoolList.tsx:49-55`）。
- 还有 3 个带可排序子标签的模块：hub / analytics / ops（`sub-tab-store.ts:12-21`），同样是客户端 state，不产生 URL。

### 1.2 magpie：单页多视图 + `?view=`，另有真正的独立 HTML 页
- GUI 为一个 `assets/index.html` + `app.js`（约 1.3 MB）；`<main class="view" id="view-<id>">` 结构，`show(v)`（`app.js:23144`）通过 `hidden` 切换视图。
- 10 个视图：主视图 `agents / sessions / library / providers / routing / gateway / usage / masking / plugins`，加上 `settings`（内含 9 个设置 tab：general / usage(Allowances) / network / models / privacy / otel / sync / plugins / about）。
- **URL 可寻址**：`syncURL()`（`app.js:23213`）把 `?view=<id>`（以及 providers 的 `?edit=`、settings 的 `?tab=`）写进地址栏；`localStorage['magpie.lastView']` 记录上次视图。
- 除单页外，magpie 还有**真正的独立页面/站点**：
  - 网关密钥页 `GET /me`（`gateway/key_page.go` + 内嵌 `keypage.html`，数据在 `/v1/magpie/me`）；
  - 插件页面 `/_magpie/plugin/<token>/<path>`（`gui/pluginpage.go`，带一次运行 token、沙箱隔离、`_magpie/bridge.js`）与插件面板；
  - 浏览器扩展 UI `internal/browserext/ext/popup.html`（扩展内页面，不是服务器路由）；
  - 官网营销站 `site/`（Cloudflare Worker `worker.js` + `public/` 静态 docs，`/docs`、`/docs/zh`、`/docs/ja` 多语言，`/download/*`、`/go/*`、`/api/latest` 等）。

### 1.3 差异要点
1. **octopus 页面不可用 URL 定位**（刷新靠 localStorage），magpie 用 `?view=` 可定位/可分享。
2. **magpie 的“页面”不止 SPA**：密钥页、插件页、官网站点都是独立 HTML/静态资源，octopus 没有这类对外页面。
3. 两者都**不是文件系统多页路由**，但 octopus 是 Next 静态导出“假单页”，magpie 是纯手写“真单页”。
4. 前端构建：octopus 需要 `pnpm build`（Next）产出 `web/out` 再嵌入；magpie 前端是手写 JS 直接 `//go:embed assets`，**无构建步骤**（测试用 `node --test` + 浏览器 e2e）。

---

## 2. 后端路由框架的差异

### 2.1 octopus：gin + 自研分组注册表
- `server.go:38 r := gin.New()`；`server.go:21` 匿名导入 `handlers` 包触发各文件 `init()`。
- `router/router.go` 维护全局 `registeredRouters`；每个 handler 用 `router.NewGroupRouter("/api/v1/xxx").Use(...).AddRoute(router.NewRoute("/path", method).Handle(fn))` 声明；`server.go:88 router.RegisterAll(r)` 一次性挂到 gin，**消费后置 nil（只能调一次）**。
- 路径参数用 gin 风格 `:id`、`:site_id`、`:aid`；插件 RPC 用 `/*path` catch-all（`plugin.go:42-44`）。
- 规模：45 个分组、约 325 条 `router.NewRoute`（非测试）。
- 分组前缀集中在三类：`/api/v1/*`（管理）、`/v1/*`（推理）、`/node/v1`（节点隧道）。

### 2.2 magpie：标准库 ServeMux（Go 1.22 模式）+ 两套 mux
- **无第三方 router**；用 `http.NewServeMux()` 配 Go 1.22+ 的 `"METHOD /pattern"` 语法。
- 两套独立 mux：
  - **网关 mux**（`internal/gateway/gateway.go:906 routes()`，约 85 条）：`"GET /{$}"`、`"GET /v1/models/{id...}"`、`"POST /v1/chat/completions"`、`"/mcp/{name}"`、`"GET /v1beta/models"`、`"POST /v1beta/models/{call...}"` 等；根路径 `"/"` 兜底返回可用 API 列表的 404。
  - **GUI mux**（`internal/gui/api.go:968 Handler()`，约 263 条 `/api/*`）：`mux.Handle("/", FileServer(staticFS))` 托管页面 + `devRoutes` + `pluginPageRoutes` + 大量 `/api/*` JSON 路由（按域分布：settings 53、library 26、usage 21、sessions 20、plugins 17、agents 12、gateway 10、masking 8 等）。
- 路径参数用 Go 1.22 的 `{id}` / `{id...}` / `{name}`；分派还靠自定义的 `routeMux`（包 `ServeMux`，为重载判定），`"GET /v1/models/{id...}"` 为 catch-all。
- `magpie web` 页面端口通过 `oneport.go` 的 `ClientPath` 把网关的客户端路由（`/v1`、`/v1beta`、`/mcp`、`/chat/completions` 等）转交给网关，实现“一个端口同时服务页面与 Agent”。

### 2.3 差异要点
| 点 | octopus | magpie |
|---|---|---|
| 路由库 | gin（第三方） | `net/http.ServeMux`（标准库，无依赖） |
| 注册方式 | 各 handler `init()` 自注册到全局表，`RegisterAll` 一次挂载 | 在 `routes()` / `Handler()` 里集中逐条 `HandleFunc` |
| 服务器数量 | 1 个 gin Engine | 2 套 mux（gateway、gui）+ 官网 Cloudflare Worker |
| 路径参数 | gin `:id`、`/*path` | Go 1.22 `{id}`、`{id...}`、`{name}` |
| 一次性约束 | `RegisterAll` 只能调一次（消费置 nil） | 无此约束，mux 构建后直接 `ServeHTTP` |

---

## 3. API 命名/前缀的差异（最直观的“路由不同”）

| 用途 | octopus | magpie |
|---|---|---|
| 管理/桌面 API | `/api/v1/<domain>/<action>`（45 组，JWT+RBAC） | `/api/<domain>/...`（GUI 本地 JSON，无账号体系，约 263 条） |
| 推理（OpenAI 兼容） | `/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/embeddings`、`/v1/models` | 同名 **且** 无前缀别名：`/chat/completions`、`/responses`、`/messages`…（双注册，见 `gateway.go:970-1003`） |
| 多媒体 | `/v1/images/*`、`/v1/audio/*`、`/v1/videos/generations`、`/v1/music/generations`、`/v1/search`、`/v1/rerank` | `/v1/images/*`、`/v1/audio/*`、`/v1/videos`、`/v1/embeddings`、`/v1/rerank`（无 `/music`、`/search` 同名实现方式不同） |
| 其他协议 | 仅 OpenAI 系（chat/responses/messages/embeddings） | 另加 **Ollama** `/api/chat`、`/api/generate`、`/api/tags`、`/api/show`、`/api/version`；**Gemini** `/v1beta/models`、`/v1beta/models/{call...}`；**Codex** `/backend-api/codex`、`/v1/codex/models`；`/alpha/search` |
| MCP | 无（插件以 `/api/v1/plugins/:id/rpc` 暴露） | 一整套 `/mcp/*`：`/mcp/{name}`、`/mcp/run/{name}`、`/mcp/magpie/{web-search,vision,image,browser,tools,hub}`、`/v1/magpie/mcp` |
| 自省/运维 | `/api/v1/ops/*`、`/api/v1/stats/*`、`/api/v1/analytics/*` | `/v1/magpie/quotas|usage|route|trace|concurrency|limit|keys|claude-runs|heap|cpu|goroutines|restart|omarchy` |
| 密钥/密钥页 | 无独立页 | `GET /me` 密钥页 + `GET /v1/magpie/me` 数据；`POST/GET /v1/magpie/keys`（签发短期 key）、`/v1/magpie/keys/oidc` |
| 节点隧道 | `/node/v1/connect` | 无（magpie 用 remote-magpie / gateway 共享替代） |

要点：
1. **octopus 严格区分 `/api/v1` 与 `/v1`**（管理 vs 推理）；magpie 没有 `/api/v1`，其管理面是 `/api/*`，推理面是 `/v1/*` **加** 无前缀别名。
2. **magpie 覆盖的协议面远多于 octopus**：Ollama、Gemini、Codex、MCP、密钥页等，是为了“让各种 Agent 直接把 magpie 当它们的厂商端点”。
3. octopus 的 `/api/v1` 域划分很细（channels/pool/site/site-channel/plan-provider/…），是典型的多供应商管理后台；magpie 的 `/api/*` 域按 GUI 视图划分（settings/library/usage/sessions/plugins/agents/…），是“本机桌面 App 的 UI 后端”。

---

## 4. 鉴权与中间件链的差异

- **octopus**（`server.go:38-90`，全局顺序）：`CustomRecovery` → （debug）`Logger` → `SecurityHeaders` → `Cors` → `AuditManagementWrite` → 静态资源 → `RegisterAll`。路由级再叠加 `Auth()`(JWT)、`APIKeyAuth()`、`RequirePermission(PermXxx)`(RBAC)、`RequireJSON()`、`LoginRateLimit()`。**有完整账号/角色体系**，且**审计白名单测试**强约束（新增 `/api/v1` 写接口必须同步白名单，否则测试失败）。
- **magpie**（`gateway.go:895-901`）：`socketEntry(dumpFirst(counted(headerLogged(holdCatalog(corsGuard(callerGuard(readOnlyGuard(mux, withCaller(keyLimited(mux)))))))))`；`Front()` 再包一层 `lanGuard`（`oneport.go:15-21`）。**没有账号体系**，鉴权靠：是否本机(loopback)、gateway key、只读 key、局域网共享开关、以及 `ClientPath` 的页面端口标记 `X-Magpie-Page-Port`。GUI `/api/*` 则靠 `magpie web` 的一次运行 key（cookie/链接）与 `isWeb`/`ownWeb` 区分浏览器与 App 窗口。

要点：octopus 是**多用户 RBAC 后台**；magpie 是**单机/小团队网关**，用“本机可信 + 密钥 + 只读 key”代替登录体系。

---

## 5. 静态资源与前端托管的差异

- **octopus**：`next build`（`output: export`）→ `web/out` → 构建脚本拷到 `static/out` → `//go:embed all:out all:out/**`（`static/static.go`）→ 非 debug 用 `middleware.StaticEmbed("/", StaticFS)`。静态中间件（`middleware/static.go:22-78`）对 `/api`、`/v1`、`/node/` 前缀放行，其余未命中且期望 HTML 时**回退 `index.html`（SPA fallback）**；`assetPrefix: "./"` 支持子路径部署；带 `sw.js` + `manifest.json` 的 PWA。
- **magpie**：`//go:embed assets`（`gui/api.go:978` 的 `FileServer`），`revalidated`/`versionedPage` 用内容哈希给脚本加 `?v=`，命中当前哈希则 `immutable`，否则按 ETag 重新验证；`devPage` 为开发模式代理；`webHandler` 对浏览器 gzip；`webGuard` 放行 `manifest.json`/`icons` 等首页屏文件；`magpie web` 与 App 共用同一 Handler（App 走进程内读取、不压缩）。官网 `site/` 则是 Cloudflare Worker + 静态 `public/`。

---

## 6. 关键差异总结（面试式回答）

1. **前端**：octopus = Next.js 静态导出单页，14 个逻辑页靠 zustand 切换，URL 不反映页面；magpie = 手写 JS 单页，10 个视图用 `?view=` 记录，另有密钥页/插件页/官网等真页面。
2. **后端**：octopus = gin + 自研分组自注册（`/api/v1`、`/v1`、`/node/v1`）；magpie = 标准库 ServeMux（Go 1.22 模式）+ 网关/GUI 两套 mux。
3. **命名**：octopus 管理面固定 `/api/v1/*`；magpie 管理面是 `/api/*`，推理面 `/v1/*` 且有**无前缀别名双注册**，并兼容 Ollama/Gemini/Codex/MCP。
4. **鉴权**：octopus 有 JWT+RBAC 多用户体系与审计强约束；magpie 无账号体系，靠本机/密钥/只读 key/局域网开关。
5. **部署**：octopus 单端口同源托管（Go 直接服前端）；magpie 网关默认 `127.0.0.1:3425`，页面可另起端口并 oneport 转发网关路由。

---

## 7. 证据（关键文件:行号）

**octopus**
- `web/src/app/page.tsx:1-5`（唯一业务页）、`web/src/app/layout-check/page.tsx`（开发页）
- `web/next.config.ts:26-46`（`output:"export"`、`assetPrefix:"./"`）
- `web/src/route/config.tsx:11,35-55`、`web/src/route/content-loader.tsx:9-17`、`web/src/components/modules/navbar/nav-store.ts:20-35`
- `web/src/components/app.tsx:185-218`（OAuth 回跳）、`web/src/components/modules/pool/PoolList.tsx:49-55`
- `internal/server/server.go:38-90`（gin、中间件、静态、RegisterAll）
- `internal/server/router/router.go:22-39,103-126`（分组自注册、单次消费）
- `internal/server/middleware/static.go:22-78`（SPA fallback、`/api|/v1|/node/` 放行）
- `internal/server/handlers/relay.go:14-35`、`media.go:12-62`、`model.go:132-137`（`/v1` 端点）
- `static/static.go:8-22`（`//go:embed all:out`）

**magpie**
- `internal/gateway/gateway.go:895-1025`（中间件链、`routes()`、全部网关路由、根兜底）
- `internal/gateway/oneport.go:15-46`（`Front`、`ClientPath`、oneport）
- `internal/gateway/key_page.go:20-39`（`/me` 密钥页）、`issue_keys.go:31`（`/v1/magpie/keys`）
- `internal/gui/api.go:968-1047`（GUI Handler、`FileServer`、`/boot.js`）
- `internal/gui/pluginpage.go:410-464`（插件页面路由）
- `internal/gui/assets/index.html:67-164`（主视图 nav、settings tabs、页面容器）、`app.js:23144-23222`（`show(v)`、`syncURL`）
- `internal/gui/web.go:27-49`（`magpie web`）、`gatewaymode.go:11-25`（gateway 模式）
- `site/worker.js:1-176`（官网 Worker 路由分发）、`site/public/docs/*`（多语言 docs）
- `docs/subsystems/gui-shell.md`、`gateway-middleware.md`、`README.md`（子系统设计参考）
