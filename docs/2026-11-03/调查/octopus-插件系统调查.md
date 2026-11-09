# Octopus 插件系统调查

> 调查对象：`D:\CodeWorkspace\octopus`（Go 后端 `internal/plugin/` + relay 钩子 + Next.js 前端 `web/src/components/modules/plugin/`）
> 调查方式：只读源码，结论均给出文件:行号；未证实处明确标注“未确认”。
> 调查时间：2026-11-03

## 1. 概览与定位

Octopus 的插件系统是**进程内 WebAssembly（WASM）插件机制**，基于 [Extism go-sdk](https://github.com/extism/go-sdk) + [wazero](https://github.com/tetratelabs/wazero) 运行时（`internal/plugin/runtime.go:10-11`）。插件**不是**子进程、不是独立 HTTP 服务，而是以 WASM 模块形式加载进主进程，通过 Extism 的 host function（`host_call`）与宿主交互。

插件可以扩展三类能力：

1. **推理转发钩子（relay middleware）**：在路由前、路由过滤、上游请求前、响应发送前等 5 个 relay 事件点介入请求/响应改写（`internal/relay/plugin_bridge.go`）。
2. **自定义 HTTP RPC**：通过 `POST/GET /api/v1/plugins/:id/rpc/*path` 把请求转给插件的 `http.rpc` 事件处理器（`internal/server/handlers/plugin.go:200-242`）。
3. **自带 UI 页面**：插件包可携带 `ui/` 静态资源，前端以 sandboxed iframe + srcdoc 加载，通过 `window.postMessage` 与宿主页面做 RPC（`web/src/components/modules/plugin/index.tsx:25-54`）。

另外有一个**合成内置插件** `octopus.magpie-node`（`internal/server/handlers/plugin.go:64-88`）：它不是 WASM 插件，只是在插件列表里代表“Magpie 节点”功能开关，用于 UI 统一展示；RPC/UI 端点会显式拒绝它。

## 2. 包结构（`internal/plugin/` 文件职责）

| 文件 | 职责 |
|---|---|
| `manifest.go` | `Manifest`/`UIManifest` 结构定义、字段校验（`ParseManifest`、`Validate`）、支持的事件/权限白名单、版本比较 `CompareVersions`、按优先级排序 `SortManifests` |
| `manager.go` | `Manager` 生命周期：安装/升级（`InstallPackage`）、启停（`SetEnabled`）、卸载（`Uninstall`/`FinalizeUninstalls`）、活动快照管理（原子指针 `active` + `swap`/`replace`/`remove`）、事件分发（`Invoke`/`RPC`/`Publish`/`eventLoop`）、调用指标（`recordCall`）与错误状态落库（`setError`/`markNeedsReinstall`） |
| `runtime.go` | WASM 运行时：`generation`（一次加载的编译产物 + 实例池）、`newGeneration` 编译并预热 `Instances` 个实例、`call` 执行插件入口函数、引用计数优雅退役（`beginRequest`/`endRequest`/`Retire`） |
| `host.go` | 宿主能力（host functions）：唯一的 Extism host function `host_call`（`host.go:26`），按 `Capability` 分发到 `kv.*`、`secret.*`、`http.request`、`notify.publish`、`log.write`，逐个做权限检查与日志脱敏（`sensitiveLogPattern`，`host.go:17`） |
| `host_http.go` | `http.request` 能力的具体实现：仅允许 HTTPS + manifest 声明的 `http_origins` 白名单 origin；SSRF 防护（`xurl.AssertSafeRequestWithPin` / `SafeDialContext`）、禁 Host/Content-Length 等头、重定向限 3 次且不跨 origin、15s 超时、响应 ≤1MiB |
| `protocol.go` | 宿主↔插件协议：`CallRequest`/`CallResponse`（宿主调插件，JSON 进 JSON 出）、`HostRequest`/`HostResponse`（插件调宿主）、`RequiredError`、决策值白名单 `validateDecision`（`continue`/`observe`/`handled`/`allow`），以及包级便捷函数 `Handles`/`Invoke`/`Publish` |
| `package.go` | 插件包（zip）解析与校验：`ReadPackage`/`ReadPackageBytes`，大小/文件数限制、路径安全（`cleanPackageName`/`safePackagePath`）、WASM 魔数校验、安装目录计算 `PluginDir`（防目录逃逸） |
| `store.go` | 持久化：插件记录 CRUD（`ListRecords`/`Record`）、配置读写（`SaveConfig`/`Config`，禁在普通配置里放含 “secret” 的键）、KV 存储（`KVGet/KVSet/KVDelete/KVList`）、加密 Secret 存储（`SecretGet/SecretSet/SecretDelete`，经 `internal/utils/crypto` 加解密） |
| `event/event.go` | 极简事件发布器：全局 `publisher` 函数指针，`SetPublisher` 由 `Manager.Init` 注入，`Publish` 供业务包解耦发布事件 |
| `manager_test.go` / `runtime_test.go` | 单元测试：快照替换语义、payload 限制、Handles 并发一致性、manifest 校验默认值 |
| `testdata/` | 目录存在但**为空**（无示例插件包） |

持久化模型在 `internal/model/plugin.go`：`Plugin`（plugins 表，含 `ManifestJSON`、`ConfigJSON`、`SHA256`、`Status`、`Enabled`、`SchemaVersion`、`Calls/Failures/TimeoutCount`、`UninstallAfter` 等）、`PluginKV`（plugin_kv 表）、`PluginSecret`（plugin_secrets 表，Value 加密存储，注释明示“可执行字节与 Secret 不放在记录里，列表不泄露”）。

## 3. 清单（manifest）与包格式

### 3.1 Manifest 字段（`internal/plugin/manifest.go:44-65`）

| 字段 | JSON | 说明 |
|---|---|---|
| `HostAPI` | `host_api` | 必须等于 `HostAPIVersion = 1`（manifest.go:13, 81） |
| `ID` | `id` | 正则 `^[a-z][a-z0-9-]{1,63}$`，且不能以 `-` 结尾（manifest.go:22, 84） |
| `Name` | `name` | 非空、≤128 字符 |
| `Version` | `version` | 语义化版本（semver 正则，manifest.go:23） |
| `Description` / `Author` | | 可选展示信息 |
| `MinHostAPI` / `MaxHostAPI` | `min_host_api`/`max_host_api` | 宿主 API 兼容区间（manifest.go:81） |
| `Entry` | `entry` | WASM 导出函数名，默认 `handle`（`DefaultEventHandler`），须匹配 Go 标识符正则（manifest.go:90-95） |
| `Events` | `events` | 订阅事件白名单（见 §8），不允许重复/未知（manifest.go:111-117） |
| `Permissions` | `permissions` | 权限白名单：`kv`、`secret`、`http`、`notify`、`log`、`relay:read`、`relay:write`、`rpc:read`、`rpc:write`（manifest.go:34-37） |
| `HTTPOrigins` | `http_origins` | 允许访问的 HTTPS origin 列表；要求 `http` 权限且必须是 `https://...`（manifest.go:132-139） |
| `Required` | `required` | 关键插件：调用失败会让整个事件 `Invoke` 返回 `RequiredError`（manager.go:437-439） |
| `Priority` | `priority` | -100..100，决定同一事件多插件的执行顺序（manager.go:131-135） |
| `TimeoutMS` | `timeout_ms` | 单次调用超时，默认 100，上限 `MaxTimeoutMillis = 2000`（manifest.go:14, 96-98） |
| `MemoryMiB` | `memory_mib` | 内存上限，默认 16，上限 64 MiB（换算成 WASM page：`MemoryMiB * 16` 页，runtime.go:38） |
| `Instances` | `instances` | 实例池大小，默认 4，上限 16（runtime.go:45-53） |
| `SchemaVersion` | `schema_version` | 插件自身数据结构版本，用于升级时触发 `plugin.migrate` 事件（manager.go:227-244） |
| `ConfigSchema` | `config_schema` | 任意 JSON（仅校验是合法 JSON；**未见**用于实际校验配置，未确认是否被前端消费） |
| `UI` | `ui` | `UIManifest{Entry, Title}`，Entry 默认 `index.html`，路径须经 `safePackagePath`（manifest.go:143-150） |

事件与权限联动校验（manifest.go:124-134）：订阅 `relay.*` 事件需要 `relay:read` 或 `relay:write`；订阅改写类事件（`before_route`/`route.filter`/`before_upstream`/`before_send`）强制要求 `relay:write`；声明 `http_origins` 必须有 `http` 权限。

### 3.2 包格式（`internal/plugin/package.go`）

插件包是一个 **zip**（前端文件选择器接受 `.octopkg,.zip`，`web/.../plugin/index.tsx:125`），结构：

```
manifest.json   # 必需，经 ParseManifest 严格解析（DisallowUnknownFields，manifest.go:70）
plugin.wasm     # 必需，WASM 模块（校验 \0asm 魔数，package.go:96-98）
ui/**           # 可选，UI 静态资源；若 manifest 声明了 ui.entry，则该文件必须存在（package.go:112-116）
```

限制（package.go:17-22）：整包 ≤20 MiB、单文件解压后 ≤16 MiB、单个 UI 资源 ≤2 MiB、文件数 ≤128。不允许其他文件名（`unsupported file in plugin package`，package.go:106），路径须经 `cleanPackageName`（禁 `\0`、反斜杠、绝对路径、`..` 逃逸），zip 内符号链接和目录条目被跳过（package.go:66-68）。解析**完全在内存中完成后才落盘**（`ReadPackage` 注释，package.go:37）。

安装后磁盘布局（`<DataDir>/plugins/<id>/`）：`plugin.wasm`、`manifest.json`、`ui/...`（manager.go:194-216）。安装用“staging 目录 + rename”原子提交，旧目录先 rename 成 `.previous-<uuid>` 备份再删除（manager.go:184-258）。

**校验/签名**：仅记录并校验 WASM 的 SHA-256（落库 `record.SHA256`，每次加载比对，`manager.go:118-122`）；**没有代码签名/来源认证机制**——任何 admin 用户上传的包都会被接受。

## 4. 生命周期

### 安装 / 升级
`POST /api/v1/plugins/install`（multipart `file`）→ `Manager.Install` → `ReadPackage` → `InstallPackage`（manager.go:158-270）：
- 已存在时禁止降级版本（`CompareVersions`，manager.go:177）、禁止 schema 降级（manager.go:220-222）、卸载中（`uninstalling`）状态禁止安装（manager.go:174）。
- 升级保留旧配置/启用状态/创建时间（manager.go:219）。
- 若已启用且新版本声明 `plugin.migrate` 事件，先在**事务**里编译新 WASM 并调用 `plugin.migrate`（payload `{"from":old,"to":new}`），失败则回滚、保留旧版（manager.go:227-244）。
- 落库成功后原子替换磁盘目录，若原启用则立刻热加载新 `generation`（`m.replace(g)`，manager.go:259-265）。

### 启用 / 停用
`POST /api/v1/plugins/:id/enable|disable` → `Manager.SetEnabled`（manager.go:296-328）：启用时校验 SHA-256、编译、先落库再 `replace` 进活动快照；失败则 `setError`（状态置 `error`、`enabled=false`、记录 `last_error`）。停用则落库 `disabled` 并从活动快照移除。

### 配置
`PUT /api/v1/plugins/:id/config` → `plugin.SaveConfig`（store.go:34-61）：≤64 个键、JSON ≤64 KiB，键名含 “secret” 一律拒绝（Secret 必须走加密 API）。

### 卸载
`DELETE /api/v1/plugins/:id`（body `{"delete_data": bool}`）→ `Manager.Uninstall`（manager.go:330-356）：
- 立即从活动快照移除（插件立刻停止工作）；
- 标记 `status=uninstalling`，可选删除 KV/Secret 数据；
- **磁盘目录要进程重启后才删**：`FinalizeUninstalls` 在启动时（`cmd/start.go:196`）清理所有 `uninstalling` 记录的目录并删除 DB 记录；卸载中不可重装（“plugin uninstall requires a restart”）。handler 返回 `{"restart_required": true}`（plugin.go:184）。

### 状态持久化
全部状态在 SQLite/GORM 的 `plugins` 表（model/plugin.go:15-34）。状态机：`disabled` / `enabled` / `error` / `needs_reinstall`（WASM 文件丢失或校验和不匹配时由 `markNeedsReinstall` 设置，manager.go:540-542）/ `uninstalling`。进程启动时 `plugin.Init(<DataDir>/plugins)`（cmd/start.go:137）加载所有 `enabled` 记录并重建 WASM 实例池。

### 优雅替换
`Manager.active` 是 `atomic.Pointer[[]*generation]` 快照（manager.go:31）。替换时旧 generation 进入 `retired` 列表并调用 `Retire()`：先 `beginRequest` 关闸拒新请求，等 `refs` WaitGroup 归零后异步 `Close()`（runtime.go:141-149）。`manager_test.go:33-79` 专门验证了“替换插件 A 不影响在途的插件 B、在途请求未完成前不关闭旧实例池”。

## 5. 运行时与隔离

- **执行方式**：进程内 WASM。`newGeneration` 用 `extism.NewCompiledPlugin` 编译（开启 WASI、wazero `WithCloseOnContextDone`），按 `Instances`（默认 4）预热实例池（`chan *instance`），调用时 `acquire`/`release` 借还实例（runtime.go:28-77）。
- **隔离边界**：WASM 沙箱 + 内存页上限（`MaxPages = MemoryMiB*16`）+ 单次调用超时（Extism `Timeout` + Go context 双层，runtime.go:39, manager.go:420）+ payload 上限 1 MiB（`MaxPayloadBytes`，protocol.go:14）。实例损坏（`broken`）时丢弃并新建一个补回池中（runtime.go:66-77）。
- **宿主接口**：插件只能通过一个 host function `host_call`（`host.go:25-54`）请求能力：`kv.*`（需 `kv` 权限）、`secret.*`（需 `secret`）、`http.request`（需 `http` + origin 白名单）、`notify.publish`（需 `notify`，写系统通知，`host.go:133-155`）、`log.write`（需 `log`，自动脱敏 `sk-...`/`Bearer ...`，`host.go:17,116`）。未知能力直接报错。除 `http.*` 外 host 调用 payload ≤64 KiB（host.go:60）。
- **无独立健康检查/退出机制**：插件没有“进程”概念，谈不上健康检查；失败计入 `failures`/`timeout_count` 指标（manager.go:522-533），加载失败置 `error` 状态。`required=true` 的插件失败会让 relay 链路返回错误（`RequiredError`）。

## 6. UI 集成

### 后端 `GET /api/v1/plugins/:id/ui-bundle`（plugin.go:280-312）
- 要求 `Auth` + `PermPluginsRead`；插件须 `enabled` 且 manifest 含 `ui`；支持 `?version=` 校验版本（防旧 bundle 缓存）。
- 响应头 `Cache-Control: no-store`、`X-Content-Type-Options: nosniff`；注释明确“UI 字节只以鉴权 JSON 返回，绝不作为宿主源 HTML”（plugin.go:279）。
- `readPluginUIBundle`（plugin.go:314-368）用 `os.OpenRoot`（Go 1.24 目录约束 API）遍历 `ui/`，拒绝符号链接、校验路径安全、逐文件限 2 MiB、总量限 20 MiB、按扩展名推 MIME，返回 `{entry, assets{name→{content,mime}}}`（content 以 JSON 字节数组/base64 形式序列化）。
- `plugin_ui_test.go:101-168` 验证：无 `Authorization` 头（包括 `?token=` query）一律 401 且不泄露内容；disabled 插件/旧版本 404；symlink 逃逸被拒。

### 前端加载（`web/src/components/modules/plugin/`）
1. `index.tsx:30-47`：`PluginPage` 生成一次性 `session = crypto.randomUUID()`，调 `getPluginUIBundle(id, version)` 拉 bundle，然后 `createPluginDocument(...)` 生成 srcdoc 文档，塞进 `<iframe sandbox="allow-scripts" referrerPolicy="no-referrer" srcDoc={...}>`（index.tsx:51）。
2. `ui-bundle.ts`：
   - 固定严格 CSP `PLUGIN_UI_CSP`（ui-bundle.ts:6）：`default-src 'none'`，脚本/样式只允许 `'unsafe-inline' data: blob:`，`connect-src 'none'`（插件 UI 不能直接发网络请求）、`frame/worker/object/form/navigate-to` 全禁。
   - `bootstrap()` 在 opaque frame 内运行：以假 origin `https://plugin.invalid/` 为 base 解析资源引用，把 bundle 内资产转成 **data URL**（CSS 里的 `url()`/`@import` 也会被重写，ui-bundle.ts:14-38），移除 `<base>`/`<meta http-equiv>`/`<iframe>` 等危险节点、清掉 `srcset`，再通过 `document.write` 重建文档（首个 CSP meta 由宿主注入，插件无法放宽，ui-bundle.ts:61-67）。
   - 向 frame 注入只读全局 `window.octopus = { pluginId, session, parentOrigin, asset() }`（ui-bundle.ts:39-42）。
3. **插件 UI → 宿主 RPC**：插件 UI 发 `postMessage({type:'octopus:plugin:rpc:v1', pluginId, session, requestId, method, path, body})`；宿主侧 `dispatchPluginRPC`（rpc.ts:29-46）校验 `event.source === iframe.contentWindow`、`event.origin === 'null'`（srcdoc opaque origin）、session 匹配、路径正则白名单（`pluginRPCPath`，rpc.ts:13-18），再调 `pluginRPC()` 打到后端 `/api/v1/plugins/:id/rpc/...`（用的是**当前登录用户**的 Bearer token，`web/src/api/endpoints/plugin.ts:54-57`），结果 postMessage 回 frame。

## 7. RPC 机制

路由（plugin.go:42-44）：`/api/v1/plugins/:id/rpc/*path`，GET/POST 均可，挂 `middleware.Auth()`（注意：**只要求登录**，不挂 `RequirePermission`，插件侧权限在 handler 内检查 manifest 的 `rpc:read`/`rpc:write`）。

处理流程（plugin.go:200-242）：
1. 内置插件直接拒绝；查记录、解析 manifest。
2. GET → 需 manifest 权限 `rpc:read`；其他方法 → `rpc:write`，且 viewer 角色禁写（plugin.go:228-231）。
3. `pluginRPCPayload` 把请求打包成 envelope：`{"method": "...", "path": "/...", "body": <原始 JSON body>}`（GET 无 body；body 必须是合法 JSON、≤1 MiB，plugin.go:244-267）。
4. `Manager.RPC(ctx, id, payload)`（manager.go:445-477）：要求该插件活动、订阅了 `http.rpc` 事件，然后以 `Event: "http.rpc"` 走与事件相同的 `generation.call` 路径（带配置、超时、指标）。

**协议不是 JSON-RPC**，是自定义 JSON envelope：宿主→插件 `CallRequest{api_version, request_id, plugin_id, event, attempt_id, payload, config}`；插件→宿主 `CallResponse{api_version, decision, payload, tag, error}`，decision 只允许 `continue/observe/handled/allow`（protocol.go:17-33, 89-96）。传输是 Extism ABI：请求 JSON 写入 WASM 线性内存，调导出函数 `entry`（默认 `handle`），读出响应 JSON（runtime.go:91-125）。`plugin_ui_test.go:33-58` 验证 GET 不带 body、POST body 原样透传、path 保留。

## 8. 事件系统

`internal/plugin/event/event.go` 只有 12 行：一个全局 `publisher func(string, any) bool`。`Manager.Init` 时 `SetPublisher(m.Publish)`（manager.go:66），业务包 import 这个轻量包即可发事件，避免对 `plugin` 包的循环依赖。

发布方（grep 确认的全部调用点）：
- `channel.health.changed`：`internal/task/key_health.go:154`、`internal/op/channel/channel.go:601`、`internal/op/channel/node_feature.go:53`
- `pool.account.changed`：`internal/relay/poolscheduler/scheduler.go:143`
- `notification.created`：`internal/op/notification/notification.go:54`
- `relay.completed`：`internal/relay/plugin_bridge.go:269`（由 `metrics.go:180` 调用）

两条分发路径：
- **同步链路式** `Invoke`（protocol.go:62 → manager.go:398-443）：relay 改写类事件用它，payload 按 priority 顺序在订阅插件间**串联传递**（上一个插件的输出 payload 是下一个的输入），任一 `required` 插件失败整体失败；无订阅者时**零开销直通**（不序列化，`manager_test.go:155-178` 证明）。
- **异步队列式** `Publish`（manager.go:479-520）：先进容量 256 的 channel，单 goroutine `eventLoop` 消费，失败按 100/200/300ms 退避重试 3 次；队列满则丢弃并 warn。用于观察类事件（`relay.completed`、`channel.health.changed` 等）。

支持事件全集（manifest.go:26-32）：`relay.request.before_route`、`relay.route.filter`、`relay.request.before_upstream`、`relay.response.before_send`、`relay.completed`、`channel.health.changed`、`pool.account.changed`、`notification.created`、`http.rpc`、`plugin.migrate`。

## 9. 安装来源 / 市场

- **只有本地上传**：`POST /api/v1/plugins/install` 接收 multipart 文件（前端 `<input type="file" accept=".octopkg,.zip">`）。代码中**没有** URL 安装、插件市场、注册表或自动更新机制（grep 无相关实现）。
- **校验**：包结构/大小/路径/WASM 魔数/manifest 校验 + 安装后每次加载比对 SHA-256；**无数字签名、无发布者信任链**。安全性完全依赖“只有 admin 能上传”。

## 10. 与推理转发（relay）的关系

`internal/relay/plugin_bridge.go` 把 4 个改写点 + 1 个观察点织入 relay 主链路（`internal/relay/relay.go`）：

| 事件 | 调用点 | 语义 |
|---|---|---|
| `relay.request.before_route` | relay.go:204（解析入站请求后、选模型前） | 插件可改写 `InternalLLMRequest`；宿主保留 `RawRequest`/`RawAPIFormat`/会话等字段的权威权（plugin_bridge.go:52-55），改写后重新 `Validate()` 并按原始出站协议重编码 wire body（`encodePluginRequestWire`，纯改 model 时直接在原 JSON 上换 `model` 字段以保留透传字段，plugin_bridge.go:73-115）。改写后重建 inbound adapter（relay.go:209-221）。失败 → 502 |
| `relay.route.filter` | relay.go:1666（选渠道组时） | 入参 `{group_id, model, candidates}`，插件返回 `exclude_channel_ids`；试图排除未知渠道直接报错（plugin_bridge.go:142-146） |
| `relay.request.before_upstream` | relay.go:685（构建出站请求后、发送前） | 插件可改出站 headers/body；鉴权/Cookie/Host 等 12 个头对插件不可见也不可写（`pluginBlockedUpstreamHeaders`，plugin_bridge.go:158-162），插件只能写 `x-octopus-*`、`content-type`、`accept`（plugin_bridge.go:210-218）；body 以 base64 JSON 传递，≤1 MiB。失败 → `errPluginProcessing`，走错误分类重试逻辑（relay.go:460-468） |
| `relay.response.before_send` | relay.go:1488（**仅非流式**响应路径；流式不经过） | 插件可改写最终响应体（base64、必须是合法 JSON、≤1 MiB），已写响应头后拒绝（plugin_bridge.go:222-259） |
| `relay.completed` | metrics.go:180 → plugin_bridge.go:261-270 | 异步观察事件：`{model, api_key_id, success, error, response, attempt_count}` |

另外 manifest 层面：订阅 `relay.*` 需 `relay:read`/`relay:write`，四个改写事件强制 `relay:write`（manifest.go:124-131）。

## 11. 权限与鉴权

### 管理面（HTTP API，plugin.go:28-45）
| 路由 | 中间件 |
|---|---|
| `GET /api/v1/plugins`、`GET /:id/ui-bundle` | `Auth()` + `RequirePermission(PermPluginsRead)`（admin/editor/viewer 都有读权限，auth/permissions.go:41,53,65） |
| `POST /install`、`POST /:id/enable|disable`、`PUT /:id/config`、`DELETE /:id` | `Auth()` + `requirePluginAdmin`（**仅 admin**，plugin.go:47-54；注意没用 `PermPluginsWrite`，虽然该权限存在且返回给前端做 `can_write`，plugin.go:100） |
| `GET|POST /:id/rpc/*path` | 仅 `Auth()`；再由 manifest 的 `rpc:read`/`rpc:write` + viewer 禁写把关 |

错误消息统一经 `pluginRedact` 脱敏（替换 EncryptionKey/JWTSecret，plugin.go:374-376）。

### 插件自身权限声明
manifest `permissions` 白名单（§3.1），在三个层面强制执行：host function 能力分发（host.go:70-79, 102-105, 133-136）、HTTP 出栈 origin 白名单（host_http.go:39-49）、RPC 端点（plugin.go:219-227）。

## 12. 测试与示例

- `internal/plugin/manager_test.go`：generation 替换不中断在途请求、retire 语义；无订阅者时大 payload 直通不报错也不序列化；有订阅者时 1 MiB 上限生效；`Handles` 在高并发 swap 下与快照一致。
- `internal/plugin/runtime_test.go`：manifest 校验与默认值（100ms/16MiB/4 实例）；注释说明 wazerotest 构造的模块不含 Extism guest ABI，完整宿主 ABI 集成测试是 placeholder。
- `internal/server/handlers/plugin_ui_test.go`：RPC envelope 行为、UI bundle 资产/大小/路径/symlink 限制、鉴权 + enabled + version 约束。
- `internal/server/handlers/node_feature_test.go`：内置 Magpie 节点伪插件的列表/开关行为。
- `web/src/components/modules/plugin/ui-bundle.test.ts`：srcdoc 中 CSP meta 先于不可信内容、`</script>` 转义防逃逸、sandbox 属性存在。
- `web/src/components/modules/plugin/rpc.test.ts`：消息校验/路径白名单/session 匹配。
- `internal/plugin/testdata/`：**空目录**，仓库内无示例插件包或示例 WASM。

## 13. 限制与安全小结

- 插件是**同进程 WASM**：隔离强（内存/超时/payload 限额、能力白名单、SSRF 防护），但崩溃/资源滥用仍可能影响主进程稳定性（wazero 提供沙箱兜底）。
- 无签名/市场，安装信任锚 = admin 账号；`requirePluginAdmin` 是硬边界。
- 卸载需重启才真正删盘；状态机把 `uninstalling` 期间的重装/启停全部拒绝。
- UI 是三重隔离：sandboxed iframe（`sandbox="allow-scripts"`，无 `allow-same-origin`）+ 严格 CSP（`connect-src 'none'` 等）+ data URL 资产 + postMessage 会话校验（origin 必须为 `'null'`、source 必须是指定 frame、session UUID 匹配）。插件 UI 的网络能力被 CSP 完全剥夺，所有数据必须经宿主 RPC 中转，而宿主 RPC 携带的是当前用户的凭据并受 manifest `rpc:*` 权限约束。
- 事件队列容量 256，满即丢弃（仅 warn）；异步事件最多重试 3 次。
- `relay.response.before_send` 不覆盖流式响应——流式内容改写/审计是明确缺口（relay.go:708-717 的流式分支无插件钩子）。

## 14. 未确认项

1. `ConfigSchema`（manifest `config_schema`）仅校验为合法 JSON，未见后端用它校验 `PUT /:id/config` 的内容；前端是否渲染 schema 表单未确认（`index.tsx` 只是 textarea 编辑原始 JSON）。
2. 是否存在官方示例插件/插件开发 SDK（仓库 `testdata/` 为空，未在仓库内发现插件侧 guest 代码模板）——未确认。
3. `CallResponse.Tag` 字段已定义（protocol.go:31）但未找到任何消费方——未确认其语义。
4. 插件 UI 内是否有官方 JS SDK 包装 `window.octopus` + postMessage RPC（前端只提供传输层，未确认插件侧辅助库）。
5. `Priority` 相同事件多插件串联时，`Tag`/`decision` 对 relay 行为是否有差异化影响（代码中 `validateDecision` 只校验合法性，relay 桥接代码未读 `decision`）——decision 目前看是纯声明式字段，未确认未来语义。
