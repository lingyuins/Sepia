# metapi 前后端技术栈、升级方案与用户可见信息调查

> 调查对象：`D:\CodeWorkspace\metapi`（TypeScript/Fastify + React/Vite + Electron + drizzle，**“中转站的中转站”**）
> 调查方式：只读源码，未修改仓库；调查日期：2026-11-03。文中路径相对仓库根 `D:\CodeWorkspace\metapi`，`文件:行号` 均可回查。
> 定位：metapi 把多个 AI 中转站（New API / One API / OneHub / DoneHub / Veloera / AnyRouter / Sub2API / OpenAI / Claude / Gemini / Codex / Antigravity 等）聚合为一个统一网关（一个 API Key、一个入口），并提供模型自动发现、智能路由、成本优化、账号/余额/签到管理、代理日志、更新中心等运维能力（`README.md:5`、`src/server/db/schema.ts:7`）。

---

## 一、前后端技术栈

### 1.1 后端（`src/server`）

**运行时与框架**

- Node.js `>= 25`（`package.json:24-26`），ESM（`"type": "module"`，`package.json:27`）。
- **Fastify 5**（`fastify` `^5.8.4`，`package.json:81`；`@fastify/cors`、`@fastify/static`，`package.json:73-74`）。
- 入口 `src/server/index.ts`：
  - 启动前先做运行时数据库引导 `ensureRuntimeDatabaseReady(...)`（`index.ts:136-140`）；
  - 从 `settings` 表加载运行期配置覆盖、执行兼容列补丁、修复时间戳、迁移站点 Key、播种默认站点、重建路由（`index.ts:142-198`）；
  - 注册 CORS 与 `/api/*` 鉴权钩子（`index.ts:204-211`），注册全部管理路由与代理路由（`index.ts:214-234`）；
  - 生产模式用 `@fastify/static` 托管 `dist/web` 并做 SPA 回退（`index.ts:237-261`）；
  - 启动一批后台调度器（签到、公告轮询、模型可用性探测、渠道恢复探活、Sub2API 托管刷新、更新中心轮询、用量聚合投影、管理快照预热、OAuth 回环监听、代理文件保留）（`index.ts:264-291`）。

**数据库 / ORM（三方言）**

- **drizzle-orm**（`^0.45.2`，`package.json:78`）为 ORM；`drizzle-kit` 做迁移生成（`package.json:110`）。
- 驱动：**SQLite `better-sqlite3`**（`^12.10.0`）、**MySQL `mysql2`**（`^3.22.3`）、**PostgreSQL `pg`**（`^8.16.3`）（`package.json:76,86,89`）。
- **Schema 实际是“以 SQLite 为唯一真源”**：`src/server/db/schema.ts` 里 27 张表全部用 `sqliteTable(...)` 声明（`schema.ts:1-3`，`sqliteTable` 出现 27 次，`mysqlTable`/`pgTable` 为 0）。MySQL/PostgreSQL 的建表/升级 SQL 由 `schemaContract.json` 经代码生成器 `schemaArtifactGenerator.ts` 产出，落到 `src/server/db/generated/mysql.bootstrap.sql`、`postgres.bootstrap.sql`、`mysql.upgrade.sql`、`postgres.upgrade.sql`。
- 运行期三方言通过 **drizzle proxy 驱动**统一：`drizzle-orm/sqlite-proxy`、`mysql-proxy`、`pg-proxy`（`db/index.ts:4-6`），连接池分别是 `better-sqlite3` / `mysql.Pool` / `pg.Pool`（`db/index.ts:44-48`）；`runtimeDbDialect` 由 `config.dbType` 决定（`db/index.ts:46`）。
- 方言选择：`parseDbType()` 默认 `sqlite`，支持 `mysql|postgres`（`config.ts:46-51,117`）；运行期还可从 `settings` 里的 `db_type/db_url/db_ssl` 动态切换（`index.ts:109-171`）。
- 迁移与引导：SQLite 走 `drizzle/` 迁移（`drizzle/0000…0027_*.sql` + `meta/_journal.json`），由 `src/server/db/migrate.ts` 的 `runSqliteMigrations()` 执行（`migrate.ts:681-704`），并带历史 schema 引导、缺失记录回填、重复列/唯一索引冲突恢复重试（`migrate.ts:52,132-150,418-441,485-525`）；外部库走 `ensureRuntimeDatabaseSchema()` 生成式建表/升级（`runtimeSchemaBootstrap.ts:445-473`）。

**关键服务依赖**

- `rate-limiter-flexible`（`^11.1.0`，`package.json:90`）限流；`node-cron`（`^4.2.1`，`package.json:87`）定时；`nodemailer`（`^8.0.4`，`package.json:88`）邮件通知；`undici`（`^6.25.0`）HTTP；`socks` 代理；`zod` 入参校验；`ws`；`minimatch`（路由 glob）；`marked`（Markdown 渲染）。
- 中间件：`src/server/middleware/auth.ts`（管理端 AUTH_TOKEN 与代理端 PROXY_TOKEN/下游 Key 鉴权）、`requestRateLimit.ts`。
- 代理核心：`src/server/proxy-core/**`（conductor、executors、providers、transformers、surfaces、runtime、orchestration、capabilities、cliProfiles）。

**共享层 `src/shared`**

- 前后端共用纯 TS/JS 模块（`.js + .d.ts` 成对，便于 Node 与浏览器两端消费）：`tokenRoutePatterns`、`tokenRouteContract`、`platformIdentity`、`siteInitializationPresets`、`sitePrimaryUrl`、`proxyLogMeta`、`apiKeyBatch`、`updateCenterReminder`、`conversationFileTypes` 等（目录 `src/shared`）。

**鉴权边界**

- `/api/*` 经 `authMiddleware`，仅 `/api/desktop/health` 与 `/api/oauth/callback/*` 公开（`index.ts:207-211`、`desktop.ts:3-6`）。
- 代理入口为 `/v1/*`（`src/server/routes/proxy/router.ts:16-19` 注释 “Auth middleware for all /v1 routes”，各子路由如 `/v1/chat/completions`、`/v1/messages`），由 `proxyAuthMiddleware` 校验下游 Key/`PROXY_TOKEN`。

### 1.2 前端（`src/web`）

- **React 18** + **ReactDOM 18**（`package.json:115-116`），入口 `src/web/main.tsx`（`BrowserRouter` + `App`，`main.tsx:1-14`）。
- **Vite 6** 构建（`package.json:123`，`vite.config.ts`），`root: 'src/web'`、产物 `dist/web`，dev 端口 5173，`/api`、`/monitor-proxy`、`/v1` 反代到后端（`vite.config.ts:15-48`）。
- **react-router-dom 7**（`package.json:117`）管理页面路由（`src/web/App.tsx:862-880`）。
- **@visactor/react-vchart 2**（`package.json:75`）图表（单独 vendor chunk，`vite.config.ts:23-29`）；图表组件在 `src/web/components/*Chart.tsx`、`pages/Dashboard.tsx` 等使用。
- **@dnd-kit**（core/sortable/utilities，`package.json:70-72`）拖拽排序，用于路由优先级桶/渠道拖拽（`src/web/pages/token-routes/SortableChannelRow.tsx:124-125`、`SortableBucketSeparator.tsx`）。
- **Tailwind CSS 4**（`@tailwindcss/vite` + `tailwindcss`，`package.json:98,120`）+ 自定义 `src/web/index.css`；自研 i18n（`src/web/i18n.tsx`，中↔英映射）与主题。
- 页面清单：`src/web/pages/*`（Dashboard、Sites、SiteAnnouncements、Accounts、Tokens、OAuthManagement、DownstreamKeys、CheckinLog、TokenRoutes、ProxyLogs、Monitors、Settings、ProgramLogs、ImportExport、NotificationSettings、Models、ModelTester、About；`App.tsx:862-880`）。

### 1.3 桌面（`src/desktop`）

- **Electron 42**（`package.json:111`）+ **electron-builder 26**（`package.json:112`）+ **electron-updater 6**（`package.json:80`）+ **electron-log 5**（`package.json:79`）。
- `src/desktop/main.ts`：创建主窗口（`contextIsolation:true`、`nodeIntegration:false`，`main.ts:144-156`）、托盘菜单（打开、重启后端、开机自启、检查更新、打开日志、退出，`main.ts:80-121`）、**本地拉起后端**子进程（`spawn(process.execPath, [dist/server/index.js], ELECTRON_RUN_AS_NODE=1)`，`main.ts:214-227`）、崩溃重启对话框（`main.ts:277-323`）、单实例锁（`main.ts:413-421`）。
- 桌面命令：`dev:desktop`、`build:desktop`、`dist:desktop`、`start:desktop`（`package.json:31,35,42,51`）。
- 打包：`electron-builder.yml`（appId `me.cita777.metapi.desktop`，mac dmg/zip、win nsis/zip、linux AppImage/deb/rpm，`files: dist/**/* + drizzle/**/*`，`asar:false`，`afterPack/afterSign` 脚本，`electron-builder.yml:1-42`）。

### 1.4 部署形态一览

| 部署形态 | 运行/构建方式 | 关键文件 | 说明 |
|---|---|---|---|
| Docker / Docker Compose | 多阶段构建；运行镜像内置 `kubectl`+`helm`；容器启动 `node dist/server/db/migrate.js && node dist/server/index.js` | `docker/Dockerfile:3-62`、`docker/docker-compose.yml:1-25`、`docker/.env.example` | 镜像 `1467078763/metapi:latest`，数据卷 `/app/data`，通过 `AUTH_TOKEN`/`PROXY_TOKEN` 鉴权 |
| K3s / Kubernetes + Helm | 仓库自带最小 chart + Deploy Helper 清单 | `deploy/k3s/chart/*`、`deploy/k3s/metapi-deploy-helper.yaml` | 支持 `image.digest` 精确部署；配合“更新中心”人工触发 |
| Zeabur | 模板引用预构建镜像 `1467078763/metapi:latest`，挂载 `/app/data` 卷 | `zeabur-template.yaml:43-78` | 一键模板部署 |
| Render | `runtime: docker`，`dockerfilePath: ./docker/Dockerfile`，免费版用外部 MySQL/TiDB | `render.yaml:1-31` | 免费实例 15 分钟休眠，需 UptimeRobot 保活（`docs/deployment.md:76-91,145-155`）|
| 桌面（Windows/macOS/Linux） | electron-builder 产出安装包，Electron 内置本地后端 | `electron-builder.yml`、`src/desktop/main.ts` | 面向个人电脑本地使用（`docs/deployment.md:210-239`）|
| 文档站 | VitePress + Mermaid 插件 | `docs/.vitepress/config.ts:1-123`、`package.json:46-48,124-125` | 中文文档站，`docs:dev/build/preview` |

---

## 二、升级方案

metapi 的“升级”由三条互相独立的链路组成：**Update Center（服务端检查 + 通知 + 部署辅助）**、**Electron 自动更新（electron-updater）**、**Docker/Zeabur/Render 部署升级**；数据库 schema 另有独立的迁移/升级机制。三者互不替代。

### 2.1 Update Center（服务端检查 / 通知 / 部署辅助）

**定位**：Update Center 是“集群内人工触发升级面板”，而不是单容器自动升级器。官方文档明确：普通 Docker Compose 单容器**用不上**它，它依赖集群里的 Deploy Helper 执行 `helm upgrade` / `kubectl rollout status`（`docs/k3s-update-center.md:21-28,161-172`）。

**HTTP 接口（`src/server/routes/api/updateCenter.ts`）**

- `GET /api/update-center/status` 读取状态（缓存优先）（`updateCenter.ts:84-86`）
- `POST /api/update-center/check` 强制刷新检查（`updateCenter.ts:88-90`）
- `PUT /api/update-center/config` 保存配置（`updateCenter.ts:92-104`）
- `POST /api/update-center/deploy` 触发部署（后台任务，SSE 日志）（`updateCenter.ts:106-195`）
- `POST /api/update-center/rollback` 触发回退（`updateCenter.ts:197-264`）
- `GET /api/update-center/tasks/:id/stream` 部署任务 SSE 日志流（`updateCenter.ts:266-322`）

**配置项（`updateCenterConfigService.ts:7-17`）**：`enabled`、`helperBaseUrl`、`namespace`、`releaseName`、`chartRef`、`imageRepository`、`githubReleasesEnabled`、`dockerHubTagsEnabled`、`defaultDeploySource`（`github-release` | `docker-hub-tag`）。默认 `imageRepository=1467078763/metapi`、`enabled=false`（`updateCenterConfigService.ts:21-33`）；配置持久化在 `settings` 表（键见 `updateCenterConfigService.ts:19`）。

**版本发现（`updateCenterVersionService.ts`）**

- GitHub：`https://api.github.com/repos/cita-777/metapi/releases`，取最新**稳定** SemVer（跳过 draft/prerelease，`updateCenterVersionService.ts:48,106-131,283-291`）。
- Docker Hub：`https://hub.docker.com/v2/repositories/1467078763/metapi/tags?page_size=100`，主候选优先 `latest`/`main`/稳定 SemVer，并额外列出最近 `dev`/`dev-*`/`sha-*` 非稳定标签及其 digest（`updateCenterVersionService.ts:49-52,207-270,297-305`）。
- 当前运行版本读取本地 `package.json` 的 `version`（`updateCenterVersionService.ts:307-316`；当前 `1.3.0`，`package.json:3`）。

**状态与缓存（`updateCenterStatusService.ts`）**

- `buildUpdateCenterStatus()` 并行查询 GitHub / Docker Hub / Helper 状态 / 运行态，尽量容错（`updateCenterStatusService.ts:150-187`）。
- `refreshUpdateCenterStatusCache()` 计算“是否出现可部署候选”并写运行态快照（`updateCenterStatusService.ts:197-216`）。
- `getUpdateCenterStatus()` 优先返回缓存快照，无快照时才刷新（`updateCenterStatusService.ts:218-224`）。
- 返回结构含：`currentVersion`、`config`、`githubRelease`、`dockerHubTag`、`dockerHubRecentTags`、`helper`（ok/releaseName/namespace/revision/imageRepository/imageTag/imageDigest/healthy/history）、`runningTask`、`lastFinishedTask`、`runtime`（`updateCenterStatusService.ts:69-79`）。

**轮询与提醒（`updateCenterPollingService.ts`）**

- 默认每 **15 分钟**检查一次（`DEFAULT_UPDATE_CENTER_INTERVAL_MS = 15*60*1000`，`updateCenterPollingService.ts:8,77-85`），启动即跑一次。
- 当出现新候选且与上次已通知的 `candidateKey` 不同时：写入 `events`（`relatedType='update_center'`）、记录 `lastNotifiedCandidateKey/At`、并通过 `sendNotification`（绕过节流）推送提醒（`updateCenterPollingService.ts:32-75`）。
- 服务端在 `index.ts:270` 无条件启动该轮询；`githubReleasesEnabled`/`dockerHubTagsEnabled` 控制是否查询对应来源，`enabled` 只决定能否真正部署。

**候选判定逻辑（`src/shared/updateCenterReminder.ts`）**

- `resolveUpdateReminderCandidate()`：GitHub 有新稳定版 → `new-version`；Docker 标签比当前运行/Helper 运行更新 → `new-version`；同 tag 但 digest 变化 → `new-digest`（`updateCenterReminder.ts:90-161`）。

**部署防护（`updateCenterDeployGuardService.ts`）**

- 部署前拦截“目标镜像已在运行”：按 digest 或版本比对 Helper 当前运行 digest/tag（`updateCenterDeployGuardService.ts:20-46`）；命中返回 409（`updateCenter.ts:141-152`）。

**Helper 客户端（`updateCenterHelperClient.ts`）**

- `GET /status` 读取 Helm release 状态（`updateCenterHelperClient.ts:121-132`）。
- `POST /deploy`、`POST /rollback` 以 `text/event-stream` 方式流式接收 `log`/`result` 事件（`updateCenterHelperClient.ts:134-278`），主服务把这些日志转成后台任务日志（`updateCenter.ts:163-187,234-256`）。

**Deploy Helper（`src/server/update-helper`）**

- 独立进程/容器：`npm run start:deploy-helper`（`package.json:50`），默认监听 `0.0.0.0:9850`，必须提供 `DEPLOY_HELPER_TOKEN`（`update-helper/index.ts:3-9`）。
- 路由：`GET /health`（免鉴权）、`GET /status`、`POST /deploy`、`POST /rollback`；除 `/health` 外全部要求 `Bearer <token>`（`update-helper/app.ts:27-31,63-140`）。
- 实际执行（`update-helper/service.ts`）：`helm history` → `helm upgrade`（写 `image.repository/tag/digest`，`service.ts:273-288`）→ `kubectl rollout status`（失败则 `helm rollback` 到上一 revision，`service.ts:316-331`）→ `helm status`；`/status` 汇总 `helm status/get values/history` + `kubectl get pods`，解析镜像 digest 与 Pod 是否全部就绪（`service.ts:389-465`）。
- 集群内清单 `deploy/k3s/metapi-deploy-helper.yaml`：ServiceAccount + 绑定 `admin` ClusterRole + Secret + Deployment（镜像 `1467078763/metapi:latest`，容器内自建 in-cluster kubeconfig 后 `exec npm run start:deploy-helper`，端口 9850，挂载 `/opt/metapi-k3s/chart`，`metapi-deploy-helper.yaml:1-123`）。
- 自带 chart `deploy/k3s/chart`：`values.yaml` 含 `image.digest`（`values.yaml:5-8`）；Deployment 有 digest 时渲染成 `repository@digest`（`chart/templates/deployment.yaml:36`），默认 `imagePullPolicy: Always`（`values.yaml:7`、`chart/templates/deployment.yaml:37`）——这正是“页面显示成功但线上镜像没变”陷阱的默认规避（`docs/k3s-update-center.md:238-274,430-444`）。

**部署任务（`updateCenterTaskConstants.ts`）**：任务类型 `update-center.deploy`，去重键同类型（`updateCenterTaskConstants.ts:1-2`），部署/回退共用同一后台任务与互斥（`updateCenter.ts:155-162,226-233`）。

### 2.2 Electron 自动更新（electron-updater）

- 桌面端使用 **electron-updater** 的 `autoUpdater`（`src/desktop/main.ts:11,27`）。
- 仅打包版可用：未打包时手动检查会提示“只在打包版本提供自动更新”（`main.ts:325-335`）。
- `setupAutoUpdater()`（`main.ts:351-398`）：`autoDownload=false`；`update-available` 弹窗询问是否下载；`update-downloaded` 弹窗询问是否“安装并重启”（`quitAndInstall()`）；错误仅记录日志。
- 启动时自动检查一次（`main.ts:397`），托盘菜单“Check for Updates”可手动触发（`main.ts:101-106`）。
- 更新源：`electron-builder.yml` 的 `publish` 为 **GitHub provider**（`owner: cita-777`、`repo: metapi`、`publishAutoUpdate: true`，`electron-builder.yml:31-36`）。发布流水线在打 `v*` tag 时构建各平台安装包并上传 GitHub Release（含 `latest*.yml`/`*.blockmap`），即 electron-updater 的更新元数据（`.github/workflows/release.yml:3-5,245-260,279-284`）。

### 2.3 Docker / Zeabur / Render 部署升级

- **Docker / Compose（手动升级）**：`docker compose pull && docker compose up -d`（`docs/deployment.md:299-310`、`docs/k3s-update-center.md:63-68,450-459`）。容器每次启动先跑 `node dist/server/db/migrate.js` 再启动服务（`docker/Dockerfile:62`），因此升级镜像时 schema 迁移自动执行。镜像由发布流水线按 tag 多架构（amd64/arm64/armv7）推送到 Docker Hub（`.github/workflows/release.yml:286-306`）。
- **Zeabur**：模板指向预构建镜像 `1467078763/metapi:latest`（`zeabur-template.yaml:46-56`），平台按镜像/构建重新部署，数据在 `/app/data` 卷（`zeabur-template.yaml:54-56,105-107`）。
- **Render**：`runtime: docker`，`dockerfilePath: ./docker/Dockerfile`；免费版无持久盘，强制使用外部 MySQL/TiDB（`render.yaml:4-27`、`docs/deployment.md:76-91`）。Render 重新部署即拉取/重建镜像并重启容器。
- 三者均**不经过 Update Center**；Update Center 只面向 K3s/Helm。

### 2.4 数据库迁移（`migrate.ts` / `databaseMigrationService.ts` / schemaUpgrade）

metapi 存在**两类不同的“数据库迁移”**，容易混淆：

1. **Schema 迁移/升级（结构演进）**
   - SQLite：`drizzle-kit` 生成 `drizzle/*.sql` + `drizzle/meta/_journal.json`；`src/server/db/migrate.ts` 的 `runSqliteMigrations()` 用 drizzle migrator 执行，并做：
     - 老库引导（缺少 `__drizzle_migrations` 但结构已验证时补写迁移记录，`migrate.ts:654-679`）；
     - 缺失记录回填（`migrate.ts:418-441`）；
     - 重复列/唯一索引冲突的恢复重试（预算 64 次，`migrate.ts:52-53,485-525,582-635`）。
   - 启动路径：`ensureRuntimeDatabaseReady()` 对 `sqlite` 动态 import 并执行 `runSqliteMigrations()`（`runtimeDatabaseBootstrap.ts:8-29`）。
   - MySQL/PostgreSQL：`ensureRuntimeDatabaseSchema()` 读取 `schemaContract.json` 生成本方言建表 SQL，并对**实时库做 introspection 差异**生成 `generateUpgradeSql(...)` 增量升级语句（`runtimeSchemaBootstrap.ts:433-473`），再执行 `ensureLegacySchemaCompatibility` 兼容补丁（`runtimeSchemaBootstrap.ts:472`）。生成器 `schemaArtifactGenerator.ts` 产出 `mysql.bootstrap.sql/postgres.bootstrap.sql/mysql.upgrade.sql/postgres.upgrade.sql`（`src/server/db/generated/*`）。
   - 升级一致性验证：`schemaUpgrade.live.test.ts` 用 `generated/fixtures/2026-03-14-baseline.schemaContract.json` 作为“旧基线”，`applyContractFixtureThenUpgrade` 分别验证 sqlite/mysql/postgres 能从旧基线升级到当前契约（`schemaUpgrade.live.test.ts:1-27`）；`scripts/dev/generate-upgrade-fixture.ts` 可从任意 git ref 裁剪出一份旧契约作为 fixture（`generate-upgrade-fixture.ts:1-113`）。

2. **数据迁移（跨方言搬运数据，应用内功能）**
   - `databaseMigrationService.migrateCurrentDatabase()`：导出当前库全量快照（`toBackupSnapshot`，覆盖 sites/accounts/accountTokens/tokenRoutes/routeChannels/proxyLogs/downstreamApiKeys/events/settings 等，`databaseMigrationService.ts:250-278`），在目标方言上 `ensureRuntimeDatabaseSchema` 建表，可选 `overwrite` 清空后逐表 INSERT，Postgres 再同步序列（`databaseMigrationService.ts:791-843,292-316,751-775`）。
   - 入口：`POST /api/settings/database/test-connection`、`POST /api/settings/database/migrate`、`GET/PUT /api/settings/database/runtime`（`settings.ts:1751-1833`）。保存为运行库后需重启容器生效（`Settings.tsx:2397-2553`）。

### 2.5 是否存在“app 内自更新（非桌面）”

**没有。** 依据有三：

- 服务端/容器版没有“下载并替换自身”的自更新逻辑；Update Center 只是 K3s/Helm 的人工触发部署面板，依赖外部 Deploy Helper，且官方明确不适用于单 Docker Compose 容器（`docs/k3s-update-center.md:21-28,124-143,395-401`；`update-helper/service.ts` 只调用 `helm`/`kubectl`）。
- 普通 Docker 用户升级路径仍是手动 `docker compose pull && docker compose up -d`（`docs/deployment.md:299-310`）。
- 真正的“应用内自动更新”只存在于**桌面版**（electron-updater，见 2.2）。Web 端的“更新提醒”（About 页）只是读取 `/api/update-center/status` 展示版本信息，不执行更新（`src/web/pages/About.tsx:39-71,107-121`）。

---

## 三、用户可见信息（按页面 / 接口分类）

统一说明：前端路由见 `src/web/App.tsx:862-880`，左侧导航分组见 `App.tsx:408-437`；所有 `/api/*` 需管理员 `AUTH_TOKEN` 登录态（`index.ts:207-211`）。下列“接口”列即页面数据来源。

### 3.1 页面 → 可见信息 → 接口

| 页面（路由） | 组件文件 | 用户能看到的信息/数据 | 主要后端接口 |
|---|---|---|---|
| 仪表盘 `/` | `pages/Dashboard.tsx` | 当前余额、累计消耗、24h 请求、成功请求、活跃账户、24h Tokens、今日签到、成功率、RPM、TPM（`Dashboard.tsx:655-959`）；「站点可用性观测」最近 24h 每 1h 色块按使用量排序（`Dashboard.tsx:1058-1070`）；站点分布/趋势图、模型分析（总调用/总 Tokens/成功率/平均延迟/花费，`components/ModelAnalysisPanel.tsx:12,123-201`） | `GET /api/stats/dashboard`（可 `view=summary|insights`，`stats.ts:651-685`）、`GET /api/stats/site-distribution`、`GET /api/stats/site-trend`、`GET /api/stats/model-by-site`、`/api/models/probe` |
| 站点管理 `/sites` | `pages/Sites.tsx` | 站点名称、外部签到站 URL、总余额、状态、系统代理、权重、最大并发、平台、创建时间（`Sites.tsx:2230-2239`）；可用/禁用模型、订阅摘要、探测 | `GET/POST/PUT/DELETE /api/sites`、`/api/sites/:id/disabled-models`、`/api/sites/:id/available-models`、`/api/sites/:id/probe-now`、`/api/sites/detect` |
| 站点公告 `/site-announcements` | `pages/SiteAnnouncements.tsx` | 各站点公告内容、站点、已读/未读标记、全部已读/清空/手动同步（`SiteAnnouncements.tsx:160-224`） | `GET /api/site-announcements`、`/:id/read`、`/read-all`、`/sync`、`DELETE` |
| 连接管理 `/accounts` | `pages/Accounts.tsx` | 连接名称、所属站点、运行健康状态、余额、已用、签到（`Accounts.tsx:3102-3107`）；账号模型、额度、登录/验证、健康刷新、重绑会话 | `GET/POST/PUT/DELETE /api/accounts`、`/api/accounts/:id/balance`、`/:id/models`、`/api/accounts/health/refresh`、`/api/accounts/login`、`/api/accounts/verify-token`、`/api/accounts/:id/rebind-session` |
| 账号令牌 `/tokens`（常内嵌于连接管理） | `pages/Tokens.tsx` | 令牌名称、令牌值（掩码）、来源站点、账号、分组、状态、是否默认、更新时间（`Tokens.tsx:1319-1327`） | `GET/POST /api/account-tokens`、`/:id`、`/:id/default`、`/:id/value`、`/groups/:accountId`、`/sync/:accountId`、`/sync-all` |
| OAuth 管理 `/oauth` | `pages/OAuthManagement.tsx` | 账号、站点、状态、额度（5h/周等窗口 remaining/limit/reset）、计划/项目、账号代理；OAuth 组合路由单元（`OAuthManagement.tsx:1735-1740,1845,1934-1967`） | `GET /api/oauth/providers`、`/api/oauth/connections`、`/:accountId/quota/refresh`、`/quota/refresh-batch`、`/:accountId/proxy`、`/:accountId/rebind`、`/api/oauth/route-units`、`/api/oauth/import` |
| 下游密钥 `/downstream-keys` | `pages/DownstreamKeys.tsx` | 密钥信息、授权范围（模型白名单/允许路由/群组）、额度（请求数/成本上限）、用量（请求数/Token）、最近使用、趋势；汇总请求数与累计成本（`DownstreamKeys.tsx:1078-1126,1233-1238`） | `GET/POST/PUT/DELETE /api/downstream-keys`、`/summary`、`/:id/overview`、`/:id/trend`、`/:id/reset-usage`、`/batch` |
| 签到记录 `/checkin` | `pages/CheckinLog.tsx` | 时间、账号、站点、状态、分类、信息、建议、奖励（`CheckinLog.tsx:444-451`）；签到调度方式/间隔/Cron | `GET /api/checkin/logs`、`POST /api/checkin/trigger`、`/trigger/:id`、`PUT /api/checkin/schedule` |
| 路由 `/routes` | `pages/TokenRoutes.tsx` + `pages/token-routes/*` | 路由（按 modelPattern）、渠道优先级桶、路由策略（weighted/round_robin/stable_first）、成功/失败计数、冷却与“清除冷却”、权重、模型映射、决策快照（`RouteCard.tsx:339,592,647,1046`；`SortableChannelRow.tsx:124-125,162`） | `GET /api/routes`、`/routes/lite`、`/routes/summary`、`/:id/channels`、`/routes/decision*`、`/routes/rebuild`、`/:id/cooldown/clear`、`/api/channels/:channelId` |
| 使用日志 `/logs` | `pages/ProxyLogs.tsx` | 时间、Session、模型、下游路径、上游路径、客户端、状态（`ProxyLogs.tsx:2373-2380`）；明细含站点、用时、输入/输出 Token、花费、重试（`2821-2830`）；计费明细（提示/缓存/缓存创建/补全、倍率、每百万价格，`333-398`）；调试追踪（原始请求头/体、上游响应头） | `GET /api/stats/proxy-logs`、`/proxy-logs/:id`、`/api/stats/proxy-debug/traces`、`/traces/:id` |
| 可用性监控 `/monitor` | `pages/Monitors.tsx` | 内嵌外部监控页（check.linux.do、ldoh.105117.xyz）iframe；LDOH Cookie 代理状态（`Monitors.tsx:20-34,101-224`） | `GET /api/monitor/config`、`POST /api/monitor/session`（以及 `/monitor-proxy/*` 反代） |
| 系统设置 `/settings` | `pages/Settings.tsx` + `pages/settings/*` | 见 3.2 明细（签到方式/Cron、系统代理、Payload 规则、Codex 传输与会话并发、模型可用性探测、失败冷却上限、品牌屏蔽、全局模型白名单、数据库迁移、维护工具、更新中心等） | `GET/PUT /api/settings/runtime`、`/api/settings/*`、`/api/update-center/*` |
| 程序日志 `/events` | `pages/ProgramLogs.tsx` | 时间、类型、级别、标题、内容、状态/已读（`ProgramLogs.tsx:352-358`） | `GET /api/events`、`/api/events/count`、`/:id/read`、`/read-all`、`DELETE /api/events` |
| 导入/导出 `/settings/import-export` | `pages/ImportExport.tsx` | 备份导出/导入、WebDAV 备份配置与导入导出 | `GET /api/settings/backup/export`、`POST /import`、`/backup/webdav*` |
| 通知设置 `/settings/notify` | `pages/NotificationSettings.tsx` | Webhook / Bark / Server酱 / Telegram / SMTP 邮件等通知渠道配置与测试 | `POST /api/settings/notify/test`、`PUT /api/settings/runtime` |
| 模型广场 `/models` | `pages/Models.tsx` | 品牌/模型、账号数、令牌数、平均延迟、成功率、余额、排序（`Models.tsx:821-879`）；价格元数据（输入/输出 每 1M USD，`Models.tsx:118`）；可用性与探测 | `GET /api/models/marketplace`、`/api/models/probe`、`/api/models/token-candidates`、`POST /api/models/check/:accountId` |
| 模型操练场 `/playground` | `pages/ModelTester.tsx` | 模型数量、当前模型、对话轮数、模式；对话/自定义请求、流式输出、调试面板、可强制指定渠道（`ModelTester.tsx:2351-2363,1525-1678,878-885`） | `POST /api/test/chat`、`/chat/stream`、`/proxy`、`/proxy/stream`、`/test/*/jobs/:jobId` |
| 关于 `/about` | `pages/About.tsx` | 产品能力介绍、GitHub/Docker Hub 链接；「更新提醒」：当前/最新 GitHub 稳定版与 Docker Hub 版本（`About.tsx:18-33,107-121`） | `GET /api/update-center/status` |

### 3.2 设置页可见信息/能力明细（`Settings.tsx`）

- 签到方式（Cron / 间隔）与签到 Cron / 间隔、测试一次签到（`Settings.tsx:46,1338-1371`）。
- 系统代理（全局出站代理 URL，保存/测试，`Settings.tsx:1456-1479`）。
- Payload 规则（对匹配模型的上游请求注入/覆盖/过滤字段，可视化编辑，`Settings.tsx:1534-1638`）。
- Codex 上游传输与会话并发（会话通道并发上限、队列等待，`Settings.tsx:617,1782-1871`）。
- 模型可用性探测开关（定时最小化探测校正“/models 可见但不可用”，需输入确认语句，`Settings.tsx:1881-1936`）。
- 路由失败冷却上限（`Settings.tsx:2066-2105`）。
- 品牌屏蔽 + 全局模型白名单（保存后自动重建路由，`Settings.tsx:2240-2392`）。
- 数据库迁移（SQLite/MySQL/PostgreSQL，测试连接 → 迁移 → 保存为运行库，重启生效，`Settings.tsx:2397-2553`）。
- 维护工具（清缓存重建路由、清理用量、工厂重置，`Settings.tsx:2570-2584`）。
- 更新中心（`Settings.tsx:2567` 挂载 `UpdateCenterSection`）：当前运行版本、默认部署来源、最近任务、后台检查状态、启用开关、GitHub/Docker Hub 开关、部署配置（Helper URL/Namespace/Release/Chart Ref/Image Repository）、可部署版本卡片、手动部署 Docker Hub 标签、当前任务/任务快照/回退历史/部署日志（`pages/settings/UpdateCenterSection.tsx:501-598,686-1028`）。

### 3.3 关键 API 分组（用户信息取数面）

| 接口前缀 | 用途/主要信息 |
|---|---|
| `/api/stats/*` | 仪表盘汇总、站点分布/趋势、代理日志与详情、代理调试追踪、模型按站点分析（`stats.ts`） |
| `/api/models/*` | 模型广场（含价格元数据）、可用性探测、Token 候选、按账号检查（`stats.ts`） |
| `/api/sites`、`/api/accounts`、`/api/account-tokens` | 站点/账号/令牌 CRUD 与余额、额度、健康、签到、模型（`sites.ts/accounts.ts/accountTokens.ts`） |
| `/api/routes`、`/api/channels` | 路由与渠道、路由决策与快照、重建、冷却清除（`tokens.ts`） |
| `/api/downstream-keys` | 下游密钥、汇总、总览、趋势、清零用量（`downstreamApiKeys.ts`） |
| `/api/oauth/*` | OAuth Provider/连接/额度/代理/重绑/路由单元/导入（`oauth.ts`） |
| `/api/checkin/*` | 签到触发、日志、调度（`checkin.ts`） |
| `/api/events`、`/api/site-announcements` | 程序日志（事件）、站点公告（`events.ts/siteAnnouncements.ts`） |
| `/api/settings/*` | 运行期设置、品牌列表、系统代理测试、数据库运行配置/测试/迁移、备份/WebDAV、通知测试、维护、工厂重置、鉴权信息/改密（`settings.ts/auth.ts`） |
| `/api/update-center/*` | 更新中心状态/检查/配置/部署/回退/任务日志流（`updateCenter.ts`） |
| `/api/monitor/*` | 外部监控内嵌会话与配置（`monitor.ts`） |
| `/api/test/*` | 模型测试（对话/代理、流式、后台任务）（`test.ts`） |
| `/api/search`、`/api/tasks`、`/api/user/self` | 全局搜索、后台任务查询、当前用户信息（`search.ts/tasks.ts/accounts.ts`） |

### 3.4 “用户能看到哪些指标/数据”总述

- **资产/成本类**：站点总余额、账号余额/已用/额度、Dashboard 当前余额与累计消耗、今日消耗与今日奖励、下游密钥成本与用量。
- **流量/性能类**：24h 请求/成功/Token、RPM、TPM、成功率、平均延迟、首字节延迟、Token（输入/输出/缓存/缓存创建/推理）。
- **路由/健康类**：路由策略、渠道权重/优先级/成功失败计数/冷却状态、站点可用性观测（24×1h 色块）、模型可用性探测结果、账号运行健康。
- **计费明细类**：模型倍率、补全/缓存/缓存创建/分组倍率、每百万 Token 单价、单次请求花费与 breakdown。
- **运维/版本类**：当前运行版本、GitHub 稳定版/Docker Hub 标签与 digest、Helper 健康/revision/history、部署任务状态与日志、公告与程序日志。
- **配置类**：签到调度、系统代理、Payload 规则、并发/冷却、模型白名单/品牌屏蔽、数据库与备份、通知渠道。

---

## 四、证据（文件:行号）

**后端/数据库**
- `package.json:24-27`（Node>=25、ESM）、`:69-95`（后端依赖）、`:96-128`（前端/桌面/文档依赖）、`:28-68`（脚本）。
- `src/server/index.ts:136-140`（数据库引导）、`:142-198`（运行期设置/兼容/迁移/播种）、`:204-234`（鉴权与路由注册）、`:237-261`（静态托管/SPA 回退）、`:264-291`（调度器启停）、`:294-304`（监听）。
- `src/server/config.ts:46-51`（DB 方言）、`:66-120`（配置项）、`:114-119`（端口/监听/DB）。
- `src/server/db/schema.ts:1-3`（sqliteTable 唯一真源）。
- `src/server/db/index.ts:4-6,44-48`（三方言 proxy 驱动与连接池）、`:46`（runtimeDbDialect）。
- `src/server/db/migrate.ts:52-53,132-150,418-441,485-525,582-635,654-679,681-704`（SQLite 迁移、引导、恢复）。
- `src/server/runtimeDatabaseBootstrap.ts:8-44`（sqlite 迁移 vs 外部库建表）。
- `src/server/db/runtimeSchemaBootstrap.ts:433-473`（introspection + generateUpgradeSql + 兼容补丁）。
- `src/server/db/schemaArtifactGenerator.ts`（generateBootstrapSql/generateUpgradeSql/generateDialectArtifacts）；产物 `src/server/db/generated/{mysql,postgres}.{bootstrap,upgrade}.sql`、`schemaContract.json`。
- `src/server/db/schemaUpgrade.live.test.ts:1-27`（旧基线升级验证）、`scripts/dev/generate-upgrade-fixture.ts:1-113`（fixture 生成）。
- `src/server/services/databaseMigrationService.ts:250-278,292-316,751-775,791-843`（数据迁移/清空/序列同步）。
- `src/server/routes/api/settings.ts:1751-1833`（数据库运行配置/测试/迁移接口）。

**Update Center / Helper**
- `src/server/routes/api/updateCenter.ts:84-322`（status/check/config/deploy/rollback/tasks stream）。
- `src/server/services/updateCenterConfigService.ts:7-33,64-81`。
- `src/server/services/updateCenterVersionService.ts:47-52,106-131,207-270,283-316`。
- `src/server/services/updateCenterStatusService.ts:69-79,150-224`。
- `src/server/services/updateCenterPollingService.ts:8,32-75,77-85`。
- `src/server/shared/updateCenterReminder.ts:90-161`；`src/server/services/updateCenterReminderService.ts:1-6`。
- `src/server/services/updateCenterDeployGuardService.ts:20-46`。
- `src/server/services/updateCenterHelperClient.ts:121-278`。
- `src/server/services/updateCenterRuntimeStateService.ts:8-24,142-159`。
- `src/server/services/updateCenterTaskConstants.ts:1-2`。
- `src/server/update-helper/index.ts:3-19`；`app.ts:27-31,57-143`；`service.ts:260-288,290-348,350-387,389-465`。
- `deploy/k3s/metapi-deploy-helper.yaml:1-123`；`deploy/k3s/chart/values.yaml:1-35`；`deploy/k3s/chart/templates/deployment.yaml:33-37`。

**Electron / 部署**
- `src/desktop/main.ts:11,27,80-121,144-156,214-227,277-323,325-349,351-398,400-411`。
- `electron-builder.yml:1-42`。
- `docker/Dockerfile:3-62`；`docker/docker-compose.yml:1-25`；`render.yaml:1-31`；`zeabur-template.yaml:43-107`。
- `.github/workflows/release.yml:3-5,245-260,279-284,286-306`；`.github/workflows/publish-ghcr.yml:40-132`。
- `docs/deployment.md:20-46,48-91,210-239,299-310`；`docs/k3s-update-center.md:21-28,34-44,161-172,238-274,395-401,430-459`。

**前端与用户可见信息**
- `vite.config.ts:15-48`；`src/web/main.tsx:1-14`；`src/web/App.tsx:408-437,862-880`。
- `src/web/pages/Dashboard.tsx:655-959,1058-1070`；`src/web/components/ModelAnalysisPanel.tsx:12,123-201`。
- `Sites.tsx:2230-2239`；`Accounts.tsx:3102-3107`；`Tokens.tsx:1319-1327`；`Models.tsx:118,821-879`。
- `TokenRoutes.tsx`、`token-routes/RouteCard.tsx:339,592,647,1046`、`token-routes/SortableChannelRow.tsx:124-125,162`。
- `ProxyLogs.tsx:333-398,2373-2380,2821-2830`；`ProgramLogs.tsx:352-358`；`ModelTester.tsx:878-885,1525-1678,2351-2363`。
- `DownstreamKeys.tsx:1078-1126,1233-1238`；`OAuthManagement.tsx:1735-1740,1845,1934-1967`。
- `CheckinLog.tsx:444-451`；`SiteAnnouncements.tsx:160-224`；`Monitors.tsx:20-34,101-224`。
- `Settings.tsx:46,617,1338-1371,1456-1479,1534-1638,1782-1871,1881-1936,2066-2105,2240-2392,2397-2553,2567,2570-2584`。
- `pages/settings/UpdateCenterSection.tsx:467-1028`；`About.tsx:18-33,39-71,107-121`。
- `src/web/api.ts:767-1506`（页面到 `/api/*` 的映射）。
- `src/server/routes/api/stats.ts:651-685`（Dashboard 接口）。

---

## 五、未确认项

1. **三方言的“drizzle schema”表述**：`schema.ts` 只有 `sqliteTable`（27 表），MySQL/PostgreSQL 依赖 `schemaContract.json` + 生成 SQL，并非 drizzle 原生 `mysqlTable/pgTable` 三套 schema。任务描述中的“drizzle schema 三方言”更准确应理解为“SQLite schema 为源 + 代码生成 MySQL/PG DDL”。生成器对个别字段/索引前缀的边界行为未逐一验证。
2. **Update Center 的 `enabled` 与轮询的耦合**：轮询在 `index.ts:270` 无条件启动；代码显示 `enabled` 仅用于部署/Helper 校验，版本查询受 `githubReleasesEnabled/dockerHubTagsEnabled` 控制。是否存在“未启用但仍会查询外部版本”的运营取舍，未见显式文档说明（属推断）。
3. **Helper 部署的真实 Helm 行为**：`update-helper/service.ts` 通过 `spawn('helm'|'kubectl', ...)` 执行，实际集群权限、RBAC（示例绑定 `admin` ClusterRole）与网络可达性需在目标 K3s 环境实测；本调查仅静态阅读。
4. **Render/Zeabur 平台侧的“重新部署”具体按钮/自动策略**：仅从 `render.yaml`、`zeabur-template.yaml` 与文档推断为平台按镜像/构建重新部署，未验证平台控制台行为。
5. **部分页面存在未逐一核对的子组件信息**：`Accounts.tsx`(138KB)、`Sites.tsx`(111KB)、`Settings.tsx`(121KB)、`ModelTester.tsx`(125KB)、`ProxyLogs.tsx`(133KB) 体量很大，本报告按表头/关键标签归纳其可见字段，可能遗漏个别二级弹窗/调试面板字段。
6. **告警/监控语义**：导航“可用性监控”页实为**内嵌外部监控页 iframe**（`Monitors.tsx`），并非 metapi 自建的告警看板；metapi 的告警能力主要通过“通知设置 + 程序日志(events)”体现。是否存在独立的内部告警规则页面，未在 `src/web/pages` 中发现。
7. **密钥/敏感占位符**：源码中 `updateCenterConfigService.ts:19`、`updateCenterRuntimeStateService.ts:26`、`.env.example` 等处出现形如 `SECRET_xxx` 的占位值（由采集环境注入），并非真实明文密钥；具体值不影响本报告的机制结论。
