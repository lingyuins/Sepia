# magpie 仓库调查：前后端技术栈 · 升级方案 · 用户可见信息

> 调查对象：`D:\CodeWorkspace\magpie`（只读，未修改任何源码）。
> 调查方式：源码静态阅读 + 关键接口/函数定位，结论均给 `文件:行号`；凡仅凭结构名推断、未逐行核实的，标注“未确认”。
> 说明：本仓库为 Go 单仓多包（`github.com/yetone/magpie`），前端资产内嵌进二进制，官网在 `site/` 作为 Cloudflare Worker 独立部署。

---

## 一、前后端技术栈

### 1.1 后端（Go 主体）

| 项目 | 事实 | 证据 |
| --- | --- | --- |
| 模块 / Go 版本 | `module github.com/yetone/magpie`，`go 1.26.8` | `go.mod:1`、`go.mod:3` |
| 桌面壳 | Wails v3，`github.com/wailsapp/wails/v3 v3.0.0-beta.24` | `go.mod:20` |
| TUI 框架 | `charmbracelet/bubbletea v1.3.10`、`lipgloss v1.1.1-0.2025…` | `go.mod:8`、`go.mod:9` |
| SQLite 驱动 | `modernc.org/sqlite v1.44.3`（纯 Go，无 CGO） | `go.mod:28` |
| 入口 | `main.go` 仅做初始化：嵌入 `THIRD_PARTY_NOTICES`、把 `version` 传给 `cli.Main(version)`；`version` 由构建期 `-ldflags "-X main.version=…"` 注入，默认为 `dev` | `main.go:14-31` |
| 命令分发 | 全部命令在 `internal/cli`。子命令含：`tui`/`web`/`serve`/`tray`/`app`/`gui`/`panel`/`autostart`/`daemon`/`agents`/`providers`/`provider`/`models`/`model`/`groups`/`gateway-key`/`accounts`/`usage`/`sessions`/`quota`/`update`/`library`/`backup`/`restore`/`webdav`/`plugin`/`s3`/`github`/`claude-code`/`settings` 等 | `internal/cli/main.go:451-560` |
| 进程角色标记 | 按启动参数记录 `gateway.Serves`（`tui`/`web`/`serve`/`tray`/`app`），供本机另一个 magpie 在 Routing 页显示“谁在提供服务” | `internal/cli/main.go:448-457` |

**`internal/` 主要包（按职责归类，非全部）**

- 网关与路由：`gateway`（约 1000+ 行 `gateway.go`，另含 `routing.go`、`decide.go`、`fallback.go`、`anthropic.go`、`responses.go`、`chat.go`、`trace.go`、`concurrency.go` 等大量网关能力）
- 桌面/页面：`gui`（`api.go` 114KB、`app.go`、`web.go`、`assets/`、`update*.go`、`whatsnew.go` 等）
- 终端：`tui`（bubbletea，见 1.3）
- Agent 适配/接线：`agent`、`agentenv`、`agentplug`、`claudebridge`、`claudecode`、`claudetui`、`codex*`（`codexcat`/`codexrelay`/`codexsummary`）等
- 供应商与账号：`provider`、`catalog`、`access`、`budget`、`gwbudget`、`usage`、`usageapi`、`stats`、`settings`、`settingsapi`、`profile`
- 插件运行时：`plugin`、`pluginrt`、`jsmod`、`middleware`
- 存储/文件：`appdir`、`filelock`、`filememo`、`sqlitedb`、`backup`、`davsync`、`library`、`sessions`
- 安全/网络：`masking`、`maskrec`、`redact`、`pii`、`netguard`、`netproxy`、`upstream`、`oidc`、`mcpauth`
- 升级/运维：`update`、`daemon`、`autostart`、`awake`、`shortcut`、`uninstall`、`proc`、`applog`、`stats`
- 其它：`beat`、`steady`、`tune`、`fx`、`lastgood`、`notices`、`browserext`、`browsermcp`、`davsync` 等

（来源：`internal/` 目录清单，`internal/gui/` 文件清单）

**数据库：没有“magpie 自己的中心库”，只有“读 Agent 自己的 SQLite”**

- `internal/sqlitedb` 本身不是数据库实现，而是一个**空导入的包装**：在进程最早阶段用 `libc.SetEnviron` 对 `os.Environ()` 做清洗（去掉没有 `=` 的坏环境项），避免 modernc 的 libc 在 macOS/Windows 上 `getenv` 直接 `os.Exit(1)`（问题 #2327）| `internal/sqlitedb/sqlitedb.go:1-45`
- 它只被这些包“空白导入 + 用 `sql.Open("sqlite", …)`”：`internal/agent/aionui.go:28`、`internal/agent/cindy.go:24`、`internal/agent/newmax.go:58`、`internal/provider/importapps.go:29`
- 具体 `sql.Open("sqlite", …)` 读取的是**各 Agent 自己的本地库**：`internal/agent/newmax.go:266`、`internal/provider/importapps.go:976`、`internal/provider/kiro.go:430`、`internal/sessions/codex_catalog.go:128`、`internal/sessions/codex_provider.go:406`、`internal/sessions/codex_resume.go:37`、`internal/sessions/cursor.go:219`
- magpie 自身状态主要落在**配置文件与日志文件**：配置目录 `appdir.Config()`（`internal/appdir/appdir.go:262`），设置文件 `settings.json`（`internal/settings/settings.go:1475`），用量账本 `usage.jsonl`（`internal/usage/usage.go:277`，路径取 `provider.Path()` 同目录）

### 1.2 前端（内嵌手写 HTML/CSS/JS，无框架、无打包器）

| 项目 | 事实 | 证据 |
| --- | --- | --- |
| 资产内嵌 | `//go:embed assets` → `var assets embed.FS`，由 `gui.Handler` 组装 `http.ServeMux`，静态文件走 `devPage(revalidated(http.FileServer(http.FS(staticFS()))))` | `internal/gui/api.go:153-154`、`internal/gui/api.go:968`、`internal/gui/api.go:977-978` |
| 页面骨架 | `index.html`（约 45KB）手写，`<script src=...>` **按顺序逐个加载**（`boot.js`→`theme.js`→`omarchy.js`→`compat.js`→`layout.js`→`fonts.js`→…→`i18n.js`→`kernel.js`→各内置插件脚本→`app.js`→`plugins.js`→`pluginpages.js`→`context.js`→各插件 page/settings→`start.js`），无任何打包产物 | `internal/gui/assets/index.html:22-38, 539-568` |
| 主脚本 | `app.js`（约 1.3MB）文件头自述“one state object per view, rendered into a list. **No framework.**” | `internal/gui/assets/app.js:1` |
| 样式 | `app.css`（约 400KB）、`context.css`、`plugins.css`、`omarchy.css` 等 | `internal/gui/assets/` 清单 |
| i18n | `i18n.js`（约 2.1MB）英文 key → 多语言字典，`t(str, vars)`，缺失回退英文 | `internal/gui/assets/i18n.js:1-12` |
| 插件内核 | `kernel.js`：窗口内插件内核（slot/error boundary/ctx.ui kit），在 `app.js` 之前加载，负责把插件页插入 `<template data-slot="page">` | `internal/gui/assets/kernel.js:1-18`、`index.html:146-147` |
| 插件隔离页 | `pluginpages.js`：社区插件自页跑在“仅 allow-scripts、独立 origin”的 iframe 沙箱里，经消息与窗口通信，权限由 `package.json` 的 `magpie.permissions` + `/api/plugin-pages/read` 校验 | `internal/gui/assets/pluginpages.js:1-21` |
| 内置前端插件 | `assets/plugins/{gateway,library,otel,redact,routing,search,sessions,usage}`，各含 `client.js`/`page.js`/`settings.js`/`i18n.js`/`.css` | `index.html:26-30,542-567`、`internal/gui/assets/plugins/` |
| 图标 | `assets/icons/`（193 个图标文件） | 目录清单 |

- 页面视图（导航）：`Agents`、`Sessions`、`Library`、`Providers`、`Routing`、`Gateway`、`Usage`、`Masking`、`Plugins`，外加 `Settings`（`index.html:67-80`）。详见第三节。

### 1.3 桌面壳 / `magpie web` / TUI / 官网

| 形态 | 事实 | 证据 |
| --- | --- | --- |
| 桌面 App（Wails v3） | `internal/gui/app.go`（`//go:build !nogui`）用 `wailsapp/wails/v3/pkg/application` 建窗口/托盘；内嵌托盘图标与动画 `tray.png`、`trayflap/`、`winTrayIcon`；`mainwindow.go` 定义 macOS 窗口行为 | `internal/gui/app.go:1-55`、`internal/gui/mainwindow.go:1-31` |
| `magpie web`（浏览器模式） | 默认监听 `127.0.0.1:3430`，复用同一 `Handler`；支持 `--addr/--lan/--no-open/--gateway/--tls-cert/--tls-key`；URL 携带访问 key（`MAGPIE_WEB_KEY` 可固定）；`ServesGateway()` 判断是否本进程服务网关，否则只是读同一份配置/账号 | `internal/cli/web_cli.go:18-103`、`internal/gui/web.go:51-145` |
| `magpie serve`（服务/网关） | `magpie serve [--web[=host:port]] [--tls-*]`：只跑网关；带 `--web` 时同进程再提供页面（即服务 `magpie daemon install --web` 运行的形态） | `internal/cli/web_cli.go:203-247` |
| 服务守护 | `internal/daemon`（`daemon.go`、`launchd.go`、`systemd.go`）：macOS 写 LaunchAgent、Linux 写 systemd user unit，执行 `magpie serve`（带页面时 `magpie serve --web=<addr>`） | `internal/daemon/` 文件与测试注释 `internal/daemon/daemon_test.go:76,240,431-436` |
| TUI | `internal/tui`（bubbletea）：`pages = ["agents","providers","routing","usage","sessions","library"]`，1–6 切换；每 Agent 一行，方向键选字段回车改 | `internal/tui/tui.go:1-4`、`internal/tui/pages.go:41` |
| 官网 | `site/` 是 **Cloudflare Worker + 静态站**：`worker.js`（约 47KB）处理 `/api/latest`、`/api/notes`、`/download/*`、`/api/partners`、`/go/*`、`/api/icon` 等，其余走 `public/` 静态资产；配置 `wrangler.jsonc`（`main: worker.js`、`assets: ./public`、KV `LEDGER`、cron `*/15 * * * *`） | `site/worker.js:1-44,73-185`、`site/wrangler.jsonc` |
| 官网打包情况 | **没有 `site/package.json`**（实测 `Test-Path site\package.json` = False）；部署直接用 `npx wrangler deploy`，构建脚本为 `node credits-cli.mjs wall`（生成贡献者墙）。即官网不是 npm 工程、无前端打包器 | `site/wrangler.jsonc`、`site/` 顶层清单（含 `credits-cli.mjs`、`worker.test.mjs`、`stable.test.mjs`、`install.test.mjs` 等测试） |
| 官网文案多语言 | `site/i18n.js`、`public/` 下大量 `omarchy/<theme>/*.webp` 主题截图 | `site/i18n.js`、`site/public/img/omarchy/` |

---

## 二、升级方案

### 2.1 总览：版本从哪来、谁去查

- **版本来源**：构建期 `-ldflags "-X main.version=…"`（`main.go:16-19`）；`update.Released(v)` 判定是否为“正式发布版”（排除 `dev`、`git describe` 的 `v0.1.0-3-gxxxx`、`dirty`），只有发布版才自我更新（`internal/update/update.go:110-119`）。
- **更新源**：`Site = https://usemagpie.ai`，feed = `Site + "/api/latest"`；`MAGPIE_UPDATE_FEED` 可覆盖（便于本地测试）（`internal/update/update.go:41-51`）。发布物在 GitHub `yetone/magpie-releases`（`site/worker.js:50`）。
- **Feed 数据结构** `Release{version, notes, url, assets{name:{url,size,sha256}}}`（`internal/update/update.go:53-66`）。
- **下载走代理**：用独立 `http.Client`，`netproxy.Func`（Settings 的 Proxy / 环境 / 系统代理，#294）（`internal/update/update.go:68-78`）。
- **双语 notes**：`LatestIn(ctx, lang)` 用 `withLang` 加 `?lang=`、`withChannel` 加 `?channel=stable`（`internal/update/update.go:86-108`、`internal/update/notes.go:133-188`）。

### 2.2 检查更新（稳定 / 其他 channel）

- **自动检查节奏**：`updateDue(s,last,now) = !NoAutoUpdate && now-last >= UpdateEvery 分钟`；`UpdateEvery` 默认 360 分钟（6h）（`internal/gui/update.go:135-137`、`internal/settings/settings.go:2307-2308`）。
- **可选间隔**：`30m/1h/6h/24h`（即 `UpdateEveries = [30,60,360,1440]`），CLI 为 `magpie update auto [on|off] [download|notify] [30m|1h|6h|24h]`（`internal/cli/update_cli.go:31,38-78`、`internal/settings/settings.go:1206-1207`）。
- **两种 channel**：
  - `UpdateChannel == ""`（默认，**每个版本一发布就推**）；
  - `UpdateChannel == "stable"`（`settings.UpdateStable`）：**一周一个版本，紧急版立即推**。
  - 证据：`internal/settings/settings.go:462-468,1071-1073,1204-1205`；`withChannel` 会追加 `?channel=stable`（`internal/update/notes.go:174-188`）。
- **Stable 的选版规则在官网**：`/api/latest?channel=stable` 走 `stable(ctx,env,now)`；`stableOf(list,now)` 取“本周（周一 00:00 UTC）开始前已出三天的最新版”或“任意带 `**Urgent release:**` 的紧急版（取更新者）”；未知时返回 503（`site/worker.js:80-92,603-660`）。`Urgent` 由 `urgentIn(notes)` 从 release notes 的固定标记识别（`site/worker.js:567-571`）。
- **检查状态机**（`updater.state`）：`checking | latest | found | downloading | ready | available | source | error | blocked`（`internal/gui/update.go:30-87`）。核心流程 `run()`：读 Settings → `update.LatestIn` → 判定 `source/latest/available/blocked/ready/found/downloading`（`internal/gui/update.go:384-494`）。
- **UI 的“已是最新”防陈旧**：`latest` 只在 5 分钟内新鲜（`freshAfter`），`GET /api/update?fresh=1` 会再问一次（`internal/gui/update.go:326-369,666-671`）。
- **CLI 检查**：`magpie update` / `magpie update check`（check 只报不装），可带 `--proxy`、`--mirror`（`internal/cli/update_cli.go:174-266`）。

### 2.3 下载 / 校验（SHA-256）/ 替换 / 重启

- **强制校验**：`download()` 若无 `asset.SHA256` 直接报错“the release lists no checksum”（`internal/update/update.go:1061-1064`）；`fetch()` 下完用 `fileHash(path)` 与 `asset.SHA256` 折半比较，不符则“does not match its checksum / …not installed”（`internal/update/update.go:1238-1245`）。
- **断点续传 + 重试**：`fetch` 用 `Range` 续传、416 时重下整包、网络中断保留已下字节下次续传；`download` 按 `retryWaits` 重试（仅对 `Network(err)` 重试）（`internal/update/update.go:1070-1082,1135-1247`）。
- **镜像兜底**：`mirrored(mirrorOf(ctx), a.URL)`；经镜像下载仍用 feed 的 SHA-256 校验（`internal/update/update.go:1143,1240-1243`；镜像检查 `internal/update/mirror.go`）。
- **进度上报**：`WithProgress(ctx, f)` + `counter` 把已下/总量报给 GUI（`update.done/total`）（`internal/update/update.go:1047-1059,1218-1227`）。

**macOS App（替换整个 .app bundle）**

- `AppAsset() = magpie-darwin-<GOARCH>.zip`（`internal/update/update.go:203`）。
- `Stage()`：下载 zip → `ditto -x -k` 解包 → 临时名 `payload`（不叫 `.app`，避免 LaunchServices 注册第二个 magpie #3965）→ `sameSigner()` 校验：`codesign --verify --deep --strict` 且 TeamIdentifier 与已装 App 一致（`internal/update/update.go:263-323`）。
- `Install(staged,bundle)`：`bundle→old` 重命名、`staged→bundle` 重命名，失败则 `old→bundle` 回滚（`internal/update/update.go:350-362`）。
- `InstallAsAdmin`：目录非本人可写时用管理员密码执行 `swapScript`（`internal/update/update.go:364-373`）。
- `Relaunch(bundle,…)`：本进程退出后按 log 重试 `open`（约 13s 多次重试，防 LaunchServices 偶发 -600），`relaunch.log` 记录（`internal/update/update.go:375-430`）。

**Windows / Linux 裸二进制（替换自身 exe）**

- `BinaryAsset()`：CLI 为 `magpie-cli-<os>-<arch>[.exe]`；GUI 非 darwin 为 `magpie-<os>-<arch>[.exe]`（`internal/update/update.go:204-213`）。
- `StageBinary→stageBinaryFor`：下载到缓存/程序旁的隐藏 `.magpie-update`；`os.Chmod 0o755`（`internal/update/update.go:547-619`）。
- `InstallBinaryAside`：Windows 上先 `tryStart(staged)`（`canStart`，为 Smart App Control 预检）、`RemoveOld` 清旧、`moveAside(exe)` 把运行中 exe 改名 `.old`、`moveIn(staged,exe)` 换入，失败则 `moveIn(old,exe)` 还原（`internal/update/update.go:700-757,889-941`）。
- `InstallBinaryAsAdmin`：需管理员时走提权（`internal/update/update.go:992-1005`）。
- `RelaunchBinary(exe, old, …)`：退出后启动新二进制，必要时用 `old` 回滚（`internal/update/update.go:1007-1030`）。

### 2.4 自动更新 / 重启等待 / 回滚

- **自动下载开关**：`NoAutoDownload` 为真时，检查到新版本只标 `found`（在 Version 行/Update 胶囊提示），点击才下载（#2296）（`internal/gui/update.go:388-393,463-467`；`internal/settings/settings.go:442-448`）。
- **自动重启**：`AutoRestart` 开时，下载完成且网关空闲后自动 `restartInto(...,bySelf=true)`（`internal/gui/update.go:80-87`；`internal/gui/update_wait.go:167-188`）。`NoAutoUpdate` 开则不做任何自动动作（`internal/settings/settings.go:436-441`）。
- **重启等空闲**：`idleAt(gateway.Busy, now)` 判断“没有会中断的请求”；`waitIdle` 在网关忙时等待，`cancelWait`/`callOff` 可取消；JSON 里 `busy/waiting/gaveUp`（#577）（`internal/gui/update_wait.go:42-160`；`internal/gui/update.go:117-126`）。
- **网关交接（handover）**：macOS/Linux 上重启时新版本先接管网关，旧进程把在飞请求做完（`internal/gui/update.go:834-848`、`internal/gui/update.go:123-127`；handover 实现见 `internal/gui/handover.go`）。
- **回滚/自愈**：
  - 失败换入还原：macOS `Install` 的 `os.Rename(old,bundle)`（`update.go:356-359`）；Windows `moveIn(old,exe)` 或 `InstallBinaryAside` 的还原分支（`update.go:745-753`）。
  - 旧二进制留存：`oldName` 生成 `exe.old` / `exe.old-2…`；`RemoveOld` 清理不再运行的旧件（`update.go:889-941`）。
  - 被替换检测：`Replaced(exe, started)` 通过 size/mtime 判定本进程是否已被别的 magpie 换掉（`update.go:946-948`），避免重复换入。
  - **Windows Smart App Control / 应用控制策略拒绝**（#894）：`BlockedError`（`internal/update/blocked.go:26-62`）；`startChecked` 让新版本启动并写 `MAGPIE_STARTED` 标记，超时/退出则杀进程并把 `old` 放回（`internal/update/blocked.go:70-167`）；被拒版本 `NoteBlocked` 记入 `update-blocked.json`，**时钟触发的检查不再下载它**，手动检查才再试一次（`internal/update/blocked.go:185-218`；`internal/gui/update.go:451-455,542-554`）。
  - **包管理器安装的二进制**：`PackagedBy(exe)` 识别 brew/pacman(AUR)/deb 等，交给包管理器升级，不自行替换（`internal/update/packaged.go:78`；`internal/cli/update_cli.go:251-258`）。
  - **容器内**：不可自替换，提示 `docker pull ghcr.io/yetone/magpie:latest`（`internal/cli/update_cli.go:246-250`）。
  - **Homebrew**：`Homebrew(exe)` 命中 `/Cellar/magpie/` 时不自替换（`update.go:233-238`）。

### 2.5 桌面 App 与 `magpie web` / 服务的差异

| 形态 | 是否运行内建更新器 | 重启方式 | 证据 |
| --- | --- | --- | --- |
| 桌面 App（`magpie app/gui/tray`） | 是：`runGUI` 内 `updates.start()` | 用 `update.Relaunch` / `RelaunchBinary`，macOS/Linux 先 `beginHandover` 交接网关 | `internal/gui/app.go:553-554`、`internal/gui/update.go:802-863` |
| `magpie web` | 是：`StartWeb` 内 `updates.start()`，并设置 `updates.autoRestart` | `restartInto(web=true)` 只置 `webReexec`，由 `Web.Wait()` 用同参数、同 key `update.Reexec` 原地重启（浏览器页不丢） | `internal/gui/web.go:60-62,111-145`、`internal/gui/update.go:830-833` |
| `magpie serve` / 守护服务 | 否：没有 GUI 更新器，`restarter` 为空 | `magpie update` 先替换二进制，再 `POST /v1/magpie/restart`；无 `restarter` 时回 `unsupported`，需“按启动它的方式重启”（如重启 systemd/launchd 服务） | `internal/gateway/restart.go:38-83`、`internal/cli/update_cli.go:268-306` |
| 通用 | `update.GUI` 由 `hasGUI`（`gui_on.go`）在 `main.go:219` 注入，决定 `BinaryAsset` 取 GUI 名还是 CLI 名 | — | `internal/gui/gui_on.go:13`、`internal/cli/main.go:219`、`internal/update/update.go:200-213` |

> `magpie update` 运行在终端里替换“当前那个 magpie”后，会通知正在服务的 magpie（`gateway.RestartPath`），本机回环、仅程序可调用（页面/远程被拒）（`internal/gateway/restart.go:51-83`）。

### 2.6 各组件自身的更新

| 组件 | 更新机制 | 证据 |
| --- | --- | --- |
| Agent CLI（Claude Code / Codex / OpenCode / Grok 等） | Agents 页显示版本与最新版；按安装来源用对应方式更新：CLI 自带 `claude update`/`codex update`/`opencode upgrade`/`grok update`，或 npm/bun/pnpm/Homebrew；`CLI{version,latest,via,command,update,app,admin,path}` | `internal/agent/cliupdate.go:1-60`（结构 33-60、`howInstalled` 255、`UpdateCLI` 1026、`CLIs` 910） |
| 社区插件（`@magpie-community/*`） | magpie 启动后不久及每小时自动更新；其他人的插件只显示“有新版”圆点，等用户点击；锁定版本的保持不动；更新不切断正在流式的回复 | `internal/plugin/updates.go:1-77`（`updateEvery=1h`、`Updates/Waiting/Updated` 33-61、`Official/Pinned` 75-80） |
| Bun（插件宿主运行时） | 每 6 小时查 GitHub 最新 Bun；**发布满 48h 才采纳**；下载→校验→试跑→切换宿主，旧的保留以回退，起不来的记 `Bad` 永不再试；`BunVersion` 为下限 | `internal/plugin/bunupdate.go:1-70`（`bunSettle=48h` 32、`bunState{Current,Previous,Bad}` 34-43） |
| 更新镜像 | `magpie update mirror <prefix>|off`，`settings.UpdateMirror`；即使走镜像，校验和仍是 usemagpie.ai 的 | `internal/cli/update_cli.go:80-108`、`internal/gui/update.go:2433` |

### 2.7 官网发布 feed 机制（site/worker.js）

| 端点 | 作用 | 证据 |
| --- | --- | --- |
| `GET /api/latest` | 最新 release 的 `{version,notes,url,published,assets{name:{url,size,sha256}}}`；`?lang=zh` 出中文 notes | `site/worker.js:5,80-92` |
| `GET /api/latest?channel=stable` | Stable 通道版本（含 `channel`,`urgent`），未知时 503 | `site/worker.js:6-9,84-88` |
| `GET /api/notes?after=&upto=&limit=` | 自 `after` 到 `upto` 的逐版 notes，新到旧；超 100 条从 KV 账本续读 | `site/worker.js:10-14,110-128` |
| `GET /download[/mac-arm64|mac-intel|windows|windows-arm64|linux|linux-arm64|<file>]` | 302 到 GitHub release 资产（`SHORT` 映射，REPO=`yetone/magpie-releases`） | `site/worker.js:30-34,53-63,131-138` |
| 数据来源/降级 | 优先 GitHub Release API（`releases/latest`、`releases/tags/v…`、`SHA256SUMS`），API 限流时退回 releases feed(Atom) 与 KV 账本里上一份完整答案 | `site/worker.js:427-560,778-807` |
| KV 账本 | `wrangler.jsonc` 的 KV `LEDGER` + cron `*/15` 定时 `keepLedger`，用于 Stable 选版与历史 notes | `site/wrangler.jsonc`、`site/worker.js:180-185,681-778` |

---

## 三、用户在正常使用中能获得的信息

> 说明：GUI 的所有数据来自两套接口——页面自身的 `/api/*`（`internal/gui/api.go` 组装，`Handler`= `internal/gui/api.go:968`）与网关的 `/v1/magpie/*` 自省接口（`internal/gateway/gateway.go:931-969`）。下表按“视图 → 能看到什么 → 对应接口 → 证据”。

### 3.1 视图与页面（`index.html` 导航）

| 视图 | 能看到的信息类内容 | 主要接口 | 证据 |
| --- | --- | --- | --- |
| **Agents** | 每个 Agent 的字段/模型、Profiles、每 Agent 的**额度面板**（`panelQuota`）、**路由面板**（`panelRouting`）、**用量面板**（`panelUsage`）；CLI 版本与最新版、drift、缺失 CLI、WSL 探测 | `GET /api/state`、`/api/agent-models/{id}`、`/api/agents/cli`、`/api/agents/install`、`/api/agents/preview/{id}`、`/api/drift` | `index.html:67-70,114-125`；`internal/gui/api.go:1059-1131`；`internal/gui/agent_models.go`、`internal/agent/cliupdate.go:910` |
| **Sessions** | 会话列表/统计/概览、进度、单会话、transcript、导出 markdown；会话的 token/成本/进度 | `GET /api/sessions`、`/stats`、`/overview`、`/progress`、`/one`、`/transcript`、`/markdown`、`/api/sessions/reads`、`/api/sessions/search` | `index.html:70`；`internal/gui/sessions.go:114-388`；`internal/gui/sessions_manage.go`、`sessions_reads.go` |
| **Library** | 给 Agent 的 MCP server / skills / instructions 清单；Library 市场、RTK、MCP 登录 | `GET /api/library`、`/library/skill`、`/library/market/servers`、`/library/market/skills`、`/library/rtk`、`/api/library/browser` | `index.html:71`；`internal/gui/library.go:146-189`、`browser.go` |
| **Providers** | 供应商/密钥/订阅账号列表、**余额/额度**、模型、账号上限/窗口上限、代理、账号名（可“Hide accounts”脱敏截图）、Turned off 分组、发现待导入配置 | `GET /api/providers`、`/api/lanes`、`/api/upstream`、`/api/login/usage`、`/api/claude/mode`、`/api/icons/{name}` | `index.html:73,127-144`；`internal/gui/providers.go:1198`（结构含 `balanceURL/balancePath/balanceToken` 等 170-180、680） |
| **Routing** | **决策轨迹/路由轨迹**、请求回放、账号/密钥窗口、被持有(held)的请求、历史；**Context 卡**（prompt 由什么构成、缓存命中、窗口占用网格、分部分内容） | `GET /api/gateway/route`、`/api/gateway/trace`、`/api/gateway/history`、`/api/gateway/held/{name}`；`/api/context`、`/api/context/reply`、`/api/context/text` | `index.html:74`；`internal/gui/trace.go:20-34,350-428`；`internal/gui/context.go:1-55`；`internal/gui/assets/context.js:1-18` |
| **Gateway** | 近期调用（trace/history）、单次调用详情、caller keys、gateway keys、并发与限流、配额/用量摘要、Omarchy 主题 | `GET /api/gateway/trace`、`/api/gateway/call`、`/api/usage/requests`、`/api/caller-keys`、`/api/plugins`（gateway 插件）；`/v1/magpie/{quotas,usage,concurrency,limit}` | `index.html:75`；`internal/gui/trace.go`、`providers.go`（`/api/gateway/call` 1220 区）、`caller_keys.go`、`internal/gui/assets/plugins/gateway/` |
| **Usage** | 用量总览（calls/errors/input/output/cache/reasoning/cost/credits/TTFT 等）、**Allowances**（额度）、Sessions、Requests 明细（可导出 CSV）、**Context** 标签（各 Agent 窗口填充评分/标签/逐请求） | `GET /api/usage`、`/api/usage/requests`、`/api/usage/heatmap`、`/api/usage/requests.csv`、`/api/usage/quotas`、`/api/usage/quotas/history`、`/api/usage/quotas/history/holds`；`/api/context` | `index.html:77`；`internal/gui/usage.go:101-403`；`internal/usageapi/usageapi.go:57-64`；`internal/usage/usage.go:522-…`（`Totals`）；`internal/gui/assets/plugins/usage/page.js:169-183`（tabs: usage/sessions/requests/context） |
| **Masking** | 脱敏记录汇总（按 kind）、记录分页（可按 kind/by/agent/provider/blocked 过滤）、单条记录、占位符原值（重启后 404）、发给厂商的脱敏 body、预览、清空 | `GET /api/masking/summary`、`/records`、`/record`、`/value`、`/sent`；`POST /api/masking/preview`、`/clear`、`/values` | `index.html:78`；`internal/gui/masking.go:59-124` |
| **Plugins** | 插件列表/更新（`/api/plugins/updates`）、市场、内置插件开关（`/api/plugins/builtin`）、插件给的 Agent、插件自页/面板 | `GET /api/plugins`、`/api/plugins/updates`、`/api/plugins/listings`、`/api/plugins/builtin`、`/api/plugins/agents`、`/api/plugin-pages`、`/api/plugin-panels` | `index.html:80,152`；`internal/gui/plugins.go`、`builtins.go`、`pluginpage.go` |
| **Settings** | 9 个标签：General、Allowances、Network and sharing、Models、Privacy、Observability、Sync and backup、Plugins、About；可见版本、数据目录、网关地址、内置插件状态、PublicURL、AllowIPs、WSL、日志路径、快捷键、JS routings、汇率(FX) | `GET /api/settings`（`settingsJSON`）、`/api/state` | `index.html:154-166`；`internal/gui/api.go:378-414`、`api.go:1367` |
| **通知 / toast** | `status()/statusErr()` 提示条（成功/警告/错误，含 agent 的 `notice`）；启动横幅（`notice`、`cliBehind`、`configBlocked`、`recovered`、`gatewayOther`）；第三方许可证文本 | — | `internal/gui/assets/app.js:396-397,2026,2051,18062-18065`；`internal/gui/api.go:288-323`（stateJSON 的 notice/cliBehind/configBlocked/recovered/gatewayOther）、`api.go:126-128`（`/api/notices`） |

### 3.2 `/v1/magpie/*` 自省接口（网关侧，用户/客户端/Agent 可见）

| 接口 | 能看到什么 | 证据 |
| --- | --- | --- |
| `GET /v1/magpie/quotas` | 每个订阅/套餐/密钥**剩余额度**（含账号与余额），可由 Agent 用来选路；受 budget 约束的 key 被拒（`refuseHeld`/`scopeQuotas`） | `internal/gateway/gateway.go:931,1061-1069`；`quotas_scope.go:29` |
| `GET /v1/magpie/usage` | 用量摘要（与 `/api/usage` 同源的 `usageapi.Usage`） | `internal/gateway/gateway.go:932`；`internal/gateway/usage_api.go:14-38` |
| `GET /v1/magpie/usage/requests` | 请求明细（受 budget/账号约束的 key 只看自己的） | `internal/gateway/gateway.go:933`；`usage_api.go:56` |
| `POST /v1/magpie/usage/requests` | 客户端上报自己的厂商调用（#1874） | `internal/gateway/gateway.go:934`；`usage_ingest.go:22-36` |
| `GET /v1/magpie/quotas/history` | 额度历史 | `internal/gateway/gateway.go:935` |
| `GET /v1/magpie/route` | 某会话这一轮去了哪里（供 Agent UI 显示） | `internal/gateway/gateway.go:938`；`session_route.go:3-97` |
| `GET /v1/magpie/trace` | 路由轨迹（route trace） | `internal/gateway/gateway.go:939` |
| `GET /v1/magpie/route-events`、`/route-events/snapshot` | 路由事件流/快照 | `internal/gateway/gateway.go:940-941`；`route_events.go:33-34` |
| `GET /v1/magpie/concurrency` | 在飞请求/并发 | `internal/gateway/gateway.go:942`；`active.go:12`、`concurrency.go` |
| `GET /v1/magpie/limit` | 调用方密钥自身的限额 | `internal/gateway/gateway.go:943`；`key_limit.go:114` |
| `GET /v1/magpie/me` | key 页数据（key 自身的页面） | `internal/gateway/key_page.go:23,953-954` |
| `GET/POST/DELETE /v1/magpie/keys` | 短时密钥签发/列出/吊销 | `internal/gateway/gateway.go:947-949`；`issue_keys.go:31` |
| `GET /v1/magpie/cpu`、`/heap`、`/goroutines` | CPU/heap profile、全部 goroutine 栈（仅回环） | `internal/gateway/gateway.go:958-965`；`cpu.go:27`、`heap.go:24`、`goroutines.go:32` |
| `POST /v1/magpie/restart` | 请求重启到 `magpie update` 已放入的版本（仅回环、仅程序） | `internal/gateway/restart.go:21-83` |
| `GET /v1/magpie/omarchy` | Omarchy 主题 | `internal/gateway/gateway.go:968-969` |
| `GET /`、`/v1`、`/api/hello` | 服务信息：`name/version/models/apis`（本机还含 `pid/serves`） | `internal/gateway/gateway.go:908-921,1050-1059` |

### 3.3 关键“指标/数据”含义（用户实际看到的数字）

- **用量 `Totals`**：`calls`、`errors`、`retried`、`loops`、`input`、`output`、`cache_read`、`cache_write`、`cache_write_1h`、`cache_unknown(+calls)`、`reasoning`、`cost`(USD)、`unpriced`、`credits/credited`、`ttft_ms`、`decode_ms/decode_out/decode_n` 等（`internal/usage/usage.go:522-…`）。
- **用量分组**：`Usage{Summary; Agents, Models, Accounts, CallerKeys, Path}`，Accounts 是“哪个订阅账号答的”（#557）（`internal/usageapi/usageapi.go:49-132`）。
- **路由轨迹**：`traceJSON`/`routeJSON` 携带 `gateway.Route`（含 session、cost、priced、unpriced 等）（`internal/gui/trace.go:20-34`）。
- **Context 卡**：按 `PARTS = system/tools/skills/memory/files/results/chat` 拆解 prompt 占用、缓存命中、400 格窗口网格（`internal/gui/assets/context.js:7-24`）。
- **脱敏记录**：`maskedRecord`（含每个值是否仍可还原 `known`）（`internal/gui/masking.go:21-57`）；汇总 `maskrec.Sum(days 1..30)`（`masking.go:113-116`）。
- **配额卡片**：`/v1/magpie/quotas` 及 `provider.RemoteCardsPath`/`RemoteRefreshPath`（远程账号卡片与刷新，`internal/gateway/gateway.go:936-937`）。
- **只读 key 可读范围**：明确列出 `/v1/magpie/quotas`、`/quotas/history`、`/usage`、`/usage/requests`、`/limit`、`/concurrency`、`/route-events` 可读、其余（模型/MCP/写）不可（`internal/gateway/readonly.go:30-71`）。

---

## 四、证据（文件:行号 汇总）

**后端/前端/桌面/官网**
- `go.mod:1`（module）、`:3`（go 1.26.8）、`:8-9`（bubbletea/lipgloss）、`:20`（wails v3.0.0-beta.24）、`:28`（modernc sqlite v1.44.3）
- `main.go:14-31`（version/embed/cli.Main）
- `internal/cli/main.go:448-560`（角色标记与命令分发）、`:219`（`update.GUI = hasGUI`）
- `internal/cli/web_cli.go:18-103`（magpie web）、`:203-247`（magpie serve）
- `internal/gui/app.go:1-55,553-554`；`internal/gui/mainwindow.go:1-31`；`internal/gui/gui_on.go:13`
- `internal/gui/api.go:153-154`（embed assets）、`:968-978`（Handler/static）、`:1059-1131`（state/agents）、`:284-324`（stateJSON）、`:378-414`（settingsJSON）、`:126-128`（notices）
- `internal/gui/web.go:51-145`（StartWeb/webReexec/Wait）
- `internal/tui/tui.go:1-4`、`internal/tui/pages.go:41`
- `internal/sqlitedb/sqlitedb.go:1-45`；`internal/agent/aionui.go:28`、`cindy.go:24`、`newmax.go:58`；`internal/provider/importapps.go:29,976`；`internal/provider/kiro.go:430`；`internal/sessions/codex_catalog.go:128`、`codex_provider.go:406`、`codex_resume.go:37`、`cursor.go:219`；`internal/usage/usage.go:277`；`internal/settings/settings.go:1475`；`internal/appdir/appdir.go:262`
- `internal/gui/assets/index.html:22-38,67-80,114-166,539-568`；`app.js:1,396-397`；`i18n.js:1-12`；`kernel.js:1-18`；`pluginpages.js:1-21`；`context.js:1-24`
- `site/worker.js:1-44,50-63,73-185,603-660,681-778`；`site/wrangler.jsonc`；`site/package.json`（不存在）

**升级**
- `internal/update/update.go:41-51,53-66,68-78,86-119,203-238,249-323,329-373,375-430,531-619,700-757,880-976,992-1030,1047-1089,1135-1247`
- `internal/update/notes.go:110-188`；`internal/update/blocked.go:26-218`；`internal/update/packaged.go:78`；`internal/update/admin.go:23-43`；`internal/update/install.go:67-150`（Settle/settle）
- `internal/gui/update.go:21-140,249-369,384-563,665-781,790-869`；`internal/gui/update_wait.go:42-205`；`internal/gui/update_beside.go:1-48`；`internal/gui/whatsnew.go:1-70,429-458`（whatsnew 路由：`/api/whatsnew`、`/seen`、`/today`）
- `internal/cli/update_cli.go:31-314`；`internal/agent/cliupdate.go:1-60,255-360,910,1026`；`internal/plugin/updates.go:1-80`；`internal/plugin/bunupdate.go:1-70`
- `internal/gateway/restart.go:21-83`；`internal/gui/web.go:60-62,111-145`
- `internal/settings/settings.go:436-468,1071-1073,1204-1207,2307-2308`

**用户可见信息**
- `internal/gui/api.go:1059`（state）、`api.go:1367`（settings）、`api.go:126`（notices）
- `internal/gui/providers.go:1198`（providers）、`:170-180,680`（balance 等字段）
- `internal/gui/usage.go:101,179,187,210,258,377,387`（usage 路由）；`internal/usageapi/usageapi.go:49-132`；`internal/usage/usage.go:522-…`（Totals）
- `internal/gui/trace.go:20-34,350-428`；`internal/gui/context.go:1-55`；`internal/gui/masking.go:21-124`
- `internal/gui/sessions.go:114-388`；`internal/gui/library.go:146-189`
- `internal/gateway/gateway.go:908-1025,1050-1069`；`internal/gateway/readonly.go:30-71`；`internal/gateway/usage_api.go:14-56`；`internal/gateway/session_route.go:3-97`；`internal/gateway/route_events.go:33-34`；`internal/gateway/key_page.go:23`；`internal/gateway/issue_keys.go:31`
- `internal/gui/assets/plugins/usage/page.js:169-183`（Usage 标签：usage/sessions/requests/context）

---

## 五、未确认项

1. **Usage 页“Allowances”标签的确切 UI 文案**：代码里 `setTab-usage` 文案为 “Allowances”（Settings 里，`index.html:157`），而 Usage 页的 tab 由 `plugins/usage/page.js` 控制（`usage`/`sessions`/`requests`/`context`）；“Allowances”与 Usage 页“用量+Allowances”是否为同一入口的两种叫法，未逐字节确认 UI 呈现。
2. **`magpie serve`/守护服务的“自更新后如何重启”**：`RestartPath` 在无 `restarter` 时返回 `unsupported`（`restart.go:76-80`），CLI 建议“按启动它的方式重启”（`update_cli.go:302-305`）；但 systemd/launchd 的具体重启由外部（服务管理器/用户）触发的完整链路未逐行核实。
3. **Windows 桌面 App 的更新件类型**：`BinaryAsset` 对 GUI 非 darwin 用 `magpie-<os>-<arch>.exe`（`update.go:204-213`），但 Windows 上是否曾以“安装器/DB 包”形式发布（而非裸 exe 自替换）未确认；仅确认代码路径是“裸 exe 自替换 + 提权”。
4. **macOS 签名校验的失败分支细节**（`codesign -dv` 的 TeamIdentifier 解析）已读，但对“无签名/自编译构建”的实际升级行为未做运行验证。
5. **官网 `stableOf` 的“本周一开始前已出三天”**：算法读自 `site/worker.js:603-660`，其具体“三天”常量名称/边界（UTC 与本地时区）未逐行核对每个常量。
6. **`/v1/magpie/route-events` 的事件字段全貌**：仅确认端点与常量的存在（`route_events.go:33-34`、`gateway.go:940-941`），未展开每个 JSON 字段。
7. **各视图 JSON 的完整字段集**：第三节按结构体名与关键字段归纳（如 `agentJSON` 的 `Models/Menu/Source/Stale/AppOut/Failover` 等），未逐字段验证前端实际渲染取舍。
8. **`Context` 标签的评分/标签算法**：`context.go` 头部说明“score + tags 由窗口/缓存算出”，具体公式未展开阅读（`internal/gui/context.go:1-55` 之后）。
9. **`site/worker.js` 的 KV 账本结构**（`ledger` 的 version→notes/published/urgent 持久化格式）仅据函数注释与 `keepLedger` 推断，未读完整 schema 常量。
