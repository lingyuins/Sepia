# new-api 前后端技术栈、升级方案与用户可见信息调查

- 调查对象：`D:\CodeWorkspace\new-api`（GitHub：`QuantumNous/new-api`）
- 调查方式：只读源码，未修改仓库任何文件
- 调查时间：2026-11-04
- 说明：文中 `文件:行号` 均相对于仓库根目录；已忽略 `.git` / `node_modules` / `dist`

---

## 一、前后端技术栈

### 1.1 后端

| 项 | 版本 / 实现 | 证据 |
|---|---|---|
| 语言 / 版本 | Go **1.26.10**（`go 1.26.10`；heroku 注释亦为 go1.26.10） | `go.mod:3`、`go.mod:4` |
| HTTP 框架 | **gin** `v1.9.1`（含 cors、gzip、static 中间件） | `go.mod:21`、`main.go` 中 `gin.New()` |
| ORM | **gorm** `v1.25.12` | `go.mod:63` |
| 主库驱动 | **SQLite** `glebarez/sqlite v1.11.0`（纯 Go，底层 modernc.org/sqlite）、**MySQL** `gorm.io/driver/mysql v1.5.7`、**PostgreSQL** `gorm.io/driver/postgres v1.5.9` | `go.mod:22`、`go.mod:61`、`go.mod:62` |
| 日志库驱动 | **ClickHouse** `gorm.io/driver/clickhouse v0.6.0`（仅用于 `LOG_SQL_DSN`，主库不支持 ClickHouse） | `go.mod:68`、`model/main.go:140-176` |
| 缓存 / 会话 | Redis `go-redis/redis/v8 v8.11.5`（可选，`REDIS_CONN_STRING`） | `go.mod:26` |
| 认证 | JWT `golang-jwt/jwt/v5 v5.3.0`；Casbin `casbin/v2 v2.135.0`（RBAC/权限）；WebAuthn `go-webauthn/webauthn v0.14.0`；2FA `pquerna/otp` | `go.mod:28`、`go.mod:16` |
| 支付 | Stripe `stripe-go/v81 v81.4.0`，另有 epay、creem、waffo、waffo-pancake SDK | `go.mod:45`、`go.mod:14` 等 |
| JS 插件引擎 | `Calcium-Ion/moejs v0.1.0-alpha.7`（Go 内嵌 JS 引擎，供 `pkg/jsplugin` 使用） | `go.mod:169`、`pkg/jsplugin/engine.go:22` |
| i18n（后端） | `nicksnyder/go-i18n/v2 v2.6.1`；语言 `zh-CN` / `zh-TW` / `en` | `go.mod:35`、`i18n/i18n.go:20-23,44` |
| 入口 | `main.go`：加载 `.env`、初始化 DB/Redis/i18n、注册路由并启动 HTTP | `main.go` |

**独立子模块（各自有 go.mod，通过 `replace ... => ./xxx` 引入）：**

| 模块 | 用途 | 版本证据 |
|---|---|---|
| `relaykit` | 协议转换库：OpenAI Chat / OpenAI Responses / Anthropic Messages / Gemini `generateContent` 四种协议之间转换请求、非流式与流式响应；可脱离宿主复用 | `relaykit/go.mod`（`go 1.25.1`，`module .../relaykit`）、`relaykit/README.md`、`go.mod:176` |
| `tokenkit` | Token 计数/分词与图片相关工具（tiktoken） | `tokenkit/go.mod`（`go 1.26`，`tiktoken-go/tokenizer v0.8.1`）、`go.mod:178` |

- `relaykit` 目录结构：`dto/`、`reasonmap/`、`relayconvert/`、`types/`（`relaykit/` 目录）。
- 主模块通过 `replace` 指向本地子模块：`go.mod:176`（relaykit）、`go.mod:178`（tokenkit）。

**`pkg/jsplugin`（JS 插件引擎）**：
- 基于 `moejs` 实现 JS 插件注册、编译、钩子调用与沙箱限制；默认调用超时 5s、并发 8（`pkg/jsplugin/engine.go:24-30`）。
- 提供 `new-api plugin lint/test` CLI 子命令，`main.go` 在 `os.Args[1]=="plugin"` 时进入（`main.go:49-52`、`pkg/jsplugin/cli.go:1-40`）。
- 插件 key/版本/清单校验见 `pkg/jsplugin/registry.go:20-40`；内置任务插件源码在 `plugins/tasks`（`plugins/embed.go`）。

**数据库支持汇总**（`common/database.go:3-8`、`model/main.go:140-198`）：
- 主库：SQLite（默认，DSN 为本地文件）、MySQL、PostgreSQL。
- 可选独立日志库：`LOG_SQL_DSN`，可为 SQLite/MySQL/PostgreSQL/**ClickHouse**（ClickHouse 仅允许作为日志库，见 `model/main.go:146-155` 的报错）。
- 默认 SQLite 路径 `one-api.db`，WAL + `busy_timeout=30000` + `_txlock=immediate`（`common/database.go:60`）。

### 1.2 前端

| 项 | 版本 / 实现 | 证据 |
|---|---|---|
| 框架 | **React** `^19.2.7`、`react-dom ^19.2.7` | `web/package.json:65,67` |
| 构建工具 | **`@rsbuild/core` `^2.1.4`** + `@rsbuild/plugin-react ^2.1.0` + `@rsbuild/plugin-tailwindcss ^2.0.3` | `web/package.json:90,91` |
| 脚本 | `dev = rsbuild dev`、`build = rsbuild build`、`preview = rsbuild preview` | `web/package.json:7,8` |
| 路由 | **`@tanstack/react-router` `^1.170.17`**（文件式路由，`@tanstack/router-plugin` 自动生成并做生产路由级代码分割） | `web/package.json:41`、`web/rsbuild.config.ts` |
| 数据请求 | **`@tanstack/react-query` `^5.101.2`**；HTTP 客户端 `axios ^1.20.0` | `web/package.json:40,48` |
| 表格 | **`@tanstack/react-table` `^8.21.3`**，虚拟滚动 `@tanstack/react-virtual` | `web/package.json:42` |
| UI 基础库 | `@base-ui/react ^1.6.0` + `tailwindcss ^4.3.2` + `lucide-react` + `@hugeicons/react`（`shadcn` 作为生成工具） | `web/package.json:27,79` |
| 图表 | `recharts 3.9.1`、`@visactor/react-vchart ^2.1.7` | `web/package.json:73` |
| 状态管理 | `zustand ^5.0.14` | `web/package.json:87` |
| i18n（前端） | **`i18next ^26.5.3`** + `react-i18next ^17.1.0` + `i18next-browser-languagedetector` | `web/package.json:54,69` |
| 包管理器 | **`bun@1.4.4`** | `web/package.json:126` |
| 构建配置 | `web/rsbuild.config.ts`：入口 `./src/main.tsx`、别名 `@ -> ./src`、`html.template ./index.html`、`output.distPath.root = dist`、开发代理 `/api`、`/v1`、`/mj`、`/pg` -> `VITE_REACT_APP_SERVER_URL`（默认 `http://localhost:3000`） | `web/rsbuild.config.ts` |

- **无 Vite**：仓库无 `vite.config.*`，`web` 目录仅有 `rsbuild.config.ts` / `vitest.config.ts` / `tsconfig*.json`（`web/` 文件清单）；`vitest` 仅用于单测。
- 目录结构（`web/src`）：`assets`、`components`、`config`、`context`、`features`、`hooks`、`i18n`、`lib`、`routes`、`stores`、`styles`、`main.tsx`、`routeTree.gen.ts`。
- 页面按 `features/*` 组织（`about/auth/channels/chat/dashboard/errors/home/keys/legal/model-pricing/models/performance-metrics/playground/pricing/profile/rankings/redemption-codes/security/setup/subscriptions/system-info/system-settings/system-update/task-plugins/usage-logs/users/wallet`），路由由 `web/src/routes/**` 生成（见 `routeTree.gen.ts`）。

**前端如何被 Go 内嵌**：
- `main.go` 用 `//go:embed web/dist` 与 `//go:embed web/dist/index.html` 将前端构建产物打进二进制（`main.go:43-47`）。
- 路由 `router.SetWebRouter` 通过 `common.EmbedFolder(BuildFS, "web/dist")` 提供静态文件，未命中时回落到 `indexPage`（`router/web-router.go:21-40`、`common/embed-file-system.go`）。
- `main.go` 还会向 `index.html` 注入 Umami / Google Analytics 脚本（`main.go:246-280`）。
- 注意：仓库工作区中 `web/dist` **不存在**（构建产物），需先 `bun run build` 才能编译 Go（`makefile` `build-web`）。

### 1.3 i18n（国际化）

- 后端：`i18n/locales/en.yaml`、`zh-CN.yaml`、`zh-TW.yaml`，支持语言 `en / zh-CN / zh-TW`（`i18n/i18n.go:44`、`i18n/i18n.go:209-210`）。
- 前端：`web/src/i18n/locales/` 下 `en.json`、`zh.json`、`zh-TW.json`、`fr.json`、`ja.json`、`ru.json`、`vi.json` 共 **7 种**界面语言；语言选项见 `web/src/i18n/languages.ts:20-27`。

### 1.4 是否有 Electron / Desktop

**有**，`electron/` 目录存在：
- `electron/main.js` 会 **spawn 打包进应用的 Go 二进制**（`new-api` / `new-api.exe`），固定监听 `PORT=3000`，桌面端 SQLite 写在应用数据目录（`electron/main.js:11,155-277`，`env.SQLITE_PATH = path.join(dataDir, 'new-api.db')` `electron/main.js:264`）。
- `electron/package.json` 的 `extraResources` 把 `../new-api`（或 `../new-api.exe`）作为 `bin/new-api` 一起打包（`electron/package.json`），构建目标：macOS dmg/zip、Windows nsis/portable、Linux AppImage/deb。
- CI：`.github/workflows/electron-build.yml`（当前仅在 `windows-latest` 构建）。

### 1.5 部署方式一览

| 部署形态 | 说明 | 证据 |
|---|---|---|
| Docker 镜像 | 多阶段构建：`oven/bun:1.4.4` 构建前端 → `golang:1.26.10-alpine` 编译后端（`-ldflags` 写入版本）→ `debian:bookworm-slim` 运行，暴露 3000 | `Dockerfile:1-8,10,29,31,40` |
| 镜像发布 | CI 构建 amd64/arm64 多架构并推送到 **Docker Hub `calciumion/new-api`**（`latest` 与 `<tag>`），cosign 签名 | `.github/workflows/docker-build.yml:1,74,84-85`、`docker-compose.yml:19` |
| docker-compose | 服务 `new-api` 使用 `image: calciumion/new-api:latest`，`./data:/data`、`./logs:/app/logs`，默认 PostgreSQL + Redis，端口 `3000:3000` | `docker-compose.yml:19-32` |
| Release 二进制 | CI 按 tag 构建 Linux(amd64/arm64)/macOS/Windows 二进制，附 checksums，通过 softprops/action-gh-release 发布 GitHub Release | `.github/workflows/release.yml:1,51,56,61,105` |
| Electron 桌面 | 打包 Go 二进制，产出安装包（Windows nsis/portable 等） | `electron/package.json`、`electron/main.js` |
| 版本号注入 | 默认 `common.Version = "v0.0.0"`，构建时用 `-X '.../common.Version=$VERSION'` 覆盖；也可由环境变量 `VERSION` 覆盖 | `common/constants.go:14`、`common/init.go:35-38`、`Dockerfile:29`、`release.yml:51` |

---

## 二、升级方案

### 2.1 结论

> **new-api 没有“应用内自更新”能力（不会在程序内下载并替换自身二进制 / 重启升级）。**
>
> 升级依赖**部署方式**：
> - Docker 部署：**重新拉取镜像**（`docker compose pull` + `up`）后重启容器；
> - 二进制部署：下载新版 **Release 二进制替换**后重启进程；
> - 桌面端：重新安装新版 **Electron 安装包**。
>
> 程序内唯一与“更新”相关的功能是**前端（仅管理员可见）的“检查更新”提示**：直接向 GitHub Releases API 查询最新 release，比较版本后显示“有新版本”徽标/对话框，并提供“前往 GitHub”链接——**只做提示与跳转，不下载、不执行升级**。

### 2.2 证据

1. **前端“检查更新”只读 GitHub，不下载**
   - `web/src/features/system-update/api.ts:45`：`fetch('https://api.github.com/repos/QuantumNous/new-api/releases?per_page=100')`，仅 `GET`，`credentials: 'omit'`。
   - `web/src/features/system-update/releases.ts`：`parseSystemVersion` / `compareSystemVersions` / `selectLatestRelease` 做版本解析与比较。
   - `web/src/features/system-update/use-system-update.ts`：`enabled: isAdmin && visible && online`（仅管理员、页面可见、在线时才检查），`checkNow` 只触发 `refetch`。
   - `web/src/features/system-update/system-update-dialog.tsx`：底部按钮为 **“前往 GitHub / Go to GitHub”**，链接 `getSystemReleaseUrl(release)` 指向 `https://github.com/QuantumNous/new-api/releases/tag/<tag>`；否则只有“重新检查 / 忽略此版本”，**无“下载/安装/重启”按钮或调用**。
   - 该组件接入位置：`components/layout/components/app-header.tsx:24,118`、`components/layout/components/public-header.tsx:30,237`、`features/system-settings/maintenance/update-checker-section.tsx:21,57`（后者展示“当前版本 / 启动时长 + 检查更新”）。`SystemUpdateAction` 对非管理员 `return null`（`system-update-action.tsx:44-49`）。

2. **后端完全没有自更新实现**
   - 全仓库非测试 Go 代码中未发现下载/替换自身的逻辑：`exec.Command` 仅用于打开 URL（`common/utils.go:34-38`），无 `os.Rename` 替换二进制、无自更新接口、无 GitHub API 调用。
   - `main.go` 的 `CHANNEL_UPDATE_FREQUENCY` → `controller.AutomaticallyUpdateChannels`（`main.go:123-128`、`controller/channel-billing.go:781`）是**上游渠道余额/模型自动刷新**，与软件版本升级无关，勿混淆。

3. **版本展示位置**
   - 后端 `GET /api/status` 返回 `"version": common.Version`（`controller/misc.go:41,51`）。
   - 全局响应头 `X-New-Api-Version`（`middleware/cors.go:18-23`）。
   - 管理员在页头（`SystemUpdateAction presentation='version'`）与系统设置“系统维护”页看到“当前版本”。

4. **发布/升级链路由 CI 与部署完成**
   - 镜像：`.github/workflows/docker-build.yml` 推送 `calciumion/new-api`（`docker-build.yml:74,84-85`）；`docker-compose.yml:19` 使用 `calciumion/new-api:latest`。
   - 二进制/安装包：`.github/workflows/release.yml`（`release.yml:61` GitHub Release）、`.github/workflows/electron-build.yml`。
   - `VERSION` 文件在仓库工作区为 **0 字节空文件**（`D:\CodeWorkspace\new-api\VERSION` Length=0，`2026/11/3`），由 CI 在打 tag 时写入（`docker-build.yml:44` “Resolve tag & write VERSION”、`Dockerfile:8` 读取）。

### 2.3 数据库迁移（升级时如何执行、是否破坏数据）

**迁移方式：启动时由 Go 代码自动执行（GORM AutoMigrate + 若干自定义迁移），无独立迁移框架/迁移脚本目录（未使用 golang-migrate 之类）。**

- 启动顺序：`main.go` `InitResources()` → `model.InitDB()`（`main.go:303`）→ `model.CheckSetup()`（`main.go:319`）→ `MigrateRetiredFrontendOptions`（`main.go:323`）→ `model.InitOptionMap()`（`main.go:327`）→ `model.InitLogDB()`（`main.go:333`）。
- 主库迁移：`model.InitDB()` 成功后，**仅主节点**执行 `migrateDB()`（`model/main.go:216-227`：`if !common.IsMasterNode { ... return nil }`）。
- `migrateDB()`（`model/main.go:325`）执行：
  - 自定义迁移：`migrateTokenKeyUniqueness`、`migratePrefillGroupUniqueness`、`migrateSubscriptionPlanPriceAmount`（价格 float→decimal）、`migrateTokenModelLimitsToText`、`migrateOptionPrimaryKey`（`model/main.go:326-341`）。
  - `DB.AutoMigrate(...)` 一次性建表/补列，覆盖 Channel、Token、User、UserSession、Option、Redemption、Ability、Log、Midjourney、TopUp、QuotaData、Task、TaskPlugin、Model、Vendor、PrefillGroup、Setup、TwoFA、SubscriptionOrder、UserSubscription、PerfMetric、SystemInstance、SystemTask、CasbinRule、AuthzRole、UserAccessToken 等（`model/main.go:342-395`）。
  - 初始化用户鉴权版本、遗留 access token 退休时间、外部身份映射（`model/main.go:386-394`）。
  - SQLite 与其它库对 `SubscriptionPlan` 分别用专用建表/`AutoMigrate`（`model/main.go:396-400`）。
- 日志库迁移：`migrateLOGDB()`（`model/main.go:404`）：`MigrateAuditLogs` → ClickHouse 走专用建表 SQL，否则 `LOG_DB.AutoMigrate(&Log{})`（`model/main.go:404-411`）。
- 自定义 dialector 处理 schema 比较（如 MySQL decimal 默认值、Postgres prepared statement 兼容），见 `model/migration_dialector.go`、`model/gorm_logger.go`。

**升级是否会破坏数据 / 版本兼容：**

- 表结构变更主要走 **GORM AutoMigrate（增量、建表/补列）**，不带版本号迁移记录，重复执行幂等。
- 有一处**显式的破坏性兼容门禁**（重要）：`ensureUserQuotaColumns()` 在迁移前检查 `users` 表 `quota / used_quota / aff_quota / aff_history` 是否为 64 位整型；MySQL/PostgreSQL 上若检测到旧的 **32 位钱包 schema，会报错拒绝启动**，要求运维先显式迁移；可用 `SKIP_64BIT_QUOTA_SCHEMA_CHECK=true` 跳过检查（`model/main.go:204,281-322`）。也就是说：普通增量升级不删数据；但从 32 位额度 schema 升级需要人工迁移，否则启动失败（不会静默清空数据）。
- SQLite 用 WAL + 事务，启动迁移失败会 `panic`/`FatalLog`（`model/main.go:200-202`）。
- `Setup.Version` 记录首次初始化时的版本（`model/main.go:101-108` `CheckSetup`），但全仓库中 **未发现用 `Setup.Version` 做升级判断/迁移分派**（仅日志打印 `InitializedAt`，`model/main.go:115`），即**没有基于版本的升级脚本链**。
- 未发现“破坏性 drop 列/删表”的自动迁移；但 GORM AutoMigrate 的边界（例如类型收窄、复杂索引变更）仍需以具体版本实测确认（见“五、未确认项”）。

---

## 三、用户在正常使用中能获得的信息

> 说明：new-api 是“LLM 网关 + AI 资产管理”面板。下面按前端页面 / 路由、对应后端接口、可见内容（指标/数据）分类。角色列：**普通用户**＝登录用户；**管理员**＝admin/root；**超管**＝super admin/root。

### 3.1 控制台通用信息（页面 + 接口 + 指标）

| 页面 / 路由 | 前端实现 | 后端接口 | 用户能看到什么 | 角色 |
|---|---|---|---|---|
| 控制台概览 `/dashboard/overview` | `web/src/features/dashboard/components/overview/*` | `GET /api/data`（管理员）、`GET /api/data/self`（本人）；`GET /api/uptime/status`；`GET /api/status` | **余额/剩余额度**、近 24h 消耗、历史累计消耗、请求总数、余额可用天数/健康度、**平均 RPM/TPM**、公告（announcements）、API 信息、FAQ、Uptime 监控面板 | 用户 / 管理员 |
| 模型调用分析 `/dashboard/models` | `features/dashboard/components/models/*`、`hooks/use-dashboard-config.tsx` | `GET /api/data[/self]`；`GET /api/perf-metrics/summary`、`GET /api/perf-metrics` | **总请求数（Total Count）**、**总消耗额度（Total Quota）**、**总 Tokens（Total Tokens）**、**缓存读/缓存写（Cache Read/Write）**、平均 RPM/TPM、模型调用趋势与消耗分布、性能（成功率/平均延迟/吞吐） | 用户 / 管理员 |
| 流量 Flow `/dashboard/flow` | `features/dashboard/components/flow/*`、`lib/flow.ts` | `GET /api/data/flow`、`GET /api/data/flow/self` | **用户→节点→令牌→分组→模型→渠道** 的用量/请求量/额度 Sankey 流量图（可按指标切换 quota/tokens/requests） | 用户（本人）/ 管理员（全部） |
| 用户分析 `/dashboard/users` | `features/dashboard/components/users/user-charts.tsx` | `GET /api/data/users` | 各用户维度用量对比 | 管理员 |
| 使用日志 `/usage-logs/common` | `features/usage-logs/*` | `GET /api/log`（管理员）/ `GET /api/log/self`；`GET /api/log/stat` / `GET /api/log/self/stat`；`GET /api/log/export` / `GET /api/log/self/export` | 每条日志的**时间/类型/内容/用户名/令牌名/模型名/额度/输入与输出 tokens/耗时/是否流式/渠道/分组/IP/请求 ID/上游请求 ID**；汇总 **quota、rpm、tpm**；错误日志（type=5） | 用户（本人）/ 管理员（全部） |
| 审计日志 `/usage-logs/audit` | `features/usage-logs`（audit section） | `GET /api/audit`（需 audit 权限）、`GET /api/audit/self` | 操作审计记录 | 管理员 / 用户（本人） |
| 任务日志 `/usage-logs/task`、`/usage-logs/drawing` | `features/usage-logs`（task/drawing section） | `GET /api/task` / `GET /api/task/self`；`GET /api/mj/` / `GET /api/mj/self`；`GET /api/task/:task_id/artifacts` | 绘图（Midjourney）与异步任务（Suno/视频等）任务状态与产物 | 用户（本人）/ 管理员（全部） |
| API 密钥 `/keys` | `features/keys/*` | `GET /api/token`、`GET /api/token/:id`、`POST /api/token/:id/key`、`GET /api/token/auto-groups` | 自己的令牌列表：名称、额度、分组、模型限制、过期时间、可用分组等 | 用户 |
| 模型广场 / 定价 `/pricing`、`/pricing/$modelId` | `features/pricing/*`、`features/model-pricing/*` | `GET /api/pricing`；`GET /api/models`（目录）、`GET /api/user/self/models` | **模型价格与倍率**：模型倍率 `model_ratio`、补全倍率 `completion_ratio`、缓存读/写倍率、按次价格 `model_price`、分组倍率 `group_ratio`、可用分组 `usable_group`、支持端点 `supported_endpoint`、供应商 `vendors`、模型元数据（上下文长度、能力、模态等）、动态计费表达式 | 公开/用户 |
| 排行榜 `/rankings` | `features/rankings/*` | `GET /api/rankings?period=today|week|month|year` | **模型 / 供应商 Token 用量排行**、份额、环比增长率、Top movers/droppers、历史趋势序列 | 公开/用户 |
| 性能指标（面板） | `features/performance-metrics/*`、dashboard `performance-overview.tsx` | `GET /api/perf-metrics/summary`、`GET /api/perf-metrics` | 指定模型/时间窗的成功率、平均延迟、吞吐等性能数据 | 公开（经 pricing 模块鉴权） |
| Playground / Chat `/playground`、`/chat/$chatId` | `features/playground/*`、`features/chat/*` | 走 `/v1/*` relay 接口（如 `/v1/chat/completions`）、`GET /v1/models` | 在线对话/测试、模型列表与流式返回 | 用户 |
| 钱包 `/wallet` | `features/wallet/*` | `GET /api/user/topup/info`、`GET /api/user/self/topup`、`POST /api/user/topup`、`POST /api/user/amount`、`POST /api/user/pay`、`/api/user/stripe|creem|waffo...`、`GET /api/user/aff`、`POST /api/user/aff_transfer` | 余额、充值方式与汇率、充值/兑换码历史、邀请返佣、支付订单 | 用户 |
| 订阅 `/subscriptions` | `features/subscriptions/*` | `GET /api/subscription/plans`、`GET /api/subscription/self`、`PUT /api/subscription/self/preference`、`POST /api/subscription/*/pay` | 套餐列表、我的订阅状态、配额重置周期/偏好 | 用户 |
| 个人资料 `/profile`、安全 `/security` | `features/profile/*`、`features/security/*` | `GET/PUT /api/user/self`、`GET /api/user/self/sessions`、`GET /api/user/self/passkey`、2FA 接口、`GET /api/user/self/groups` | 账户资料、余额、分组、登录会话、2FA/Passkey 状态 | 用户 |
| 关于 `/about`、协议/隐私 | `features/about/*`、`features/legal/*` | `GET /api/about`、`GET /api/user-agreement`、`GET /api/privacy-policy`、`GET /api/notice`、`GET /api/home_page_content` | 站点介绍、用户协议、隐私政策、公告、首页内容 | 公开 |
| 首页 `/` | `features/home/*` | `GET /api/home_page_content`、`GET /api/status` | 首页 Hero/统计/特性/使用流程、站点信息与导航开关 | 公开 |

### 3.2 管理端信息

| 页面 / 路由 | 前端实现 | 后端接口 | 用户能看到什么 | 角色 |
|---|---|---|---|---|
| 渠道 `/channels` | `features/channels/*` | `GET /api/channel/`、`/search`、`/models`、`/:id`、`/test`、`/update_balance`、`/ops` 等（`router/channel-router.go`） | 渠道列表与状态、余额、可用模型、测试结果、优先级/分组、上游模型更新检测 | 管理员 |
| 模型管理 `/models/metadata`、`/models/vendors`、`/models/deployments` | `features/models/*`（section-registry：Models / Vendors / Deployments） | `GET /api/models/`、`/vendors`、`/deployments`、`/missing`、`/sync_upstream/*` | 模型元数据、供应商、缺失模型、上游同步、io.net 部署 | 管理员 |
| 用户管理 `/users` | `features/users/*` | `GET /api/user/`、`/search`、`/:id`、`POST /api/user/manage` 等 | 用户列表、额度、分组、状态、登录绑定、2FA 统计 | 管理员 |
| 兑换码 `/redemption-codes` | `features/redemption-codes/*` | `GET /api/redemption/`、`/search`、`/:id` | 兑换码列表、状态、额度、有效期 | 管理员 |
| 订阅管理 `/subscriptions`（管理视图） | `features/subscriptions/*` | `GET /api/subscription/admin/plans`、`/users/:id/subscriptions` 等 | 套餐配置、用户订阅、重置/失效 | 管理员 |
| 系统信息 `/system-info` | `features/system-info/*` | `GET /api/system-info/instances`、`DELETE /api/system-info/stale-instances`、`GET /api/system-task/list|current|:task_id`、`GET /api/performance/stats` | **在线实例（多节点）**、系统任务历史/当前任务、性能与磁盘/日志统计 | 超管 |
| 任务插件 `/task-plugins` | `features/task-plugins/*` | `GET /api/plugin/task`、`/runtime/status`、`/marketplace/sources`、`/:key/versions` 等 | 插件列表/版本/运行时状态、市场源 | 超管 |
| 系统设置 `/system-settings/*` | `features/system-settings/*`（site/auth/billing/models/request-policies/security/content/operations/maintenance） | `GET/PUT /api/option/`、`/model_pricing`、`/request_policy` 等 | 站点与品牌、认证、计费与支付、模型与倍率、请求策略、安全限制、控制台内容、运维（含**系统维护：当前版本、启动时长、检查更新**） | 超管 |

### 3.3 “用户能看到哪些指标/数据”小结

- **额度类**：余额 `quota`、已用 `used_quota`、请求次数 `request_count`、消耗额度、近 24h 消耗、可用天数。
- **用量类（按时间/模型/用户/渠道/分组/令牌维度）**：请求数 `count`、tokens `token_used`、缓存读 `cache_read_tokens`、缓存写 `cache_creation_tokens`、额度 `quota`；聚合 RPM/TPM。
- **性能类**：成功率、平均延迟、吞吐（`/api/perf-metrics`）。
- **价格类**：模型倍率、补全倍率、缓存倍率、按次价格、分组倍率、可用分组、支持端点、供应商。
- **排行类**：模型/供应商 token 排行、份额、增长率、movers/droppers、历史趋势。
- **日志类**：完整请求/消费/错误/管理/充值/退款/登录日志字段（`model/log.go` Log 结构：`type/content/user_id/username/token_name/model_name/quota/prompt_tokens/completion_tokens/use_time/is_stream/channel/channel_name/token_id/group/ip/request_id/upstream_request_id/other`）。
- **系统类**：版本号 `version`、启动时间 `start_time`、在线实例、系统任务、Uptime、公告/API 信息/FAQ。
- 版本相关：`GET /api/status` 的 `data.version`，以及响应头 `X-New-Api-Version`。

---

## 四、证据（文件:行号）

**后端技术栈 / 入口**
- `go.mod:3` heroku goVersion go1.26.10；`go.mod:4` `go 1.26.10`
- `go.mod:16` casbin v2.135.0；`go.mod:21` gin v1.9.1；`go.mod:22` glebarez/sqlite v1.11.0；`go.mod:26` go-redis v8.11.5；`go.mod:28` jwt v5.3.0；`go.mod:45` stripe-go v81.4.0；`go.mod:61-63` mysql/postgres/gorm；`go.mod:68` clickhouse driver；`go.mod:169` moejs
- `go.mod:176` replace relaykit => ./relaykit；`go.mod:178` replace tokenkit => ./tokenkit
- `relaykit/go.mod`（module、`go 1.25.1`）；`relaykit/README.md`（四协议转换说明）
- `tokenkit/go.mod`（`go 1.26`、tiktoken-go）
- `main.go:43-47` `//go:embed web/dist`、`indexPage`；`main.go:49-52` `plugin` 子命令
- `main.go:303` `model.InitDB()`；`main.go:319` `CheckSetup`；`main.go:323` `MigrateRetiredFrontendOptions`；`main.go:327` `InitOptionMap`；`main.go:333` `InitLogDB`
- `main.go:123-128` `CHANNEL_UPDATE_FREQUENCY` → `AutomaticallyUpdateChannels`（渠道刷新，非软件升级）
- `common/constants.go:14` `Version = "v0.0.0"`；`common/init.go:35-38` 环境变量 `VERSION` 覆盖
- `common/database.go:3-8` 数据库类型常量；`common/database.go:60` SQLite DSN
- `model/main.go:140-198` `chooseDB`（SQLite/MySQL/PostgreSQL/ClickHouse-by-log）；`model/main.go:146-155` ClickHouse 仅日志库
- `pkg/jsplugin/engine.go:24-30` 默认超时/并发；`pkg/jsplugin/registry.go:20-40` 插件 key/版本校验；`pkg/jsplugin/cli.go:1-40` plugin CLI
- `router/web-router.go:21-40` 前端静态服务；`common/embed-file-system.go` 嵌入文件系统

**前端技术栈 / 构建 / 内嵌**
- `web/package.json:7-8` rsbuild dev/build；`:27` @base-ui/react；`:40` react-query；`:41` react-router；`:42` react-table；`:48` axios；`:54` i18next；`:65,67` react/react-dom 19.2.7；`:69` react-i18next；`:73` recharts；`:79` tailwindcss；`:87` zustand；`:90-91` @rsbuild/core、plugin-react；`:126` bun@1.4.4
- `web/rsbuild.config.ts`（入口、别名、代理、dist 输出、tanstackRouter 代码分割）
- `web/src/i18n/languages.ts:20-27` 7 种界面语言；`web/src/i18n/locales/*.json`
- `i18n/i18n.go:44` 加载 en/zh-CN/zh-TW；`i18n/i18n.go:209-210` `SupportedLanguages`

**升级方案证据**
- `web/src/features/system-update/api.ts:45` `https://api.github.com/repos/QuantumNous/new-api/releases?per_page=100`
- `web/src/features/system-update/releases.ts` `parseSystemVersion` / `compareSystemVersions` / `selectLatestRelease` / `getSystemReleaseUrl`
- `web/src/features/system-update/use-system-update.ts` `enabled: isAdmin && visible && online`
- `web/src/features/system-update/system-update-dialog.tsx` “前往 GitHub”按钮
- `web/src/features/system-update/system-update-action.tsx:44-49` 非管理员不渲染
- `web/src/components/layout/components/app-header.tsx:24,118`、`public-header.tsx:30,237`、`features/system-settings/maintenance/update-checker-section.tsx:21,57` 接入
- `controller/misc.go:41,51` `/api/status` 返回 `version`
- `middleware/cors.go:18-23` `X-New-Api-Version`
- `Dockerfile:1-8,10,29,31,40` 多阶段构建与版本注入
- `.github/workflows/docker-build.yml:1,74,84-85` 镜像发布 `calciumion/new-api`
- `.github/workflows/release.yml:1,51,56,61,105` 二进制 Release
- `.github/workflows/electron-build.yml` 桌面构建
- `docker-compose.yml:19-32` 镜像/端口/数据卷/DB DSN
- `common/utils.go:34-38` `exec.Command` 仅打开 URL（无自更新）

**DB 迁移证据**
- `model/main.go:216-227` 仅主节点迁移
- `model/main.go:281-322` `ensureUserQuotaColumns` 32 位额度 schema 门禁；`:285-286` `SKIP_64BIT_QUOTA_SCHEMA_CHECK`
- `model/main.go:325-341` `migrateDB` 自定义迁移；`:342-395` `AutoMigrate` 表清单；`:396-400` SubscriptionPlan；`:404-411` `migrateLOGDB`
- `model/migration_dialector.go` 自定义 dialector；`model/token_migration.go`、`model/option_primary_key_migration.go`、`model/frontend_option_migration.go` 自定义迁移实现
- `model/main.go:101-108` `CheckSetup` 写 Setup.Version；`model/main.go:115` 仅打印 InitializedAt（未用于升级判断）

**用户可见信息证据**
- `controller/usedata.go:31,48,63,85,104` 额度/流量数据接口实现
- `controller/log.go:57,75,98,103,173,178,203,230` 日志/统计/导出/搜索实现
- `controller/rankings.go:10` `GetRankings`；`service/rankings.go:19-100` 排行榜数据结构
- `controller/pricing.go:38` `GetPricing`（返回 pricing/vendors/group_ratio/usable_group/supported_endpoint）
- `controller/perf_metrics.go:15,44` 性能指标接口
- `controller/misc.go:179,186,193,197,212` Notice/About/UserAgreement/PrivacyPolicy/HomePageContent
- `controller/misc.go:41-...` GetStatus 返回大量站点/开关/展示配置
- `model/log.go` `Log` 结构字段与日志类型常量（1 充值/2 消费/3 管理/4 系统/5 错误/6 退款/7 登录）
- `router/api-router.go:28,30,32,38-45,283,299,323,343,351` 主要接口注册
- `router/dashboard.go` 兼容旧版 OpenAI billing 接口 `GET /dashboard/billing/subscription|usage`、`/v1/dashboard/billing/*`
- 前端页面实现：`web/src/features/dashboard/`、`usage-logs/`、`pricing/`、`rankings/`、`wallet/`、`subscriptions/`、`system-info/`、`performance-metrics/`；导航 `web/src/hooks/use-sidebar-data.ts`、`web/src/hooks/use-top-nav-links.ts`
- 仪表盘指标定义：`web/src/features/dashboard/hooks/use-dashboard-config.tsx`（Total Count / Total Quota / Total Tokens / Cache Read / Cache Write / Average RPM / Average TPM；Last 24h usage / Historical Usage / Request Count）；`web/src/features/dashboard/types.ts` `QuotaDataItem`/`FlowQuotaDataItem`

---

## 五、未确认项

1. **当前实际发布版本号**：仓库 `VERSION` 文件为 0 字节（`D:\CodeWorkspace\new-api\VERSION` Length=0），版本由 CI 打 tag 时写入（`docker-build.yml:44`）；无法仅从工作区确定“当前线上版本”。
2. **`web/dist` 未包含在仓库**：调查时 `web/dist` 不存在，无法核对实际打包产物与路由清单是否与源码路由 100% 一致（本文以前端源码 `routes/` 与 `features/` 为准）。
3. **GORM AutoMigrate 的破坏性边界**：未见显式 drop 列/删表逻辑，但 AutoMigrate 对“类型收窄、默认值/索引变更”等边界行为依赖 GORM 具体实现，需按具体升级版本实测确认；仓库**无版本化迁移记录表**，也无 downgrade。
4. **32 位额度 schema 迁移的具体手工步骤**：代码只做“拒绝启动”门禁（`model/main.go:281-322`），未在仓库内找到对应的转换脚本/文档；实际迁移 SQL 需参考官方发布说明（未在本次只读范围内确认）。
5. **`Setup.Version` 的用途**：确认仅写入与日志打印，未发现用于升级判断；但不排除外部/未来版本使用（当前源码未使用）。
6. **多节点迁移一致性**：迁移只在 `IsMasterNode` 执行（`model/main.go:216-227`），非主节点如何被 `NODE_NAME`/master 选举标记的完整机制未在本次深挖（`common/node_identity.go` 未展开）。
7. **前端“检查更新”是否对匿名访客可见**：`SystemUpdateAction` 对非管理员 `return null`（`system-update-action.tsx:44-49`），据此判断匿名/普通用户看不到；但 `public-header` 也引入了该组件，实际是否受角色门控以运行结果为准（源码层面为 null 不渲染）。
8. **前端 i18n 语言与后端通知语言的关系**：前端 7 种界面语言，后端通知/日志语言仅 en/zh-CN/zh-TW（`i18n/i18n.go:209-210`），两者映射细节未逐一核对。
