# sub2api 前后端技术栈、升级方案与用户可见信息调查

> 调查对象：`D:\CodeWorkspace\sub2api`（Go(ent) + Vue3 + PostgreSQL/Redis 的「AI API 网关 / 订阅配额分发」平台）
> 调查方式：只读源码，未修改仓库；调查日期：2026-11-03。文中路径相对仓库根 `D:\CodeWorkspace\sub2api`，`文件:行号` 均可回查。
> 定位：sub2api 把多家 AI 上游（Claude / OpenAI / Gemini / Antigravity / Grok / Kimi / GLM / DeepSeek / MiniMax 等）的订阅/配额，通过分组（Group）、渠道账号（Account）、API Key 与计费倍率，聚合为一个统一 API 网关对外分发，并提供用户面板、管理后台、订阅计费、渠道监控、运维监控、插件、备份、审计等能力（`README.md:180` 项目自述、`backend/internal/config/config.go:70` Config 结构、`backend/internal/server/router.go:120` `/api/v1`）。

---

## 一、前后端技术栈

### 1.1 后端（`backend/`）

**语言与运行时**

- **Go `1.27.3`**（`backend/go.mod:3`；Docker 构建镜像 `golang:1.27.3-alpine`，`Dockerfile:11`）。
- 模块名 `github.com/Wei-Shaw/sub2api`（`backend/go.mod:1`）。
- **没有 `backend/main.go`**：真正的入口是 `backend/cmd/server/main.go`，其它命令在 `backend/cmd/{jwtgen,profit-preview,cleanup-ingress-reject-logs}`（`backend/cmd/server/main.go:1` 起）。

**Web 框架与依赖注入**

- **Gin `v1.9.1`**（`backend/go.mod:21`）；路由装配见 `backend/internal/server/router.go:23-137`。
- **Google Wire `v0.7.0`**（`backend/go.mod:26`）做编译期依赖注入，生成 `backend/cmd/server/wire_gen.go`（`//go:generate go run ...wire`，`backend/cmd/server/main.go:3`；`initializeApplication` 定义于 `wire.go:34`、`wire_gen.go:35`）。

**ORM / 数据库**

- **entgo.io/ent `v0.14.5`**（`backend/go.mod:6`），schema 在 `backend/ent/schema/*`，生成代码在 `backend/ent/*`。
- 数据库为 **PostgreSQL**：`lib/pq v1.10.9`（`backend/go.mod:32`）；`InitEnt` 用 `entsql.Open(dialect.Postgres, dsn)` 打开连接（`backend/internal/repository/ent.go:116-130`）。
- `modernc.org/sqlite v1.44.3`（`backend/go.mod:65`）**仅用于单元/集成测试**（`_ "modernc.org/sqlite"` 出现在多个 `*_test.go`，如 `internal/repository/user_repo_email_lookup_unit_test.go:18`），运行时不用 SQLite。
- **SQL 迁移是 schema 的权威来源**（非 ent 自动建表）：注释明确「SQL 迁移文件是 schema 的权威来源（source of truth）」（`internal/repository/ent.go:133-135`），启动时 `applyMigrationsFS(ctx, drv.DB(), migrations.FS)`（`ent.go:136-143`）。
  - 迁移文件位于 `backend/migrations/*.sql`，用 `//go:embed *.sql` 打包进二进制（`backend/migrations/migrations.go:33`）；文件数量很多（`001_init.sql` … `2xx_*.sql`，含 `*_notx.sql` 非事务迁移）。
  - 迁移执行器 `backend/internal/repository/migrations_runner.go`：`schema_migrations(filename, checksum, applied_at)` 记录表（`:23-29`）、`pg_try_advisory_lock(694208311321144027)` 多实例串行化（`:51`、`:574-598`）、SHA256 checksum 校验与「已应用迁移不可修改」保护（`:201-271`）、历史 checksum 兼容白名单（`:77-99`）。
  - 另有 Atlas 迁移目录 `backend/ent/migrate/migrate.go`（`schema.NewMigrate`，`:50`），并在启动时对齐 `atlas_schema_revisions` 基线（`migrations_runner.go:31-46,169`）。

**缓存 / 中间件 / 异步**

- **Redis**：`github.com/redis/go-redis/v9 v9.22.0`（`backend/go.mod:36`）；`RedisConfig{Host,Port,Username,Password,DB}`（`internal/config/config.go:1662-1667`）；Docker 运行 `redis:8-alpine`（`deploy/docker-compose.yml:281-283`）。
- 缓存用途举例：更新检查结果缓存（`internal/service/update_service.go:31-32,596-638`）、面板限流计数、配额/并发原子计数、聚合缓存等（`internal/config/config.go:974-983`）。
- 定时与并发：`robfig/cron/v3`（`go.mod:38`）、`alitto/pond/v2`（`:12`）、`zeromicro/go-zero v1.9.4`（时间轮 `timing_wheel_service.go:9`）。

**gRPC / 插件（go-plugin）**

- **`google.golang.org/grpc v1.83.2`**（`go.mod:61`）用于插件宿主：`internal/service/plugin_runtime.go:22-25,111`、`plugin_host_services.go:10-11,138`。
- **`hashicorp/go-plugin v1.8.0`**（`go.mod:29`）做本地进程插件（`plugin_runtime.go:22`）；管理端插件页 `/admin/plugins`（`frontend/src/views/admin/PluginsView.vue`）。

**支付 / 对象存储 / 其它**

- 支付：`stripe-go/v85`（`go.mod:44`）、`smartwalle/alipay/v3`（`:41`）、`wechatpay-go`（`:52`）；前端对应 Stripe / Airwallex 支付页。
- 对象存储/备份：`aws-sdk-go-v2/service/s3`（`go.mod:17`），用于备份 S3 配置。
- 认证/安全：`golang-jwt/jwt/v5`、`go-webauthn/webauthn`（Passkey）、`pquerna/otp`（TOTP）、阿里云/腾讯云验证码（`go.mod:8,45`）。
- 配置/日志：`spf13/viper`（`:42`）、`go.uber.org/zap`（`:54`）；金额用 `shopspring/decimal`（`:40`）；令牌估算 `tiktoken-go/tokenizer`（`:51`）。

**启动流程（`backend/cmd/server/main.go`）**

1. 版本与构建信息：`Version/Commit/Date/BuildType` 为包变量，可被 ldflags 注入（`:32-38`）；`Version` 若未注入则读 `//go:embed VERSION`（`:29-30,46-51`）。当前 `backend/cmd/server/VERSION` = **`0.2.19`**。
2. 首次运行检测 `setup.NeedsSetup()`，支持 CLI setup / Docker 自动 setup（`AUTO_SETUP`）/ Web setup wizard（`:77-91,97-132`）。
3. 正常模式 `runMainServer()`：加载 `config.LoadForBootstrap()` → 初始化日志 → `initializeApplication(buildInfo)` → 启动插件与 Prompt Audit → 监听并等待 SIGINT/SIGTERM 优雅关闭（`:134-195`）。

**HTTP 路由结构（`backend/internal/server/router.go`）**

- 全局中间件：RequestLogger、SessionBindingContext、Logger、CORS、SecurityHeaders（含 CSP nonce）、ServerTiming（`router.go:59-71`）。
- 若前端已 embed，则挂载带设置注入的前端中间件（`router.go:73-90`）。
- 路由分组（`router.go:99-137`）：
  - `/health`、`/setup/status` 等公共路由（`routes/common.go:10-31`）。
  - **`/api/v1`** 面板 API：Auth / User / ModelPlaza / Admin / MCP / MCPOAuth / Gateway / Payment / Page（`router.go:120-136`）。
  - **`/v1`** AI 网关（API Key 鉴权）：`/v1/messages`、`/v1/chat/completions`、`/v1/responses`、`/v1/models`、`/v1/usage`、`/v1/sub2api/billing`、图片/视频/语音/搜索等（`routes/gateway.go:180-186,192-330`）。
  - `/v1beta`（Gemini）、`/backend-api`/`/codex`（Codex）、`/antigravity`（`routes/gateway.go:348-355,390-398,504-522`）。

**部署形态（后端视角）**

- **单二进制内嵌前端**：`-tags embed` 时 `//go:embed all:dist` 把 `backend/internal/web/dist` 打进二进制（`internal/web/embed_on.go:1,29-30`）；不带 `embed` 标签时前端返回 404「Frontend not embedded」（`internal/web/embed_off.go:1,32-37`）。
- Docker 多阶段：Stage1 构建 frontend → Stage2 `go build -tags embed` → Stage4 alpine 运行时（`Dockerfile:23-98,108-161`）。

### 1.2 前端（`frontend/`）

**框架与版本（`frontend/package.json`）**

| 类别 | 依赖 | 版本 | 出处 |
|---|---|---|---|
| 框架 | `vue` | `^3.5.43` | `package.json:32` |
| 构建 | `vite` | `^5.0.10` | `package.json:58` |
| 路由 | `vue-router` | `^4.2.5` | `package.json:36` |
| 状态 | `pinia` | `^2.1.7` | `package.json:30` |
| 国际化 | `vue-i18n` | `^11.4.16` | `package.json:35` |
| 图表 | `chart.js` / `vue-chartjs` | `^4.4.1` / `^5.3.0` | `package.json:25,33` |
| HTTP | `axios` | `^1.20.0` | `package.json:24` |
| 支付 | `@stripe/stripe-js` / `@airwallex/components-sdk` | `^9.0.1` / `^1.30.2` | `package.json:21,19` |
| 其它 | `@vueuse/core`、`xlsx`、`marked`、`dompurify`、`driver.js`、`qrcode`、`file-saver`、`vue-draggable-plus` | — | `package.json:22-37` |
| 样式 | `tailwindcss` | `^3.4.0` | `package.json:56` |
| TS / 测试 | `typescript ~5.6.0`、`vue-tsc ^2.2.0`、`vitest ^2.1.9` | — | `package.json:57,61,60` |

- 包管理器 **pnpm**（`pnpm-lock.yaml`；Docker `corepack prepare pnpm@9`，`Dockerfile:29`；构建脚本 `pnpm --dir frontend run build`，`Makefile:44`）。
- 构建脚本：`build = check:i18n && vue-tsc -b && vite build`、`typecheck`、`test/test:run`、`lint`（`package.json:6-17`）。

**目录结构（`frontend/src`）**

- `api/`（`client.ts`、`url.ts`、`keys.ts`、`usage.ts`、`payment.ts`、`modelPlaza.ts`、`channelMonitor*.ts`、`admin/` 子目录等）；`stores/`（`auth`、`app`、`adminSettings`、`subscriptions`、`payment`、`announcements`、`onboarding`、`adminCompliance`）；`router/index.ts`；`views/`（`user/`、`admin/`、`auth/`、`public/`、`setup/`、`HomeView.vue`、`KeyUsageView.vue`、`ModelPlazaView.vue`）；`components/`、`composables/`、`features/`（`channel-monitor-v2`、`prompt-audit`）、`i18n/`、`constants/`、`types/`、`utils/`。
- 页面按 `frontend/src/views/{admin,user,auth,public,setup}` 分组，详见第三节。

**是否内嵌到 Go（embed）还是独立部署**

- **生产内嵌**：Vite `build.outDir = '../backend/internal/web/dist'`（`vite.config.ts:103`），产物被 Go `-tags embed` 内嵌进单二进制（`internal/web/embed_on.go:29-30`）。因此生产是「一个 Go 二进制 + 内嵌 SPA」，同域部署。
- **开发独立**：`pnpm dev` 起 Vite（端口 `VITE_DEV_PORT` 默认 3000），把 `/api`、`/v1`、`/setup` 反代到后端 `VITE_DEV_PROXY_TARGET`（默认 `http://localhost:8080`）（`vite.config.ts:80-84,153-169`）。
- 开发模式下 Vite 插件还会从 `/api/v1/settings/public` 拉公开配置注入 `window.__APP_CONFIG__`（`vite.config.ts:53-78`），与生产 `embed_on.go:145-218` 的注入逻辑对齐。
- 前端 API 基址默认 **`/api/v1`**（`src/api/url.ts:1`），网关地址用 `buildGatewayUrl`（同域 origin + `/v1` 等路径，`url.ts:32-43`）。i18n 运行时版（避免 CSP unsafe-eval）在 `vite.config.ts:97-100` 配置。

**Vite 分包**：`vendor-vue`、`vendor-ui`、`vendor-chart`、`vendor-i18n`、`vendor-stripe`、`vendor-misc`（`vite.config.ts:111-149`）。

### 1.3 部署一览

| 部署方式 | 构建/运行 | 关键文件 | 说明 |
|---|---|---|---|
| 本地源码编译 | `make build`（前端 `pnpm build` + 后端 `go build`）；后端 `backend/Makefile` 用 `-ldflags "-X main.Version=$(VERSION)"` | `Makefile:36-44`、`backend/Makefile:1-15`、`backend/scripts/resolve-version.sh` | 版本优先级：精确 git tag > `cmd/server/VERSION` 文件 |
| Docker / Compose | 多阶段构建（frontend→go embed→alpine）；运行镜像 `weishaw/sub2api:latest`，挂 `./data` | `Dockerfile:1-161`、`deploy/docker-compose.yml:18-29,235-292` | 依赖 `postgres:18-alpine`、`redis:8-alpine`；`AUTO_SETUP=true` |
| GoReleaser（发布产物） | 交叉编译 linux/win/mac × amd64/arm64，`-tags embed`，`-X main.BuildType=release`；产出 tar.gz/zip + `checksums.txt` + 多架构 Docker 镜像/manifest（DockerHub + GHCR） | `.goreleaser.yaml:9-69,124-169`、`.goreleaser.simple.yaml:10-60`、`Dockerfile.goreleaser:1-63` | 简化版只发 x86_64 GHCR 镜像 |
| GitHub Actions | `release.yml`：prepare→build-frontend→build-binaries→release→sync-version-file | `.github/workflows/release.yml:83-191,193-408` | tag `v*` 触发；发布后把 `cmd/server/VERSION` 回写为发布版本（`:378-408`） |
| 一键脚本（Linux + systemd） | `deploy/install.sh install/upgrade/rollback`，systemd 单元 `Restart=always` | `deploy/install.sh`、`deploy/sub2api.service:1-33` | 从 GitHub Releases 下载对应平台包并替换，详见第二节 |

---

## 二、升级方案

sub2api 的「升级」由三条线组成：**① 管理端在线自更新（管理员在 UI 点「更新」→ 后端下载 GitHub Release 资产、校验、原子替换二进制 → 重启）**；**② 脚本/部署侧升级（`install.sh` / Docker 镜像 tag / GoReleaser 发布）**；**③ 数据库迁移（二进制启动时自动执行内嵌 SQL）**。下面逐一说明。

### 2.1 版本号来源

- 编译期变量：`Version`、`Commit`、`Date`、`BuildType`（`backend/cmd/server/main.go:32-38`）。
- `Version` 优先由 **ldflags `-X main.Version=...`** 注入；未注入则读内嵌 `VERSION` 文件，再缺省 `0.0.0-dev`（`main.go:40-51`）。
- **`BuildType` 默认 `source`**，GoReleaser / Dockerfile 里用 `-X main.BuildType=release`（`main.go:37`、`.goreleaser.yaml:32`、`Dockerfile:95`）。
- Makefile 的版本来自 `backend/scripts/resolve-version.sh`：优先精确 git tag（`vX.Y.Z`），否则读 `backend/cmd/server/VERSION`（`backend/Makefile:3-4`、`resolve-version.sh:9-23`）。
- 当前仓库 `backend/cmd/server/VERSION` = `0.2.19`。`BuildInfo{Version,BuildType}` 经 wire 注入到 `UpdateService`（`cmd/server/wire_gen.go:250-251,385-388`、`internal/service/wire.go:46-49`）。

### 2.2 检查更新（GitHub Releases + 缓存 TTL）

- 服务：`internal/service/update_service.go`。
- 仓库固定 `githubRepo = "Wei-Shaw/sub2api"`（`:33`）；缓存 key `update_check_cache`、TTL **1200s（20 分钟）**（`:31-32`）。
- `CheckUpdate(ctx, force)`：非 force 先读缓存（`:133-139`）→ 拉取最新 release → 失败时回退缓存并带 `Warning`（`:141-156`）→ 结果写缓存（`:158-160`）。
- `fetchLatestRelease`：`strings.TrimPrefix(tag,"v")` 得 latest，`compareVersions(current,latest) < 0` 判定有更新（`:402-433`）；版本比较为三段整数、忽略 `-suffix`（`:640-669`）。
- GitHub 客户端：`internal/repository/github_release_service.go`。
  - `FetchLatestRelease` → `https://api.github.com/repos/{repo}/releases/latest`（`:123-147`）。
  - `FetchRecentReleases` → `.../releases?per_page=N`（N 上限 100，`:149-179`）。
  - 可选鉴权 `UPDATE_GITHUB_TOKEN`（仅对 `api.github.com` 加 `Authorization: Bearer`，`:68,101-103`）；允许代理，代理失败默认禁止回退直连（`:34-70`）；重定向到非 GitHub API 域名会剥离 Authorization（`:82-92`）。
- HTTP 接口（管理员）：`GET /api/v1/admin/system/check-updates?force=true`（`routes/admin.go:676`、`handler/admin/system_handler.go:71-81`）；`GET /api/v1/admin/system/version`（`admin.go:675`、`system_handler.go:62-69`）。

### 2.3 下载 / 校验 / 替换 / 重启

- 下载与校验核心 `applyReleaseAssets`（`update_service.go:181-280`）：
  1. 按 `runtime.GOOS_GOARCH` 选资产（`getArchiveName` 返回 `linux_amd64` 这类，`:439-443`），并要求存在 `checksums.txt`（`:187-194`）。
  2. **`validateDownloadURL` 安全限制**：必须 `https`，host 只允许 `github.com`、`*.github.com`、`objects.githubusercontent.com`、`*.objects.githubusercontent.com`（常量 `allowedDownloadHost`/`allowedAssetHost`，`:36-37`，校验 `:447-469`）。
  3. 下载到**可执行文件同目录**的临时目录（保证 `os.Rename` 同文件系统原子性，`:222-228`）；`maxDownloadSize = 500MB`（`:40`，下载器再以 `LimitReader` 二次兜底 `github_release_service.go:198-224`）。
  4. **校验和**：仅当 release 含 `checksums.txt` 时校验（`:237-241`）；`verifyChecksum` 对文件算 SHA256 并在 `checksums.txt` 中按「hash filename」匹配（`:471-506`）。若资产无 checksums 则跳过（不阻断）。
  5. **解压**：`extractBinary` 仅提取名为 `sub2api`/`sub2api.exe` 的普通文件；带 Zip-Slip/路径穿越防护（拒绝 `..`）、只接受 `tar.TypeReg`、`LimitReader` 限 500MB 防解压炸弹（`:508-594`）。
  6. **原子替换**：`chmod 0755` → `os.Rename(current, current.backup)` → `os.Rename(new, current)`；第二步失败则把 backup 换回（`:249-279`）。成功后保留 `.backup` 供回滚（`:277-279`）。
- **重启**：`POST /api/v1/admin/system/restart`（`admin.go:680`、`system_handler.go:203-229`）在返回响应后 500ms 调 `sysutil.RestartServiceAsync()`，即「优雅退出 `os.Exit(0)`，由 systemd `Restart=always` 拉起」（`internal/pkg/sysutil/restart.go:23-48`）。更新/回滚接口返回 `need_restart: true`，提示重启。
- **长任务与幂等**：更新/回滚上下文与 HTTP 请求生命周期解耦，超时 **15 分钟**（`systemUpdateTimeout`，`system_handler.go:25-44`），并有系统操作锁 + 幂等键（`Idempotency-Key`，`system_handler.go:85-129,167-199,250-264`）。

### 2.4 回滚（allowed rollback versions）

- **本地回滚**：无 body 时 `Rollback()` 把 `current.backup` 换回 current（`update_service.go:283-304`）。
- **指定版本回滚**：`ListRollbackVersions` / `RollbackToVersion`（`:306-361`）。
  - 只暴露**比当前版本更旧**的版本，最多 **3 个**（`maxRollbackVersions = 3`，`:43`）；拉取最近 **15** 条 release 过滤（`rollbackFetchPageSize = 15`，`:45`）；跳过 draft/prerelease、去重、按语义版本降序（`:365-400`）。
  - 目标版本必须命中上述允许列表，否则返回 `ErrRollbackVersionNotAllowed`（`ROLLBACK_VERSION_NOT_ALLOWED`，`:27,329-349`）；`v` 前缀会去掉（`:330`）。
- 接口：`GET /api/v1/admin/system/rollback-versions`、`POST /api/v1/admin/system/rollback`（`admin.go:677,679`、`system_handler.go:131-199`）。
- 测试覆盖：`update_service_test.go`（列表过滤/去重/上限/排序、拒绝当前/更新/过旧/不存在版本、接受 `v` 前缀；`:81-187`）。
- 前端交互：`frontend/src/components/common/VersionBadge.vue`。**仅 release 构建显示更新/在线回滚入口**（`isReleaseBuild = buildType==='release'`，`:731-732`）；source 构建只显示「有更新」提示与 git pull 指引（`:234-291`、`:398-417`）。在线回滚失败时还给出脚本/Docker 两种手动回滚命令（`scriptRollbackCommand`/`dockerRollbackCommand`，`:701-729`）。

### 2.5 buildType = source / release 的差异

- 后端：`BuildType` 只是随 `UpdateInfo` 上报（`update_service.go:88,154,431,621`），后端不硬性禁止 source 构建更新；真正决定「在线更新可用性」的是**发布渠道是否存在对应平台的 Release 资产**。
- 前端：`buildType` 来自 `GET /admin/system/check-updates`（`src/api/admin/system.ts:21,37`，`stores/app.ts:263-268`），据此决定：
  - `release`：显示「更新」按钮，`POST /admin/system/update`（`VersionBadge.vue:295-356`），并可在线回滚。
  - `source`：只给 GitHub Release 链接 + 「源码构建请 git pull 升级」提示（`VersionBadge.vue:234-291`）。
- 发布侧：GoReleaser 与 Dockerfile 均设 `BuildType=release`（`.goreleaser.yaml:32`、`Dockerfile:95`），源码 `make build` 默认 `source`（`main.go:37`）。

### 2.6 数据库迁移如何执行

- 迁移 SQL 内嵌于二进制（`migrations.FS`，`internal/repository/ent.go:139`）。
- 启动时 `InitEnt` 调 `applyMigrationsFS`，10 分钟超时并以 `initializeDatabaseWithRetry` 重试（`ent.go:136-143`）。
- 执行器要点（`migrations_runner.go`）：
  - 建 `schema_migrations` 表并按文件名排序执行（`:23-29,121-130`）。
  - **PostgreSQL advisory lock** 串行化多实例（`:51,574-598`）。
  - 按 **SHA256 checksum** 记录/校验；已应用迁移被改动会报错并给出回退提示（`:201-271`）；对历史误改有白名单兼容（`:77-99`）。
  - 支持 `*_notx.sql` 非事务迁移（如 `CREATE INDEX CONCURRENTLY`，`:53,506`）。
  - 与 Atlas `atlas_schema_revisions` 基线对齐（`:31-46,169`）。
- 因为迁移随二进制升级一起发布，**升级（替换二进制）→ 重启 → 新版本自动执行新增迁移**；回滚二进制时不会自动「降级 schema」（回滚只替换可执行文件）。

### 2.7 Docker / GoReleaser 部署的升级方式

- **Docker**：把镜像 tag 改为新版本（如 `weishaw/sub2api:<version>`）后 `docker compose up -d` 重建容器（`deploy/docker-compose.yml:19`）；数据在命名卷 `sub2api_data`（`:32,322-325`）。容器内二进制启动时自动跑迁移。在线「自更新」对容器场景不适用（替换容器内文件无意义），前端也提示改用镜像 tag 回滚（`VersionBadge.vue:716-725`）。
- **GoReleaser / GitHub Actions**：tag `v*` 触发 `release.yml`，构建各平台归档 + `checksums.txt` + 多架构镜像，并回写 `cmd/server/VERSION`（`.goreleaser.yaml:34-49,124-169`、`release.yml:378-408`）。
- **脚本安装（Linux + systemd）**：`deploy/install.sh` 提供 `install` / `upgrade|update` / `rollback <version>` / `status` 等子命令（`:1085-1220`）；从 GitHub Releases 拉对应平台 archive 与 `checksums.txt`（`:617-621`）、替换后 `systemctl` 重启（`:888,950`）；回滚即安装指定旧版本（`:893-950,1138-1146`）。systemd 单元 `Restart=always`（`deploy/sub2api.service:13`）。

---

## 三、用户可见信息（按页面 / 接口分类）

> 说明：前端路由定义见 `frontend/src/router/index.ts:20-750`；页面文件在 `frontend/src/views/*`。面板 API 前缀 `/api/v1`（`router.go:120`），网关 API 前缀 `/v1`（`routes/gateway.go:180`）。下表「接口」列即页面数据来源。

### 3.1 用户侧（普通用户）

| 页面（路由） | 组件文件 | 用户能看到的信息/数据 | 主要后端接口 |
|---|---|---|---|
| 首页 `/home` | `views/HomeView.vue` | 站点介绍、公开配置（站点名/Logo/公告入口） | `GET /api/v1/settings/public`（`routes/auth.go:243-246`） |
| 用户仪表盘 `/dashboard` | `views/user/DashboardView.vue` + `components/user/dashboard/*` | **余额**（`UserDashboardStats.vue:5-18`）、**API Key 数/活跃数**（`:20-32`）、**今日/累计请求**（`:34-46`）、**今日/累计成本（实际扣费 / 标准计费）**（`:48-67`）、**今日/累计 Token（输入/输出/缓存）**（`:70-98`）、**RPM/TPM**（`:100-118`）、**平均响应时长**（`:120-132`）、**分平台用量与配额**（按平台成本/请求/Token + 日/周/月配额进度，`:135-224`）、**用量趋势图**、**模型用量图**、**最近用量 Top5**（`DashboardView.vue:5-11,33-37`） | `GET /api/v1/usage/dashboard/stats`、`/dashboard/trend`、`/dashboard/models`、`/dashboard/snapshot-v2`、`POST /dashboard/api-keys-usage`（`routes/user.go:114-119`）；`GET /api/v1/user/platform-quotas`（`user.go:48`） |
| 使用记录 `/usage` | `views/user/UsageView.vue` | 请求明细：时间、模型、请求数、输入/输出/缓存 Token、总 Token、标准成本、实际成本；按日期/模型/分组/Key/请求类型/流式过滤；错误记录 | `GET /api/v1/usage`、`/usage/stats`、`/usage/errors`、`/usage/errors/:id`（`routes/user.go:106-120`） |
| API 密钥 `/keys` | `views/user/KeysView.vue` | Key 列表、分组、配额（额度/限速）、启用状态、最近使用、按天用量（近 N 天） | `GET/POST/PUT/DELETE /api/v1/keys`（`user.go:83-90`）；`GET /api/v1/user/api-keys/:id/usage/daily`（`user.go:47`） |
| 我的订阅 `/subscriptions` | `views/user/SubscriptionsView.vue` | 订阅套餐、有效期、额度使用/进度、状态 | `GET /api/v1/subscriptions`、`/active`、`/progress`、`/summary`（`user.go:137-143`） |
| 购买订阅 `/purchase` | `views/user/PaymentView.vue` | 套餐列表、价格、支付方式（Stripe/Airwallex/微信/支付宝）、订单确认 | `routes/payment.go` 注册的支付接口（`router.go:134`）；前端 `api/payment.ts` |
| 我的订单 `/orders` | `views/user/UserOrdersView.vue` | 订单号、套餐、金额、支付状态、时间 | 支付/订单相关接口（`api/payment.ts`，`router.go:134`） |
| 支付结果 `/payment/result`、`/payment/stripe`、`/payment/airwallex`、`/payment/qrcode` | `views/user/PaymentResultView.vue`、`StripePaymentView.vue`、`AirwallexPaymentView.vue`、`PaymentQRCodeView.vue` | 支付状态、二维码、结果回执 | 同上（`router/index.ts:353-410`） |
| 卡密兑换 `/redeem` | `views/user/RedeemView.vue` | 输入卡密兑换余额/额度、兑换历史 | `POST /api/v1/redeem`、`GET /api/v1/redeem/history`（`user.go:130-134`） |
| 邀请返利 `/affiliate` | `views/user/AffiliateView.vue` | 邀请码/链接、已邀请人数、返利额度、划转 | `GET /api/v1/user/aff`、`POST /api/v1/user/aff/transfer`（`user.go:41-42`） |
| 可用渠道 `/available-channels` | `views/user/AvailableChannelsView.vue` | 当前可用渠道/分组、模型可用性、限速信息 | `GET /api/v1/channels/available`（`user.go:100-103`）；`GET /api/v1/groups/available`、`/groups/rates`（`user.go:93-97`） |
| 渠道状态 `/monitor` | `views/user/ChannelStatusView.vue`（V1/V2） | 各渠道/模型可用性、状态色块、最近探测结果 | `GET /api/v1/channel-monitors`、`/:id/status`（`user.go:146-150`）；V2：`/api/v1/channel-monitor-v2/{dimensions,snapshot,models,matrix,errors,users}`（`user.go:153-163`） |
| 模型广场 `/model-plaza` | `views/ModelPlazaView.vue` | 模型列表、平台、价格/倍率、上下文、可用性 | `routes/model_plaza.go` 注册（`router.go:129`）；前端 `api/modelPlaza.ts` |
| Key 用量查询 `/key-usage`（公开） | `views/KeyUsageView.vue` | 用 API Key 查询该 Key 的今日/累计用量与模型统计 | 网关 `GET /v1/usage`（`gateway.go:210`、`gateway_handler.go:1590-1634`） |
| MCP 连接 `/settings/mcp` | `views/user/MCPView.vue` | MCP 连接配置、授权列表 | `GET /api/v1/user/mcp/config`、`/mcp/grants`（`user.go:32-38`） |
| 个人资料 `/profile` | `views/user/ProfileView.vue` | 邮箱/身份绑定、通知邮箱、TOTP、Passkey、改密 | `GET/PUT /api/v1/user/profile`、`/user/password`、`/user/totp/*`、`/user/passkeys`（`user.go:31-79`） |
| 公告 | 全局（`stores/announcements.ts`） | 公告标题/内容、已读标记 | `GET /api/v1/announcements`、`POST /:id/read`（`user.go:123-127`） |
| 批量图片指引 `/batch-image` | `views/user/BatchImageGuideView.vue` | 批量图片任务指引/用量 | 网关 batch image 接口（`gateway.go:258-265`） |

**用户侧「网关」可见计费信息**：
- `GET /v1/sub2api/billing` 返回当前 API Key 的有效计费倍率：`group_rate_multiplier`、`user_rate_multiplier`、`resolved_rate_multiplier`、峰值倍率（`peak_*`）、`effective_rate_multiplier`、时区（`gateway_key_billing.go:16-31,80-108`）。
- `GET /v1/usage` 返回该 Key 的 today/total 用量、按天用量、模型统计（`gateway_handler.go:1590-1634`）。

### 3.2 管理员侧

| 页面（路由） | 组件文件 | 管理员能看到的信息/数据 | 主要后端接口 |
|---|---|---|---|
| 管理仪表盘 `/admin/dashboard` | `views/admin/DashboardView.vue` | **API Key 总数/活跃**（`:12-30`）、**账号总数/正常/异常**（`:32-55`）、**今日/累计请求**（`:57-75`）、**今日新增用户/用户总数**（`:77-95`）、**今日/累计 Token**（`:98-169`）、**成本三分：实际扣费 / 账号成本 / 标准计费**（`:113-130,149-166`）、**RPM/TPM**（`:172-196`）、**平均响应时长/活跃用户**（`:198-216`）、模型分布图、Token 趋势图、用户用量 Top12 趋势、用户消费排行（`:267-344`） | `GET /api/v1/admin/dashboard/{stats,snapshot-v2,realtime,trend,models,groups,api-keys-trend,users-trend,users-ranking,user-breakdown}`、`POST /api/v1/admin/dashboard/{users-usage,api-keys-usage}`（`routes/admin.go:283-298`） |
| 运维监控 `/admin/ops` | `views/admin/ops/OpsDashboard.vue` | 实时 QPS/流量、并发统计、账户可用性、吞吐趋势、延迟直方图、错误趋势/分布、OpenAI Token 统计、告警规则/事件、错误日志、请求错误、上游错误、入口拒绝、系统日志 | `/api/v1/admin/ops/*`：`concurrency`、`user-concurrency`、`account-availability`、`realtime-traffic`、`dashboard/{snapshot-v2,overview,throughput-trend,latency-histogram,error-trend,error-distribution,openai-token-stats}`、`alert-rules`、`alert-events`、`errors`、`request-errors`、`upstream-errors`、`ingress-rejections`、`system-logs`、`ws/qps`（`admin.go:195-280`） |
| 审计日志 `/admin/audit-logs` | `views/admin/AuditLogView.vue` | 操作人、动作、对象、时间、IP、结果 | `/api/v1/admin/audit-logs*`（`admin.go:134` 注册 `registerAuditLogRoutes`） |
| 用户管理 `/admin/users` | `views/admin/UsersView.vue` | 用户列表、余额、并发、平台配额、属性、用量 | `/api/v1/admin/users*`（`admin.go:303-325`） |
| 分组管理 `/admin/groups` | `views/admin/GroupsView.vue`（+ 多个 `groups*.ts`） | 分组、平台、倍率、图片/视频/搜索/音频定价、模型白名单、利润率控制、排序 | `/api/v1/admin/groups*`（`admin.go:338-350`） |
| 渠道（定价） `/admin/channels/pricing` | `views/admin/ChannelsView.vue` | 渠道/模型定价、倍率、默认价、批量维护 | `/api/v1/admin/channels*`（`admin.go:791`） |
| 渠道监控 `/admin/channels/monitor` | `views/admin/ChannelMonitorView.vue` + `features/channel-monitor-v2` | 渠道/模型监控、配额、状态矩阵、错误 | `/api/v1/admin/channel-monitors*`、`/channel-monitor-v2*`（`admin.go:817,857`） |
| 账号管理 `/admin/accounts` | `views/admin/AccountsView.vue` | 上游账号、平台、凭证、额度、健康、用量、批量更新、额度探测 | `/api/v1/admin/accounts*`（`admin.go:366-423`） |
| 订阅管理 `/admin/subscriptions` | `views/admin/SubscriptionsView.vue` | 订阅列表、进度、分配/批量分配、续期、重置额度、撤销/恢复 | `/api/v1/admin/subscriptions*`（`admin.go:684-699`） |
| 使用记录 `/admin/usage` | `views/admin/UsageView.vue` | 全站用量明细/统计、搜索用户/Key、清理任务 | `/api/v1/admin/usage*`（`admin.go:708-716`） |
| 卡密 `/admin/redeem` | `views/admin/RedeemView.vue` | 卡密列表、批量生成/更新、使用记录 | `/api/v1/admin/redeem*`（`admin.go:553` 附近，`registerRedeemCodeRoutes`） |
| 优惠码 `/admin/promo-codes` | `views/admin/PromoCodesView.vue` | 优惠码、使用次数、明细 | `/api/v1/admin/promo-codes*`（`admin.go:564,566`） |
| 代理 `/admin/proxies` | `views/admin/ProxiesView.vue` | 代理列表、协议、可用性、绑定 | `/api/v1/admin/proxies*`（`admin.go:531`） |
| 系统设置 `/admin/settings` | `views/admin/SettingsView.vue` | 站点信息/品牌、注册/登录、OAuth、支付、倍率、超时/限流/冷却、模型白名单、通知、插件等 | `/api/v1/admin/settings*`（`admin.go:575-610`） |
| 风控中心 `/admin/risk-control` | `views/admin/RiskControlView.vue` | 内容审核配置、规则、违规记录 | `/api/v1/admin/risk*`、`content-moderation`（`admin.go:176`） |
| 提示词审计 `/admin/prompt-audit` | `features/prompt-audit/PromptAuditView.vue` | 审计事件、配置、运行时、删除 | `/api/v1/admin/prompt-audit/*`（`admin.go:138-149`） |
| 插件 `/admin/plugins` | `views/admin/PluginsView.vue` | 插件列表、安装、能力、运行状态 | `/api/v1/admin/plugins*`（`admin.go:109` 注册） |
| 公告 `/admin/announcements` | `views/admin/AnnouncementsView.vue` | 公告 CRUD、发布状态 | `/api/v1/admin/announcements*`（`admin.go:450`） |
| 备份 `/admin/backup` | `views/admin/BackupView.vue` | 备份列表、S3 配置、镜像存储、计划、下载/恢复 | `/api/v1/admin/backup*`（`admin.go:646-668`） |
| 邀请返利（管理）`/admin/affiliates/{invites,rebates,transfers}` | `views/admin/affiliates/*` | 邀请记录、返利记录、划转记录、线下提现 | `/api/v1/admin/affiliate*`（`admin.go:131,839`） |
| 支付管理 `/admin/orders/dashboard`、`/admin/orders`、`/admin/orders/plans` | `views/admin/orders/*` | 支付看板、订单、套餐 | `/api/v1/admin/...` 支付相关（`router.go:134` 的 `RegisterPaymentRoutes`） |
| 系统版本/更新（顶部版本徽标） | `components/common/VersionBadge.vue` | 当前版本、最新版本、是否有更新、更新/回滚入口、Release 说明 | `/api/v1/admin/system/{version,check-updates,rollback-versions,update,rollback,restart}`（`admin.go:672-681`） |

### 3.3 关键指标归类（用户能看到哪些数据）

- **资产/余额类**：用户余额（`UserDashboardStats.vue:5-18`）、订阅额度与进度、卡密兑换余额、邀请返利额度；管理员侧账号余额/额度、用户余额。
- **用量/Token 类**：请求数（今日/累计）、Token（输入/输出/缓存创建/缓存读取/总量）、按模型/按分组/按平台聚合；网关 `/v1/usage` 按 Key 用量。
- **成本/计费类**：标准计费 vs 实际扣费（用户侧两者并列，`UserDashboardStats.vue:56-63`）；管理员侧「实际扣费 / 账号成本 / 标准计费」三口径（`admin/DashboardView.vue:113-166`）；计费倍率（`/v1/sub2api/billing` 的 group/user/peak/effective 倍率）。
- **性能类**：RPM、TPM、平均响应时长、活跃用户/活跃 Key、并发统计、延迟直方图、吞吐趋势（用户 + 管理员）。
- **健康/可用性类**：渠道状态、账号可用性、模型矩阵、渠道监控、错误分布/趋势、上游错误、入口拒绝（管理员）；用户侧渠道状态只读视图。
- **运维/审计类**：告警规则/事件、错误日志、请求错误、系统日志、审计日志、备份、插件、版本更新（管理员）。

---

## 四、证据（文件:行号）

**后端技术栈 / 入口 / 路由**
- `backend/go.mod:1`（模块）、`:3`（go 1.27.3）、`:6`（ent v0.14.5）、`:21`（gin v1.9.1）、`:26`（wire）、`:29`（go-plugin）、`:32`（lib/pq）、`:36`（go-redis v9）、`:53`（go-zero）、`:61`（grpc）、`:44`（stripe）、`:65`（sqlite，仅测试）。
- `backend/cmd/server/main.go:29-51`（VERSION 内嵌 + ldflags Version/Commit/Date/BuildType）、`:55-95`（main/setup/runMainServer）、`:134-195`（启动与优雅关闭）。
- `backend/cmd/server/VERSION:1`（`0.2.19`）。
- `backend/cmd/server/wire_gen.go:250-251,385-388`（ProvideUpdateService 注入 BuildInfo）；`backend/cmd/server/wire.go:34,70-81`。
- `backend/internal/server/router.go:59-71`（全局中间件）、`:73-90`（embed 前端）、`:99-137`（路由分组 `/api/v1`、`/v1` 等）。
- `backend/internal/server/routes/common.go:10-31`（/health、/setup/status）。
- `backend/internal/server/routes/gateway.go:180-186`（`/v1` 与 `/v1/sub2api/billing`）、`:210`（`/v1/usage`）、`:348-355,390-398,504-522`（v1beta/codex/antigravity）。
- `backend/internal/repository/ent.go:104-167`（InitEnt、Postgres 方言、`applyMigrationsFS`）、`:133-143`（SQL 迁移为权威）。
- `backend/migrations/migrations.go:1-34`（`//go:embed *.sql`）。
- `backend/internal/repository/migrations_runner.go:23-29`（schema_migrations 表）、`:48-52`（advisory lock）、`:77-99`（checksum 兼容白名单）、`:114-119`（ApplyMigrations）、`:201-271`（checksum 校验/执行）。
- `backend/internal/config/config.go:70,79`（Config/Redis）、`:739-741`（ServerConfig host/port）、`:1603-1607`（DatabaseConfig）、`:1662-1667`（RedisConfig）。

**前端技术栈**
- `frontend/package.json:6-17`（脚本）、`:18-37`（dependencies，vue/vite/pinia/vue-router/vue-i18n/chart.js/axios/stripe/airwallex）、`:39-62`（devDependencies，tailwind/typescript/vue-tsc/vitest）。
- `frontend/vite.config.ts:80-84`（dev 端口/代理目标）、`:103`（outDir `../backend/internal/web/dist`）、`:111-149`（manualChunks）、`:153-169`（dev proxy `/api`、`/v1`、`/setup`）。
- `frontend/src/api/url.ts:1`（默认 `/api/v1`）、`:17-43`（API/网关地址构造）。
- `frontend/src/api/client.ts:21-28`（axios baseURL/timeout）。
- `backend/internal/web/embed_on.go:1,29-30`（`-tags embed` / `//go:embed all:dist`）、`:145-218`（注入 `__APP_CONFIG__`）；`backend/internal/web/embed_off.go:1,32-37`（无 embed 返回 404）。
- `Dockerfile:11,23-98,108-161`（多阶段与 `-tags embed`、`BuildType=release`）。

**升级方案**
- `backend/internal/service/update_service.go:31-46`（缓存 TTL、仓库、allowedDownloadHost/allowedAssetHost、maxDownloadSize、maxRollbackVersions、rollbackFetchPageSize）、`:133-176`（CheckUpdate/PerformUpdate）、`:181-280`（applyReleaseAssets 下载/校验/原子替换）、`:283-361`（Rollback/ListRollbackVersions/RollbackToVersion）、`:365-400`（回滚候选过滤/上限）、`:439-469`（archiveName + URL 白名单）、`:471-506`（SHA256 校验）、`:508-594`（解压与 Zip-Slip/体积防护）、`:596-638`（缓存）、`:640-669`（版本比较）。
- `backend/internal/service/update_service_test.go:81-187`（回滚列表/拒绝目标/`v` 前缀）。
- `backend/internal/repository/github_release_service.go:34-70`（客户端/代理/UPDATE_GITHUB_TOKEN）、`:123-179`（latest/recent release API）、`:181-227`（DownloadFile 体积限制）、`:229-246`（checksums）。
- `backend/internal/handler/admin/system_handler.go:25-44`（15 分钟超时 + 请求解耦）、`:62-81`（version/check-updates）、`:85-129`（PerformUpdate 幂等/锁）、`:131-199`（rollback-versions/rollback）、`:203-229`（restart）。
- `backend/internal/server/routes/admin.go:672-682`（system 路由）。
- `backend/internal/pkg/sysutil/restart.go:23-48`（优雅退出重启，依赖 systemd Restart=always）。
- `backend/Makefile:3-7`（`VERSION`/LDFLAGS `-X main.Version`）、`backend/scripts/resolve-version.sh:9-23`（tag > VERSION）。
- `.goreleaser.yaml:9-49`（builds/archives/checksum，`-X main.BuildType=release`）、`:56-169`（Docker 镜像/manifest）；`.goreleaser.simple.yaml:10-60`。
- `Dockerfile:56-98`（构建参数 VERSION/COMMIT/DATE + `-X main.BuildType=release`）；`Dockerfile.goreleaser:1-63`。
- `.github/workflows/release.yml:83-191`（build-frontend/build-binaries）、`:193-408`（release 发布 + 版本注释 + sync-version-file）。
- `deploy/install.sh:534-621`（latest 版本/下载/checksums）、`:866-950`（upgrade 与指定版本安装）、`:1085-1220`（子命令分发，含 rollback）；`deploy/sub2api.service:1-33`。
- `deploy/docker-compose.yml:18-29`（应用镜像）、`:235-292`（postgres:18-alpine / redis:8-alpine）。
- 前端更新 UI：`frontend/src/components/common/VersionBadge.vue:234-356`（source 提示 vs release 更新按钮）、`:377-630`（回滚面板）、`:701-729`（脚本/Docker 手动回滚命令）、`:731-732`（`buildType==='release'` 判定）；`frontend/src/api/admin/system.ts:21-106`；`frontend/src/stores/app.ts:243-277`。

**用户可见信息**
- `frontend/src/router/index.ts:20-750`（全部路由，用户/管理员分组）。
- `frontend/src/views/user/DashboardView.vue:5-37`（Dashboard 组件与数据加载）。
- `frontend/src/components/user/dashboard/UserDashboardStats.vue:3-224`（余额/Key/请求/成本/Token/RPM/TPM/响应时长/分平台配额）、`:227-402`（统计字段与格式化）。
- `frontend/src/api/usage.ts:23-56`（`PlatformDashboardStats`/`UserDashboardStats` 字段定义）。
- `backend/internal/handler/usage_handler.go:451-515`（DashboardStats/Trend/Models）、`:517-575`（DashboardSnapshotV2）、`:577-608`（模型/分组统计转换）。
- `backend/internal/server/routes/user.go:29-164`（用户全部接口：user/keys/groups/channels/usage/announcements/redeem/subscriptions/channel-monitors/v2）。
- `backend/internal/handler/gateway_key_billing.go:16-31,35-108`（`/v1/sub2api/billing` 倍率字段）。
- `backend/internal/handler/gateway_handler.go:1590-1634`（`/v1/usage`：today/total、按天、模型统计）。
- `backend/internal/server/routes/admin.go:283-298`（admin dashboard）、`:195-280`（ops 指标与告警）、`:303-325`（用户）、`:338-350`（分组）、`:684-699`（订阅）、`:708-716`（usage）、`:791-857`（渠道/监控）。
- `frontend/src/views/admin/DashboardView.vue:11-216`（管理员仪表盘指标卡片）、`:267-344`（图表/排行）。
- `frontend/src/api/admin/system.ts:21-37`（VersionInfo/build_type）。

---

## 五、未确认项

1. **任务给出的入口 `backend/main.go` 实际不存在**：后端入口是 `backend/cmd/server/main.go`（已在本文按实际路径引用）。若需 `backend/main.go` 的旧版结构，请确认是否为历史版本。
2. **checksums 缺失时是否仍执行更新**：`applyReleaseAssets` 只在存在 `checksums.txt` 时校验（`update_service.go:237-241`），无 checksums 时按代码逻辑会跳过校验直接替换（未强制失败）。这是否为刻意容忍发布资产缺失，代码未注释说明，属静态推断。
3. **`verifyChecksum` 的匹配规则**：按「`hash  filename`」两列精确匹配 `filepath.Base`（`update_service.go:492-505`），对 checksums 文件格式有强假设；若发布端采用 `sha256sum` 带 `*`/`./` 前缀等变体，可能匹配失败（未在发布产物上实测）。
4. **source/release 的后端行为边界**：后端仅上报 `build_type`，未在服务端禁止 source 构建执行 `PerformUpdate`（`update_service.go`/`system_handler.go` 未见 buildType 分支）；是否依赖「source 构建无对应平台 Release 资产」来间接阻断，未找到显式校验。
5. **回滚与 schema 的关系**：回滚只替换可执行文件（`update_service.go:283-304`），迁移执行器只有「前进」没有自动降级（`migrations_runner.go`）。旧版本二进制遇到已升级的 schema 是否完全兼容，取决于各迁移是否向后兼容，未逐一评估全部 `2xx` 迁移。
6. **管理员仪表盘底部大文件字段未逐条核对**：`AccountsView.vue`（~2483 行）、`ChannelsView.vue`（~1595 行）、`UsageView.vue`（~831 行）体量大，本文按路由/表头与接口归纳其可见信息，可能遗漏个别二级弹窗/调试面板字段。
7. **部分管理页对应精确接口前缀**：`admin.go` 中对部分 CRUD（如 redeem/promo/proxy/backup/settings 的细分）通过 `registerXxxRoutes` 间接注册，本文给出前缀与代表行号（如 `admin.go:531,553,564,575,646`），未逐条列出每个子路由的完整方法+路径。
