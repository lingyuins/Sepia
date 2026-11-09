# magpie 插件系统调查

> 调查对象：`D:\CodeWorkspace\magpie`（只读源码，未做任何修改）
> 调查方式：先读权威设计文档（`docs/subsystems/`），再逐文件读代码；忽略 `.git`、`node_modules`
> 结论均标注文件路径（尽量带行号）；拿不准处写"未确认"
> 交付时间：2026-11-03

---

## 1. 概览与定位

magpie 的"插件系统"不是一套，而是**两套并行的系统**，职责和运行位置完全不同：

### 1.1 系统 A：内置插件（built-in plugins，Go 包）

- 定位："magpie 里不是人人都需要的功能"，每个是 `plugins/` 下的一个 Go 包，用户可整体关闭（`docs/subsystems/plugin-runtime.md:3-7`）。
- 运行时：`internal/pluginrt/`（4 个文件：`pluginrt.go` 注册表与生命周期、`config.go` plugins.yml 与分层合成、`schema.go` 设置 JSON Schema）。注册表用 `pluginrt.Register(Manifest)`，由各插件包 `init()` 调用；`plugins/all` 导入全部 14 个内置插件，且**只有 `main.go` 导入 `plugins/all`**（`plugins/all/all.go:1-3`、`internal/pluginrt/pluginrt.go:128-152`）。
- 开关：`plugins.yml`（预设 `all`/`essentials` + 每个插件一行 `{id, enabled, config}`），窗口 Plugins › Built-in 页与 `magpie plugin builtin` CLI 操作（`internal/pluginrt/config.go:24-58`、`internal/gui/builtins.go:110-157`）。
- 边界约束（有测试守着）：核心代码不导入任何插件、插件之间互不导入，只通过 service 通信（`docs/subsystems/plugin-runtime.md:33-36`、`internal/pluginrt/pluginrt.go:7-10`）。

### 1.2 系统 B：社区/OpenCode 插件（npm 包，Bun 宿主）

- 定位：把"厂商可能反对在官方 App 之外使用"的订阅做成插件——插件负责登录和发请求，与 OpenCode 的插件机制相同（opencode-gemini-auth 之类），magpie 直接运行同一批 OpenCode v1 API（`@opencode-ai/plugin`）的 npm 包（`internal/plugin/store.go:1-17`）。
- 宿主：一个 **Bun 子进程**跑 `internal/plugin/host.js`（约 3177 行），Go 侧 `internal/plugin/host.go`（2057 行）通过 stdin/stdout 的 JSON 行协议通信（`internal/plugin/host.go:35-36` 嵌入 host.js；协议见 `host.js:1-26`）。
- 清单：`plugins.json`（`{plugins:[{spec, off, options}], config, prefer}`）+ 安装目录 `<settings>/plugins/`（npm 包在 `node_modules/`）+ 登录态 `plugin-auth.json`（OpenCode auth.json 形状）（`internal/plugin/store.go:47-72`、`store.go:121-157`）。
- 一个插件可以是多种类型：provider（auth hook）、middleware（网关 JS）、agent、page/panel、tool（MCP）、gateway hooks（异步请求钩子）、pi 包。

### 1.3 文档权威性

`docs/subsystems/plugin-runtime.md` 是内置插件+窗口内插件+网关扩展点的权威文档（P0–P6 演进路线都在里面）；`provider-plugins.md` 权威说明 provider 插件；`plugin-tools.md` 权威说明插件工具；`gateway-middleware.md`、`gateway-request-hooks.md` 权威说明两类网关 JS。本次调查的代码结论与这些文档一致。

---

## 2. 插件类型矩阵

| 类型 | 用途 | 声明/注册点 | 运行位置 | 关键源码 |
|---|---|---|---|---|
| **provider 插件** | 登录某订阅、列出模型、用插件自己的 fetch 发请求（替代内置订阅实现） | OpenCode `auth` hook（package.json `main`/`exports` 或 index 文件） | **Bun 插件宿主**（host.js 进程）；请求经 `plugin.Fetch` 流式回传 | `internal/plugin/host.go:1831`（Fetch）、`internal/provider/plugins.go:24-32`（`plugin://` base）、`docs/subsystems/provider-plugins.md:31` |
| **agent 插件** | 给 magpie 的 Agents 页加一个外部编码 agent（如 aider） | package.json `magpie.agent` 或 `*.agent.js/.mjs` 文件 | **magpie 自身进程**（`internal/agentplug`，经 `internal/jsmod` 编译，与中间件共用模块编译） | `internal/plugin/agent.go:14-41`、`docs/subsystems/gateway-middleware.md:31` |
| **middleware（网关 JS）插件** | 改写在网关上流动的 agent 请求/回复、provider 尝试、账号跳过、流事件 | package.json `magpie.middleware`（别名 `magpie.gateway`）或 `*.middleware.js/.gateway.js` | **网关进程内**，moejs（Go 写的 JS 引擎）编译运行 | `internal/plugin/middleware.go:15-53`、`internal/middleware/middleware.go:1-14`、`docs/subsystems/gateway-middleware.md:3` |
| **gateway hooks（异步请求钩子）** | 路由前注入上下文（onRequest）、观察完成态回复（onResponse），可 await I/O | package.json `magpie.hooks` 或 `*.hooks.{js,mjs,ts,mts}` | **Bun 插件宿主**（与 provider 同一 host） | `internal/plugin/gateway.go:16-46`、`docs/subsystems/gateway-request-hooks.md:112-125` |
| **page/panel 插件** | 在窗口加自己的页面（sidebar Plugins 组）、在 magpie 页面里嵌面板、往行菜单加项 | package.json `magpie.page` / `magpie.panels` / `magpie.actions` / `magpie.permissions` | **窗口内 iframe 沙箱**（`sandbox="allow-scripts"`），文件由 `/_magpie/plugin/<token>/` 提供 | `internal/plugin/page.go:145-233`、`internal/gui/pluginpage.go:42-66`、`internal/gui/assets/pluginpages.js:86` |
| **tool（MCP）插件** | 把自己的 OpenCode `tool` hook 工具作为 MCP server 提供给所有 agent | OpenCode `tool` hook（`{tool:{name: tool({description,args,execute})}}`） | 运行在 **Bun 宿主**；以 `/mcp/magpie/tools` MCP server 暴露 | `internal/plugin/tools.go:22-31`、`internal/gateway/tools_mcp.go:34`、`docs/subsystems/plugin-tools.md:3` |
| **builtin 插件** | magpie 自身可关闭的功能块（页面、网关扩展点消费者等） | `pluginrt.Register(Manifest)`（Go init） | Go 进程内（网关/窗口/CLI 同一运行时） | `internal/pluginrt/pluginrt.go:35-72`、`plugins/all/all.go:5-19` |
| **pi 包/扩展** | pi coding agent（@earendil-works/pi-coding-agent）的包与扩展，作为插件加载 | package.json `pi` 字段 / `pi-package` keyword / 依赖 pi | **Bun 宿主**（`pi.js` 用 pi 自己的 loader 加载，provider 由 pi 的 ModelRuntime 组合） | `internal/plugin/pi.js:1-19`、`internal/plugin/store.go:885-934` |

**网关五个扩展点**（P3/P4，由内置插件 serve，社区插件经 `plugins/middleware` 或 tool 钩子接入）：

| 扩展点 | 接口包 | 失败策略 | 内置消费者 |
|---|---|---|---|
| observer（用量观察） | `internal/usage`（`usage.Observer`/`Observe`） | 只读，不影响请求 | `plugins/otel` |
| router strategy（路由策略） | `internal/routes`（`routes.Strategy`/`Register`，`Order(pool,seats)`） | fail open（非排列→原顺序） | `plugins/rotation`（9 个 Builtin 策略）、`plugins/groups`（ServeGroups 门控）、JS 插件的 `strategies` |
| middleware（请求/回复改写） | `internal/gwmw`（`gwmw.Agent`：`Begin/Request/Wrap/End`，`Tried`/`Accounts`） | fail open | `plugins/middleware`（`middleware.Chain`） |
| masker（脱敏） | `internal/gwmw`（`gwmw.Masker`，`Mask`） | **fail closed**（要脱敏而无 masker → 503，什么都不发） | `plugins/redact` |
| tool（magpie 自答工具） | `internal/gwtool`（`Serve(name)`，`Gate`/`Served`） | off 即 off | `plugins/search`（`WebSearch`）、`plugins/tools`（`PluginTools`） |
| budget（密钥限额） | `internal/gwbudget`（`gwbudget.Limiter`，`Reserve`） | **fail closed**（`ErrUnlimited` → 503） | `plugins/budget` |

依据：`docs/subsystems/plugin-runtime.md:172-260`、`internal/gwtool/gwtool.go:1-13`、`internal/gwbudget/gwbudget.go:1-10`、`internal/gwmw/gwmw.go:1-17`、`internal/routes/routes.go:1-12`。

---

## 3. 包规范（package.json 的 `magpie.*` 字段）

插件目录 = 一个 npm 包（安装在 `<settings>/plugins/node_modules/<name>`）或一个本地文件夹/文件。`internal/plugin/store.go:324-341` 用 `IsPath`/`IsGit`/`pkgName` 正则区分 spec 形态。

### 3.1 OpenCode 侧（宿主加载）

| 字段 | 含义 | 源码 |
|---|---|---|
| `main` / `exports` | OpenCode 插件入口；两者都无则找 `index.ts/tsx/js/mjs/cjs`（`INDEX_FILES`） | `store.go:498-500`、`host.js:1209`、`host.js:1315-1321` |
| `bin`（仅 bin） | 不是插件：安装成功但不加载（多半是 MCP server，应装到 Library） | `store.go:507-551`（`notPlugin`） |

### 3.2 magpie 侧（`magpie.*`）

| 字段 | 形状 | 约束/行为 | 源码 |
|---|---|---|---|
| `magpie.middleware` / `magpie.gateway` | 网关模块文件路径 | 与 `magpie.hooks` 命名空间分开，同一文件不会同时当两种加载 | `middleware.go:33-52`、`gateway-request-hooks.md:26-28` |
| `magpie.hooks` | 异步钩子文件（.js/.mjs/.ts/.mts） | Bun 直接跑 TS；gateway-only 包无需 main/exports | `gateway.go:21-46`、`gateway-request-hooks.md:29-34` |
| `magpie.agent` | agent 文件 | 无 main/exports 时只跑在 magpie 进程内 | `agent.go:26-41` |
| `magpie.page` | `{title, titles, file, icon, replaces, service:{port,option}, keepAlive}` | file 必须是包内 `.html`；icon 在页面文件夹内；`replaces` ∈ `{sessions,library,routing,usage}`；`service` 仅当包同时有宿主部分 | `page.go:32-54`、`page.go:180-200`、`page.go:108` |
| `magpie.panels` | `[{slot,file,title,titles}]` | slot ∈ `{agents,usage,settings,provider}`；每插件最多 8 个 | `page.go:64-72`、`page.go:100-104`、`page.go:94` |
| `magpie.actions` | `[{id,menu,title,titles}]` | menu ∈ `{provider,agent}`；id 匹配 `^[a-z0-9][a-z0-9-]{0,39}$`；无 page 则无 action；最多 8 个 | `page.go:77-94`、`page.go:214-223` |
| `magpie.permissions` | `["usage:summary","providers:list"]` | 页面/面板可读白名单，均不含秘密 | `page.go:96-98`、`page.go:227-231` |
| `magpie.options` | 任意 JSON 对象 | 只是 Options 编辑器的示例起点；真正的值是 plugins.json 的 `options`（middleware 的 `ctx.options`） | `middleware.go:58-74`、`gateway-middleware.md:15` |
| `magpie.icon` | https URL 或 `data:image/` URI，≤192KiB | 市场/行内图标 | `market.go:81-93` |
| `magpie.name` | 字符串 | 插件自报名（OwnName） | `internal/gui/plugins.go:109-110` |
| `pi` / `pi-package` keyword / 依赖 `@earendil-works/pi-coding-agent` | — | 识别为 pi 包，走 `pi.js` 加载 | `store.go:891-934`、`pi.js:42-57` |

插件短名规则：去 scope，再去掉 `middleware-`/`opencode-`/`tools-`/`page-`/`agent-` 前缀与 `-auth` 后缀（`store.go:709-733`）。该短名决定 MCP 工具名前缀、菜单寻址、市场短名解析（`resolve.go:25-33`）。

**目录结构示例**（来自 testdata 与市场）：
- provider：`package.json{main:"index.js"}` + index.js（OpenCode 插件函数）
- middleware：`@magpie-community/middleware-param-override`，`magpie.middleware` 指向网关模块（`internal/plugin/market.json:28-37`）
- page：`magpie-usage-glance`：`{magpie:{page:{title,file:"page/index.html",icon},permissions:["usage:summary"]}}`（`internal/plugin/testdata/github/raw/someone/page-glance/main/package.json:5-8`）
- agent：`@magpie-community/agent-aider`：`{magpie:{agent:"./aider.agent.js"}}`（`.../agent-aider/main/package.json:10`）

---

## 4. 安装与市场

### 4.1 市场（market）

- 内置副本：`internal/plugin/market.json`（`//go:embed`，`market.go:33-40`），远端源为社区仓库 `registry.json`：`https://raw.githubusercontent.com/magpie-community/plugins/main/registry.json`；`MAGPIE_PLUGIN_MARKET` 可改地址，值 `"off"` 则只用内置副本（`market.go:42-44`）。
- Listing 字段：`package/name/icon/providers/community/replaces/kind`（kind：`middleware`/`agent`/`tools`/`page`，缺省为 provider）+ 多语言 `summary`（`market.go:51-60`）。非 community 的条目只对搜索可见、不给短名解析（`market.go:46-50`）。
- npm 信息：每个包问 registry.npmjs.org（版本/描述/发布者/周下载量），磁盘缓存 `plugin-npm.json`（`market.go:66-79`、`market.go:104-153`）。
- GitHub 话题发现：topic `magpie-plugin` 的仓库也会出现在 Plugins 页（未经任何人审查，单独分区）（`github.go:18-25`）。
- registry 选择顺序：magpie 设置 → 环境变量/登录 shell（`BUN_CONFIG_REGISTRY` 等）→ `bunfig.toml`/`.npmrc` → 「国内镜像」npmmirror → npm 官方（`registry.go:50-103`）。

### 4.2 安装

- `Add`→`resolve` 短名→`add`：npm 包 `bun add --ignore-scripts`（脚本不跑，同 OpenCode）；git spec（`github:owner/repo`、`git+https://`、`owner/repo` 简写等）经 bun 安装；本地路径直接作为 target（`store.go:399-496`、`store.go:737-803`、`store.go:324-341`）。
- `npm:<name>` 强制取 npm 同名包（`resolve.go:16-51`）。
- `@magpie-community/*` 包或 npm 没有的版本，回落到社区仓库 **dist 分支**（tgz + `integrity` sha512 校验，只装索引指定的哈希）（`dist.go:26-38`、`dist.go:82-84`）。
- 装前 `notPlugin` 检查（无 main/exports/index 且非 pi/middleware/agent/page 的包装完也会被卸掉）；pi 包自动补装 pi 本体（`store.go:507-551`、`store.go:936-950`）。
- Bun 首次需要时下载：版本下限 `BunVersion = "1.4.2"`，按平台选构建（musl/baseline/Windows ARM），带 SHA-256 校验，`MAGPIE_BUN` 可指定自己的 bun；跑不起来的版本进 `state.json` 的 `Bad` 永不重试，前一个 Bun 留作回退（`bun.go:32-59`、`bunupdate.go:1-11`、`bunupdate.go:31-104`）。Bun 自动更新：每 6 小时问 GitHub，发布满 48 小时才采用（`bunupdate.go:3-10`、`bunupdate.go:32`）。

### 4.3 更新与移除

- `Update`：npm 包取最新版（含被 pin 的也升级并解除 pin），git 包重新拉取（`bun update` + 刷新 clone refs）；git 插件的新旧对比靠 `node_modules/<pkg>/.bun-tag`/bun.lock 记录的 commit 与 GitHub API/ls-remote（`store.go:553-594`、`gitrev.go:1-11`）。
- `Remove`/`SetOff`/`Prefer`/`SetOptions`：改 plugins.json 并 `Restart()` 宿主；移除会卸掉 npm 包并清掉它的 prefer 选择（`store.go:607-701`、`store.go:728-742`）。
- 两个插件签同一 provider id 时：`prefer` 映射决定谁serve，否则最后一个（同 OpenCode）；UI 显示 Clash 行与 "Use for …" 按钮（`store.go:650-675`、`internal/plugin/host.go:544-584`、`docs/subsystems/provider-plugins.md:165-181`）。

---

## 5. 生命周期与状态

### 5.1 内置插件（plugins.yml）

- 文件：`appdir.Config()/plugins.yml`，字段 `preset`（`essentials`|`all`）、`settled`、`plugins:[{id,enabled,config}]`（只存用户改过的）（`config.go:24-58`）。新装一律 `all`（`Settle`，`config.go:192-214`）。
- 合成四层（`Compose`，后者覆盖前者）：`default`（全开+Defaults）→ `preset`（essentials 关掉非 Essential）→ `user`（用户行，config 按 schema 校验，不允许的值丢弃保留默认）→ `in use`（`InUse>0` 强制开启，读不到也算在用）（`config.go:126-175`、`config.go:104-123`）。
- 状态机：`off/pending/active/failed/disposed`（`pluginrt.go:83-89`）。`Reconcile`：先按依赖倒序 disposing（要关的 + 依赖了被关者提供服务的），再反复 apply 到没有新插件能激活为止；API 不兼容（`^1` 主版本相同、次版本不新于宿主）或 Apply 出错/panic → `failed`，不影响其他插件（`pluginrt.go:206-330`、`pluginrt.go:391-402`）。
- Ctx：`OnDispose`/`Contribute`/`Provide`/`Service`，dispose 逆序执行，单个 panic 只记日志（`pluginrt.go:93-119`、`pluginrt.go:332-354`）。
- 设置 schema：`Manifest.Settings` 结构体经 `SchemaOf` 生成 JSON Schema（`x-order`、`title`、`description`、`enum`、`min/max`；只支持 bool/int/float64/string），`SetConfig` 只收 schema 允许的值（`schema.go:26-105`、`schema.go:178-192`、`config.go:234-261`）。
- 写文件：进程内 mutex + `plugins.yml.lock` 文件锁、整体重写（临时文件+rename）、留 `.bak`（`config.go:285-323`）。
- 刷新：`pluginrt.Refresh`（每次读）/`KeepFresh`（网关进程每 2s 及文件变化时刷新，监视 plugins.yml、settings.json、providers.json、plugins.json、plugin-tools.json）（`config.go:334-395`、`internal/gateway/gateway.go:784`）。
- 窗口侧：`GET/POST /api/plugins/builtin`（开关/设置；在用中的插件拒绝关闭）；boot.js 拿 `builtinsOff`/`builtinConfig`（`internal/gui/builtins.go:110-157`、`internal/gui/builtins.go:82-104`）。

### 5.2 社区插件（plugins.json）

- 安装/更新/移除/开关/options/prefer 都写 plugins.json 并重启宿主；旧宿主**退休**（retire）：空闲即杀，忙则等其调用结束（最多 10 分钟）再杀（`store.go:728-776`）。
- 别的 magpie 改了插件（终端里 `magpie plugin add` 等）：`checkList` 发现 stamp 变化就丢弃 provider 缓存、标记 `hostStale`，下次 get 时换宿主（`store.go:295-317`、`host.go:883-901`）。
- 登录态 `plugin-auth.json`：OpenCode auth.json 形状（可在两者间搬运），读写有 `plugin-auth.json.lock`（10 秒过期抢占），写坏留 lastgood（`store.go:124-152`、`host.js:963-1008`）。

---

## 6. 运行时细节

### 6.1 Bun 宿主（provider/pi/gateway hooks）

**启动**（`host.go:903-1085`）：`bun run host.js`（cwd 为 settings 目录）；`init` RPC 带 `authPath/modelsDevPath/piPath/directory/config/prefer/options/plugins/loadMillis/listMillis`；每个插件 20s 加载预算（`pluginLoadWait`），models hook 20s（`pluginListWait`），整体 init 2 分钟；models.dev 目录缺失时后台同步 30s（`host.go:954-1084`、`host.go:1087-1095`）。

**协议**（`host.js:1-26`）：父→子 `{id,method,params}`，子→父 `{id,result|error}`；fetch 流式：先 `{event:"head",status,headers}`，再多个 `{event:"chunk",data}`（base64），最后 `{id,result:null}`；`{method:"abort"}` 取消、`{method:"credit",{id,n}}` 归还字节。未请求事件：`log/toast/auth/recovered/renewing/signIn/load/usage`（`host.go:1199-1249`）。

**背压与限额**：每流 window 512KiB、全局 64MiB、帧 64KiB（+2048 结构开销）、单行 32MiB；head 上限 64KiB/256 项；调用总数 `callLimit`；读 Body 空闲 30 分钟放弃（`host.go:165-212`、`host.go:1881-1895`）。

**加载**（`host.js:1271-1357`）：并行 import 各插件（`loadPlugins`），顺序保持 plugins.json 顺序（后者钩子胜出）；PluginInput = `client`（auth.set 写 plugin-auth.json、tui.showToast、app.log、config.get）+ `project/directory` + `Bun.$` + magpie 扩展 `option/options/load/usage/account`（`host.js:1272-1284`）；每个插件导出多个函数都注册（`servers()`）；config hook 串行、各 20s。

**fetch 路径**（`host.js:2800-2873`）：sdkHeaders（AI SDK 包头）+ loader options 的头 + magpie 给的头 + 各插件 `chat.headers` hook 的头；`o.fetch`（loader 的）优先于全局 fetch；401 且带 refresh token 时区分"access 过期"与"登录失效"（`staleAccess`）。

**magpie→插件的请求头**（网关在 `forwardOnce` 设置，host.js 的 `unsaid` 再剥掉，防止流向厂商）：`x-magpie-work-dir`、`x-magpie-work-git`、`x-magpie-session`、`x-magpie-subagent`、`x-magpie-fallback`（`internal/provider/plugins.go:38-72`、`host.js:589-613`、`docs/subsystems/provider-plugins.md:127-161`）。

**代理**：宿主启动时记下 `*_PROXY`（改名为 `MAGPIE_*_PROXY`），每个 fetch 按请求的 proxy 选择决定；系统代理变了（≥15s，无代理时 2s）或网关地址变了就换宿主（`host.go:1724-1827`、`host.go:1737-1778`）。

**自报数据**：`input.magpie.usage(...)`（插件自发请求计入 ledger，kind 如 `cache_keepalive`）、`input.magpie.load(...)`（模型繁忙度，5 分钟新鲜期，供 Busy models last）（`internal/usage/plugin_calls.go`、`internal/plugin/load.go:9-115`、`docs/subsystems/provider-plugins.md:81-93`）。

### 6.2 moejs 网关中间件（进程内）

- `internal/middleware`：每个中间件编译一次（`internal/jsmod`，与 agent 插件共用模块编译），每中间件一个 `sync.Pool`，每请求取一个 runtime（请求内钩子共享 `ctx.state`）（`middleware.go:1-14`、`middleware.go:44-74`）。
- 时限：`onRequest/onAccount/onProvider/onResponse` 250ms、`onEvent` 50ms、加载 1s、JS 策略 20ms、`onUsage` 250ms（队列 1024，满则丢弃计数）（`middleware.go:34-40`、`points.go:45-53`、`gateway-middleware.md:46-51`）。
- hooks：`onRequest`（`ctx.reject` 可拒绝）、`onAccount`（`{skip:true}` 跳过账号，ref 为 安装级随机密钥 `account-ref.key` 的派生值）、`onProvider`（每次尝试前，可按厂商改体）、`onEvent`（SSE 事件，按 `export const events` 过滤）、`onResponse`（整包）（`docs/subsystems/gateway-middleware.md:24-29`）。
- 挂载点：`plugins/middleware` 的 Apply 注册 `gwmw.Use(middleware.Chain{})` + `usage.Observe(middleware.Points())`；只对 agent 请求生效（`serveAgent`），magpie 自身请求走 `serve` 不过中间件（`plugins/middleware/middleware.go:28-32`、`gateway-middleware.md:24`、`gateway-middleware.md:33`）。

### 6.3 窗口内核（kernel.js）

- `internal/gui/assets/kernel.js` 在 i18n.js 后、插件脚本与 app.js 前加载，全部 classic script 共享作用域，即 `window.magpie`（`docs/subsystems/plugin-runtime.md:38-42`）。
- API：`magpie.plugin(id, setup)`（ctx：`config`、`onConfig`、`onDispose`、`slots.add`、`ui`）、slot `page`（part `{key,html,loads,mount,…hooks}` 变 `<main id="view-<key>" data-feature="<plugin>">`）、`magpie.start()`/`magpie.call(key,hook,…)`（`loading/draft/render/open/relang`）、`magpie.pages()`、`magpie.strings({zh,…})`、错误边界（hook 抛错→"This page stopped working"+Try again）、ui kit（`el/button/toggle/menu/row/form`）（`docs/subsystems/plugin-runtime.md:44-78`、`internal/gui/assets/kernel.js:130`、`kernel.js:192-218`）。
- 每个内置插件的 `Client` 文件在 `internal/gui/assets/plugins/<id>/`，`TestBuiltinClientsLoaded` 守着 index.html 的加载顺序（`docs/subsystems/plugin-runtime.md:84-95`）。

### 6.4 pluginrt 的职责小结

注册表（Register/Registered）、Manifest 定义、Reconcile 状态机、Ctx（服务提供/消费）、plugins.yml 读写与四层合成、设置 JSON Schema、进程级 Refresh/KeepFresh（`internal/pluginrt/pluginrt.go`、`config.go`、`schema.go` 三个文件；无其他文件）。

---

## 7. 页面与面板沙箱

- 前缀：`/_magpie/plugin/<token>/<页面文件夹内相对路径>`（`internal/gui/pluginpage.go:40-42`）。token = HMAC-SHA256(spec)（面板为 `spec + "\x00panel\x00" + i`）取前 16 字节 hex，密钥是每次运行随机 32 字节（`pluginpage.go:52-66`、`pluginpage.go:186-190`）。
- CSP（`pagePolicy`）：`sandbox allow-scripts`、`default-src 'none'`、script/style/img/font/media 只准自己文件夹（`ownSources` 按 wails:// 或 http(s) 双 scheme 命名）、`connect-src 'none'`、`frame-src 'none'`、`form-action 'none'`、`frame-ancestors 'self'`（`pluginpage.go:223-270`）。另加 `X-Content-Type-Options: nosniff`、`Referrer-Policy: no-referrer`、`Cache-Control: no-cache`（`pluginpage.go:357-364`）。
- iframe：`sandbox="allow-scripts"`、`referrerpolicy="no-referrer"`、`allow=""`；10 秒不说 hello 判坏；离开 10 分钟释放 frame（`keepAlive` 可保活）；来去时通知 `visibility`（`pluginpages.js:76-119`、`docs/subsystems/plugin-runtime.md:274`）。
- 桥：页面引 `<script src="_magpie/bridge.js">`（`pluginBridgePath`，与页面文件同路径提供）；`postMessage` 双向，窗口只回自己的 frame（`ev.source`）；桥 API：`magpie.ready/theme/onTheme/options.get/set/read/context/service/onAction/visible/onVisibility`（`pagebridge.js:1-47`、`pagebridge.js:115 起`、`pluginpages.js:45`）。
- 路由：`GET /api/plugin-pages`、`GET /api/plugin-panels`、`POST /api/plugin-pages/read`、`POST /api/plugin-pages/options`（options 限每插件每秒一次）（`pluginpage.go:414-526`）。
- 读白名单：`usage:summary`（总量+前 20 模型，无密钥/账号/会话）与 `providers:list`（id+name），服务端按包声明再校验一次（`pluginpage.go:528-584`、`page.go:96-98`）。
- API 隔离：`apiFramed` 拒绝 `Origin: null`、`Sec-Fetch-Dest` 为 frame/embed/object、跨站子资源的 `/api/` 请求（桌面窗口 API 无密钥，靠这个挡插件页面）（`pluginpage.go:391-408`、`docs/subsystems/plugin-runtime.md:274`）。
- 文件 containment：`FileIn`/`inside` 用 `proc.RealPath`（跟随 Windows junction）确保不逃出插件文件夹（`page.go:294-348`）。
- 面板：slot `agents/usage/settings/provider`，每插件 ≤8；框架同页面沙箱，共享 reads/options；高度上报 0..2400px；坏了只坏自己（`page.go:64-72`、`docs/subsystems/plugin-runtime.md:285-304`）。
- 替换内置页：`replaces` ∈ `{sessions,library,routing,usage}`，plugins.json 顺序第一个赢；内置页仍在，插件关了回来（`page.go:106-108`、`pluginpage.go:172-175`）。
- 有宿主部分的插件页面可带本地服务（`magpie.page.service {port, option}`），仅 `magpie web` 模式下经 magpie 代服（`servePageService`）；打开页面会 `Warm` 宿主（等 ≤7s）（`page.go:44-62`、`pluginpage.go:272-309`、`pluginpage.go:331-337`）。

---

## 8. 插件工具（MCP）

- 服务端：Streamable HTTP，`/mcp/magpie/tools`（`internal/gateway/tools_mcp.go:34`）；`serveMCP` 守卫（网页请求拒绝、远程要密钥），body 上限 4MiB（`tools_mcp.go:38-51`、`plugin-tools.md:12`）。
- 收集：host.js 的 `toolsOf`（名字匹配 `^[A-Za-z0-9_-]{1,48}$` 且有 execute，重名后者胜）、zod v3/v4 或裸 JSON Schema 转参数 schema（`host.js:1532-1660`、`plugin-tools.md:9`）。
- 命名：`tools/list` 给 `<plugin短名>_<tool>`，短名 ≤24 字符，总名 ≤64，重名加 `_2/_3`（`tools_mcp.go:61-75`、`plugin-tools.md:12`）。
- 调用：`tools/call` 先过调用密钥的预算限额（`reserveFor`，超限/Key limits 插件关 → isError），再 `plugin.RunTool`；`ToolTimeout` 55s（低于 MCP 客户端 60s）；超时的调用槽留给自己的 abort（`tools.go:55-78`、`plugin-tools.md:24-25`、`plugin-tools.md:33`）。
- 门控：`plugins/tools`（Essential）serve `gwtool.PluginTools`；关则列表空、调用答"插件已关"，且**不启动宿主**（`plugins/tools/tools.go:24-28`、`plugin-tools.md:31`、`tools_mcp.go:79-80`）。
- 在用计数：宿主每次加载后写 `plugin-tools.json`（settings 目录，整体写、变了才写）；`ToolsOn` 数"开着且有工具"的插件（`tools.go:80-167`）。
- 每个给工具的插件同时是 Library 里的一个 `http` MCP server（`/mcp/magpie/tools/<short>`），可被 MCP router 接管（`internal/library/plugin_tools.go`、`plugin-tools.md:35-40`）。
- 工具结果只回给调用 agent，不进厂商、不脱敏（`tools_mcp.go:28-31`、`plugin-tools.md:27`）。

---

## 9. 内置插件清单（14 + all）

| id | 名称 | 类型/消费点 | Requires | Essential | InUse | Parts / Client | 源码 |
|---|---|---|---|---|---|---|---|
| `gateway` | Gateway page | 窗口页面（nav.page）；网关本身是核心 | window, gateway | 是 | 网关密钥数（`access.List`） | nav.page `gateway`；Client `keys.js, client.js`；唯一设置 `callsPage`（enum 10/20/50/100，默认 20） | `plugins/gateway/gateway.go:16-33` |
| `routing` | Routing | 窗口页面 | window, gateway | 是 | 用户路由组数（`provider.NamedGroups`） | nav.page `routing`；Client `i18n.js, routing.css, client.js` | `plugins/routing/routing.go:10-21` |
| `usage` | Usage | 窗口页面 | window | 是 | 无（网关照常计数） | nav.page `usage`；Client `i18n.js, client.js, page.js` | `plugins/usage/usage.go:8-15` |
| `sessions` | Sessions | 窗口页面 | window | 否 | 设置里录制作话（`GatewayConversations`） | nav.page `sessions`；Client `i18n.js, sessions.css, client.js` | `plugins/sessions/sessions.go:10-29` |
| `library` | Library | 窗口页面 | window | 否 | 分给 agent 的条目数（`lib.InUse`） | nav.page `library`；Client `i18n.js, library.css, client.js` | `plugins/library/library.go:10-19` |
| `otel` | Observability | observer 点（`usage.Observe(otlp.New())`） | window, settings, gateway | 否 | settings/`MAGPIE_OTEL_*` 开启导出 | settings.section `otel`；Client `i18n.js, page.js` | `plugins/otel/otel.go:16-44` |
| `rotation` | Routing strategies | router strategy 点（注册 `routes.Builtin` 全部） | providers, gateway | 是 | 被这些策略路由的 provider/组数 | part `rotation` | `plugins/rotation/rotation.go:17-32` |
| `groups` | Routing groups | router strategy 点（`provider.ServeGroups` 门控） | window, providers | 否 | `provider.NamedGroups` | part `groups` | `plugins/groups/groups.go:14-29` |
| `middleware` | JS gateway plugins | middleware 点（`gwmw.Use(middleware.Chain)`+`usage.Observe(middleware.Points())`） | gateway | 是 | `middleware.On()`（plugins.json 开着的中间件数） | 无 Client | `plugins/middleware/middleware.go:20-33` |
| `redact` | Privacy masking | masker 点（`gwmw.Mask(masking.Masker)`） | settings, gateway | 是 | `masking.Wanted` | nav.page `masking`；Client `i18n.js, redact.css, client.js, page.js` | `plugins/redact/redact.go:16-33` |
| `search` | Web search | tool 点（`gwtool.Serve(gwtool.WebSearch)`） | settings, gateway | 是 | 搜索 API 数 + 选定 searcher | settings.js 在窗口（不论开关都加载） | `plugins/search/search.go:17-31` |
| `budget` | Key limits | budget 点（`gwbudget.Limit(budget.Limiter{})`） | settings, gateway | 是 | 带限额的网关密钥数 | 无 | `plugins/budget/budget.go:17-31` |
| `tools` | Plugin tools | tool 点（`gwtool.Serve(gwtool.PluginTools)`） | gateway | 是 | `plugin.ToolsOn` | 无 | `plugins/tools/tools.go:14-28` |
| `warmups` | Warm-up and check-in | 设置项（无网关点） | window, settings | 否 | settings 里的 warmup/checkin 计数 | part `warmups` | `plugins/warmups/warmups.go:9-31` |
| `all` | （聚合包） | 仅 import 上面全部；只有 main.go 导入它 | — | — | — | — | `plugins/all/all.go:1-19` |

要点：
- 10 个 Essential（essentials 预设仍开）；`sessions/library/otel/groups/warmups` 非 Essential（`docs/subsystems/plugin-runtime.md:31`）。
- 所有网关点的"关"都是真关：插件 off → 没有 observer/策略/中间件/masker/limiter/tool；masking 与 budget 是 fail-closed，策略与中间件 fail-open（`docs/subsystems/plugin-runtime.md:449-460`）。
- 插件关掉后窗口里 `data-feature="<id>"` 的导航/行/入口随之隐藏（`internal/gui/builtins.go:11-17`）。

---

## 10. 权限与安全

| 面 | 机制 | 源码 |
|---|---|---|
| 代码信任 | 装插件=信任它：provider/tool 代码以用户权限跑在 Bun 宿主 | `plugin-tools.md:32` |
| Bun 供应 | 按平台下载+ SHA-256 校验（`bunSums`）；坏版本拉黑回退；`MAGPIE_BUN` 自备 | `bun.go:32-59`、`bunupdate.go:100-104` |
| 安装 | `bun add --ignore-scripts`（不跑安装脚本）；装完不是插件的包会被卸掉 | `store.go:789-803`、`store.go:452-460` |
| dist 分支 | 只装 `index.json` 指定 sha512 integrity 的 tgz | `dist.go:33-34`、`dist.go:82-84` |
| 页面沙箱 | iframe `allow-scripts` + CSP 禁网 + 每运行随机 token + 文件 containment + `apiFramed` 挡 /api | §7 |
| 数据最小化 | 页面只能读 `usage:summary`/`providers:list`；`onUsage` 行是白名单副本（无 body/prompt/错误文本/会话/id）；账号 ref 是 安装级随机密钥 `account-ref.key` 的派生值（每安装随机密钥存 `account-ref.key`） | `pluginpage.go:96-98`、`points.go:15-17`、`gateway-middleware.md:27` |
| 凭证 | `plugin-auth.json`（0600、文件锁、lastgood）；登录 OAuth 的 access/refresh 只在宿主按账号 scope 读取 | `store.go:124-152`、`host.js:794-860` |
| 请求头防泄漏 | magpie 加的 `x-magpie-*` 头在出口被 `unsaid` 剥掉，厂商看不到 | `host.js:589-613` |
| 失败策略 | 中间件/策略 fail-open；masking/budget fail-closed；observer 只读；budget 超限 429、无限额器 503 且什么都不发 | `gwmw.go:1-17`、`gwbudget.go:1-10`、`plugin-runtime.md:449-460` |
| 包签名 | 未确认：npm 包依赖 npm registry 自身 TLS 与 npm integrity；dist 分支有 sha512 校验；未见对任意 npm/git 插件的代码签名/作者签名机制 | — |

---

## 11. 与其他子系统的关系

- **provider**：moved/retired 订阅由社区插件接管（`migrations.json`、`movers` 映射、`min` 版本要求）；插件账号 `Account.Agent=="plugin"`，请求经 `plugin.Fetch` 走 `Provider.Do`，绕过内置适配器；magpie 仍管路由/协议转换/配额/宿主（`docs/subsystems/provider-plugins.md:9-31`、`internal/provider/plugins.go:24-32`）。插件扩展：checkin、options、deferred usage、load、searches、serviceTiers、decision API、embeddings/video（`provider-plugins.md` 各节）。
- **gateway**：五个扩展点全在网关进程；`ListenAndServe` 里 `gwtool.Gate()`、`gwmw.Refresh(pluginrt.Refresh)`、`gwbudget.Refresh(pluginrt.Refresh)`、`pluginrt.KeepFresh(...)`（`internal/gateway/gateway.go:753-784`）。给插件 fetch 传 work-dir/session/subagent/fallback 头（`internal/provider/plugins.go:38-72`）。
- **usage**：`usage.Observer`/`Observe`（observer 点在 usage 包，因为每行都是 usage Record）；`magpie_deferred` 字段；`usage.magpie_cache_unknown`（`docs/subsystems/plugin-runtime.md:181-187`、`provider-plugins.md:35-53`）。
- **routes**：策略点与 `routes.Builtin` 9 策略（`plugins/rotation`）。
- **library**：给工具的插件自动成为 Library MCP server（`internal/library/plugin_tools.go`）。
- **gui**：Plugins 页（市场/已装/内置三个面）：`internal/gui/plugins.go`（`/api/plugins*`、`/api/plugin-signin*`）、`builtins.go`、`pluginpage.go`、`assets/plugins.js`、`assets/pluginpages.js`、`assets/pagebridge.js`、`assets/kernel.js`。
- **settings**：`PluginOptions`（每插件选项）、`ChinaMirror`、`NPMRegistry`、OTel 导出、Privacy 脱敏开关（`provider-plugins.md:73-77`、`registry.go:57`）。
- **cli**：`magpie plugin …`（add/rm/update/on/off/options/use/login/logout/move/builtin/preset/registry/dump）（`internal/cli/plugins_cli.go:24-37`）。
- **agent**：agent 插件（`magpie.agent`）在 `internal/agentplug`，与中间件共用 `internal/jsmod` 编译（`agent.go`、`gateway-middleware.md:31`）。

---

## 12. 证据索引（文件:行号）

**设计文档**
- `docs/subsystems/plugin-runtime.md`（516 行）：总纲；P0 运行时/文件/窗口开关（:23-31）、窗口内核（:38-95）、P2 页面外迁（:97-170）、P3 五点（:172-260）、P4 社区插件（:262-276）、P5 面板/替页（:278-354）、P6 菜单（:356-391）、合成（:393-407）、运行路径（:409-434）、失败与兼容（:436-460）
- `docs/subsystems/provider-plugins.md`：moved/retired、deferred usage、无份额窗口、钱包余额、cache unknown、签到、options、自报 usage/load、 searches、serviceTiers、sampling、decision、work-dir/session/fallback 头、双插件同 id、慢插件、401 语义
- `docs/subsystems/plugin-tools.md`：MCP 工具全链
- `docs/subsystems/gateway-middleware.md`：moejs 中间件全链
- `docs/subsystems/gateway-request-hooks.md`：异步钩子契约

**核心代码**
- `internal/pluginrt/pluginrt.go:35-72`（Manifest）、`:128-152`（Register）、`:206-278`（Reconcile）、`:83-89`（State）
- `internal/pluginrt/config.go:51-58`（File）、`:126-175`（Compose）、`:197-214`（Settle）、`:219-269`（Set/SetConfig/SetPreset）、`:334-395`（Refresh/KeepFresh）
- `internal/pluginrt/schema.go:50-105`（SchemaOf）、`:178-192`（Validate）
- `internal/plugin/store.go:47-72`（Entry/List）、`:324-341`（IsPath/IsGit）、`:399-496`（Add/add）、`:553-594`（Update）、`:607-701`（Remove/SetOff/Prefer/SetOptions）、`:709-733`（ShortName/Called）
- `internal/plugin/host.go:447-494`（host 结构）、`:728-776`（Restart/retire）、`:874-930`（get/start）、`:956-1085`（startOn+init）、`:1092-1095`（等待常量）、`:1112-1140`（read）、`:1199-1249`（dispatch）、`:1603-1644`（call）、`:1680-1686`（Plugins）、`:1831-1878`（Fetch）
- `internal/plugin/host.js:1-26`（协议）、`:1271-1357`（loadPlugins）、`:1496`（auths  prefer）、`:1532-1679`（tools）、`:2764-2873`（fetchAs）
- `internal/plugin/providers.go:24-150`（Method/Model/Provider）
- `internal/plugin/middleware.go:15-53`（Middleware 发现）、`:58-75`（OptionsExample）
- `internal/plugin/page.go:21-108`（WindowSpec/PanelSlots/Replaceable/PageReads）、`:145-233`（Window 解析）、`:294-348`（containment）
- `internal/plugin/agent.go:14-57`、`internal/plugin/gateway.go:21-78`（hooks 发现）、`:132-146`（HasGateway）、`:174-260`（GatewayRun）
- `internal/plugin/tools.go:41-78`（Tools/RunTool）、`:90-125`（noteTools）、`:148-167`（ToolsOn）
- `internal/plugin/market.go:33-60`（内置市场/Listing）、`:181`（缓存路径）；`registry.go:50-103`（Registry）；`github.go:18-52`（Topic/Tagged）；`dist.go:26-84`；`bun.go:32-59`；`bunupdate.go:31-104`；`installcache.go:12-40`；`gitrev.go:1-11`；`load.go:9-115`；`resolve.go:16-107`
- `internal/middleware/middleware.go:1-14`（包定位）、`:34-40`（时限）、`:44-119`（mw/set/current）；`points.go:45-53`（strategy/usage 限额）；`run.go:27-38`（Chain/On）
- `internal/gwtool/gwtool.go:18-24`（WebSearch/PluginTools）、`:50-72`（Serve/Served）
- `internal/gwbudget/gwbudget.go:21-35`（Limiter/Refusal）、`:45-65`（Limit）
- `internal/gwmw/gwmw.go:29-90`（Agent/Run/Tried/Account）
- `internal/routes/routes.go:1-12`（包定位）、`:21-70`（Seat）
- `internal/gateway/tools_mcp.go:33-80`（ToolsMCPPath/toolPrefix/servedTools）；`internal/gateway/gateway.go:753-784`（Gate/Refresh/KeepFresh）
- `internal/gui/plugins.go:95-173`（pluginEntryJSON）、`:239-260`（pluginsState）、`:583-821`（/api/plugins* 路由）
- `internal/gui/builtins.go:28-80`（builtinJSON/builtinsState）、`:110-157`（builtin 路由）
- `internal/gui/pluginpage.go:40-66`（前缀/token）、`:131-198`（windowParts）、`:223-270`（pagePolicy/ownSources）、`:311-389`（servePluginPage）、`:391-408`（apiFramed）、`:414-526`（路由）、`:528-584`（pageRead）
- `internal/gui/assets/pluginpages.js:18-119`（frame/hello/idle）；`internal/gui/pagebridge.js:1-47`（API 文档头）
- `internal/plugin/market.json`（内置市场条目）；`internal/plugin/testdata/**`（tools/page-glance/agent-aider 等示例）
- `plugins/*/*.go`（14 个 Manifest，见 §9 表）

---

## 13. 未确认项

1. **插件包签名/作者验证**：除 dist 分支 tgz 的 sha512 与 Bun 自身 SHA-256 外，未发现对 npm/git 插件的代码签名或发布者签名机制（设计上"安装即信任"）。是否有计划中的机制：未确认。
2. **`noFramedAPI`/`webGuard`**：文档提到 `noFramedAPI` 拒绝沙箱帧的 `/api/` 请求、`webGuard` 放行前缀（`plugin-runtime.md:274`），本次只读到 `apiFramed`（`pluginpage.go:396`）与路由注册处；`webGuard`/`noFramedAPI` 的具体实现位置未逐行确认（疑在 `internal/gui/api.go` 或 oneport/web 相关文件）。
3. **`internal/agentplug`**：agent 插件的实际加载/编译目录未逐文件读（只从 `agent.go`、`gateway-middleware.md:31` 与 GUI 行确认其存在与职责）。
4. **`internal/jsmod`**：只确认它是中间件与 agent 插件共用的模块编译层（`gateway-middleware.md:31`），内部实现未读。
5. **`settleWaiting`/`Updated`/`migrate*.go`**：订阅迁移（moved/retired）与自动更新记录的细节仅在 `provider-plugins.md` 与 `store.go` 引用处见到，未逐文件核对。
6. **tools_mcp.go 的 `serveMCP` 细节**（网页拒绝/远程要密钥）依据 `plugin-tools.md:12` 与 `tools_mcp.go:50-51` 调用点，未读 `serveMCP` 本体。
7. **`magpie.plugin dump --json`** 输出格式依据 `config.go:119-123`（Composed 的 JSON tag）与文档，未跑命令验证。

---

## 14. 与 octopus 插件系统的关键差异点（各一句，供 lead 对比）

> 说明：本次只读了 magpie 一侧源码；octopus 一侧未调查，以下每条只陈述 magpie 的事实作为对比轴，octopus 的对应情况均为**未确认**。

1. **双系统并存**：magpie 同时有"Go 内置插件"（pluginrt + plugins.yml，可整块关闭）和"OpenCode 兼容 npm 插件"（Bun 宿主 + plugins.json）两套互不干扰的插件体系；octopus 是否同构：未确认。
2. **中间件跑在网关进程内**：magpie 的网关 JS 中间件用 moejs（Go 内 JS 引擎）在网关进程执行（µs 级、可挂在每个流事件上），provider 插件才跑 Bun 宿主；octopus 的插件执行模型：未确认。
3. **页面沙箱令牌化**：magpie 社区插件的窗口页面是 `sandbox="allow-scripts"` iframe，文件经每次运行随机 HMAC-SHA256 token 提供，CSP 断网、`apiFramed` 挡 API、只能读两个白名单数据；octopus 的插件 UI 隔离方式：未确认。
4. **扩展点 + 失败策略分级**：magpie 把网关开放为 observer/strategy/middleware/masker/tool/budget 六个点，每个点有明确 fail-open 或 fail-closed 策略，且每个点由一个可关闭的内置插件 serve；octopus 的扩展点与失败语义：未确认。
5. **"在用即开"强制态**：magpie 内置插件只要 `InUse>0`（有路由组、有密钥限额、有中间件开着……）无论 plugins.yml 怎么写都保持开启，且拒绝手动关闭；octopus 是否有等价机制：未确认。
