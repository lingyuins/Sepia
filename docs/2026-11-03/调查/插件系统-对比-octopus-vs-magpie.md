# octopus 与 magpie「插件系统」完整对比

> 对象：`D:\CodeWorkspace\octopus`、`D:\CodeWorkspace\magpie`
> 方式：只读源码逐文件核对，结论标注 `文件:行号`；逐项目细节见同目录
> `octopus-插件系统调查.md`、`magpie-插件系统调查.md`。

---

## 0. 结论速览

| 维度 | octopus | magpie |
|---|---|---|
| 插件机制 | **单一**：WASM（Extism / wazero）插件，**事件驱动** | **多种并存（约 8 类）**：① OpenCode **provider 插件**（Bun 宿主，npm/git/本地）② **agent 插件**（magpie 进程内 jsmod）③ JS **网关中间件**（moejs，进程内）④ **gateway 异步钩子**（Bun 宿主）⑤ **页面/面板/行菜单插件**（沙箱 iframe）⑥ **插件工具**（作为 MCP 提供给 Agent）⑦ Go **内置插件**（`pluginrt`，编译期注册）⑧ **pi 包/扩展**（Bun 宿主，pi loader） |
| 插件语言 | 任意能编译到 WASM（Extism PDK） | provider/页面/中间件：**JS/TS**；内置：**Go** |
| 隔离与沙箱 | WASM 沙箱（wazero/Extism），硬性资源限制（`timeout≤2000ms`、`mem≤64MiB`、`instances≤16`、`payload≤1MiB`），`permissions` 显式声明 | provider 插件在独立 **Bun 进程**（`host.js` 经 stdin/stdout）运行；页面/面板用 **iframe + 一次性 token + 读取白名单**；内置插件与核心同进程（编译期受信） |
| 扩展点 | relay 钩子：`relay.request.before_route`、`relay.route.filter`、`relay.request.before_upstream`、`relay.response.before_send`、`relay.completed`；`channel.health.changed`、`pool.account.changed`、`notification.created`、`http.rpc`、`plugin.migrate`；插件自带 UI 页 | provider/agent/account/auth/usage；网关 `observer`(OTel)、`router strategy`(路由策略)、`middleware`(JS 中间件)、`tool`(MCP 工具)、`budget`(key 限额)；页面/面板；设置 schema |
| 清单/元数据 | `manifest.json`：`host_api/id/name/version/entry/events/permissions/http_origins/required/priority/timeout_ms/memory_mib/instances/config_schema/ui` | provider 插件用 `package.json` 的 `magpie.*`（main/exports/middleware/page/panels/tools/options/provider…）；内置用 `pluginrt.Manifest`（ID/API/Requires/Provides/Essential/Name/Parts/Defaults/Settings/Client…）；市场用 `market.json`/`registry.json` |
| 安装来源 | **上传插件包**（zip：`manifest.json` + `plugin.wasm` + `ui/*`），落 DB；无在线市场 | **npm 包 / git 仓库 / 本地文件**；内置 **市场**（`market.json` 内置副本 + 社区 `registry.json` + npm 搜索 + GitHub）；受管 Bun 运行时 |
| 市场 | 无 | 有 |
| UI 集成 | `GET /api/v1/plugins/:id/ui-bundle` → 前端 `createPluginDocument` 装进隔离 iframe（CSP/session） | 内置插件页面经 `kernel.js` 的 `page` slot 变成 `<main class="view">`；社区页面 `GET /_magpie/plugin/<token>/…` + `_magpie/bridge.js`（沙箱、不触达 `/api`，读取需 `POST /api/plugin-pages/read` 声明） |
| RPC | `POST/GET /api/v1/plugins/:id/rpc/*path`（`middleware.Auth()` + catch-all）→ 转发给 WASM 的 `http.rpc` | provider 插件通过 `host.js` 的 stdin/stdout 消息协议；无通用公网 RPC 端点 |
| 管理鉴权 | `Auth()` + `RequirePermission(PermPluginsRead)` 读；`requirePluginAdmin`(仅 admin) 写 | 本机 GUI `/api/plugins*`（无账号体系，靠 `magpie web` 的运行 key / 本机） |
| 与推理转发 | 插件**可改请求体、裁剪路由候选、改响应**（relay 事件，改请求需 `relay:write`） | provider 插件**拥有订阅的上游请求**与登录/刷新/用量；middleware 插件**可改请求与回复**；内置 `rotation` 提供路由策略、`search`/`vision` 提供工具 |
| 状态与开关 | `plugins` 表（`enabled/status`：disabled/enabled/error/needs_reinstall/uninstalling）+ `plugin_kv`/`plugin_secrets` 表 | provider 插件 `plugins.json`（`spec/off/options/config/prefer`）；内置 `plugins.yml`（`preset/settled/plugins`）+ `plugins.json`（中间件选项） |

一句话：**octopus 的插件系统是“单一 WASM 事件插件 + 自带 UI + 管理端安装”的企业式扩展**；**magpie 的插件系统是“多语言、多机制、带市场”的生态式扩展**——provider 插件（Bun/OpenCode）负责订阅接入、Go 内置插件负责产品功能开关、JS 中间件/策略/工具负责网关行为、页面插件负责 UI。

---

## 1. octopus 的插件系统

### 1.1 定位与架构
插件是 **WebAssembly（Extism SDK + wazero 运行时，启用 WASI）** 模块，由宿主（Go 主进程）以事件回调方式调用；插件可声明**自带 UI**（`ui/` 资源，经 iframe 展示）。包结构：`internal/plugin/`（`manifest.go`、`manager.go`、`runtime.go`、`host.go`、`host_http.go`、`protocol.go`、`package.go`、`store.go`、`event/`）。

### 1.2 清单与包格式（`manifest.go`、`package.go`）
- `Manifest`（`manifest.go:44-65`）：`HostAPI`（=1）、`ID`（`^[a-z][a-z0-9-]{1,63}$`）、`Name`、`Version`（semver）、`Entry`（默认 `handle`）、`Events`、`Permissions`、`HTTPOrigins`、`Required`、`Priority`、`TimeoutMS`、`MemoryMiB`、`Instances`、`ConfigSchema`、`UI{Entry,Title}`。
- 支持的事件（`:26-32`）：`relay.request.before_route`、`relay.route.filter`、`relay.request.before_upstream`、`relay.response.before_send`、`relay.completed`、`channel.health.changed`、`pool.account.changed`、`notification.created`、`http.rpc`、`plugin.migrate`。
- 支持的权限（`:34-37`）：`kv`、`secret`、`http`、`notify`、`log`、`relay:read`、`relay:write`、`rpc:read`、`rpc:write`；`relay.*` 事件强制要求 `relay:read`/`relay:write`（`:124-131`）。
- 包上限：`MaxPackageBytes=20MiB`、`MaxWASMBytes=16MiB`、`MaxAssetBytes=2MiB`、`MaxPackageFiles=128`；包内只允许 `manifest.json`、`plugin.wasm`（校验 `\0asm` 魔数）、`ui/*`（`package.go:17-118`）；SHA256 校验（`manager.go:118-120`）。
- 资源默认值：`TimeoutMS` 默认 100（上限 2000）、`MemoryMiB` 默认 16（上限 64）、`Instances` 默认 4（上限 16）（`manifest.go:96-107`）。

### 1.3 运行时与隔离（`runtime.go`、`host.go`）
- `generation` 用 `extism.NewCompiledPlugin`（内存页=MemoryMiB*16、`MaxHttpResponseBytes=1MiB`、`Timeout`），预建 `Instances` 个实例放入 `pool`（`runtime.go:28-55`）；调用时 `acquire/release`，坏实例销毁重建（`:57-77`）。
- 宿主函数 `host_call`（`host.go:25-54`）：插件发 `HostRequest{capability,payload}`，宿主按能力执行 `kv.*`/`secret.*`/`http.request`/`notify.publish`/`log.write`（`:56-131`），按 manifest 权限校验，日志/通知内容做敏感信息脱敏（`:17,116,150`）。
- 进程内一次性 generations 可 `Replace`（新版本加载后旧 generation `Retire` 等引用归零再 Close，`runtime.go:141-149`），实现热升级/平滑卸载。

### 1.4 生命周期与管理（`manager.go` + `internal/server/handlers/plugin.go`）
- 状态：`disabled / enabled / error / needs_reinstall / uninstalling`（`model/plugin.go:5-11`）。
- 接口：`GET /api/v1/plugins`、`GET /api/v1/plugins/:id/ui-bundle`（读权限）；`POST /install`（**multipart 上传** `.octopkg/.zip`）、`POST /:id/enable`、`POST /:id/disable`、`PUT /:id/config`、`DELETE /:id`（`requirePluginAdmin` 仅 admin）；`POST/GET /api/v1/plugins/:id/rpc/*path`（`handlers/plugin.go:28-45`）。
- 安装/升级：**禁止降级**（版本与 schema）、升级保留旧配置/启用态、若声明 `plugin.migrate` 则在**事务**里迁移数据（失败回滚旧版），落库后原子替换目录并热加载（`manager.go:158-270`）。
- 卸载：立即从活动快照移除，但**磁盘目录需进程重启后才真删**（`FinalizeUninstalls` 启动时清理），`uninstalling` 期间禁止重装（`manager.go:330-356`）。
- `Manager.Load` 只加载 `Enabled && status==enabled` 的记录（`manager.go:87-106`）；事件异步队列（`queue` 容量 256，满即丢），`Publish` 广播、`Invoke` 有返回（`protocol.go:62-80`）。
- 账本：`plugins` 表 + `plugin_kv`（KV）+ `plugin_secrets`（密钥，加密存储）三张表分离，列表不暴露密钥/可执行字节（`model/plugin.go:13-53`）。

### 1.5 UI 集成（三重隔离）
- 后端 `GET /:id/ui-bundle` 只以**鉴权 JSON** 返回 `ui/` 资源（`Auth+PermPluginsRead`，`no-store`/`nosniff`，拒符号链接、逐文件≤2MiB、总量≤20MiB）。
- 前端 `web/src/components/modules/plugin/`（`index.tsx`、`ui-bundle.ts`）生成一次性 `session`，把 bundle 装进 `<iframe sandbox="allow-scripts" srcdoc>`；固定严格 CSP（`default-src 'none'`、`connect-src 'none'`、脚本只 `'unsafe-inline' data: blob:`），资产转 data URL，移除危险节点。
- 插件 UI 只能 `postMessage`（校验 `source===iframe.contentWindow`、`origin==='null'`、session UUID）→ 宿主 `dispatchPluginRPC` → 携带**当前登录用户**凭据调 `/api/v1/plugins/:id/rpc/...`。
- 另有内置「Magpie 节点」合成插件记录用于页面上开关节点特性（`handlers/plugin.go:64-80`，无 WASM/无 UI）。

### 1.6 介入推理转发（`relay/plugin_bridge.go`：4 改写 + 1 观察）
| 事件 | 调用点 | 语义 |
|---|---|---|
| `relay.request.before_route` | 解析入站后、选模型前 | 改写 `InternalLLMRequest`，宿主保留 wire/会话等权威字段，重编码 wire；失败→502 |
| `relay.route.filter` | 选渠道组时 | 入参 `{group_id,model,candidates}`，返回 `exclude_channel_ids`（未知渠道报错） |
| `relay.request.before_upstream` | 构建出站请求后 | 改出站 headers/body；**12 个敏感头（Authorization/Cookie/Host 等）对插件不可见也不可写**，只能写 `x-octopus-*`/`content-type`/`accept`；失败走错误分类重试 |
| `relay.response.before_send` | **仅非流式**响应路径 | 改写最终响应体；**流式不经过（明确缺口）** |
| `relay.completed` | metrics | 异步观察 `{model,api_key_id,success,error,response,attempt_count}` |

- 事件由 `internal/plugin/event`（`SetPublisher`/`Publish`）暴露给其他包（如 poolscheduler 发 `pool.account.changed`）。订阅 `relay.*` 需 `relay:read/write`，四个改写事件强制 `relay:write`。

---

## 2. magpie 的插件系统

### 2.1 插件类型矩阵（八种 + 三个运行位置）
| 类型 | 用途 | 声明位置 | 运行位置 |
|---|---|---|---|
| provider 插件 | 登录订阅、列模型、用插件 fetch 发上游请求 | OpenCode `main`/`exports` | **Bun 宿主**（`host.js` 进程） |
| agent 插件 | 给 Agents 页加外部编码 agent | `magpie.agent` | magpie 自身进程（`internal/agentplug`+`jsmod`） |
| middleware（网关 JS） | 改请求/回复、provider 尝试、账号跳过、流事件、注册策略 | `magpie.middleware`/`magpie.gateway` | **网关进程内**（moejs） |
| gateway 异步钩子 | 路由前注入上下文、观察完成回复（可 await I/O） | `magpie.hooks` | **Bun 宿主** |
| page/panel/action | 加页面、嵌面板、加行菜单项 | `magpie.page/panels/actions/permissions` | 窗口内 **iframe 沙箱**（`/_magpie/plugin/<token>/`） |
| tool（MCP） | 插件工具作为 MCP server 给所有 Agent | OpenCode `tool` hook | **Bun 宿主**，经 `/mcp/magpie/tools` 暴露 |
| builtin 插件 | magpie 自身可关闭的功能块 | `pluginrt.Register(Manifest)`（Go init） | Go 进程内 |
| pi 包/扩展 | pi coding agent 的包/扩展 | `pi` 字段 / `pi-package` keyword | **Bun 宿主**（`pi.js`） |

> 运行位置只有三处：**Bun 宿主进程**（provider/gateway-hooks/tool/pi）、**网关进程内 moejs**（middleware）、**窗口 iframe**（page/panel）；内置插件是 Go 进程内编译期代码。

### 2.1b 网关六个扩展点（P3/P4，各由一个可关闭的内置插件 serve）
| 扩展点 | 接口 | 失败策略 | 内置消费者 |
|---|---|---|---|
| observer | `usage.Observer` | 只读，不影响请求 | `plugins/otel` |
| router strategy | `routes.Strategy`（`Order(pool, seats)`） | **fail-open**（非排列→原顺序） | `plugins/rotation`（9 策略）、JS 插件 `strategies` |
| middleware | `gwmw.Agent`（Begin/Request/Wrap/End） | **fail-open** | `plugins/middleware` |
| masker（脱敏） | `gwmw.Masker` | **fail-closed**（要脱敏无 masker→503，什么都不发） | `plugins/redact` |
| tool | `gwtool.Serve(name)` | off 即 off | `plugins/search`、`plugins/tools` |
| budget（key 限额） | `gwbudget.Limiter` | **fail-closed**（`ErrUnlimited`→503） | `plugins/budget` |

> 内置插件还有「在用即开」强制态：`InUse>0`（有路由组/密钥限额/中间件开着/有库项…）时无论 `plugins.yml` 怎么写都保持开启并拒绝手动关闭（`plugin-runtime.md:31`）。

### 2.2 包规范与清单
- **provider 插件**：`package.json` 的 `magpie.*` 字段（`main`/`exports`/`middleware`/`gateway`/`page`/`panels`/`tools`/`options`/`provider`/`agents` 等），按 OpenCode 约定；`plugins.json` 的 `Entry{Spec,Off,Options}`（`store.go:47-72`）。
- **内置插件**：`pluginrt.Manifest{ID, API:"^1", Requires, Provides, Essential, Name, Sub, One, Many, InUse, Apply, Parts{Slot,Key}, Defaults, Settings, Client}`（`plugin-runtime.md:26`）；`Settings` 结构体自动生成 JSON Schema 与设置表单（`schema.go`）。
- **市场**：`market.json`（内置副本，`market.go:33-40`）列出 `{package,name,icon,providers,kind:"middleware|agent|tools|page",community,replaces,summary{en,zh}}`；运行时另拉社区 `registry.json`（`raw.githubusercontent.com/magpie-community/plugins`）+ npm 信息（registry.npmjs.org 缓存）+ GitHub topic `magpie-plugin`（`market.go`、`github.go`、`registry.go`）。
- **安装与供应链**：npm 包 `bun add --ignore-scripts`、git spec、本地路径；`@magpie-community/*` 可回落社区 **dist 分支**（tgz + sha512 校验）；Bun 首次需要时下载（下限 `1.4.2`、SHA-256、坏版本拉黑回退）；registry 选择链：magpie 设置→env/shell→bunfig/npmrc→国内镜像→npm 官方（`store.go`、`dist.go`、`bun.go`/`bunupdate.go`、`registry.go`）。

### 2.3 运行时（三条执行路径）
- **Bun 宿主**：`host.js`（136KB）与 Go `host.go`（61KB）协作，管理插件进程、加载模块、转发 auth/fetch/provider；`bun.go`/`bunupdate.go` 下载/更新 Bun；`dist.go` 打包、`github.go` 拉取、`updates.go` 更新。
- **内置插件运行时**：`internal/pluginrt`（`pluginrt.go` 12KB、`config.go`、`schema.go`）维护 registry，按依赖 `Requires/Provides` 拓扑 `Reconcile`，状态 `off/pending/active/failed/disposed`；`Ctx` 提供 `OnDispose/Contribute/Provide/Service/Config`。
- **网关中间件**：`internal/middleware`（`middleware.go`、`run.go`、`points.go`）用 moejs 在网关进程执行，限制 `onRequest/onAccount/onProvider/onResponse` 250ms、`onEvent` 50ms、加载 1s，失败开放（`gateway-middleware.md`）。

### 2.4 内置插件清单（`plugins/`）
`gateway`（Gateway 页，Essential）、`routing`（Routing 页）、`usage`（Usage 页）、`sessions`、`library`、`rotation`（除 Smart 外的路由策略）、`middleware`（JS 中间件宿主）、`redact`（隐私脱敏）、`search`（Web 搜索）、`otel`（可观测导出）、`budget`（key 限额）、`warmups`（预热/签到）、`tools`（MCP 工具）、`groups`（路由组）。每个可开关，`plugins.yml` 记录 `preset`（essentials/all）、`settled`、`plugins[{id,enabled,config}]`（`plugin-runtime.md:29-31`）。

### 2.5 页面/面板与工具
- 页面：内置插件经 `kernel.js` 的 `page` slot 生成视图（`plugin-runtime.md:38-59`）；社区页面走 token 化 iframe + bridge（`pluginpage.go:42-49`）。
- 面板：`magpie.panels` 挂到内置页的 slot（如 `agents` 的 `plugin-slot`）。
- 工具：插件工具统一由 MCP router 暴露给所有 Agent。

### 2.6 权限与安全
- provider 插件：独立 Bun 进程、OpenCode 的插件 API 边界；登录态在 `plugin-auth.json`（OpenCode `auth.json` 形状）。
- 页面/面板：一次性 token（`pageSecret` 每次运行随机）、CSP 只允许自身文件、无网络、不能直接访问 `/api`，读取需声明（`pluginpage.go:29-49`）。
- 内置插件：编译期受信，`TestPluginsImportOnlyCore` 保证插件不互相 import、核心不 import 插件（`plugin-runtime.md:33-36`）。

---

## 3. 逐项差异

1. **机制数量**：octopus 只有一种（WASM 事件插件）；magpie 有五种（provider/内置/中间件/页面/工具），分别解决“接入订阅 / 产品功能开关 / 网关行为 / UI / 工具供给”。
2. **语言与运行时**：octopus 任意→WASM（Extism/wazero，进程内沙箱）；magpie 以 JS/TS 为主（Bun 独立进程 + moejs 网关内 + 浏览器内），内置用 Go。**octopus 的隔离更强、资源约束更硬；magpie 的能力面更广、开发门槛更低。**
3. **扩展点重心**：octopus 插件重心是**转发链路钩子**（改请求/裁剪路由/改响应）+ 自带 UI；magpie 把网关拆成**多个具名扩展点**（observer/strategy/middleware/tool/budget），provider 插件另负责上游接入。
4. **安装与分发**：octopus 无市场，靠上传 zip 包 + 管理端；magpie 有**市场/registry/npm/git/本地**，并自带 Bun 运行时管理。
5. **UI 形态**：octopus 插件 UI 是“附带 iframe 资源”；magpie 既有内置插件页（与产品 UI 同构的 slot），也有社区沙箱页。
6. **鉴权模型**：octopus 走后台 RBAC（读权限 + admin 才能装/卸载）；magpie 无账号体系（本机 GUI + 运行 key）。
7. **状态管理**：octopus 用 DB 表（含 KV/secret）；magpie 用文件（`plugins.json`、`plugins.yml`、`plugin-auth.json`、`affinity.json` 等）。
8. **插件能否改推理**：两者都能，但方式不同——octopus 通过 relay 事件改 DTO；magpie 通过 provider 插件（拥有 fetch）+ middleware（onRequest/onResponse）。

---

## 4. 扩展点对照表

| 能力 | octopus | magpie |
|---|---|---|
| 改请求体 | `relay.request.before_route`（`relay:write`） | middleware `onRequest`；provider 插件 fetch |
| 裁剪/改路由候选 | `relay.route.filter` | middleware `strategies`（注册为路由策略）；内置 `rotation` |
| 改上游请求前 | `relay.request.before_upstream` | middleware `onProvider` |
| 改响应/流事件 | `relay.response.before_send` | middleware `onEvent`/`onResponse` |
| 观测/导出 | `relay.completed` + `log.write` | observer(OTel) 内置插件 + `onUsage` |
| 提供工具 | 无原生 MCP 工具点（走 RPC） | 插件工具（MCP `plugin-tools`） |
| 提供 UI 页 | `ui/` + `/ui-bundle` iframe | kernel `page` slot / `/_magpie/plugin/<token>/` |
| 面板嵌入产品页 | 无 | `magpie.panels` slot |
| 通知 | `notify.publish` | 插件工具/内置（无统一 notify 权限） |
| KV/密钥存储 | `plugin_kv`/`plugin_secrets` + `kv.*`/`secret.*` | `plugins.json` options / `plugin-auth.json` / 文件 |
| 路由策略扩展 | ❌（固定 5 种 GroupMode） | ✅ `routes.Register`（`rotation` 即内置消费者） |
| 市场 | ❌ | ✅ `market.json` + `registry.json` + npm |

---

## 5. 证据（关键文件:行号）

**octopus**
- `internal/plugin/manifest.go:12-161`（常量/事件/权限/Manifest/校验）、`protocol.go:17-96`（CallRequest/HostRequest/decision）、`runtime.go:28-153`（Extism 编译/实例池/调用）、`host.go:25-164`（host_call 能力与权限）、`package.go:17-155`（zip 包校验/上限）
- `internal/plugin/manager.go:29-120`（Manager/Load/代际/校验和）、`internal/model/plugin.go:5-61`（状态/表结构）、`internal/server/handlers/plugin.go:28-80`（路由/鉴权/内置节点）
- `internal/relay/plugin_bridge.go:23-71`（before_route 介入）、`internal/plugin/event/event.go`
- 前端：`web/src/components/modules/plugin/index.tsx`、`ui-bundle.ts`（iframe 隔离）

**magpie**
- `internal/plugin/store.go:1-110`（OpenCode 插件包语义/Entry/List/runBun）、`internal/plugin/market.go:28-40`（市场内置副本）、`market.json`
- `internal/pluginrt/pluginrt.go`、`config.go`、`schema.go`（内置插件运行时/清单/schema）
- `plugins/all/all.go`（全部内置插件）、`plugins/rotation/rotation.go:19-49`（策略注册）
- `internal/gui/pluginpage.go:29-49,410-464`（页面/面板沙箱与读取白名单）
- `docs/subsystems/plugin-runtime.md`、`provider-plugins.md`、`plugin-tools.md`、`gateway-middleware.md`（权威设计）
- `internal/middleware/{middleware.go,run.go,points.go}`（JS 中间件）、`internal/gwmw`

---

### 附：未确认项
- octopus 插件安装的 HTTP 入口细节（上传包的表单/字段、是否支持 URL 安装）未逐行展开，仅确认 `ReadPackage` 从请求流读取 zip 包。
- magpie `host.js`（136KB）/`host.go`（61KB）的完整消息协议与 provider 钩子细节未逐行展开，本对比按 `store.go`/`provider-plugins.md`/`plugin-runtime.md` 归纳。
