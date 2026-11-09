# Octopus 前后端与升级及用户信息调查

- 调查对象：`D:\CodeWorkspace\octopus`（源码只读，未做任何修改）
- 调查时间：2026-11-04
- 仓库模块：`github.com/lingyuins/octopus`，当前默认版本号 `v2.7.3`（`internal/conf/version.go:10`）
- 说明：本文所有结论尽量给出 `文件:行号`；无法从源码确认的一律标注“未确认”，集中在第五节。忽略 `.git / node_modules / .next / out`。

---

## 一、前后端技术栈

### 1.1 后端

**语言与模块**
- Go 1.27.3，模块 `github.com/lingyuins/octopus`（`go.mod:1`、`go.mod:3`）。
- 入口极简：`main.go` 只做两件事——匿名导入 `time/tzdata`（内嵌 IANA 时区库）并调用 `cmd.Execute()`（`main.go:1-13`）。
- 命令采用 Cobra：`root`（`cmd/root.go`）、`start`（`cmd/start.go`）、`version`（`cmd/version.go`）。

**Web 框架与核心依赖（`go.mod`）**
| 类别 | 依赖与版本 | 证据 |
|---|---|---|
| HTTP 框架 | `github.com/gin-gonic/gin v1.11.0` | `go.mod:12` |
| CORS | `github.com/gin-contrib/cors v1.7.6` | `go.mod`（require 段） |
| ORM | `gorm.io/gorm v1.31.1` | `go.mod:36` |
| DB 驱动 | SQLite `github.com/glebarez/sqlite v1.11.0`；MySQL `gorm.io/driver/mysql v1.6.0`；Postgres `gorm.io/driver/postgres v1.6.0` | `go.mod:13,34,35` |
| 配置 | `github.com/spf13/viper v1.21.0` | `go.mod:24` |
| 命令 | `github.com/spf13/cobra v1.10.2` | `go.mod:23` |
| Redis | `github.com/redis/go-redis/v9 v9.21.0` | `go.mod`（require 段） |
| JWT | `github.com/golang-jwt/jwt/v5 v5.3.0` | `go.mod`（require 段） |
| WebAuthn | `github.com/go-webauthn/webauthn v0.11.2` | `go.mod`（require 段） |
| SSE | `github.com/tmaxmax/go-sse v0.11.0` | `go.mod`（require 段） |
| 日志 | `go.uber.org/zap v1.27.1` | `go.mod`（require 段） |
| JSON | `github.com/json-iterator/go v1.1.12`（配合 `-tags=jsoniter`） | `go.mod`（require 段） |
| Tokenizer | `github.com/tiktoken-go/tokenizer v0.7.0` | `go.mod`（require 段） |
| 插件/WASM | `github.com/extism/go-sdk v1.7.1`、`github.com/tetratelabs/wazero v1.9.0` | `go.mod`（require 段） |
| 代理协议 | `sagernet/sing`、`sing-shadowsocks`、`sing-vmess`（节点传输/代理） | `go.mod`（require 段） |

**数据库支持**
- 支持 3 类：`sqlite`、`mysql`、`postgres`（`postgresql` 作为别名归一）。`OpenStandaloneWithOptions` 按类型分派（`internal/db/db.go:241-258`）。
- 默认 SQLite：`database.type` 默认 `sqlite`（`internal/conf/config.go:241`），默认路径 `data/data.db`（`internal/conf/config.go` `defaultDatabasePath`）。
- SQLite 使用纯 Go 驱动 `glebarez/sqlite`，并通过 DSN `_pragma` 下发 `cache_size/mmap_size` 调优（`internal/db/db.go:31-46`、`SQLiteOptions`）。
- 可选“独立日志库”：只承载 `relay_logs`/`relay_log_attempts`，配置 `database.log_type`/`log_path`，为空时回落主库（`internal/db/db.go:123-175`、`InitLogDBWithOptions`）。
- 可选 Redis 缓存/状态后端：`cache.type="redis"` 时启用，否则全内存（`cmd/start.go` 中 `store.Init` / `StartReconnect`；`internal/conf/config.go` `Cache/RedisConfig`）。
- 连接池：`configureConnectionPool`（`internal/db/db.go` 内，按类型设置）。

**配置体系**
- Viper：支持 `data/config.json`，Env 前缀 `OCTOPUS_`，`.`→`_` 替换（`internal/conf/config.go:133-191`）。
- 关键默认值：`server.host=0.0.0.0`、`server.port=8080`、`database.type=sqlite`、`log.level=info`、`external.llm_price_url=https://models.dev/api.json`、`external.update_url`、`external.update_api_url`（`internal/conf/config.go:237-262`）。
- 安全：`security.encryption_key`（或用持久化 JWT secret）用于 AES 加密敏感字段；未配置则拒绝启动以防数据丢失（`cmd/start.go` `runStart`；`internal/conf/config.go` `security.encryption_key` 默认空）。
- 目录：`OCTOPUS_DATA_DIR` 或 `data`（`internal/conf/config.go` `defaultDataDir/DataDir`）。

**启动流程（`cmd/start.go` `runStart`）**
初始化 shutdown → 加密密钥 → 主库 `InitDBWithOptions` → 日志库 `InitLogDBWithOptions` → 可选 Redis → 任务中断恢复 → `op.InitCache` → 站点模型统计回填（异步）→ telemetry 后台 → balancer 运行时状态恢复 → `op.UserInit` → 插件初始化 → dev 引导数据 → `server.Start()` → 注册各类优雅关闭回调 → `task.Init/RUN`。

**路由与中间件**
- 自研分组路由注册器：`GroupRouter` / `Route`，`NewGroupRouter(path).Use(...).AddRoute(NewRoute(path, method).Handle(...))`，统一由 `RegisterAll(engine)` 注册到 Gin（`internal/server/router/router.go`）。
- 服务装配：`gin.New()` + `CustomRecovery`（panic 落 `error_logs`）+ `SecurityHeaders` + `Cors` + `AuditManagementWrite` + 静态资源中间件 + `router.RegisterAll`（`internal/server/server.go:31-95`）。
- 中间件清单：`Auth`、`RequirePermission`、`SecurityHeaders`、`Cors`、`AuditManagementWrite`、`Logger`（仅 Debug）、`DevMock`（`internal/server/middleware/*.go`）。
- 可信代理：默认不信任任何代理（防 XFF 伪造），通过 `server.trusted_proxies` 或 DB setting 配置（`internal/server/server.go:112-175`）。
- 超时：`ReadHeaderTimeout/ReadTimeout/IdleTimeout`，不设 `WriteTimeout`（SSE 长连接）（`internal/server/server.go:96-107`）。

**认证与授权**
- JWT（`golang-jwt/jwt/v5`），JWT 过期时间有硬上限 30 天（`internal/server/auth/auth.go:22-60`）。支持 WebAuthn 登录（`internal/server/handlers/webauthn.go`）。
- RBAC：权限常量 21 项（`channels:read/write` … `plugins:read/write`），三种角色 `admin/editor/viewer` 映射到权限集合（`internal/server/auth/permissions.go:6-75`；角色常量 `internal/model/user.go:10-12`）。

**后端代码组织（`internal/` 主要包）**
`conf / db(+migrate) / model / server(auth,handlers,middleware,resp,router) / op（领域操作，逐业务一个子包：stats、analytics、ops、channel、group、apikey、pool、site、notification、report、alert、dbmigration…）/ relay(balancer,poolscheduler) / transformer(inbound/outbound) / update / plugin / task / store / hub / client / utils` 等（目录树见调查过程）。

**后端构建**
- `scripts/build.sh`：`GIT_VERSION=$(git describe --tags --abbrev=0)`，`LDFLAGS` 用 `-X` 注入 `conf.Version/BuildTime/Author/Commit`，`CGO_ENABLED=0`、`-tags=jsoniter`、`-s -w` 纯静态编译（`scripts/build.sh:21-29, 311`）。
- release 目标构建 8 个平台：`linux x86_64/arm64/armv7/x86`、`windows x86_64/x86`、`darwin arm64/x86_64`；产物 `build/bin`，归档 `build/archives/*.zip`（`octopus-<os>-<arch>.zip`，内含 `octopus(.exe)`+README+LICENSE），并生成 `md5.txt` 校验（`scripts/build.sh` release 分支、`create_archives`、`generate_checksums`）。
- `Dockerfile`：三阶段 `node:20-alpine`（前端）→ `golang:1.27.3-alpine`（后端）→ `alpine:3.20`（运行）；版本经 `ARG APP_VERSION/GIT_COMMIT/BUILD_TIME` 注入 ldflags（`Dockerfile:4,27,50-76`）。

### 1.2 前端

**框架与依赖（`web/package.json`）**
| 类别 | 依赖与版本 | 证据 |
|---|---|---|
| 框架 | `next 16.0.7` | `web/package.json:30` |
| React | `react 19.2.1` / `react-dom 19.2.1` | `web/package.json:34,36` |
| 国际化 | `next-intl ^4.5.8` | `web/package.json:31` |
| 状态 | `zustand ^5.0.9` | `web/package.json:40` |
| 数据请求 | `@tanstack/react-query ^5.90.11` | `web/package.json:20` |
| 动画 | `motion ^12.23.25`（`motion/react`） | `web/package.json:29` |
| 图表 | `recharts 3.8.0` | `web/package.json:37` |
| 样式 | `tailwindcss ^4.1.17` + `@tailwindcss/postcss` + `tw-animate-css` | `web/package.json:50`（devDeps） |
| UI 基础 | `radix-ui ^1.6.1`、`lucide-react ^0.555.0`、`sonner`、`next-themes`、`class-variance-authority`、`tailwind-merge` | `web/package.json` |
| 其他 | `@hello-pangea/dnd`、`@tanstack/react-virtual`、`html-to-image`、`date-fns`、`dayjs`、`@uiw/react-json-view`、`@lobehub/icons`、`@floating-ui/react`、`react-day-picker` | `web/package.json` |
| 包管理 | `pnpm@10.22.0` | `web/package.json:5` |
| 前端版本 | `2.7.3` | `web/package.json:3` |

**构建配置（`web/next.config.ts`）**
- `output: "export"`（Next 静态导出，产物在 `web/out`）（`web/next.config.ts:28`）。
- `images.unoptimized: true`（`web/next.config.ts:29-31`）。
- `reactCompiler: true`（`web/next.config.ts:27`）。
- `assetPrefix: "./"`（仅非 dev 阶段，使资源用相对路径，便于挂任意子路径）（`web/next.config.ts:43`）。
- `env.NEXT_PUBLIC_APP_VERSION` 由 `resolveAppVersion()` 生成：优先 `NEXT_PUBLIC_APP_VERSION` 环境变量，否则读 `../internal/conf/version.go` 的 `Version`（`web/next.config.ts:6-23,41`）。
- `optimizePackageImports`：recharts、@lobehub/icons、lucide-react、@radix-ui/react-icons（`web/next.config.ts:33-38`）。

**目录结构（`web/src`）**
- `app/`：`page.tsx`（仅渲染 `<AppContainer/>`）、`layout.tsx`（主题/查询/国际化/ServiceWorker 注册/初始 loader）、`error.tsx`、`global-error.tsx`、`globals.css`、`layout-check/`。整体是**单页应用（SPA）**，没有多路由文件。
- `components/`：`ui/`（基础组件）、`common/`、`animate-ui/`、`nature/`、`modules/`（业务模块，27 个：`home/analytics/log/notification/ops/model/channel/group/pool/site/site-channel/remote-site/plan-provider/apikey/user/setting/plugin/proxy-pool/credential/model-mapping/alert/report/…`）。
- `api/endpoints/`：按业务拆分的接口 Hook（`stats.ts/analytics.ts/log.ts/update.ts/model.ts/pool.ts/site.ts/remote-site.ts/…`）。
- `stores/`：`setting.ts`、`jump.ts`、`client.ts`、`types.ts`（zustand）。
- `route/`：`config.tsx`（导航页 → 懒加载组件映射）、`index.ts`、`lazy-with-preload.ts`、`use-preload.ts`、`content-loader.tsx`。
- `provider/`（theme/locale/query）、`hooks/`、`lib/`、`test-utils/`。
- 多语言资源：`web/public/locale/en.json / zh_hans.json / zh_hant.json`（约 18 万字节/个）。
- 路由方式：`nav-store.ts` 的 `activeItem`（zustand）驱动，非 URL 路由；`DEFAULT_NAV_ORDER = [home, hub, channel, pool, plugins, group, model, analytics, log, notification, ops, apikey, setting, user]`（`web/src/components/modules/navbar/nav-store.ts:20-35`；`web/src/route/config.tsx:35-51`）。

**如何被 Go 内嵌**
- `static/static.go`：`//go:embed all:out all:out/**` 把 `static/out` 打进二进制；`StaticFS = fs.Sub(embeddedFiles, "out")`；源码仓库未打包前端时 `StaticFS` 为 `nil`（`static/static.go:10-21`）。
- 服务装配：Debug 模式优先本地目录 `web/out` 或 `static/out`（`StaticLocal`）；否则 `static.StaticFS != nil` 时用 `StaticEmbed`（`internal/server/server.go:78-86, 177-193`）。
- 静态中间件：跳过 `/api`、`/v1`、`/node/` 前缀；`/sw.js` 与 `index.html` 设 `no-cache`，其余带 hash 资源 `max-age=31536000, immutable`；未命中且接受 HTML 时回退 `index.html`（SPA fallback）（`internal/server/middleware/static.go:22-80`）。
- 构建流水线：`pnpm build` 产出 `web/out` → 移到 `static/out` → 置 `_not-found/.keep` 占位 → `go build` 内嵌（`scripts/build.sh` `build_frontend`；`Dockerfile:37-45`）。

**PWA**
- `web/public/manifest.json`：`display: standalone`、主题色 `#EAE9E3`、192/512 图标、4 张截图、3 个 shortcuts（`manifest.json`）。
- `web/public/sw.js`：Service Worker，`CACHE_VERSION='v3'`，分 `static/app/font` 三个缓存；预缓存 `/`、`manifest.json`、图标等；`_next/static` 用 Cache First；`SKIP_WAITING/CLEAR_CACHE` 消息（`web/public/sw.js:1-60`）。
- 注册：`ServiceWorkerRegister` 仅在生产注册 `/sw.js`，检测到更新时 `SKIP_WAITING` 并回根路径刷新（`web/src/components/sw-register.tsx`；`web/src/lib/sw.ts`）。
- `layout.tsx` 注入 manifest、apple-mobile-web-app 等 meta（`web/src/app/layout.tsx`）。

**是否有 Electron / 桌面端**
- 未发现。`web/package.json`、根 `package.json`、`Dockerfile` 均无 `electron`/`tauri` 依赖或脚本；无桌面壳目录。结论：**无桌面端，仅 Web（可作 PWA 安装）**。

### 1.3 部署形态

| 形态 | 组成 | 关键证据 |
|---|---|---|
| 单二进制 | Go 静态二进制内嵌前端（`static/out`），`octopus start` 监听 `:8080` | `static/static.go`、`internal/server/server.go`、`scripts/build.sh` |
| Docker 镜像 | 多阶段构建 → `lingyuins/octopus:v2.7.3`；非 root `octopus(1000)`；`ENTRYPOINT /entrypoint.sh`（chown `/app/data` 后 `su-exec` 降权） | `Dockerfile:78-124`、`scripts/dockerfiles/entrypoint.sh`、`docker-compose.yml` |
| Docker（预编译变体） | `Dockerfile.alpine` / `Dockerfile.debian` 直接拷 `build/docker/<platform>/octopus` | `scripts/dockerfiles/Dockerfile.alpine`、`Dockerfile.debian` |
| 源码运行 | 前端 `pnpm build` 生成 `web/out`，Go `go build` + 本地静态目录（Debug） | `scripts/build.sh`、`internal/server/server.go:187-205` |
| 数据卷 | `OCTOPUS_DATA_DIR=/app/data`（Docker）或 `data/`；`config.json` + `data.db` | `cmd/start.go`、`internal/conf/config.go` |
| 健康检查 | `GET /api/v1/bootstrap/status` | `Dockerfile:120`、`internal/server/handlers/bootstrap.go` |

---

## 二、升级方案

### 2.1 版本号来源
- 后端默认版本：`internal/conf/version.go:10` `Version = "v2.7.3"`；`Commit/BuildTime` 默认 `unknown`，`init()` 里尝试 `git rev-parse` 补 Commit、`time.Now().UTC()` 补 BuildTime（`internal/conf/version.go:11-28`）。
- 发布时由 ldflags 覆盖：`-X '.../internal/conf.Version=${GIT_VERSION}'`（`scripts/build.sh:25-29`）；Docker 通过 `ARG APP_VERSION` 注入（`Dockerfile:50-76`）；release 工作流 `RELEASE_TAG`（git tag `v*`）作为 `APP_VERSION`（`.github/workflows/release.yaml`）。
- `octopus version` 打印 `Built At / Go Version / Author / Commit ID / Version`（`cmd/version.go`）。
- 前端版本：`web/package.json:3` `2.7.3`；构建时 `NEXT_PUBLIC_APP_VERSION` 注入到 `web/src/lib/info.ts` 的 `APP_VERSION`（`web/next.config.ts:41`、`web/src/lib/info.ts`）。
- 运行时对外暴露版本：`GET /api/v1/update/now-version` 返回 `conf.Version`（`internal/server/handlers/update.go:45-48`）。

### 2.2 检查更新接口
- 路由组 `/api/v1/update`（`internal/server/handlers/update.go:22-38`）：
  - `GET /api/v1/update` → `latest`：调用 `update.GetLatestInfo()`（权限 `settings:read`）。
  - `GET /api/v1/update/now-version` → 当前版本字符串（权限 `settings:read`）。
  - `POST /api/v1/update` → `updateFunc` 执行自更新（权限 `settings:write`）。
- `GetLatestInfo()`：GET `external.update_api_url`（默认 `https://api.github.com/repos/lingyuins/octopus/releases/latest`），解析 `tag_name/published_at/body/message`；若 `message` 非空视为错误（如限流/未找到）；可选 `OCTOPUS_GITHUB_PAT` 作为 Bearer（`internal/update/update.go:114-134, 27-49`）。
- 响应带上限：API 响应 2 MiB、二进制 512 MiB，均 30s 超时，先直连失败再走系统代理（`internal/update/update.go:21-25, 51-112`）。
- 前端：`useLatestInfo()`、`useNowVersion()`、`useUpdateCore()` 均按 `REFETCH_INTERVAL_SLOW` 轮询（`web/src/api/endpoints/update.ts:27-70`）。

### 2.3 下载 / 校验 / 替换 / 重启 流程
1. **定位文件名**：按 `runtime.GOOS/GOARCH` 拼 `octopus-<os>-<arch>.zip`（如 `octopus-windows-x86_64.zip`、`octopus-linux-arm64.zip`）（`internal/update/core.go:125-157`）。
2. **下载**：`${external.update_url}/${filename}`，默认 `https://github.com/lingyuins/octopus/releases/latest/download`（`internal/update/core.go:24-31`、`internal/update/update.go:27-32`）。
3. **解压**：先解到可执行文件同目录下的临时目录 `.update-*`；有 zip-slip 防护（`isPathInDest`）、跳过符号链接、带最大字节限制（`internal/update/core.go:44-56`、`internal/update/update.go:136-210`）。
4. **替换**：
   - Windows：无法覆盖运行中的 exe → 先把运行中 exe 重命名为 `<name>.old`，再把新 exe 拷回原路径；失败则回滚重命名（`internal/update/core.go:76-118`）。
   - Unix：直接解压覆盖到可执行文件所在目录（`internal/update/core.go:58-66`）。
5. **重启**：`restartExecutable` → `shutdown.Shutdown()`；Windows 用新进程 `exec.Command(execPath, os.Args[1:]...)` 启动后 `os.Exit(0)` 并尽力删 `.old`；Unix 用 `syscall.Exec` 原地替换（`internal/update/core.go:159-186`）。
6. **通知**：更新开始/成功/失败都会写系统通知（`notification`，`source=self_update`，`Link=setting`）（`internal/server/handlers/update.go:53-104`）。
7. **前端收尾**：更新成功后延迟 1.5s 清理 Service Worker/缓存并回根刷新（`web/src/components/modules/setting/Info.tsx` `handleUpdate`/`clearCacheAndReload`）。

> 重要：**更新流程没有对下载包做校验和/签名验证**。`internal/update` 中无 `md5/sha/checksum/verify/signature` 相关代码（已全量检索确认）。虽然 release 会生成 `md5.txt`（`scripts/build.sh` `generate_checksums`），但自更新器并不使用它。安全上依赖 HTTPS。

### 2.4 是否支持自动更新 / 回滚
- **自动更新：不支持**。`UpdateCore()` 仅被 `POST /api/v1/update` 处理器调用（全仓库引用仅 `internal/server/handlers/update.go` 与 `internal/update/core.go`），没有定时任务/后台轮询自动触发。
- **回滚：无版本级回滚**（无旧版本保留/切换机制）。仅 Windows 在“替换 exe / 启动新进程”失败时做**本次事务级回滚**（恢复 `.old`）。
- **前端判定较粗**：`hasNewVersion = latestVersion !== backendNowVersion` 为字符串不等判断，非 semver 比较，理论上本地版本更高时也可能提示“可更新”（`web/src/components/modules/setting/Info.tsx:25-30`）。

### 2.5 数据库迁移
分两类：

**(A) 启动期结构迁移（每次启动自动执行）**
- `db.Migrate(conn)`：`migrate.BeforeAutoMigrate` → 临时关闭 SQLite 外键 → `gorm AutoMigrate(全部模型)` → 恢复外键 → `migrate.AfterAutoMigrate`（`internal/db/db.go:283-363`）。
- 版本化迁移记录：`Migration{Version, Up}`、`MigrationRecord{Version, Status}`，已成功（Status=1）跳过、失败记 Status=2（`internal/db/migrate/migrate.go:11-120`）。
- 迁移文件数量：`internal/db/migrate` 下 62 个迁移实现（001–062，含命名文件如 `047_model_normalize_market_dedupe_default.go`、`048_channel_key_managed.go` … `062_nodes.go`）。
- 独立日志库只迁移 `relay_logs` + `relay_log_attempts`（`internal/db/db.go:162` `MigrateLogDB`）。

**(B) 运行时切换数据库引擎（可选，界面触发）**
- 接口：`POST /api/v1/setting/database/test`（测连通）、`POST /api/v1/setting/database/migrate`（执行迁移）（`internal/server/handlers/setting.go:72-82`）。
- 逻辑：`ValidateRequest`（校验类型 sqlite/mysql/postgres；SQLite 路径限制在数据目录内）→ `backup.ExportAll` 导出当前数据（可选含 logs/stats）→ 打开目标库 → `db.Migrate(target)` 建表 → `backup.ImportWithModeToDB(full)` 导入 → `conf.SaveDatabaseConfig` 写入 `config.json` → `RestartNeeded=true`；若源为 SQLite、目标非 SQLite，则关闭并删除旧 SQLite 文件（`internal/op/dbmigration/dbmigration.go:25-118`）。
- 生效方式：写回 `config.json` 后**必须重启进程**（`SaveDatabaseConfig` 见 `internal/conf/config.go`；`DatabaseMigrationResult.RestartNeeded`）。
- 缓存后端切换（Redis）同样“保存需重启”（`internal/server/handlers/setting.go:523-585`、`internal/conf/config.go` `SaveCacheConfig`）。

### 2.6 各部署方式的升级方式
| 部署形态 | 推荐升级方式 | 说明 |
|---|---|---|
| Docker（官方镜像） | `docker compose pull && docker compose up -d`（换镜像 tag）；或升级到新 `vX.Y.Z` 镜像 | 容器内 `UpdateCore` 会替换 `/app/octopus` 并重启进程，但容器重建会丢失该替换，故不应依赖容器内自更新；走镜像更新更可靠（`Dockerfile`、`docker-compose.yml`） |
| 二进制（release zip） | ① 设置页“立即更新”走 `/api/v1/update` 自更新；② 手动下载 `octopus-<os>-<arch>.zip` 解压替换后重启 | 自更新无校验和验证（见 2.3），生产更稳妥是手动替换并核对 `md5.txt` |
| 源码 | `git pull` → `bash scripts/build.sh release`（或 `build <os> <arch>`）→ 替换二进制并重启 | 需前端 `pnpm build` 产物在 `static/out` 才能内嵌（`scripts/build.sh`） |
| 数据库 | 启动自动迁移；跨引擎切换用设置页数据库迁移（需重启） | 切换前建议先备份；SQLite→其他会删旧 sqlite 文件（`internal/op/dbmigration/dbmigration.go:104-112`） |
| 前端缓存 | 更新后前端会清 SW 缓存并回根刷新；也可手动“强制刷新” | `Info.tsx`；`sw.js` 对 `/sw.js`、`index.html` 设 no-cache（`middleware/static.go:43-49`） |

---

## 三、用户在正常使用中能获得什么信息

前端为单页管理台，导航固定 14 个一级模块（顺序可自定义）：`home / hub / channel / pool / plugins / group / model / analytics / log / notification / ops / apikey / setting / user`（`nav-store.ts:20-35`）。下按“页面 → 可见信息 → 前端组件 → 后端接口”整理。权限按角色（admin/editor/viewer）裁剪，见 `internal/server/auth/permissions.go`。

### 3.1 Home（首页概览）
| 信息类内容 | 具体指标 | 前端 | 后端接口 |
|---|---|---|---|
| 今日概览 Hero | 平均响应时延、今日花费、今日调用次数（成功/总 + 成功率）、今日 Token（含缓存复用率） | `home/hero.tsx` | `GET /api/v1/stats/today`、`GET /api/v1/ops/cache` |
| 概览卡片 | 请求数、成功率、总 Token、总成本、Provider/API Key/Model 数量、Fallback 率（时间范围 7d/30d/90d/all；活跃/全部两种口径） | `home/analytics-overview.tsx` | `GET /api/v1/analytics/overview` |
| 趋势图 | 按日/时段请求等趋势（recharts 懒加载） | `home/chart.tsx` | `GET /api/v1/stats/daily`、`/hourly` |
| 活跃热力图 | 近 54 周日请求量热力图 | `home/activity.tsx` | `GET /api/v1/stats/daily` |
| 排行榜 | 渠道 / API Key 排行（按成本、请求数、Token） | `home/rank.tsx` | `GET /api/v1/stats/channel`、`/apikey` |

### 3.2 Analytics（分析）
子标签：`cache / utilization / route-health / channel-model / evaluation / latency`（`web/src/components/modules/analytics/index.tsx:31-38`、`sub-tab-store.ts:20`）。
| 子标签 | 可见信息 | 后端接口 |
|---|---|---|
| utilization | 按 Provider / Model / API Key 的请求数、Token、成本、成功率明细 | `GET /api/v1/analytics/utilization` |
| route-health（分组健康） | 每个分组健康分/状态（healthy/warning/degraded/down）、失败次数、最近失败时间、失败渠道明细、Auto 组实时快照 | `GET /api/v1/analytics/group-health` |
| channel-model | 渠道 × 模型维度的请求/Token/成本/成功率 | `GET /api/v1/analytics/channel-model` |
| evaluation | 语义缓存评估（命中率、条目、命中/未命中、存储响应数等） | `GET /api/v1/analytics/evaluation` |
| latency | 延迟分布：平均/P50/P95/P99、首字时间（FTUT）平均/P50/P95/P99、延迟直方图分桶 | `GET /api/v1/analytics/latency-distribution` |
| cache | 语义缓存运行态（复用 ops 的 Cache 面板） | `GET /api/v1/ops/cache` |

其他只在接口层暴露、UI 未直接列标签的还有 `provider-breakdown / model-breakdown / apikey-breakdown / auto-strategy`（`internal/server/handlers/analytics.go:17-60`）。

### 3.3 Log（日志）
子视图：转发日志 + 错误日志（`web/src/components/modules/log/index.tsx`、`ErrorLogView.tsx`）。
| 信息类内容 | 具体字段 | 后端接口 |
|---|---|---|
| 转发日志列表 | 时间、请求模型、API Key、客户端 IP、端点类型、渠道、实际模型、输入/输出 Token、语义缓存命中、缓存读 Token、思考强度/Token/字符数、首字时间、用时、费用、计费窗口、错误、若干次尝试记录、总尝试次数、是否测试 | `GET /api/v1/log/list`、`/api/v1/log/detail?id=` |
| 错误日志 | 来源（backend/frontend）、级别（panic/error/unhandledrejection/uncaught）、消息、堆栈、请求方法/路径、ClientIP、UA、页面 URL、路由 ID、版本 | `GET /api/v1/error-log/list` |
| 日志清理 | 清空转发日志/内容、清空错误日志 | `DELETE /api/v1/log/clear`、`/log/clear-contents`、`/api/v1/error-log/clear` |

字段定义见 `internal/model/log.go:27-58`（`RelayLog`）与 `internal/model/error_log.go:12-33`。

### 3.4 Notification / Alert / Report（通知、告警、报表）
通知模块分组：`messages / alerts / delivery / reports`；通知类型 `alert/report/channel_expire/system/site/backup/usage/key_health`，严重级 `info/success/warning/error/critical`（`web/src/components/modules/notification/index.tsx:40-42`；`internal/model/notification.go`）。
| 信息类内容 | 可见内容 | 后端接口 |
|---|---|---|
| 通知收件箱/归档 | 通知列表、未读数、详情、标记已读/未读/全读、删除、归档 | `/api/v1/notification/list`、`/unread-count`、`/detail/{id}`、`/mark-read`、`/mark-all-read`、`/delete/{id}` |
| 告警规则与历史 | 告警规则、通知渠道、告警历史 | `/api/v1/alert/rule/list`、`/alert/notif/list`、`/alert/history` |
| 投递偏好/策略 | 通知偏好、通知策略、投递记录 | `/api/v1/notification/preference/*`、`/policy/*`、`/delivery/list` |
| 报表 | 报表计划（日/周/月）、报表历史；可选指标：overview、top_models、top_channels、top_apikeys、cost_breakdown、error_analysis、daily_trend | `/api/v1/report/schedule/*`、`/report/history/list`、`/report/metrics` |

### 3.5 Ops（运维中心）
子标签：`telemetry / quota / health / maintenance / system / audit`（`web/src/components/modules/ops/index.tsx:16-21`、`sub-tab-store.ts:21`）。
| 子标签 | 可见信息（要点） | 后端接口 |
|---|---|---|
| telemetry | Hero（运行时长、总请求、平均延迟、错误率、活跃连接、内存 MB）、运行时信号（P95、RPS、内存、趋势点）、数据库健康（状态/问题/修复数）、会话与配额活动、提示缓存（条目/命中率/命中/未命中）、Provider 健康列表（请求数/成功率/平均延迟/健康状态）、下钻快捷入口 | `GET /api/v1/ops/telemetry` |
| quota | 密钥总数、启用/可用/过期/受限/无限/耗尽数量、按模型配额数、RPM/TPM/最高成本/最高 Token 合计、逐 Key 用量（请求数、成本、Token、成功率、状态） | `GET /api/v1/ops/quota` |
| health | 数据库/缓存/任务运行时是否 OK、近期错误数、健康分组计数（健康/警告/降级/宕机/空）、失败分组明细 | `GET /api/v1/ops/health` |
| maintenance | 维护策略概览（重试、熔断、响应过滤等当前生效配置） | `GET /api/v1/ops/system`（系统摘要含这些字段） |
| system | 版本/Commit/构建时间/仓库、数据库类型、公开 API BaseURL、代理 URL、日志保留策略、统计保存间隔、同步间隔、导入/导出开关、AI 路由配置、渠道/分组/Key 数量、维护策略 | `GET /api/v1/ops/system` |
| audit | 管理写操作审计日志（列表+详情） | `GET /api/v1/audit/list`、`/api/v1/audit/detail` |

数据结构见 `internal/model/ops.go`（`OpsCacheStatus`、`OpsQuotaSummary:74`、`OpsHealthStatus:100`、`OpsSystemSummary:121`、`OpsTelemetrySummary:165`）。

### 3.6 Model（模型市场与价格）
- 市场视图：汇总（模型数、覆盖率、去重渠道数、平均延迟、最后更新时间）+ 每条模型（名称/规范名/变体、输入/输出/缓存读/缓存写价格、渠道数、启用 Key 数、平均延迟、成功率、成功/失败请求数、渠道明细）（`internal/model/llm.go:61-90` `ModelMarketItem/ModelMarketSummary/ModelMarketResponse`；`web/src/components/modules/model/index.tsx`）。
- 相关接口：`GET /api/v1/model/market`、`/api/v1/model/list`、`/api/v1/model/channel`、`/api/v1/model/capabilities`、`/api/v1/model/price-category/list`、`/api/v1/model/price-schedule/list`、`GET /api/v1/model/last-update-time`；更新价格 `POST /api/v1/model/update-price`；价格分类/峰谷计划的增改删（`internal/model/llm.go`；`web/src/api/endpoints/model.ts:204-260`）。
- 价格字段：`LLMPrice{input,output,cache_read,cache_write}`（`internal/model/llm.go:5-9`）。

### 3.7 Channel / Group（渠道与分组）
- 渠道页：渠道列表、分组、批量分组、启用/禁用、拉取模型、按 Key 拉模型、渠道测试/Key 检查、模型测试进度、最后同步时间/触发同步（`web/src/api/endpoints/channel.ts`；`internal/server/handlers/channel.go`）。
- 分组页：分组列表/创建/更新/删除、自动分组、删除全部、清理不可用项、分组 AI 路由生成进度、分组测试/草稿测试与进度（`web/src/api/endpoints/group.ts`；`internal/server/handlers/group.go`、`route.go`）。
- 相关统计：`GET /api/v1/stats/channel` 提供渠道请求/成本/Token 等。

### 3.8 Hub（远程站点/聚合站，`remote-site` 模块）
子标签：`sites / site-channels / automation / balance / tokenplan`（`sub-tab-store.ts:19`）。
| 信息类内容 | 可见信息 | 后端接口 |
|---|---|---|
| 站点 | 远程站点列表（类型 new-api/veloera/done-hub/one-hub/sub2api/octopus/anyrouter/aihubmix/axonhub/claude-code-hub/sapi 等）、配额、健康状态、最后同步、公告 | `/api/v1/remote-site/list`、`/site/list`、`/announcement/list` |
| 站点渠道 | 站点渠道卡片（可用性、模型路由、Key、模型禁用、分组投影、历史） | `/api/v1/site-channel/list` |
| 自动化 | 自动检测站点类型、刷新/全部刷新、站点同步、签到、兑换、使用历史 | `/api/v1/site/detect`、`/remote-site/refresh*`、`/site/sync-all`、`/site/checkin-all`、`/redemption/*`、`/usage-history/*` |
| 余额 | 各厂商余额（plan-provider balance 类：DeepSeek/Kimi/SiliconFlow/OpenRouter/StepFun/302AI/Novita/OpenAI/TokenRhythm）、余额历史/曲线/预测 | `/api/v1/plan-provider/balance/*`、`/balance-history/*` |
| 套餐 | TokenPlan 类（MiniMax/智谱/StepFun/SenseNova/MiMo/Codex/百炼/火山/Kimi Coding/GLM Team 等）用量 | `/api/v1/plan-provider/tokenplan/*`、`/wallet/transactions/{id}` |

模型定义见 `internal/model/remote_site.go`、`internal/model/plan_provider.go`。

### 3.9 Pool（号池）与 Account/余额用量
- 号池：池列表/创建/更新/删除；池内账号（平台、类型、模型、额度快照、状态、可调度、并发、优先级、代理、过期、请求数/错误数/Token、临时不可调度原因、鉴权错误计数等）；账号测试、额度查询、刷新 Token、恢复、临时不可调度、批量刷新/清理/测试、导入导出、定时测试与结果、不可调度规则、代理池（`web/src/api/endpoints/pool.ts`、`proxy-pool.ts`；`internal/model/pool.go`）。
- 接口：`/api/v1/pool/list`、`/api/v1/pool/{id}/account/*`、`/api/v1/pool/{id}/scheduled-test/*`、`/api/v1/pool/unsched-rules/*`、`/api/v1/proxy-pool/*`、`/api/v1/pool/oauth/*`。
- 余额与用量也从 Hub 的 balance/tokenplan、`balance-history`、`usage-history` 获得（见 3.8）。

### 3.10 API Key（密钥统计）
- 列表/创建/更新/删除、启用状态、过期时间、MaxCost/MaxTokens、支持模型、允许分组、RPM/TPM、按模型配额、IP 白名单、标签、排除渠道（`internal/model/apikey.go` `APIKey`；`web/src/api/endpoints/apikey.ts`）。
- 统计：`GET /api/v1/apikey/stats`（每 Key 请求/成本/Token/成功率）；另有 `GET /api/v1/stats/apikey` 与 `GET /api/v1/ops/quota` 提供 Key 维度用量（`internal/server/handlers/apikey.go:46-50`、`stats.go:54`）。

### 3.11 Setting（设置 / 系统信息）
设置项（可拖拽排序）：`info / appearance / ai-route / auto-strategy / account / semantic-cache / log / system / llmsync / backup / redis / webdav / webauthn / normalize / pool / proxy-pool / nodes`（`web/src/components/modules/setting/index.tsx:38-56`）。
| 设置页可见信息/能力 | 说明 | 后端接口 |
|---|---|---|
| info（关于） | 当前版本、最新版本、GitHub 仓库、可更新提示、前端/后端版本不一致提示、强制刷新缓存 | `/api/v1/update`、`/api/v1/update/now-version` |
| system | 运行时系统信息（同 ops/system 的摘要视图） | `/api/v1/ops/system` |
| account | 修改用户名/密码 | `/api/v1/user/change-username`、`/change-password` |
| backup / webdav | 数据库导入导出、WebDAV 备份/恢复 | `/api/v1/setting/export`、`/import`、`/api/v1/backup/webdav/*` |
| redis / database | 缓存后端配置与连接测试、数据库连接测试与迁移 | `/api/v1/setting/cache/*`、`/database/test`、`/database/migrate` |
| ai-route / auto-strategy / semantic-cache / retry / circuit-breaker / response-filter | AI 路由服务、自动策略、语义缓存、重试、熔断、响应过滤等运行时策略 | `/api/v1/setting/*` |
| pool / nodes / proxy-pool | 号池设置、节点、代理池 | `/api/v1/pool/*`、`/api/v1/nodes/*`、`/api/v1/proxy-pool/*` |
| normalize / model-mapping | 模型名归一化规则、模型映射 | `/api/v1/model/normalize/*`、`/api/v1/model-mapping/*` |
| nodes | 本地节点列表与探测（可写标记、探测结果） | `/api/v1/nodes`、`/api/v1/nodes/{id}/test` |

### 3.12 User（用户管理）与登录
- 用户列表、创建、改角色、删除；角色 `admin/editor/viewer`（`web/src/api/endpoints/user.ts`、`internal/model/user.go:10-12`、`internal/server/handlers/user.go`）。
- 登录：JWT 登录、WebAuthn 登录/注册（`/api/v1/user/login`、`/api/v1/webauthn/*`）；首次部署有 `bootstrap/status` 与 `bootstrap/create-admin`。

### 3.13 对外转发 API（供使用者调用）
- `POST /v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/embeddings`（`internal/server/handlers/relay.go`）；节点隧道路径 `/node/v1`（`internal/server/handlers/node.go`）；媒体相关 `/v1`（`internal/server/handlers/media.go`）。
- 这些是“被使用”的 API，用户在 Log 页看到其请求明细（见 3.3）。

---

## 四、证据（文件:行号）

**后端/构建**
- `go.mod:1`（module）、`:3`（go 1.27.3）、`:12`（gin v1.11.0）、`:13`（glebarez/sqlite）、`:23-24`（cobra/viper）、`:34-36`（mysql/postgres/gorm）。
- `main.go:1-13`；`cmd/root.go`；`cmd/start.go`（`runStart`）；`cmd/version.go`。
- `internal/conf/version.go:10`（`Version="v2.7.3"`）、`:11-28`（Commit/BuildTime）。
- `internal/conf/config.go:133`（Load）、`:237-262`（setDefaults，含 `external.update_url/update_api_url`、`database.type=sqlite`、`security.encryption_key`）。
- `internal/db/db.go:102`（InitDBWithOptions）、`:123`（InitLogDBWithOptions）、`:241-258`（OpenStandaloneWithOptions 三种 DB 分派）、`:283-363`（Migrate + AutoMigrate）。
- `internal/db/migrate/migrate.go:11-120`（Migration/MigrationRecord/runMigrationsWithRecord）。
- `internal/op/dbmigration/dbmigration.go:25`（ValidateRequest）、`:74-118`（Migrate 流程）、`:104-112`（SQLite→非 SQLite 清理）。
- `internal/server/server.go:31-95`（Start：中间件装配/静态资源/RegisterAll）、`:177-193`（resolveLocalStaticDir）。
- `internal/server/router/router.go`（GroupRouter/Route/RegisterAll）。
- `internal/server/middleware/static.go:12-80`（StaticEmbed/StaticLocal/static 缓存与 SPA 回退）。
- `internal/server/auth/permissions.go:6-75`（权限与角色映射）；`internal/server/auth/auth.go:22-60`（JWT 过期钳制）。
- `scripts/build.sh:21-29`（GIT_VERSION/LDFLAGS）、`:311`（go build）、`:360`（create_archives）、`:428-439`（md5 校验）。
- `Dockerfile:4,27,50-76`（多阶段构建与版本注入）、`:78-124`（运行阶段/非 root/健康检查/入口）。

**前端**
- `web/package.json:3`（版本 2.7.3）、`:20,29-31,34,36-37,40,50`（关键依赖版本）。
- `web/next.config.ts:6-23`（版本解析）、`:27`（reactCompiler）、`:28`（output export）、`:33-38`（optimizePackageImports）、`:41`（NEXT_PUBLIC_APP_VERSION）、`:43`（assetPrefix "./"）。
- `static/static.go:10-21`（go:embed all:out、StaticFS）；`internal/server/server.go:78-86`（本地/内嵌静态资源二选一）。
- `web/public/manifest.json`（PWA manifest）；`web/public/sw.js:1-60`（SW 缓存策略）；`web/src/components/sw-register.tsx`（注册）；`web/src/lib/sw.ts`。
- `web/src/components/modules/navbar/nav-store.ts:20-35`（默认导航顺序）；`sub-tab-store.ts:19-21`（Hub/Analytics/Ops 子标签）。
- `web/src/route/config.tsx:35-51`（ROUTES 映射）。
- `web/src/app/page.tsx`（单页）、`web/src/app/layout.tsx`（全局 Provider/PWA meta）、`web/src/components/app.tsx:136`（AppContainer）。

**升级**
- `internal/server/handlers/update.go:22-38`（路由组 + 权限）、`:41-51`（latest/getNowVersion）、`:53-104`（updateFunc + 通知）。
- `internal/update/update.go:21-25`（大小上限）、`:27-49`（update_url/API URL/PAT）、`:114-129`（GetLatestInfo）、`:131-204`（unzip/extractFile/isPathInDest 防 zip-slip）。
- `internal/update/core.go:16-73`（UpdateCore 下载/解压/替换）、`:76-118`（Windows 替换/回滚）、`:125-157`（平台文件名）、`:159-186`（重启）。
- `web/src/api/endpoints/update.ts:27-70`（useLatestInfo/useNowVersion/useUpdateCore）。
- `web/src/components/modules/setting/Info.tsx:1-30`（更新 UI 与版本比较）、`handleUpdate/clearCacheAndReload`。

**用户可见信息**
- `internal/server/handlers/stats.go:30-55`（stats 路由）；`internal/model/stats.go:3-29`（StatsMetrics 指标字段）。
- `internal/server/handlers/analytics.go:17-60`（analytics 路由）；`internal/model/analytics.go:33-156`（Overview/Utilization/GroupHealth/LatencyDistribution）。
- `internal/server/handlers/ops.go:15-36`（ops 路由）；`internal/model/ops.go:74,100,121,165`（Quota/Health/System/Telemetry 结构）。
- `internal/model/llm.go:5-9`（LLMPrice）、`:61-90`（ModelMarketItem/Summary/Response）。
- `internal/model/log.go:27-58`（RelayLog/RelayLogListItem）；`internal/model/error_log.go:12-33`（ErrorLog）。
- `internal/model/notification.go`（类型/严重级）；`internal/model/report.go:1-70`（ReportSchedule/History/metrics）。
- `internal/model/remote_site.go`（站点类型/健康状态）；`internal/model/plan_provider.go`（余额/套餐厂商分类）；`internal/model/pool.go`（AccountPool/PoolAccount）；`internal/model/apikey.go`（APIKey 字段）。
- 前端页面：`web/src/components/modules/home/{index,hero,analytics-overview,activity,rank,chart}.tsx`、`analytics/index.tsx`、`ops/index.tsx`、`log/index.tsx`、`notification/index.tsx`、`model/index.tsx`、`setting/index.tsx`。

---

## 五、未确认项
1. **GORM AutoMigrate 的具体表清单是否随模型增删完全覆盖**：`internal/db/db.go:292-360` 之后的模型列表未逐行核对完整（列表较长，已确认包含核心 User/Channel/Group/APIKey/Stats*/RelayLog/ErrorLog/Notification/Site/Pool 等，但延伸到末尾的模型数量未逐项计数）。如需精确表清单可再逐行展开。
2. **`REFETCH_INTERVAL_SLOW` / `REFETCH_INTERVAL_CONFIG` 的具体秒数**：在 `web/src/api/constants.ts`（未展开），仅确认“慢轮询”语义，未确认具体值。
3. **Docker 容器内执行“立即更新”的实际行为**：源码层面 `UpdateCore` 会替换容器内 `/app/octopus` 并重启进程；但容器重建后是否持久化取决于运行时（镜像层只读 + 无持久挂载时不会持久）。未实测，属推断。
4. **自更新是否校验 release 资产完整性**：已确认 `internal/update` 无 checksum/签名校验代码；但未确认运维/CI 是否另有外部校验环节。
5. **`/api/v1/model/market` 等服务端缓存 TTL 与去重规则细节**：仅确认接口与字段，未逐条核对 `op.ModelMarketGet*` 的缓存实现。
6. **前端版本号是否总是等于后端版本**：前端 `web/package.json` 为 2.7.3、后端 `conf/version.go` 默认 v2.7.3，构建时二者分别注入；若发版流程未同步，可能出现不一致（Info 页已有“版本不一致”提示），实际发版一致性未验证。
7. **“普通用户”与“管理员”权限边界的 UI 表现**：已确认 RBAC 角色与权限集合，但前端是否对所有按钮按权限逐项隐藏未逐页核对（仅确认后端 `RequirePermission` 与 `HasPermission` 生效）。
8. **迁移编号是否有空缺/跳号**：`internal/db/migrate` 统计到 62 个非测试 `.go` 文件（001–062 连续命名 + 若干命名文件），未逐一验证每个 Version 常量的连续性。
