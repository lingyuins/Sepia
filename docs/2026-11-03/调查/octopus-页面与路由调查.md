# octopus 页面与路由调查

> 调查对象：`D:\CodeWorkspace\octopus`（Go 后端 + `web/` 下的 Next.js 前端）
> 调查方式：只读源码，逐文件阅读；忽略 `node_modules`、`.next`、`out`、`.git` 内部内容（仅说明其存在与用途）。
> 结论均标注文件路径（必要时带行号）。拿不准处显式写“未确认”。

---

## 1. 概览（技术栈、目录结构、版本）

### 1.1 技术栈与版本

| 层 | 技术 | 版本 / 依据 |
|---|---|---|
| 后端语言 | Go | `go 1.27.3`（`go.mod:3`） |
| HTTP 框架 | **gin** | `github.com/gin-gonic/gin v1.11.0`（`go.mod:12`） |
| CORS | gin-contrib/cors | `v1.7.6`（`go.mod:11`） |
| ORM | gorm | `gorm.io/gorm v1.31.1`（`go.mod:36`） |
| CLI | cobra | `spf13/cobra v1.10.2`（`go.mod:23`） |
| 前端框架 | **Next.js**（App Router） | `next 16.0.7`（`web/package.json:30`） |
| UI 运行时 | React | `react 19.2.1` / `react-dom 19.2.1`（`web/package.json:34,36`） |
| 语言 | TypeScript | `typescript ^5.9.3`（`web/package.json:52`） |
| 样式 | Tailwind CSS 4 | `tailwindcss ^4.1.17`（`web/package.json:50`） |
| 数据请求 | TanStack Query | `@tanstack/react-query ^5.90.11`（`web/package.json:20`） |
| 客户端状态 | Zustand 5 | `zustand ^5.0.9`（`web/package.json:40`） |
| 国际化 | next-intl | `next-intl ^4.5.8`（`web/package.json:31`） |
| 包管理 | pnpm | `pnpm@10.22.0`（`web/package.json:5`） |
| 前端应用版本 | 2.7.3 | `web/package.json:3` |

### 1.2 顶层目录结构（与路由相关的部分）

```
octopus/
├── main.go                 # 入口，仅调用 cmd.Execute()
├── cmd/                    # cobra 命令：root.go / start.go / version.go
├── internal/
│   ├── server/
│   │   ├── server.go        # gin.New()，挂中间件 + 静态资源 + router.RegisterAll
│   │   ├── router/router.go # 路由"自注册"注册表框架（GroupRouter/Route/RegisterAll）
│   │   ├── handlers/        # 各业务 handler；每个文件 init() 里 NewGroupRouter(...) 自注册
│   │   └── middleware/      # auth / cors / security / static / audit / rbac / rate_limit ...
│   ├── relay/              # 转发核心（relay.Handler），被 /v1 路由调用
│   └── ...                 # op / model / db / hub / plugin 等业务包
├── static/
│   ├── static.go           # //go:embed all:out 把 static/out 嵌入二进制
│   └── out/                # 前端静态导出（gitignore；本检出仅含 .keep）
├── web/                    # Next.js 前端
│   ├── next.config.ts      # output: "export" 静态导出等
│   ├── package.json
│   ├── out/                # next build 产物（静态导出，gitignore）
│   ├── public/             # 静态资源（locale/*.json、manifest.json、sw.js、图标、screenshot/）
│   └── src/
│       ├── app/            # App Router 页面（极薄，仅 2 个真实路由）
│       ├── components/     # app.tsx 壳 + modules/* 各业务模块
│       ├── route/          # 客户端"路由表"/keep-alive 渲染器
│       ├── api/            # API client 与 endpoint hooks
│       └── stores/ provider/ hooks/ lib/ ...
└── scripts/build.sh        # 构建脚本：构建前端并把 web/out 拷到 static/out
```

---

## 2. 前端页面清单

### 2.1 关键结论：真正的 Next.js 页面只有 2 个

`web/src/app/` 下**没有** `route.ts`、没有动态段 `[xxx]`、没有路由组 `(group)`、没有 `loading.tsx`/`not-found.tsx`/`template.tsx`。全部文件如下（`web/src/app` 递归结果）：

```
web/src/app/layout.tsx        # 根布局
web/src/app/page.tsx          # 唯一业务页面：/  → <AppContainer/>
web/src/app/globals.css       # 全局样式
web/src/app/error.tsx         # 根段错误边界（App Router 约定文件）
web/src/app/global-error.tsx  # 全局错误边界（App Router 约定文件）
web/src/app/layout-check/page.tsx  # 布局展示页（开发用；gitignore 见 .gitignore:81）
```

### 2.2 Next.js App Router 真实路由表

| URL 路径 | 文件 | 说明 |
|---|---|---|
| `/` | `web/src/app/page.tsx` | 唯一入口页，`export default function Home(){ return <AppContainer/> }`。整个管理后台都从这一个页面挂起，再靠客户端状态切换内部界面。 |
| `/layout-check` | `web/src/app/layout-check/page.tsx` | **开发用布局展示页**：用 `QueryClient({ queries: { enabled: false } })` + 写死的测试数据渲染 GroupCard / GroupListItem / CreateDialogContent，不连后端（页面内注释“布局展示页，使用测试数据，不连接后端”）。所在目录被 `web/.gitignore` 忽略（`.gitignore:81 web/src/app/layout-check/`），属于本地调试页，非正式功能。 |

约定文件（非独立 URL）：`web/src/app/layout.tsx`（根 `<html>`/`<body>`、注入 `#initial-loader` 启动动画、Provider 栈：Theme→Query→Locale→Tooltip、ServiceWorker 注册、Toaster）；`web/src/app/error.tsx`（段级错误边界 UI）；`web/src/app/global-error.tsx`（全局错误边界 UI）。

> 因此：**octopus 前端不是"多页面站点"**，而是“单入口 + 客户端状态路由的 SPA”。URL 层面几乎只有一个 `/`。

### 2.3 SPA 内部“逻辑页面”（14 个顶级模块）

真正的“页面切换”发生在客户端。顶级页面清单来自 `web/src/route/config.tsx:35-50` 的 `ROUTES`，其 id 集合 `ROUTE_IDS = DEFAULT_NAV_ORDER`，权威定义在 `web/src/components/modules/navbar/nav-store.ts:20-35`。

| 逻辑页面（NavItem id） | 组件 | 大致做什么 |
|---|---|---|
| `home` | `@/components/modules/home` → `Home` | 首页：Hero 概览、活动热力图、趋势图（成本/请求数/token/成功率多指标叠加）、排行、分析概览卡片 |
| `hub` | `@/components/modules/remote-site` → `RemoteSite` | Hub：远程站点聚合，含子标签 Sites / Site Channels / Automation / Balance / TokenPlan（见 §2.4） |
| `channel` | `@/components/modules/channel` → `Channel` | 渠道配置：内置快速创建模板、密钥管理、同步、延迟、模型声明、代理模式、请求重写、参数覆盖等 |
| `pool` | `@/components/modules/pool` → `Pool` | 账号池管理：池列表 + 详情（账号搜索/过滤/OAuth 授权/批量操作/导入导出） |
| `plugins` | `@/components/modules/plugin` → `Plugin` | 插件管理：安装/启停/配置/卸载，插件自带 UI 以 iframe 文档渲染（隔离 CSP） |
| `group` | `@/components/modules/group` → `Group` | 路由分组：均衡策略、分组折叠列表、分组测试、AI 路由生成、维护操作、CC Switch 深链 |
| `model` | `@/components/modules/model` → `Model` | 模型市场：卡片/可用端点/价格分类三视图、多维过滤、价格编辑、峰值/谷时计费规则 |
| `analytics` | `@/components/modules/analytics` → `Analytics` | 分析：Channel×Model / Utilization / Route Health / Channel-Model / Evaluation / Latency / Cache 子标签 |
| `log` | `@/components/modules/log` → `Log` | 日志：转发请求列表/详情（请求体/响应、候选行、诊断）+ `ErrorLogView` 后端错误日志 |
| `notification` | `@/components/modules/notification` → `Notification` | 通知中心：消息（收件箱/归档）、告警（规则/历史）、投递（渠道/策略/偏好）、报表（计划/历史），SSE 推送 |
| `ops` | `@/components/modules/ops` → `Ops` | 运维：Telemetry / Quota / Health / Maintenance / System / Audit 子标签 |
| `apikey` | `@/components/modules/apikey` → `APIKeyPage` | API Key 管理：白名单、过期、最大花费、RPM/TPM、按模型配额、IP/CIDR |
| `setting` | `@/components/modules/setting` → `Setting` | 设置卡片：信息/外观/自动策略/AI Route/账户/语义缓存/日志/系统/备份/WebDAV/WebAuthn/归一化/账号池/节点/代理池等 |
| `user` | `@/components/modules/user` → `User` | 用户与角色管理 |

### 2.4 二级“子页面”：模块内 Tabs（有独立子标签体系，但不是 URL 路由）

Hub / Analytics / Ops 三个模块有可排序、可隐藏的子标签，定义在 `web/src/components/modules/navbar/sub-tab-store.ts:12-21`：

- `hub`：`sites` / `site-channels` / `automation` / `balance` / `tokenplan`
- `analytics`：`cache` / `utilization` / `route-health` / `channel-model` / `evaluation` / `latency`
- `ops`：`telemetry` / `quota` / `health` / `maintenance` / `system` / `audit`

这些子标签同样是客户端 state 切换，**不产生 URL 变化**。

### 2.5 非路由的“状态页面”（由认证状态决定渲染，不占 URL）

`web/src/components/app.tsx:429-482` 中 `AppContainer` 按状态渲染：加载动画 → 首启引导 `FirstRunSetup` → API Key 登录态 `APIKeyDashboard` → 未登录 `LoginForm` → 主界面（NavBar + ContentLoader）。这些是“页面”，但不是路由。

---

## 3. 前端路由机制

### 3.1 Next.js 版本与 router 类型

- **Next.js 16.0.7，App Router**（`web/package.json:30`；目录结构为 `src/app/`，使用 `layout.tsx`/`page.tsx` 约定）。**不是** Pages Router。
- 根布局 `web/src/app/layout.tsx`，唯一业务页面 `web/src/app/page.tsx`。

### 3.2 next.config 的关键配置（`web/next.config.ts`）

| 配置 | 值 | 行号 | 影响 |
|---|---|---|---|
| `output` | `"export"` | 28 | **静态导出**（SSG，`next build` 产出 `out/`，无服务端运行时） |
| `images.unoptimized` | `true` | 29-31 | 静态导出不能跑图片优化服务 |
| `reactCompiler` | `true` | 27 | React 编译器开启 |
| `assetPrefix` | 非 dev 阶段 `"./"` | 43 | **相对资源前缀**，使 UI 可挂在任意路径前缀下 |
| `env.NEXT_PUBLIC_APP_VERSION` | 由 `resolveAppVersion()` 注入 | 40-42 | 版本号；回退读 `../internal/conf/version.go` |
| `experimental.optimizePackageImports` | recharts / lobe icons / lucide / radix-icons | 32-39 | 打包优化 |

- **没有** `rewrites`、没有 `redirects`、没有 `basePath`、**没有** Next.js `i18n` 路由配置。
- `assetPrefix` 在开发服务器阶段置空、其余阶段为 `"./"`（`next.config.ts:26,43`）。

### 3.3 中间件 middleware.ts

- **不存在** `web/src/**/middleware.ts`（递归搜索无结果）。Next.js 层没有中间件。
- 所有 HTTP 中间件都在 Go 侧（见 §4.4）。

### 3.4 i18n 机制（客户端）

- 用 `next-intl` 的 `NextIntlClientProvider`，但**没有 `[locale]` 动态段**、没有 next-intl 的路由中间件。
- 语言由客户端 store 决定：`web/src/provider/locale.tsx:18-38` 从 `useSettingStore()` 读 `locale`，再 `NextIntlClientProvider` 注入 `messages`。
- 语言包静态放在 `web/public/locale/{en,zh_hans,zh_hant}.json`（`locale.tsx:8-16`），构建时被复制到 `out/locale/*.json`（已确认 `web/out/locale/` 存在）。

### 3.5 动态路由参数

- Next.js 层：**无动态段**。
- 业务层“参数”：不是路径参数，而是 **URL query string**，且仅用于少数跨页面回跳：
  - OAuth 回跳：后端 302 到 `/pool?oauth=success|error&pool_id=N&msg=...`（`internal/server/handlers/pool_oauth.go:719`）；前端在 `web/src/components/app.tsx:185-218` 解析 `window.location.search`，toast 并切到 `pool` 模块，然后清理 `oauth`/`msg`（保留 `pool_id` 由 Pool 列表消费）。
  - Pool 列表消费 `pool_id`：`web/src/components/modules/pool/PoolList.tsx:49-55` 读取 `pool_id` 后 `history.replaceState` 清理。
- 其余所有页面内导航都是内存 state，不写 URL。

### 3.6 客户端导航封装（`web/src/route/`）

| 文件 | 作用 |
|---|---|
| `web/src/route/config.tsx` | 路由表：`lazyWithPreload` 注册 14 个顶级模块 → `ROUTES`（含 icon/component）；`ROUTE_IDS = DEFAULT_NAV_ORDER`；`CONTENT_MAP` 由 id 映射到懒加载组件 |
| `web/src/route/content-loader.tsx` | **keep-alive 渲染器**：已访问过的路由组件保持挂载，仅用 `display:none` + `aria-hidden` + `inert` 切换可见性；避免重复请求/recharts 重建（`:9-17` 注释） |
| `web/src/route/lazy-with-preload.ts` | `lazyWithPreload`：给 `React.lazy` 组件加 `preload()`（预取 chunk） |
| `web/src/route/use-preload.ts` | `usePreload`：按 routeId 触发 `CONTENT_MAP[id].preload()` |
| `web/src/route/index.ts` | 统一导出 |

### 3.7 导航状态与“路由表”真身

- 权威路由 id 列表：`web/src/components/modules/navbar/nav-store.ts` 的 `DEFAULT_NAV_ORDER`（`:20-35`）；`route/config.tsx:11` 的 `ROUTE_IDS` 直接复用。
- `useNavStore()`（`nav-store.ts:142-277`）：`activeItem` 用 **zustand persist 到 localStorage（key=`nav-storage`）**；切换时记录 `prevItem`/`direction` 供动画；`setActiveItem` 会校验目标在 `visibleItems` 内。
- 注意 `AGENTS.md:176-180` 提到“两份 DEFAULT_NAV_ORDER”，`navbar/nav-order.ts` 是**死代码**，真正生效的是 `nav-store.ts`。

### 3.8 跨模块跳转封装

- `web/src/stores/jump.ts`：`useJumpStore().requestJump(target)` 统一处理“跳到某站点/站点账号/站点渠道/渠道卡片”，内部会切 `activeItem` 与 Hub 子标签，并把待处理目标放进 `pending`，`getJumpTargetRoute()` 决定落到哪个顶级路由。
- `web/src/lib/nav-handoff.ts`：`pendingLogFilter` 用于“点击失败渠道 → 跳到 log 并预填筛选”。

### 3.9 无 Next 路由 API（重要）

源码中 `next/navigation` / `usePathname` / `useRouter` **命中 0 次**（`AGENTS.md:173`；本次 grep 亦未见）。页面切换完全靠 `useNavStore().activeItem` + `<ContentLoader/>`（`web/src/components/app.tsx:553`）。**不要在新增代码里引入 Next 路由 API 做页面切换**。

### 3.10 PWA / Service Worker

- `web/src/app/layout.tsx:122` 挂 `<ServiceWorkerRegister/>`，`web/src/components/sw-register.tsx` 注册 `sw.js`；`web/public/sw.js` 是手写 SW（静态资源缓存优先，`PRECACHE_URLS=['/','/manifest.json',...]`）。
- `manifest.json` 用相对 `"start_url": "./"`、`"scope": "./"`（`web/public/manifest.json`）。

---

## 4. 后端 HTTP 路由

### 4.1 路由库与注册机制

- 使用 **gin**（`internal/server/server.go:38 r := gin.New()`）。
- **不是**在某个函数里几十行集中 `r.GET(...)`，而是**约定式自注册**：
  - 每个 `internal/server/handlers/*.go` 文件在 `func init()` 里调用 `router.NewGroupRouter("/api/v1/xxx").Use(...).AddRoute(router.NewRoute("/path", http.MethodX).Handle(fn))`。
  - `internal/server/router/router.go` 维护全局注册表 `registeredRouters`；`server.go:21` 用 `_ "…/internal/server/handlers"` 匿名导入以触发所有 `init()`；`server.go:88 router.RegisterAll(r)` 一次性把所有分组挂到 `gin.Engine`。
  - `router.go:103-126 RegisterAll`：**消费后置 `registeredRouters = nil`，一个进程只能调一次**（`AGENTS.md:113,229`）。
  - 支持的 method：GET/POST/PUT/DELETE/HEAD/OPTIONS/PATCH（`router.go:11-19`）。
- 路由条数量级：`handlers/*.go` 中 `router.NewRoute(` 约 **325 处（非测试）**，约 330 处含测试。故下文**按功能分组概括**，不逐条罗列。

### 4.2 三大 API 域（前缀分离）

| 前缀 | 定位 | 鉴权 | 典型使用方 |
|---|---|---|---|
| `/api/v1/...` | 管理控制台 API（后台业务） | `middleware.Auth()`（JWT）+ `RequirePermission(...)` RBAC | 管理 UI（`web/src/api/endpoints/*`） |
| `/v1/...` | **OpenAI 兼容代理/推理端点**（转发核心） | `middleware.APIKeyAuth()` | 外部客户端（Claude Code / SDK / curl 等） |
| `/node/v1/...` | 本地节点隧道 | 节点自校验（见 handler） | Magpie 节点连接器 |

### 4.3 路由分组与主要端点（按功能归类）

> 下面每个 `/api/v1/...` 分组均在对应 handler 的 `init()` 内注册；行号指该分组定义处。

**A. 启动与认证**

| 前缀 | 主要端点 | 说明 |
|---|---|---|
| `/api/v1/bootstrap` | `GET /status`、`POST /create-admin` | 首启状态与创建初始 admin（无 Auth，仅限初始化窗口；`bootstrap.go:18-28`） |
| `/api/v1/user` | `POST /login`（LoginRateLimit）、`POST /create`、`POST /change-password`、`POST /change-username`、`GET /status`、`GET /list`、`POST /update-role`、`DELETE /delete/:id` | 登录 + 用户管理（`user.go:19-64`） |
| `/api/v1/webauthn` | `GET /status`、`POST /login/begin`、`POST /login/finish`、`GET /credentials`、`POST /register/begin`、`POST /register/finish`、`DELETE /credentials/:id` | Passkey/WebAuthn 登录与凭证（`webauthn.go:21-50`） |

**B. 渠道 / 分组 / 模型**

| 前缀 | 主要端点（节选） | 说明 |
|---|---|---|
| `/api/v1/channel` | `/list`、`/create`、`/update`、`/enable`、`/batch-group`、`/delete/:id`、`/fetch-model`、`/fetch-models-per-key`、`/test`、`/check-keys/:id`、`/test-model`、`/group/{list,create,update,delete/:id}`、`/sync`、`/last-sync-time` | 渠道 CRUD/测试/分组（`channel.go:28-116`） |
| `/api/v1/channel-migration` | `POST /migrate`、`POST /migrate-all` | 渠道迁移（`channel_migration.go:16`） |
| `/api/v1/group` | `/list`、`/create`、`/update`、`/auto-group`、`/test`、`/test-draft`、`/test/progress/:id`、`/delete-all`、`/purge-unavailable`、`/delete/:id` | 路由分组（`group.go:23-74`） |
| `/api/v1/route` | `POST /ai-generate`、`GET /ai-generate/status/:id`、`/progress/:id`、`/result/:id`、`/stream/:id` | AI 路由生成（`route.go:20`） |
| `/api/v1/model` | `/list`、`/create`、`/channel`、`/market`、`/normalize/{rules,generate,progress/:id,stream/:id,coverage}`、`/update`、`/delete`、`/update-price`、`/last-update-time`、`/capabilities`、`/price-category/*`、`/price-schedule/*` | 模型市场/价格/归一化（`model.go:28-131`） |
| `/api/v1/model-mapping` | `GET ""`、`GET /:id`、`POST ""`、`PUT /:id`、`DELETE /:id` | 模型名映射规则（`model_mapping.go:17-30`） |

**C. 站点 / Hub / 账号池**

| 前缀 | 主要端点（节选） | 说明 |
|---|---|---|
| `/api/v1/site` | `/list`、`/archived`、`/import/all-api-hub`、`/import/metapi`、`/account/sync/:id`、`/account/checkin/:id`、`/sync-all`、`/checkin-all`、`/:id/available-models`、`/create`、`/update`、`/enable`、`/detect`、`/batch`、`/account/{create,update,enable}`、`/delete/:id`、`/archive/:id`、`/restore/:id`、`/account/delete/:id` | 站点管理（`site.go:36-81`） |
| `/api/v1/remote-site` | `/list`、`/create`、`/update`、`/delete/:id`、`/refresh/:id`、`/refresh-all`、`/detect`、`/models/:id`、`/pricing/:id`、`/site-types` | 远程站点（`remote_site.go:22-71`） |
| `/api/v1/site-discovery` | `GET /discover` | 站点发现（`remote_site.go:73-80`） |
| `/api/v1/site-channel` | `GET /list`、`GET /:siteId`、`GET /:siteId/account/:accountId`、`.../model-history`、`POST .../keys`、`PUT .../source-keys`、`.../group-projection`、`.../model-routes`、`.../model-disabled`、`.../projected-channel-settings`、`POST .../manual-models`、`.../manual-models/delete`、`.../model-routes/reset` | 站点渠道（`site_channel.go:23-40`） |
| `/api/v1/remote-site-token` | `GET /list/:site_id`、`POST /sync/:site_id`、`POST /sync-to-channel`、`GET /export/:site_id` | 站点令牌（`remote_site_token.go:20`） |
| `/api/v1/usage-history` | `GET ""`、`/summary`、`/hourly`、`/models/:site_id`、`POST /sync/:site_id`、`/sync-all` | 站点用量历史（`usage_history.go:17-46`） |
| `/api/v1/balance-history` | `GET /list/:site_id`、`/chart/:site_id`、`POST /capture/:site_id`、`GET /prediction/:site_id` | 余额历史（`balance_history.go:16-36`） |
| `/api/v1/checkin` | `GET /status/:site_id`、`POST /execute/:site_id`、`/execute-all`、`GET /history/:site_id` | 签到（`checkin.go:16-36`） |
| `/api/v1/redemption` | `POST /redeem`、`/redeem-all`、`GET /history/:site_id` | 兑换（`redemption.go:17-34`） |
| `/api/v1/announcement` | `GET /list`、`/list/:site_id`、`POST /refresh/:site_id`、`/refresh-all` | 公告（`announcement.go:16`） |
| `/api/v1/pool` | `/list`、`/create`、`/update`、`/delete/:id`、`/:id/account/{list,create,update/:aid,delete/:aid,test,quota/:aid,refresh-token/:aid,recover/:aid,temp-unsched/:aid,restore-proxy/:aid,batch-refresh,batch-clear-error,batch-test,export}`、`/import`、`GET /:id/account/:aid` | 账号池（`pool.go:19-125`） |
| `/api/v1/pool/oauth` | `GET /initiate`（Auth）、`GET /callback`（无 Auth，用 state 校验） | OAuth 授权（`pool_oauth.go:34-43`） |
| `/api/v1/pool/unsched-rules` | `POST /create`、`/update/:id`、`DELETE /delete/:id`、`GET /list` | 池调度规则（`pool_unsched_rules.go:18-40`） |

**D. 用量统计 / 分析 / 运维**

| 前缀 | 主要端点 | 说明 |
|---|---|---|
| `/api/v1/stats` | `GET /today`、`/daily`、`/hourly`、`/total`、`/channel`、`/apikey` | 统计（`stats.go:29-56`） |
| `/api/v1/analytics` | `GET /overview`、`/utilization`、`/evaluation`、`/group-health`、`/provider-breakdown`、`/model-breakdown`、`/channel-model`、`/auto-strategy`、`/apikey-breakdown`、`/latency-distribution` | 分析（`analytics.go:17-58`） |
| `/api/v1/ops` | `GET /cache`、`/quota`、`/health`、`/system`、`/telemetry` | 运维（`ops.go:15-36`） |
| `/api/v1/log` | `GET /list`、`/detail`、`DELETE /clear`、`/clear-contents`、`GET /stream`（SSE） | 转发日志（`log.go:21-45`） |
| `/api/v1/audit` | `GET /list`、`/detail` | 管理审计（`audit.go:16-26`） |
| `/api/v1/error-log` | `POST /report`、`GET /list`、`/detail`、`DELETE /clear` | 错误日志（`error_log.go:57-78`） |
| `/api/v1/notification` | `/list`、`/unread-count`、`/detail/:id`、`/delivery/list`、`GET /stream`、`/mark-read`、`/mark-unread`、`/mark-all-read`、`/archive`、`/unarchive`、`/delete/:id`、`/archived`、`/preference/{list,save,delete/:id}`、`/policy/{list,create,update,delete/:id}` | 通知中心（`notification.go:22-43`） |
| `/api/v1/alert` | `/rule/{list,create,update,delete/:id}`、`/notif/{list,create,update,delete/:id,test}`、`/history` | 告警（`alert.go:24-37`） |
| `/api/v1/report` | `/schedule/{list,create,update,delete/:id,test}`、`/history/list`、`/metrics` | 报表计划（`report.go:19-46`） |

**E. API Key / 凭证 / 设置 / 备份 / 更新**

| 前缀 | 主要端点（节选） | 说明 |
|---|---|---|
| `/api/v1/apikey` | `POST /create`、`GET /list`、`POST /update`、`DELETE /delete/:id`（JWT+RBAC）；`GET /stats`、`GET /login`（**APIKeyAuth**） | API Key 管理（`apikey.go:22-56`） |
| `/api/v1/api-credential` | `/list`、`/create`、`/update`、`/delete/:id`、`/api-types`、`/cli-tools` | 凭证/CLI 工具（`api_credential.go:17-46`） |
| `/api/v1/cli-export` | `POST /generate` | CLI 导出（`cli_export.go:18`） |
| `/api/v1/verification` | `POST /run`、`POST /run-for/:id`、`GET /probes` | 校验探针（`verification.go:27`） |
| `/api/v1/setting` | `GET /list`、`POST /ai-route/models`、`POST /set`、`GET /export`、`POST /import`、`POST /database/{test,migrate}`、`GET /cache/config`、`POST /cache/{test,save}` | 设置（`setting.go:41-70+`） |
| `/api/v1/backup/webdav` | `GET/POST /config`、`POST /test`、`POST /backup`、`restore`、`list`、`delete` | WebDAV 备份（`webdav.go:20`） |
| `/api/v1/update` | `GET ""`、`GET /now-version`、`POST ""` | 自更新（`update.go:21-38`） |
| `/api/v1/proxy-pool` | `/list`、`/references/:id`、`/delete/:id`、`/create`、`/update`、`/test` | 代理池（`proxy_pool.go:17-31`） |
| `/api/v1/nodes` | `GET ""`、`POST ""`、`PUT /:id`、`POST /:id/rotate-token`、`POST /:id/test`、`DELETE /:id` | 本地节点（`node.go:34-42`） |
| `/api/v1/plan-provider` | `/balance/list`、`/tokenplan/list`、`/balance/categories`、`/tokenplan/categories`、`/wallet/transactions/:id`、`/add`、`/refresh/:id`、`PUT /credentials/:id`、`DELETE /:id` | 套餐供应商（`plan_provider.go:22-38`） |

**F. 插件**

| 前缀 | 主要端点 | 说明 |
|---|---|---|
| `/api/v1/plugins` | `GET ""`、`GET /:id/ui-bundle`（读权限）、`POST /install`、`POST /:id/enable`、`/disable`、`PUT /:id/config`、`DELETE /:id`（管理员） | 插件管理（`plugin.go:28-38`） |
| `/api/v1/plugins/:id/rpc` | `POST /*path`、`GET /*path` | 插件 RPC 通配分发（`plugin.go:42-44`，注释说明用单条 catch-all 规避 gin 路由树不可变） |

**G. OpenAI 兼容代理/推理端点（`/v1`，APIKeyAuth）**

| 前缀 | 端点 | 说明 |
|---|---|---|
| `/v1` | `POST /chat/completions`、`POST /responses`、`POST /messages`、`POST /embeddings` | 核心转发（`relay.go:14-35`，统一走 `relay.Handler(...)`） |
| `/v1` | `GET /models` | 模型列表（`model.go:132-137`） |
| `/v1` | `POST /images/generations`、`/audio/speech`、`/videos/generations`、`/music/generations`、`/search`、`/rerank`、`/moderations` | 多媒体（JSON，`media.go:12-45`） |
| `/v1` | `POST /images/edits`、`/images/variations`、`/audio/transcriptions` | 多媒体（multipart，`media.go:47-62`） |

**H. 节点隧道**

| 前缀 | 端点 | 说明 |
|---|---|---|
| `/node/v1` | `GET /connect` | 本地节点连接（`node.go:24`；不走 `middleware.Auth`，由 handler 自校验令牌） |

### 4.4 全局中间件（`internal/server/server.go:38-90`）

按挂载顺序：`gin.CustomRecovery`（panic 记录 errorlog）→ 仅 debug 的 `middleware.Logger()` → `middleware.SecurityHeaders()` → `middleware.Cors()`（白名单，`*` 时禁用 credentials，`middleware/cors.go`）→ `middleware.AuditManagementWrite()`（管理写操作审计白名单，`middleware/audit.go:22+`）→ 静态资源（`StaticLocal`（debug 本地）或 `StaticEmbed`（嵌入））→ `router.RegisterAll(r)`。

分路由级中间件常见组合：`middleware.Auth()`（JWT 校验 + 注入 user_id/role，`middleware/auth.go:19-55`）、`middleware.APIKeyAuth()`（API Key 校验，`middleware/auth.go:57+`）、`middleware.RequirePermission(auth.PermXxx)`（RBAC，`middleware/rbac.go`）、`middleware.RequireJSON()`（强制 JSON body）、`middleware.LoginRateLimit()`（登录限流）。

### 4.5 特别的路由机制：审计白名单 + 测试守护

`internal/server/middleware/audit.go` 的 `auditedManagementWriteRoutes` 是**白名单**；`internal/server/audit_route_test.go` 的 `TestAllManagementWriteRoutesAreAudited` 会遍历全部注册路由，任何 `/api/v1/` 下的 POST/PUT/PATCH/DELETE 若既不在白名单、也不在测试的 `exemptFromAudit`（每条需书面理由）里，**测试直接失败**（`AGENTS.md:125`）。这意味着新增管理写接口必须同步更新审计白名单。

---

## 5. 前后端对接与静态资源

### 5.1 前端调 API 的 base

- `web/src/api/client.ts:5`：`export const API_BASE_URL = (process.env.NEXT_PUBLIC_API_BASE_URL || '.').trim();`
- 即**默认相对当前 origin**（`.` → 相对路径）。请求 URL = `${API_BASE_URL}${path}`（`client.ts:130`），所有 endpoint 都以 `/api/v1/...` 写死（如 `client.ts:44`）。
- 因此**默认部署 = 同源**：前端与 Go 后端同一进程/端口，前端直接向 `/api/v1/...` 发请求。
- 单独本地开发时可设 `NEXT_PUBLIC_API_BASE_URL="http://127.0.0.1:8080"`（`web/README.md:29`）；此时是跨域直连，需要后端 CORS 白名单放行。
- 认证头：`Authorization: Bearer <token>`，token 来自 zustand auth store（`client.ts:141-147`）。

### 5.2 没有 next rewrites / vite proxy

- 前端**没有** `rewrites`（`next.config.ts` 无该字段），也**不是 Vite**（是 Next.js，无 vite proxy）。
- 生产/整体运行时**不需要代理**：Go 直接托管前端静态文件，同源调用即可。
- 唯一“代理层”是 Go 的 gin 路由本身（`/api`、`/v1`、`/node/` 与静态资源共存于同一 engine）。

### 5.3 静态资源如何由 Go 提供（embed static/out）

1. `web/next.config.ts:28` `output: "export"` → `pnpm build`（`web/package.json:9`）产出 `web/out/`（本检出 `web/out/` 已存在，含 `index.html`、`404.html`、`_not-found.html`、`layout-check.html`、`_next/`、`locale/`、`screenshot/` 等）。
2. 构建脚本 `scripts/build.sh:240-261` 把 `web/out` 移动/拷贝为 `static/out`（`rm -rf static/out; mv web/out static/`）。
3. `static/static.go:8-22`：`//go:embed all:out all:out/**` 把 `static/out` 嵌入二进制；`StaticFS` 在 `init()` 里 `fs.Sub(..., "out")` 暴露。
4. `internal/server/server.go:79-86`：非 debug 且有 `StaticFS` 时 `r.Use(middleware.StaticEmbed("/", static.StaticFS))`；debug 时用 `StaticLocal("/", <web/out 或 static/out>)` 从磁盘读（`server.go:187-200 resolveLocalStaticDir`，优先 `web/out` 再 `static/out`）。
5. **`static/out` 与 `web/out` 的关系**：`web/out` 是 `next build` 的原始静态导出目录；`static/out` 是**同一个导出被拷入 Go 源码树、供 `//go:embed` 使用**的位置。`static/out` 内容被 gitignore（`.gitignore:3 static/out/*`），仓库只保留 `static/out/.keep`；`web/out` 也被 `web/.gitignore` 忽略。二者是“源产物”与“嵌入副本”的关系，**不是两份独立代码**。
6. `Dockerfile` 前端构建阶段 `RUN NEXT_PUBLIC_APP_VERSION=... pnpm build` 后 `COPY --from=frontend-builder /build/out ./static/out`，并给空 `_not-found` 补 `.keep`（`Dockerfile`）。

### 5.4 静态中间件的“SPA fallback”行为（关键）

`internal/server/middleware/static.go:22-78`：
- 只处理 GET/HEAD；路径以 `/api`、`/v1`、`/node/` 开头则放行给后续 handler（`:32-35`）。
- 命中真实文件则直接服务：`/sw.js` 设 `no-cache,no-store`，`/` 与 `.html` 设 `no-cache`，其余静态资源 `public, max-age=31536000, immutable`（`:43-51`）。
- 未命中且请求“接受 HTML”（或没有扩展名）→ **回退服务 `/index.html`**（`:57-77`）。这使任意未知路径（如 `/pool`、`/anything`）都返回同一个 SPA 外壳，由客户端接管。

### 5.5 相对资源前缀（assetPrefix）

`next.config.ts:43` 非 dev 阶段 `assetPrefix: "./"`：导出的 `index.html` 里 `_next` 等资源用相对路径，配合 Go 的 `StaticEmbed("/", …)`，使 UI 可部署在任意路径前缀（子路径反代）下而不改构建。`layout.tsx:28-30` 的 `manifest.json`/`favicon.ico`/`apple-icon.png` 也写相对 `./`。

### 5.6 前端 API endpoint 覆盖

`web/src/api/endpoints/*.ts`（约 40 个文件：alert/analytics/apikey/channel/group/model/log/pool/site/site-channel/notification/ops/setting/plugin/report/update/usage-history/webauthn/...）与后端 `/api/v1/*` 分组基本一一对应，**类型手写维护，无 OpenAPI 代码生成**（`web/README.md:136`）。

---

## 6. 路由特点与注意点

1. **单入口 SPA + 客户端状态路由，而非文件系统多页路由**：URL 只有 `/`（和开发用 `/layout-check`），14 个“页面”靠 `useNavStore().activeItem` + `ContentLoader` 切换。不要按“每个页面一个 page.tsx”去理解本仓库。
2. **keep-alive 渲染**：`web/src/route/content-loader.tsx` 让访问过的模块**保持挂载**（`display:none`+`inert` 隐藏），因此**切 tab 不会触发 `useEffect` cleanup**，轮询/订阅需按可见性门控（`AGENTS.md:174,301`）。
3. **三大 API 域按前缀与鉴权分离**：`/api/v1`（JWT+RBAC 管理）、`/v1`（APIKeyAuth 的 OpenAI 兼容代理）、`/node/v1`（节点隧道）。前端只调 `/api/v1`；`/v1` 是给外部客户端的推理入口。
4. **约定式自注册 + 单次 RegisterAll**：所有路由分散在各 handler 的 `init()`，靠匿名导入汇聚；`registeredRouters` 消费即置 nil，进程内只能 RegisterAll 一次（涉及路由的测试注意状态污染）。
5. **动态段与 catch-all**：后端路径参数用 gin 风格 `:id` / `:site_id` / `:aid` / `:siteId` / `:accountId` 等；插件 RPC 用 `/*path` catch-all（`plugin.go:43-44`）。前端无路径动态段。
6. **静态托管 SPA fallback**：任意未命中、且期望 HTML 的路径都返回 `index.html`；`/api`、`/v1`、`/node/` 前缀被放行给后端 handler，避免被静态中间件吞掉（`static.go:32-35`）。
7. **URL 不反映当前页面**：刷新后当前 tab 由 `localStorage['nav-storage']` 持久化的 `activeItem` 恢复（`nav-store.ts:255-256`），而不是由 URL 决定；只有 OAuth 回跳/pool 深链用 query 参数（`app.tsx:185-218`、`PoolList.tsx:49-55`）。
8. **静态导出 + 相对前缀**：`output:"export"`、`images.unoptimized`、`assetPrefix:"./"` 支持“嵌入二进制 + 任意子路径部署”，无 SSR、无服务端渲染边界（`web/README.md:139`）。
9. **i18n 是客户端方案**：`next-intl` 的 `NextIntlClientProvider` + 客户端 store 选语言，无 `[locale]` 路由段、无 i18n 中间件；语言包为 `public/locale/*.json` 静态资源。
10. **PWA**：`sw.js` + `manifest.json`（相对 scope），前端壳带 Service Worker 缓存。
11. **审计强约束**：新增 `/api/v1` 写接口必须同时进 `middleware/audit.go` 白名单，否则 `audit_route_test.go` 失败（`AGENTS.md:125`）。
12. **两份 `DEFAULT_NAV_ORDER` 陷阱**：权威是 `navbar/nav-store.ts`，`navbar/nav-order.ts` 是死代码；新增顶级页面需同步 `nav-store.ts`、`route/config.tsx`、三份 locale JSON（`AGENTS.md:176-180`）。
13. **开发调试用 `/layout-check`** 页面被 gitignore，是本地布局校验工具，不应视为正式路由。

---

## 7. 证据（关键文件路径:行号 清单）

**前端 · App Router 页面与配置**
- `web/src/app/page.tsx:1-5` — 唯一业务页 `Home → <AppContainer/>`
- `web/src/app/layout.tsx:11-136` — 根布局、Provider 栈、`#initial-loader`、SW 注册、相对 manifest/favicon
- `web/src/app/error.tsx:9-77` — 段级错误边界
- `web/src/app/global-error.tsx:6-107` — 全局错误边界
- `web/src/app/layout-check/page.tsx:1-32` — 开发布局展示页（无后端）
- `web/next.config.ts:26-46` — `output:"export"`、`images.unoptimized`、`assetPrefix:"./"`、`reactCompiler`、`env`
- `web/package.json:9,30,34,36,50,52` — build 脚本 / next 16.0.7 / react 19.2.1 / tailwind 4 / ts
- `web/tsconfig.json:22-24` — `@/*` → `./src/*`
- `web/README.md:16` — “App Router 作为壳入口，实际屏幕切换在 `src/components/app.tsx` 客户端”
- `web/README.md:139` — 静态导出 + 相对 assetPrefix 说明
- 无 `middleware.ts`、无 `loading.tsx`、无 `not-found.tsx`、无 `route.ts`、无 `[xxx]`/`(group)`（`web/src/app` 递归清单）

**前端 · 客户端路由**
- `web/src/route/config.tsx:11,20-50,52-55` — `ROUTE_IDS=DEFAULT_NAV_ORDER`、14 个 `ROUTES`、`CONTENT_MAP`
- `web/src/route/content-loader.tsx:9-17,18-70` — keep-alive 渲染器
- `web/src/route/lazy-with-preload.ts:3-10` — `lazyWithPreload`
- `web/src/route/use-preload.ts:5-13` — `usePreload`
- `web/src/route/index.ts:1-5` — 导出
- `web/src/components/app.tsx:136-560`（切换点 `553` `ContentLoader activeRoute={activeItem}`；认证分支 `429-482`；OAuth 回跳 `185-218`）
- `web/src/components/modules/navbar/nav-store.ts:4-18`（NavItem）、`20-35`（DEFAULT_NAV_ORDER）、`142-277`（persist store）
- `web/src/components/modules/navbar/sub-tab-store.ts:12-21` — Hub/Analytics/Ops 子标签
- `web/src/components/modules/navbar/navbar.tsx:18-41` — NavBar 使用 `ROUTES`/`usePreload`
- `web/src/components/modules/navbar/nav-order.ts:1-16` — 死代码（`AGENTS.md:297` 说明）
- `web/src/stores/jump.ts:30-97` — 跨模块跳转
- `web/src/lib/nav-handoff.ts:14-24` — 日志筛选交接
- `web/src/provider/locale.tsx:8-38` — next-intl 客户端 i18n
- `web/src/api/client.ts:5,130,141-147,162-197` — API base 与请求封装
- `web/src/app/layout.tsx:28-30` / `web/public/manifest.json` / `web/public/sw.js` — PWA 相对路径

**后端 · 服务器与路由框架**
- `main.go:1-11`、`cmd/start.go:149`（`server.Start()`）— 入口
- `internal/server/server.go:38-90` — gin.New、中间件、静态资源、`router.RegisterAll`
- `internal/server/server.go:187-200` — `resolveLocalStaticDir`（debug 从 `web/out`/`static/out` 读）
- `internal/server/router/router.go:22-39,54-91,103-126,128-155` — 注册表/Route/RegisterAll/单次消费
- `internal/server/middleware/static.go:22-78` — 静态服务与 SPA fallback、缓存头、`/api|/v1|/node/` 放行
- `internal/server/middleware/auth.go:19-55`（Auth）、`57+`（APIKeyAuth）
- `internal/server/middleware/cors.go:49-92` — CORS 白名单
- `internal/server/middleware/audit.go:22+` — 审计白名单
- `static/static.go:8-22` — `//go:embed all:out all:out/**`
- `scripts/build.sh:204-264`（`web/out → static/out`）、`Dockerfile`（frontend-builder → `COPY /build/out ./static/out`）
- `.gitignore:3-4`（`static/out/*`、仅留 `.keep`）、`web/.gitignore`（`/out/`）、`.gitignore:81`（`web/src/app/layout-check/`）

**后端 · 各路由分组 init()（节选，详见 §4.3）**
- `internal/server/handlers/relay.go:14-35`（`/v1` 核心转发）
- `internal/server/handlers/media.go:12-62`（`/v1` 多媒体）
- `internal/server/handlers/model.go:28-137`（`/api/v1/model` + `/v1/models`）
- `internal/server/handlers/channel.go:27-116`
- `internal/server/handlers/group.go:22-78`
- `internal/server/handlers/pool.go:19-125`、`pool_oauth.go:33-43`
- `internal/server/handlers/site.go:35-81`、`remote_site.go:21-81`、`site_channel.go:23-40`
- `internal/server/handlers/apikey.go:22-56`、`user.go:19-64`、`webauthn.go:21-50`、`bootstrap.go:17-29`
- `internal/server/handlers/setting.go:41-70+`、`update.go:21-38`、`webdav.go:20`、`proxy_pool.go:17-31`、`node.go:22-42`
- `internal/server/handlers/plugin.go:28-45`（含 `/*path` catch-all）
- `internal/server/handlers/log.go:21-45`、`audit.go:16-26`、`error_log.go:57-78`、`notification.go:22-43`、`alert.go:24-37`、`report.go:19-46`、`analytics.go:17-58`、`stats.go:29-56`、`ops.go:15-36`、`usage_history.go:17-46`
- `internal/server/handlers/pool_oauth.go:719-720` — 302 → `/pool?oauth=...&pool_id=...`
- `internal/server/audit_route_test.go`（`TestAllManagementWriteRoutesAreAudited`）、`AGENTS.md:125,173-180,229,297,301`

**规模数据**
- `handlers/*.go` 中 `router.NewRoute(` 约 325 处（非测试）/ 330 处（含测试）— 由 `Select-String` 统计
- 顶级逻辑页面 14 个 — `nav-store.ts:20-35` / `route/config.tsx:35-50`

---

### 附：未确认项
- `web/out/` 与 `web/src/app` 的关系已确认为构建产物与源码；但 `web/out` 是否为当前源码的“最新”构建产物未逐文件比对（仅确认存在与结构）。
- 各子标签（Hub/Analytics/Ops）内部具体小组件的完整功能边界，本报告按 `web/README.md:96-117` 与组件首部注释概括，未逐一通读每个子组件实现。
