# magpie 页面与路由调查

> 调查对象：`D:\CodeWorkspace\magpie`（Go 桌面应用/本机网关，含 `internal/gui`、`internal/gateway`、`site/` 等）
> 调查方式：只读源码，逐文件阅读；忽略 `.git`、`node_modules`。`internal/gui/assets` 是内嵌的前端产物/手写资源（非构建产物，见 §1.1）。结论均标注文件路径（必要时带行号）；拿不准处写“未确认”。

---

## 1. 概览（技术栈、目录结构、版本）

### 1.1 技术栈与版本

| 层 | 技术 | 版本 / 依据 |
|---|---|---|
| 语言 | Go | `go 1.26.8`（`go.mod:3`） |
| 桌面壳 | **Wails v3** | `github.com/wailsapp/wails/v3 v3.0.0-beta.24`（`go.mod`） |
| 后端路由 | **标准库 `net/http` ServeMux** | Go 1.22+ 的 `"METHOD /pattern"` 语法（`internal/gateway/gateway.go:907`、`internal/gui/api.go:977`） |
| 前端 | **手写 HTML/CSS/JS，无框架、无打包器** | `internal/gui/assets/index.html`(45 KB) + `app.js`(≈1.3 MB) + `i18n.js`(≈2.1 MB)；`//go:embed assets` |
| 站点（官网） | Cloudflare Worker + 静态 `public/` | `site/worker.js`、`site/wrangler.jsonc`、`site/public/**` |
| 终端界面 | TUI | `internal/tui` |
| 数据库 | SQLite | `internal/sqlitedb`（依据包名，未逐文件核对） |

### 1.2 顶层目录结构（与路由/页面相关部分）

```
magpie/
├── main.go
├── internal/
│   ├── gateway/          # 本机推理网关：OpenAI/Anthropic/Ollama/Gemini/Codex/MCP 等协议 + /v1/magpie/* 自省
│   │   ├── gateway.go     # Handler()：中间件链 + routes() 全量路由（≈85 条）
│   │   ├── key_page.go    # GET /me 密钥页（内嵌 keypage.html）
│   │   ├── oneport.go     # magpie web 页面端口转交网关客户端路由（ClientPath）
│   │   └── ...            # chat.go / responses.go / anthropic.go / codex_backend.go / search_mcp.go / tools_mcp.go ...
│   ├── gui/               # 桌面 App 与 `magpie web` 的页面与本地 API
│   │   ├── api.go         # Handler()：FileServer(assets) + /api/*（≈263 条）
│   │   ├── pluginpage.go  # 插件页面 /_magpie/plugin/<token>/... 与面板
│   │   ├── app.go/web.go  # Wails 窗口 & `magpie web` 浏览器模式
│   │   ├── gatewaymode.go # 网关模式（隐藏部分页面）
│   │   └── assets/        # index.html / app.js / i18n.js / plugins/{gateway,library,otel,redact,routing,search,sessions,usage}
│   ├── browserext/        # 浏览器扩展（ext/popup.html、background.js…）
│   ├── plugin/ pluginrt/  # 插件运行时
│   ├── provider/ agent/   # 供应商与各 Agent 配置
│   └── ...
├── site/                 # 官网 usemagpie.ai：Cloudflare Worker + 静态 docs（en/zh/ja）
└── docs/subsystems/      # 子系统设计参考（gui-shell.md、gateway-middleware.md 等）
```

---

## 2. 页面清单

magpie 的“页面”分四类：**桌面 GUI 单页内的视图**、**网关自渲染 HTML 页**、**插件页面**、**官网静态站**。

### 2.1 桌面 GUI 视图（`internal/gui/assets/index.html`）

`index.html` 是单页，`.view` 容器按 id 隐藏/显示（`app.js:23190` 的 `for (const id of ["agents","providers","plugins","settings",...]) $("#view-"+id).hidden = v !== id`）。主导航（`index.html:67-81`，`#nav`）与页面容器：

| 视图 id | 来源 | 说明 |
|---|---|---|
| `agents` | `index.html:114` `#view-agents` | Agent 列表 + Profiles + 配额/路由/用量面板 + 插件槽 |
| `sessions` | 由内置插件 `sessions` 提供 | 各 Agent 的项目会话文件夹与会话 |
| `library` | 由内置插件 `library` 提供 | 写入各 Agent 的指令/MCP/技能库 |
| `providers` | `index.html:127` `#view-providers` | 供应商、密钥、订阅账号、发现导入 |
| `routing` | 由内置插件 `routing` 提供 | 路由分组、账号/密钥、请求回放 |
| `gateway` | 由内置插件 `gateway` 提供 | 网关状态、地址、近期调用 |
| `usage` | 由内置插件 `usage` 提供 | 用量、Allowances、Context |
| `masking` | 由内置插件 `redact` 提供 | 隐私脱敏设置与记录 |
| `plugins` | `index.html:152` `#view-plugins` | 插件（内置/市场/已安装） |
| `settings` | `index.html:154` `#view-settings` | 设置页，内含 9 个 tab（见 §2.2） |

> 视图切换：`show(v)`（`app.js:23144`）判定 gateway 模式/内置插件开关后切 `hidden`；`syncURL()`（`app.js:23213`）把 `?view=<id>`（providers 还有 `?edit=`、settings 有 `?tab=`）写入地址栏；`localStorage['magpie.lastView']` 记录上次视图（`app.js:23174`）。

### 2.2 Settings 页的 9 个 tab（`index.html:156-164`）

`general`（General）· `usage`（Allowances）· `network`（Network and sharing）· `models`（Models）· `privacy`（Privacy）· `otel`（Observability）· `sync`（Sync and backup）· `plugins`（Plugins）· `about`（About）。
另有面板/托盘里的一组 tab（`#ptabs`，`index.html:107-112`）：agents / usage / stats / routing；以及 Warm-up 的 tab（`index.html:336-342`）。这些都属 GUI 内部 tab，不是 HTTP 路由。

### 2.3 网关自渲染 HTML 页

| URL | 来源 | 用途 |
|---|---|---|
| `GET /me` | `internal/gateway/key_page.go:20` + 内嵌 `keypage.html` | 网关密钥持有者用浏览器打开：输入 key，显示该 key 的名称/限额/用量/模型与各 Agent 接入配置（数据在 `/v1/magpie/me`，`key_page.go:23`） |
| `/_magpie/sign-in`（POST）+ 401 页 | `internal/gui/web.go:283,298`（`webSignInPath`、`webTurnedAway`） | `magpie web` 未带 key/cookie 时的“粘贴 key 登录”表单页 |
| OAuth 登录结果页（一次性 loopback 端口，成功/失败卡片） | `internal/provider/signin.go:402,1002`（`signInPage`，内嵌 `magpie.svg`） | 各厂商（Codex、ChatGPT、Google 等）OAuth 回调落地页 |
| MCP 服务器 OAuth 结果页（一次性 loopback `/callback`） | `internal/mcpauth/mcpauth.go:893,1036`（`page()`） | Library 中给 MCP 服务器做 OAuth 登录的回调结果页 |

### 2.4 插件页面/面板（`internal/gui/pluginpage.go`）

| URL | 来源 | 用途 |
|---|---|---|
| `/_magpie/plugin/<token>/<file>` | `pluginpage.go:42,414-415` | 社区插件的自有页面（沙箱 iframe，一次运行 token 鉴权） |
| `_magpie/bridge.js` | `pluginpage.go:47,49` | 页面↔窗口的消息桥 |
| `GET /api/plugin-pages` / `GET /api/plugin-panels` / `POST /api/plugin-pages/read` | `pluginpage.go:416-464` | 页面/面板注册与受限读取 |

### 2.5 浏览器扩展页面

`internal/browserext/ext/popup.html`（及 `background.js`、`popup.js`、`cdp.js`）——扩展自身的弹窗 UI，不是服务器路由（`BrowserExtensionPath=/v1/magpie/browser/extension` 由网关提供给扩展/客户端）。

### 2.6 官网静态站（`site/`，Cloudflare Worker）

`site/worker.js` 按路径分发（`worker.js:80-176`）：

| 路径 | 说明 |
|---|---|
| `/api/latest`、`/api/latest?channel=stable` | 最新版本/稳定通道信息 |
| `/api/notes` | 版本变更说明（`?lang=zh`） |
| `/api/partners` | 合作方列表 |
| `/go/<id>/keys|site[/<region>]`、`/go/<id>/home|readme` | 合作方/赞助商跳转并计数 |
| `/api/icon?url=` | 代理抓取远端图标 |
| `/r/...` | PostHog 代理 |
| `/download`、`/download/mac-arm64|mac-intel|windows|windows-arm64|linux|linux-arm64`、`/download/<file>` | 下载 |
| `/docs`、`/docs/zh`、`/docs/ja` → `/docs[/lang]/start` | 使用指南（多语言） |
| `/<lang>/`（`/zh/`、`/ja/`、`/de/`） | 首页多语言（`i18n.js` 的 `LANGS`） |
| `/` | 按浏览器语言重定向或英文首页 |
| `/install.sh` | 安装脚本 |

静态文档在 `site/public/docs/{*.html, zh/, ja/}`（start / import / integrate / intent / masking / mcp-router / plugins / privacy / remote / docker / contributors 等），`site/public/{index.html, import.html}` 为首页/导入页。

---

## 3. HTTP 路由注册与分组

### 3.1 两套 mux

1. **网关 mux**：`internal/gateway/gateway.go:906 routes()`，`mux := &routeMux{ServeMux: http.NewServeMux()}`，逐条 `mux.HandleFunc("METHOD /pattern", handler)`，约 **85 条**；`Handler()`（`:895-901`）把它包进中间件链。
2. **GUI mux**：`internal/gui/api.go:968 Handler()`，`mux := http.NewServeMux()`；`mux.Handle("/", FileServer(staticFS))` 托管页面，然后 `devRoutes(mux)`、`pluginPageRoutes(mux, isWeb(w))` 以及大量 `mux.HandleFunc("…/api/…")`，**约 263 条 `/api/*`**（另含 `/boot.js`、`/dev` 等）。

> 两套 mux 相互独立：网关默认 `127.0.0.1:3425`（`gateway.go:50 DefaultAddr`，`Addr()` 见 `:280-282`）；GUI 页面由 Wails 内嵌窗口或 `magpie web` 的端口服务。`magpie web` 的页面端口通过 `oneport.go` 的 `ClientPath`（`:43`）把网关的客户端路由转交给网关，实现“一个端口同时服务页面和 Agent”。

### 3.2 网关路由（`gateway.go:908-1024`，按功能归类）

| 类别 | 代表路由 | 说明 |
|---|---|---|
| 信息/根 | `GET /{$}`、`GET /v1`、`GET /v1/{$}`、`GET /api/hello`、`GET /api/version` | 网关信息；`/api/hello` 供 Claude Desktop 探测；根 `"/"` 兜底返回可用 API 列表的 404（`:1022-1024`） |
| OpenAI 兼容 | `POST /v1/chat/completions`、`/chat/completions`、`POST /v1/responses`、`/responses`、`GET /v1/responses`（WebSocket）、`POST /v1/messages`、`/messages`、`/v1/messages/count_tokens`、`POST /v1/embeddings`、`/embeddings`、`POST /v1/rerank`、`/rerank` | 双注册（有/无 `/v1`），核心透传 |
| 模型 | `GET /v1/models`、`/models`、`GET /v1/models/{id...}`、`GET /muse-code/models`、`GET /v1/codex/models` | 模型列表/详情（catch-all） |
| 多媒体 | `POST /v1/images/generations|edits`、`/v1/audio/transcriptions|translations|speech`、`POST /v1/videos`、`GET /v1/videos/{id}[/content]` | 图像/语音/视频 |
| Ollama 兼容 | `GET /api/tags`、`POST /api/show`、`POST /api/chat`、`POST /api/generate` | 供只认 Ollama 的客户端 |
| Gemini | `GET /v1beta/models`、`POST /v1beta/models/{call...}` | Gemini 协议 |
| Codex | `CodexPath="/backend-api/codex"` + `/`、`GET /v1/codex/models` | Codex 后端 |
| MCP | `"/mcp/{name}"`、`"/mcp/run/{name}"`、`/mcp/magpie/web-search|vision|image|browser|tools|hub[/*]`、`/_magpie/claude-mcp/{token}` | MCP 路由/代理 |
| 自省与运维 | `GET /v1/magpie/quotas[|/history]`、`/usage`、`/usage/requests`、`/route`、`/trace`、`/route-events[|/snapshot]`、`/concurrency`、`/limit`、`/mcp`、`/keys`(POST/GET)、`/keys/oidc`、`/me`、`/claude-runs`、`/heap`、`/cpu`、`/goroutines`、`/restart`、`/omarchy`、`/browser[|/extension]` | 配额/用量/路由轨迹/密钥签发/诊断/浏览器扩展 |
| 其他 | `POST /v1/systemone`、`POST /v1/alpha/search`（`/alpha/search`）、`/chat/completions`… | SystemOne、搜索等 |

### 3.3 GUI `/api/*` 路由（`internal/gui/api.go` 等，约 263 条，按域分布）

统计 `mux.HandleFunc` 的 `/api/<domain>` 首段（非测试）：

| 域 | 条数 | 域 | 条数 |
|---|---|---|---|
| `settings` | 53 | `plugins` | 17 |
| `library` | 26 | `agents` | 12 |
| `usage` | 21 | `gateway` | 10 |
| `sessions` | 20 | `masking` | 8 |
| `signin` | 6 | 其他（icons/update/whatsnew/tune/importapps/omarchy/plugin-pages/caller-keys/archive/context/…） | 各 2–4 |

代表端点：`GET/POST /api/state`、`/api/set`、`POST /api/settings`（及大量 `/api/settings/*`）、`/api/agents/{action}/{id}`、`/api/agents/restart/{id}`、`/api/providers…`、`/api/usage/quotas`、`/api/masking/*` 等。

### 3.4 中间件与鉴权

- **网关**（`gateway.go:895-901`）：
  `socketEntry(dumpFirst(counted(headerLogged(holdCatalog(corsGuard(callerGuard(readOnlyGuard(mux, withCaller(keyLimited(mux))))))))))`
  `Front()`（`oneport.go:15-21`）再包 `lanGuard`（局域网/共享/来源校验）。鉴权靠：是否 loopback、gateway key、只读 key、局域网共享开关、`X-Magpie-Page-Port` 标记；**无登录账号体系**。
- **GUI**（`api.go`）：一个进程内的本地 JSON API；`magpie web` 用一次运行的 key（链接/cookie）与 `isWeb`/`ownWeb` 区分浏览器与 App 窗口；`held` 让一次请求的多次读共享一次 catalog 构建（`gui-shell.md`）。

### 3.5 网关模式（`gatewaymode.go:11-25`）

`magpie web --gateway`（或机器上没有任何 Agent 且无 GUI）时，页面只显示 Providers / Gateway / Routing / Usage / Sessions / Plugins / Settings，隐藏 Agents、Library 及桌面项；`show()` 会把被隐藏视图的地址重定向到 Providers（`app.js:23148`）。

---

## 4. 端点分类表（网关 → 上游协议）

| 客户端协议 | 网关入口 | 上游协议/说明 |
|---|---|---|
| OpenAI Chat | `POST /v1/chat/completions`（及无 `/v1`） | 透传/转换到各供应商 |
| OpenAI Responses | `POST /v1/responses`、WebSocket `GET /v1/responses` | 支持 SSE 与 WebSocket |
| Anthropic Messages | `POST /v1/messages`、`/v1/messages/count_tokens` | Claude Code 等 |
| Gemini | `POST /v1beta/models/{call...}` | Gemini 协议 |
| Ollama | `POST /api/chat`、`/api/generate`、`GET /api/tags`、`POST /api/show` | 只认 Ollama 的客户端 |
| Codex | `CodexPath + "/"`、`/v1/codex/models` | Codex 后端 |
| 多媒体 | 图像/编辑、语音转写/翻译/合成、视频 | 各家图像/语音/视频模型 |
| 自省/管理 | `/v1/magpie/*` | 配额、用量、路由轨迹、密钥签发、诊断 |

（具体转发/转换逻辑在 `internal/gateway/chat.go`、`responses.go`、`anthropic.go`、`codex_backend.go`、`search.go`、`video.go` 等，本报告不逐一切入。）

---

## 5. 插件页面与静态站点路由

- **插件页面机制**（`pluginpage.go:29-49,410-464`）：打包进插件 `package.json` 的 `magpie.page` / `magpie.panels`，由 `/_magpie/plugin/<token>/…` 以沙箱 iframe 提供；token 由本次运行随机 `pageSecret` 签名，跨进程失效；页面只能读自己目录文件、不能访问 `/api`，需要 magpie 数据时通过 `POST /api/plugin-pages/read`（只允许 package 声明的读取）。面板按 slot 挂到 `agents` 等内置页（`index.html:124` 的 `data-slot`）。
- **内置插件页面**：`internal/gui/assets/plugins/{gateway,library,otel,redact,routing,search,sessions,usage}` 由窗口内核（`kernel.js`）加载，注册为 GUI 的视图/设置页。
- **官网站点**：`site/` 是独立 Cloudflare Worker（`worker.js`）+ 静态 `public/`，不在 Go 进程内；路由见 §2.6。`wrangler.jsonc` 定义部署。

---

## 6. 路由特点与注意点

1. **双 mux、双端口**：网关（默认 `127.0.0.1:3425`）与 GUI 页面各自一套 `http.ServeMux`；`magpie web` 可用 oneport 在同一端口同时服务页面与网关客户端路由（`ClientPath` 只转 `/v1`、`/v1beta`、`/mcp`、`/chat/completions`、`/me` 等，**不转 `/api/*` 与 `/_magpie/*`**）。
2. **标准库路由，Go 1.22 语法**：`"GET /v1/models/{id...}"`（catch-all）、`"GET /{$}"`（仅根）、`"/mcp/{name}"`；无第三方 router。
3. **推理端点双注册**：几乎所有 OpenAI 兼容端点同时挂 `/v1/...` 与无前缀 `/...`，方便把不同客户端指向同一地址。
4. **多协议面**：OpenAI Chat/Responses、Anthropic Messages、Gemini、Ollama、Codex、MCP 都在同一网关；这是 magpie 与 octopus 最大的不同（octopus 只有 OpenAI 系 + 媒体）。
5. **页面 URL 可寻址**：GUI 用 `?view=`/`?edit=`/`?tab=`（`syncURL`），与 octopus 的“URL 不变”不同。
6. **插件页面沙箱化**：`/_magpie/plugin/<token>/…` 只服务插件目录、独立 CSP、无网络、不能访问 `/api`。
7. **无账号体系**：网关/GUI 用 loopback + gateway key + 只读 key + 局域网共享开关来授权，而非用户登录。
8. **网关模式**：`magpie web --gateway` 隐藏桌面相关页面，只留网关所需。

---

## 7. 证据（关键文件路径:行号 清单）

**前端 · GUI 页面与视图**
- `internal/gui/assets/index.html:67-81`（主导航视图）、`:107-112`（面板 tab）、`:114-164`（各 view 容器与 settings tabs）、`:336-342`（Warm-up tabs）
- `internal/gui/assets/app.js:23144-23222`（`show(v)` 视图切换、`syncURL()`）
- `internal/gui/assets/plugins/{gateway,library,otel,redact,routing,search,sessions,usage}/`（内置插件页面）
- `internal/gui/assets/{app.css,app.js,i18n.js,boot.js,kernel.js}`（无框架手写前端）
- `internal/gui/web.go:27-49`（`magpie web` 浏览器模式）、`internal/gui/gatewaymode.go:11-25`（网关模式）
- `docs/subsystems/gui-shell.md:10-28`（GUI 子系统职责总表）

**后端 · 网关路由**
- `internal/gateway/gateway.go:895-901`（中间件链 `Handler`）、`:906-1025`（`routes()` 全量路由、根兜底）
- `internal/gateway/oneport.go:15-46`（`Front`、`clientPrefixes`、`ClientPath`）
- `internal/gateway/key_page.go:20-39`（`/me` 密钥页）、`issue_keys.go:31`（`/v1/magpie/keys`）、`route_events.go:33-34`（路由轨迹）
- `internal/gateway/{chat.go,responses.go,anthropic.go,codex_backend.go,search_mcp.go,tools_mcp.go}`（各协议处理）
- 路径常量：`browser.go:36-42`、`claude_runs.go:25`、`codex_backend.go:41`、`codex_catalog.go:23`、`cpu.go:27`、`goroutines.go:32`、`heap.go:24`、`hub_mcp.go:33,38`、`image_mcp.go:43`、`own_mcp.go:24`、`restart.go:21`、`search_mcp.go:30`、`tools_mcp.go:34`、`vision_mcp.go:28`

**后端 · GUI API 与插件页**
- `internal/gui/api.go:968-1047`（GUI `Handler`、`FileServer(assets)`、`/boot.js`）
- `internal/gui/pluginpage.go:29-49,410-464`（插件页面/面板路由与受限读取）
- `internal/gui/{app.go,providers.go,settings.go,sessions.go,usage.go,plugins.go,groups.go}`（各域 `/api/*`）

**站点**
- `site/worker.js:1-176`（Worker 路径分发）、`site/wrangler.jsonc`、`site/i18n.js`（LANGS）
- `site/public/{index.html,import.html,docs/*.html,docs/zh/*,docs/ja/*}`

**版本/配置**
- `go.mod:3`（go 1.26.8）、`go.mod`（wails v3.0.0-beta.24）
- `internal/gateway/gateway.go:48-50`（`DefaultAddr = "127.0.0.1:3425"`）
- `docs/subsystems/README.md`、`docs/subsystems/gateway-middleware.md`（子系统设计参考）

**规模数据**
- 网关 `mux.HandleFunc` 约 85 条；GUI `/api/*` 约 263 条（由 `Select-String` 统计，非测试）
- GUI 视图 10 个（9 主视图 + Settings），Settings 内含 9 个 tab

---

### 附：未确认项
- GUI 视图 `sessions/library/routing/gateway/usage/masking` 的具体实现文件分散在 `assets/plugins/*` 与 `app.js` 大文件中，本报告按 `gui-shell.md` 与视图注册概括，未逐一通读每个插件页的全部逻辑。
- SQLite/GORM 等数据层细节未逐文件核对（以包名与文档为据）。
