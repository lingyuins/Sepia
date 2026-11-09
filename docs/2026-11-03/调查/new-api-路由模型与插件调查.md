# QuantumNous/new-api：路由模型与插件系统调查

- 调查对象：`D:\CodeWorkspace\new-api`（Go + Gin 的 AI 网关）。
- 调查方式：只读源码与仓库内文档；未执行构建、测试或网络请求，未修改该仓库。
- 证据口径：下文 `文件:行号` 均相对仓库根目录。路由与管理 API 以代表性端点为主，插件协议与请求级选路尽量展开到具体类型、函数和字段。

## 1. 概览与技术栈

### 1.1 项目定位

New API 是自托管 AI 网关，把多个厂商的模型服务聚合到统一入口，在同一控制台中管理渠道、访问权限、用量和成本。它不是单纯的 OpenAI 反向代理，核心包括：

1. **模型聚合与请求级路由**：按 Token/用户分组、模型名、渠道优先级与权重选择上游；支持渠道亲和、多 Key、重试和自动禁用。
2. **多协议接入与转换**：OpenAI Chat Completions / Responses、Anthropic Messages、Gemini，另有图片、音频、Embedding、Rerank、Midjourney 和异步任务协议。
3. **计费与配额**：钱包、API Token 配额、订阅套餐；支持旧倍率/按次价格和表达式定价，含缓存、图片、音频、任务用量事实。
4. **管理控制台与访问控制**：用户、分组、渠道、模型、日志、支付、OAuth/OIDC、WebAuthn、TOTP、Casbin 权限。
5. **JavaScript 任务插件**：在 Go 进程内执行厂商任务驱动，支持原生厂商 URL、OpenAI Responses/Video/Image 协议、任务轮询、产物下载和结算。

依据：`README.zh_CN.md:59-63`、`README.zh_CN.md:134-157`、`AGENTS.md:5-25`。

### 1.2 技术栈

| 层 | 技术与证据 |
|---|---|
| 主后端 | Go `1.26.10`、Gin `1.9.1`、GORM `1.25.12`；`go.mod:1-4`、`go.mod:18-22`、`go.mod:61-64` |
| 前端 | React `19.2.7`、TypeScript、Rsbuild、TanStack Router/Query/Table、Zustand、Tailwind CSS 4、Base UI；`web/package.json:26-48`、`web/package.json:65-87`、`web/package.json:89-110` |
| 包管理/构建 | Bun `1.4.4`；`web/package.json:7-24`、`web/package.json:126` |
| 独立协议模块 | `relaykit/` 是独立 Go 模块，负责四种文本协议 DTO、请求/响应/流式转换；HTTP、鉴权、渠道、数据库、计费留在宿主；`relaykit/README.md:1-5` |
| Token 模块 | `tokenkit/` 独立负责文本与图片 Token 计数/估算；`AGENTS.md:22-24` |
| 数据存储 | SQLite/MySQL/PostgreSQL；独立日志库另支持 ClickHouse；Redis + 内存缓存；`README.zh_CN.md:223-230` |
| 插件运行时 | `github.com/Calcium-Ion/moejs v0.1.0-alpha.7`、`github.com/MoeForge/moego v0.1.0-alpha.1`；`go.mod:168-173` |
| 其他关键依赖 | AWS SDK Bedrock、Casbin、OIDC、WebAuthn、JWT、gorilla/websocket、expr-lang/expr、Stripe；`go.mod:11-17`、`go.mod:27-30`、`go.mod:44-45`、`go.mod:110` |
| 桌面封装 | Electron 42 + electron-builder，包装 Go 二进制，支持 Windows/macOS/Linux 和系统托盘；`electron/README.md:1-4`、`electron/package.json:26-30` |

## 2. 页面与 HTTP 路由

### 2.1 后端路由注册与全局中间件

`router.SetRouter` 的注册顺序是管理 API → 旧 dashboard 兼容接口 → Relay → 任务插件宿主协议 → 视频 → 通用任务 → 插件动态路由 → Web/NoRoute（`router/main.go:15-40`）。主进程使用 `gin.New()`，挂载 PanicRecover、RequestId、Version、I18n 和日志，再注册路由（`main.go:179-199`）。Relay 全局另有 CORS、请求解压、请求体清理和统计中间件（`router/relay-router.go:15-19`）。

| 前缀/入口 | 代表性端点 | 鉴权与处理 |
|---|---|---|
| `/api` | `GET /api/status`、`GET/POST /api/setup`、`GET /api/pricing` | 管理 API 全局 gzip、AccessTokenAudit、BodyStorageCleanup、GlobalAPIRateLimit；公开、UserAuth、AdminAuth、RootAuth 分级；`router/api-router.go:17-47` |
| `/api/user` | 登录、注册、刷新/退出、当前用户、支付、Passkey/2FA | `UserAuth` 保护自助接口，用户管理走 `AdminAuth`；`router/api-router.go:75-174` |
| `/api/channel` | CRUD、测试、模型抓取、状态、多 Key、优先级排序 | `AdminAuth` + Casbin `RequirePermission`；读取渠道明文 Key 还需 Root、限流、禁用缓存和安全验证；`router/channel-router.go:20-55`、`router/channel-router.go:57-102` |
| `/api/token` | Token CRUD、批量删除、读取 Key | `UserAuth` + TokenOperationAudit；`router/api-router.go:283-297` |
| `/api/subscription` | 套餐、当前订阅、购买；`/api/subscription/admin` 管理 | 用户/管理员分组；`router/api-router.go:176-205` |
| `/api/option` | 系统配置、请求策略、模型价格、渠道亲和缓存 | `RootAuth`；`router/api-router.go:212-234` |
| `/api/plugin/task` | 插件列表/上传/版本/激活/启停/dry-run/图标/市场源 | `RootAuth`；`router/api-router.go:263-279` |
| `/api/log`、`/api/data`、`/api/audit` | 用量日志、统计、导出、审计 | 用户查看自身，管理员查看全局；`router/api-router.go:299-356` |
| `/api/models`、`/api/vendors`、`/api/deployments` | 模型元数据、厂商、部署 | `AdminAuth`；`router/api-router.go:388-440` |
| 旧 dashboard | `GET /dashboard/billing/subscription`、`GET /v1/dashboard/billing/usage` | `TokenAuth`，兼容旧客户端；`router/dashboard.go:10-22` |
| `/v1/models` | `GET /v1/models`、`GET /v1/models/:model` | `TokenAuth`；按 Claude/Gemini/OpenAI 头部选择列表格式；`router/relay-router.go:21-44` |
| `/v1` 文本/多模态 | `POST /chat/completions`、`/messages`、`/responses`、`/embeddings`、`/rerank` | `TokenAuth` → `ModelRequestRateLimit` → `Distribute` → `controller.Relay`；`router/relay-router.go:71-180` |
| `/v1` 音视频 | `/audio/speech`、`/audio/transcriptions`、`/audio/translations`、图片 `/edits` | 同上；图片生成/编辑宿主协议在另一注册器；`router/relay-router.go:127-148` |
| `/v1` 其他 | `/completions`、`/responses/compact`、`/alpha/search`、`/tools/search`、`/tools/fetch`、`/moderations` | 同上；`router/relay-router.go:101-125`、`router/relay-router.go:163-166` |
| `/v1beta` | `POST /v1beta/models/*path` | Gemini；`TokenAuth`、限流、选渠道；`:predictLongRunning` 先尝试插件；`router/relay-router.go:193-221` |
| `/pg` | `POST /pg/chat/completions` | 控制台 Playground，使用 `UserAuth` 而非 API Token；`router/relay-router.go:64-70` |
| `/mj`、`/:mode/mj` | `/submit/imagine`、`/submit/action`、`/task/:id/fetch` 等 | `TokenAuth` + `Distribute`；图片代理 `GET /mj/image/:id` 在鉴权之前注册；`router/relay-router.go:182-190`、`router/relay-router.go:224-244` |
| `/v1/tasks` | `POST /v1/tasks/:key`、`GET /v1/tasks/:key`、artifacts 与 content | 通用任务插件 API；`TokenAuth`，产物内容也接受签名产物访问凭证；`router/task-router.go:9-37` |
| `/v1/responses` | POST create、GET `/:response_id`、GET WebSocket | create 先走宿主插件协议判定，未认领模型回退普通 Relay；`router/task-plugin-protocol-router.go:27-36`、`router/task-plugin-protocol-router.go:51-52`、`router/relay-router.go:76-79` |
| `/v1/videos` | POST create、GET `/:task_id`、GET/HEAD `/:task_id/content`、`POST /:video_id/remix` | OpenAI Video 宿主协议/任务插件；`router/task-plugin-protocol-router.go:45-56`、`router/video-router.go:26-32` |
| `/v1/video/generations` | POST 创建、GET `/:task_id` | 另一视频任务入口；`router/video-router.go:10-30` |
| `/v1/images` | `POST /v1/images/generations`、`POST /v1/images/edits` | 插件 `openai_image` 宿主协议，未认领模型回退普通 Image Relay；`router/task-plugin-protocol-router.go:37-44` |
| 插件原生 URL | 厂商自己的路径及 `/{pluginKey}` 前缀副本 | 动态 inner Gin；NativeRouteCredential → TokenAuth → 性能/限流 → PrepareTaskPluginRoute → Distribute → RelayTask；`router/plugin-router.go:108-157` |
| Web/NoRoute | 静态资源与 SPA index | 先尝试插件动态路由，再静态文件，未命中 `/v1`、`/api`、`/assets` 返回 API 404，其余回退 `index.html`；`router/web-router.go:21-46` |

补充：`/api/authz/catalog` 暴露权限目录（`router/authz-router.go:13-18`）。`/v1/files`、fine-tune 等只是显式 `501 Not Implemented`，不是已实现的上游转发（`router/relay-router.go:168-179`）。

### 2.2 鉴权中间件要点

- `UserAuth` / `AdminAuth` / `RootAuth` 最终都走 `authHelper`，支持浏览器会话和 scoped PAT，检查用户状态与角色，并对管理写操作留审计（`middleware/auth.go:52-96`、`middleware/auth.go:132-148`）。Casbin 权限由 `RequirePermission` 独立执行（`middleware/auth.go:410-419`）。
- Relay 的 `TokenAuth` 接受 Bearer，也兼容 Anthropic `x-api-key`、Gemini `x-goog-api-key`/`?key=`、Midjourney `mj-api-secret` 和 WebSocket 子协议凭证；验证 Token 状态、过期、余额、IP 白名单和用户状态（`middleware/auth.go:528-646`）。
- Token 可覆盖用户分组，但必须属于用户可用分组且仍存在于倍率配置；`auto` 是特例（`middleware/auth.go:622-639`）。
- 管理员的 API Key 若采用 `<key>-<channelId>` 形式，可生成最高等级 Token pin；普通用户禁止指定渠道（`middleware/auth.go:703-721`）。
- `Distribute` 还执行 Token 模型白名单、模型必填和 Playground 分组校验；失败通过 `abortWithOpenAiMessage` 终止，不进入上游（`middleware/distributor.go:48-99`）。

### 2.3 前端页面与路由

前端没有 `web/src/pages` 目录；当前页面实现主要在 `web/src/features`，路由在 `web/src/routes`。`routeTree.gen.ts` 明确是 TanStack Router 自动生成的路由树，不应手改（`web/src/routeTree.gen.ts:7-9`）。受保护布局在 `beforeLoad` 中解析会话，无用户或 accessToken 时跳 `/sign-in`（`web/src/routes/_authenticated/route.tsx:25-42`）。

| 页面路径 | 功能/实现区域 |
|---|---|
| `/` | 首页，`features/home` |
| `/sign-in`、`/sign-up`、`/register`、`/forgot-password`、`/reset`、`/otp`、`/oauth`、`/user/reset` | 认证与账号恢复，`features/auth` |
| `/setup` | 初始化向导，`features/setup` |
| `/pricing`、`/pricing/$modelId` | 公共模型价格与详情，`features/pricing`、`features/model-pricing` |
| `/rankings` | 用量排行，`features/rankings` |
| `/about`、`/privacy-policy`、`/user-agreement` | 关于与法律页面 |
| `/dashboard`、`/dashboard/$section` | 数据看板，`features/dashboard` |
| `/channels` | 渠道管理，`features/channels` |
| `/keys` | API Token 管理，`features/keys` |
| `/models`、`/models/$section` | 模型管理/广场，`features/models` |
| `/playground`、`/chat/$chatId`、`/chat2link` | 调试与聊天，`features/playground`、`features/chat` |
| `/usage-logs`、`/usage-logs/$section`、`/usage-logs/audit` | 用量与审计日志，`features/usage-logs` |
| `/users`、`/redemption-codes` | 用户与兑换码管理 |
| `/wallet`、`/subscriptions` | 充值与订阅，`features/wallet`、`features/subscriptions` |
| `/profile`、`/security` | 个人资料、会话/PAT/Passkey/2FA 等，`features/profile`、`features/security` |
| `/task-plugins` | 插件列表、上传、版本、市场，`features/task-plugins` |
| `/system-settings/{auth,billing,content,models,operations,request-policies,security,site}` | 系统设置及各自 `$section` |
| `/system-info` | 多实例/系统信息，`features/system-info` |
| `/401`、`/403`、`/404`、`/500`、`/503`、`/errors/$error` | 错误页，`features/errors` |

路由文件和生成树的对应关系可见 `web/src/routeTree.gen.ts:11-74`、`web/src/routeTree.gen.ts:76-300`。Feature 目录还包括 `performance-metrics` 与 `system-update`。

### 2.4 是否内嵌、是否有桌面端

- **Web 内嵌到 Go 二进制**：`main.go` 用 `//go:embed web/dist` 和 `//go:embed web/dist/index.html`（`main.go:43-47`），再组成 `router.WebAssets`（`main.go:195-199`）。`SetWebRouter` 从嵌入 FS 提供静态文件与 SPA 回退（`router/web-router.go:15-45`）。因此必须先构建前端（`README.zh_CN.md:251-264`）。
- **有 Electron/Desktop**：`electron/main.js` 是入口，生产打包把 Go 二进制放进 `extraResources`，Windows 还支持 NSIS 与 portable（`electron/package.json:5-12`、`electron/package.json:31-45`、`electron/package.json:85-116`）。开发时分别运行 Go、Rsbuild 和 Electron（`electron/README.md:25-44`）。

## 3. 请求级路由模型

### 3.1 核心数据模型与关系

| 类型 | 关键字段 | 在选路/计费中的作用 |
|---|---|---|
| `User` | `Id`、`Group`、`Quota`、`UsedQuota`、`Role`、`Status`、`Setting` | 用户默认分组与钱包；订阅可改变分组；`model/user.go:79-115` |
| `Token` | `UserId`、`Key`、`Group`、`RemainQuota`、`UnlimitedQuota`、`ModelLimits*`、`AllowIps`、`CrossGroupRetry`、`AutoGroups` | API 凭证与第二层配额/模型/IP/分组限制；`model/token.go:14-35` |
| `Channel` | `Type`、`Key`、`Status`、`Models`、`Group`、`Priority`、`Weight`、`ModelMapping`、`BaseURL`、`AutoBan`、`Setting`、`ChannelInfo` | 一个上游连接及路由配置；`model/channel.go:23-60` |
| `ChannelInfo` | `IsMultiKey`、`MultiKeyStatusList`、`MultiKeyPollingIndex`、`MultiKeyMode` | 多 Key 状态、禁用原因和轮询游标；`model/channel.go:64-72` |
| `Ability` | 复合主键 `(Group, Model, ChannelId)`、`Enabled`、`Priority`、`Weight`、`Tag` | Channel × Group × Model 的物化路由索引；`model/ability.go:18-26` |
| `SubscriptionPlan` | `TotalAmount`、`UpgradeGroup`、`DowngradeGroup`、`AllowWalletOverflow`、重置周期 | 套餐额度、分组升降级与钱包溢出策略；`model/subscription.go:145-190` |
| `UserSubscription` | `UserId`、`PlanId`、`AmountTotal`、`AmountUsed`、有效期、`UpgradeGroup`、`PrevUserGroup` | 用户实际订阅实例与配额消耗；`model/subscription.go:252-281` |
| `Option` | `Key`、`Value` | 系统配置与旧定价 JSON map；含 `RetryTimes`、自动禁用、倍率等；`model/option.go:21-24`、`model/option.go:146-191` |
| `Pricing` | `ModelRatio`、`ModelPrice`、缓存/图片/音频倍率、`BillingMode`、`BillingExpr`、`BillingPluginVariants` | 对前端展示的有效价格与端点能力；`model/pricing.go:17-51` |

关系可以概括为：

```text
User 1 ── N Token
User N ── 1（当前默认）Group（字符串逻辑分组，不是本报告发现的独立 GORM Group 实体）
Channel N ── M Model / Group，经 Ability 展开为 (group, model, channel_id) 路由行
User 1 ── N UserSubscription N ── 1 SubscriptionPlan
Token/User/Subscription 分别约束请求准入和资金来源；Ability 只决定“哪些 Channel 可用”
```

Channel 的 `Models` 与 `Group` 都是逗号分隔字符串，保存/更新时展开成 Ability；Ability 复制 Channel 的启用状态、优先级、权重和 Tag（`model/ability.go:225-263`、`model/ability.go:270-339`）。优先级拖拽排序同时更新 `channels.priority` 与 `abilities.priority`，并用 revision 防止并发覆盖（`model/channel_priority.go:58-105`）。

订阅不是 Ability 的直接成员，但可通过 `UpgradeGroup` 改变用户分组，从而间接改变可选渠道（`model/subscription.go:484-557`）。配额预扣按最早到期的有效订阅顺序尝试，并以 `request_id` 做幂等记录（`model/subscription.go:1303-1405`）。

### 3.2 候选索引、过滤器与权重算法

有两条等价目标但公式略有差异的选路实现：

1. **内存缓存路径**：`group2model2channels[group][model]` 保存启用 Channel ID，`channelsIDM` 保存所有 Channel。初始化时按优先级降序排列（`model/channel_cache.go:20-79`）。
2. **数据库路径**：未启用内存缓存时直接查询 `abilities`，按 `priority DESC, weight DESC` 取候选（`model/ability.go:108-173`）。

共同规则：

- 先精确模型名，未命中再尝试 `ratio_setting.RoutingMatchModelName` 归一化名称（`model/channel_cache.go:131-138`、`model/ability.go:120-128`）。
- 过滤器求值顺序固定为 `request_path` → `task_plugin_endpoint` → `task_plugin_identity` → `responses_websocket`（`model/channel_constraint.go:10-35`）。这覆盖高级自定义渠道路径、插件身份/类型、Responses WebSocket 能力（`model/channel_constraint.go:91-139`）。
- 第 N 次重试选择第 N 个不同的优先级层（高到低）；超过层数后钳到最低层，而不是直接无渠道（`model/channel_cache.go:151-168`、`model/ability.go:129-149`）。
- 固定 Token pin 优先于亲和与随机；origin task pin 次之。`ResolvedPin` 按 Rank 选赢家，同一 Channel 的多个 pin 合并更严格的 RetryMode（`dto/channel_constraints.go:75-120`）。
- 严格会话亲和不可用时直接失败；普通 `prefer` 可回退随机选路（`service/channel_select.go:329-367`）。

**内存路径权重**：同优先级层内，若全为 0，则每个有效权重为 100；若平均权重小于 10，则整体乘 100，再随机抽取（`model/channel_cache.go:170-214`）。

**数据库路径权重**：每个 Ability 的实际抽取权重是 `Weight + 10`，所以 0 权重也有基础机会（`model/ability.go:151-167`）。两种实现的零权重/小权重细节不能完全等同。

### 3.3 渠道亲和（Channel Affinity）

- 规则按模型正则、路径正则、User-Agent 包含条件匹配；Key 可来自 `context_int`、`context_string`、请求头或 JSON 的 `gjson` 路径（`service/channel_affinity.go:291-337`、`service/channel_affinity.go:550-590`）。
- Cache key 可由规则名、模型、使用分组和亲和值组成；TTL 取规则值或全局默认（`service/channel_affinity.go:339-351`、`service/channel_affinity.go:591-628`）。
- 首次尝试优先使用缓存的 Channel ID，并再次检查渠道启用、过滤器和分组/模型能力（`service/channel_select.go:332-359`）。
- `strict` 在亲和失败时禁止重试，`prefer` 可换渠道；亲和尝试本身消耗一次尝试，后续从正常优先级层开始时减 1 偏移（`service/channel_affinity.go:633-648`、`service/channel_affinity.go:683-709`）。
- 成功请求由 `Distribute` 尾部调用 `RecordChannelAffinity` 写入 HybridCache；`SwitchOnSuccess` 可改为记录最终成功渠道（`middleware/distributor.go:133-138`、`service/channel_affinity.go:729-758`）。
- 规则还支持 `ParamOverrideTemplate`，与渠道参数覆盖合并，并在管理日志中记录来源与指纹而非完整亲和值（`service/channel_affinity.go:532-547`、`service/channel_affinity.go:711-727`）。控制器只提供统计/清理接口（`controller/channel_affinity_cache.go:12-54`）。

### 3.4 `/v1/chat/completions` 到 upstream：选路全流程

| 步骤 | 代码与动作 | 结果/分支 |
|---|---|---|
| 1. 路由与鉴权 | `POST /v1/chat/completions` 绑定 `RelayFormatOpenAI`，依次执行 `SystemPerformanceCheck`、`TokenAuth`、`ModelRequestRateLimit`、`Distribute`（`router/relay-router.go:71-107`）。 | Token/用户/IP/额度或限流失败则终止。 |
| 2. 解析路由模型 | `getModelRequest` 从 JSON body 读取唯一顶层 `model`；也检查其类型并保留可重放 body（`middleware/distributor.go:285-320`、`middleware/distributor.go:390-481`）。 | 无模型或重复/非法模型字段则 400。 |
| 3. Token 与分组校验 | 校验 Token 模型白名单；`TokenAuth` 已确定 `usingGroup`，可用 Token.Group 覆盖用户默认组（`middleware/distributor.go:53-98`、`middleware/auth.go:622-642`）。 | 禁止模型或分组则 403。 |
| 4. 组装约束 | 添加 request path、任务插件身份等过滤器，再解析是否已有 pin（`middleware/distributor.go:42-48`）。 | 普通 Chat 通常只有 request path；共享插件端点会有更多约束。 |
| 5. 首次选 Channel | `SelectChannelForRequest` 先 ResolvedPin，再首次会话亲和，最后 `CacheGetRandomSatisfiedChannel`（`service/channel_select.go:297-396`）。 | 无候选返回 503；pin 禁用返回 400/403。 |
| 6. 按 model+group 抽候选 | 内存或 DB 查 Ability，按 retry 选择优先级层，再按权重随机（`model/channel_cache.go:117-217`、`model/ability.go:108-173`）。 | `auto` 分组按配置顺序逐组尝试，可启用跨组重试（`service/channel_select.go:114-207`）。 |
| 7. 写入渠道上下文 | `SetupContextForSelectedChannel` 写入 Channel ID/Type/BaseURL/设置/模型映射/参数与 Header 覆盖（`middleware/distributor.go:586-669`）。 | 插件身份不符会失败。 |
| 8. 选上游 Key | 同一函数调用 `GetNextEnabledKey`：多 Key 可随机或轮询，跳过已禁用 Key，并把 Key/索引写入上下文（`middleware/distributor.go:671-684`、`model/channel.go:211-295`）。 | 无可用 Key 返回渠道错误；全 Key 禁用可令整个 Channel 自动禁用。 |
| 9. 请求 DTO 与 RelayInfo | `controller.Relay` 调用 `GetAndValidateRequest`，再 `GenRelayInfo` 生成协议、原始模型、分组、用户与计费上下文（`controller/relay.go:123-138`、`relay/helper/valid_request.go:22-60`）。 | 非法请求 400/413。 |
| 10. 预估价与预扣 | `PrepareRequestBilling` 估算输入 Token，`ModelPriceHelper` 选择旧倍率/固定价/表达式，再 `PreConsumeBilling` 创建 BillingSession（`relay/request_billing.go:22-68`）。 | 价格缺失或余额/Token/订阅不足则失败；免费模型可跳过。 |
| 11. 初始化本次尝试 | 进入 `for retry <= common.RetryTimes`，`getChannel` 首次复用中间件已选 Channel 的快照，并 `BeginAttempt`（`controller/relay.go:167-188`、`controller/relay.go:298-329`）。 | 后续重试重新走 CacheGetRandomSatisfiedChannel。 |
| 12. 模型映射与适配器 | `TextHelper.InitChannelMeta` 将 ChannelType 映射 ApiType；`ModelMappedHelper` 支持链式 model_mapping 和循环检测；`GetAdaptor` 按 ApiType 选 OpenAI/Claude/Gemini/AWS 等适配器（`relay/compatible_handler.go:25-74`、`relay/helper/model_mapped.go:14-68`、`relay/relay_adaptor.go:50-128`）。 | `OriginModelName` 用于路由/计费，`UpstreamModelName` 用于实际上游。 |
| 13. 转换与参数处理 | 可透传原 body，否则调用 Adaptor 转换；依次移除禁用字段、应用 ParamOverride，并记录协议转换链（`relay/compatible_handler.go:98-150`）。 | 可经 RelayKit 把 OpenAI Chat 转成其他上游协议。 |
| 14. 构造并发送 HTTP | `GetRequestURL` 决定 URL，OpenAI 普通渠道拼接 BaseURL + 原请求路径；Claude/Gemini 到 OpenAI 上游时改为 `/v1/chat/completions`（`relay/channel/openai/adaptor.go:114-190`）。`DoApiRequest` 设置适配器 Header、HeaderOverride、代理后发送（`relay/channel/api_request.go:323-349`、`relay/channel/api_request.go:457-513`）。 | 实际上游地址依 Channel 而定；OpenAI 示例最终为 `{baseURL}/v1/chat/completions`。 |
| 15. 响应转换与结算 | 非 200 进入统一错误处理；成功由 Adaptor `DoResponse` 输出统一 Usage，再 `PostTextConsumeQuota` 计算实际 quota、更新用量、写消费日志并 `SettleBilling`（`relay/compatible_handler.go:152-186`、`service/text_quota.go:400-468`、`service/text_quota.go:545-560`）。 | 实际费用与预扣差额补扣或退款。 |
| 16. 失败重试/禁用 | `DecideRelayRetry` 综合 strict affinity、pin、skip-retry 标记、剩余次数和状态码；`ProcessChannelError` 在命中禁用条件且 AutoBan 时异步禁用（`controller/relay.go:217-233`、`service/relay_error.go:18-80`、`service/channel.go:29-80`）。 | 重试到下一优先级/分组；否则返回最终错误。 |
| 17. 成功亲和 | `Distribute.c.Next` 返回后，若 Channel 非空且 HTTP 状态 < 400，记录亲和（`middleware/distributor.go:133-138`）。 | 后续同亲和 Key 可优先粘到成功渠道。 |

注意：默认 `RetryTimes = 0`，即不额外重试（`common/constants.go:139-143`）。默认自动禁用状态码仅为 401，且总开关 `AutomaticDisableChannelEnabled = false`（`setting/operation_setting/status_code_ranges.go:17-18`、`common/constants.go:133-135`）。默认重试状态码覆盖 1xx、3xx、401-407、409-499、500-503、505-523、525-599，但 504/524 永远排除（`setting/operation_setting/status_code_ranges.go:19-38`、`setting/operation_setting/status_code_ranges.go:70-85`）。

### 3.5 多 Key 与状态禁用

- Key 支持换行分隔，特殊情况下也支持 JSON 数组；多 Key 才能进入 Random/Polling 模式（`model/channel.go:187-209`）。
- Polling 模式通过每 Channel 锁和 `MultiKeyPollingIndex` 实现轮询，随机模式只从 Enabled Key 中随机（`model/channel.go:224-294`）。
- 禁用某个 Key 会更新其状态、原因和时间；如果仍有其他 Key，整个 Channel 可保持启用。全部 Key 不可用时 Channel 变为 `ChannelStatusAutoDisabled`（`model/channel.go:678-727`）。
- Channel 状态同步更新缓存与 Ability；禁用时从 `group2model2channels` 移除（`model/channel.go:743-821`、`model/channel_cache.go:251-274`）。状态常量：Enabled=1、手动禁用=2、自动禁用=3（`common/constants.go:272-274`）。
- 渠道测试成功时可按 `AutomaticEnableChannelEnabled` 恢复自动禁用渠道，但该开关默认也关闭（`service/channel.go:82-92`、`common/constants.go:133-135`）。

### 3.6 多协议转发结构

`channel.Adaptor` 统一约定初始化、URL、Header、各协议请求转换、DoRequest、DoResponse 和模型列表（`relay/channel/adapter.go:18-35`）。`relayHandler` 按 RelayMode 分派（`controller/relay.go:35-59`）：

| 协议/能力 | 宿主 handler | 要点 |
|---|---|---|
| OpenAI Chat | `TextHelper` | 模型映射、Adaptor 转换/透传、参数覆盖、Usage 结算；`relay/compatible_handler.go:25-186` |
| Anthropic Messages | `ClaudeHelper` | Claude DTO 与系统提示处理，可转其他协议；`relay/claude_handler.go:22-156` |
| Gemini | `GeminiHelper` / `GeminiEmbeddingHandler` | generateContent 与 embedding/batch；`relay/gemini_handler.go:22-260` |
| OpenAI Responses | `ResponsesHelper` | Responses 与 compact，compact 只转发允许字段；`relay/responses_handler.go:18-100` |
| Images | `ImageHelper` | JSON/multipart，按实际上游 `n` 预留，按响应图片数量结算；`relay/image_handler.go:24-196` |
| Audio | `AudioHelper` | speech/transcription/translation，按音频或文本 Usage 结算；`relay/audio_handler.go:18-77` |
| Embedding / Rerank | `EmbeddingHelper` / `RerankHelper` | 各自 DTO 转换后统一文本计费；`relay/embedding_handler.go:19-93`、`relay/rerank_handler.go:20-105` |
| Realtime / Responses WS | `WssHelper` / Responses WebSocket | WebSocket 上游也走渠道、认证、重试；`/v1/realtime` 在 query 中取模型（`middleware/distributor.go:482-485`）。 |
| Midjourney | `RelayMidjourney*` | `/mj` submit/fetch/image 等独立任务路径；`controller/relay.go:336-384` |
| 任务插件 | `RelayTask` / 协议桥 | JS TaskAdaptor，统一提交、落库、轮询、产物与结算；见第 4 节。 |

RelayKit 支持 OpenAI Chat、Responses、Claude、Gemini 四种文本协议的请求、非流式响应与流式事件互转；`ConvertRequest`/`ConvertResponse` 自动选直接或多跳路径，并返回 Steps、Quality、Diagnostics（`relaykit/README.md:1-24`）。缓存 Token、工具调用、多模态和 Usage 也经统一 DTO（`relaykit/README.md:26-48`）。

### 3.7 计费、配额与订阅

#### 旧模式

- `ModelPrice`：按次费用，预扣为 `modelPrice × QuotaPerUnit × groupRatio × OtherRatios`（`relay/helper/price.go:175-203`）。
- `ModelRatio`：预扣主要按估算输入 Token × 输入预扣倍率 × modelRatio × groupRatio（`relay/helper/price.go:93-126`）。
- 文本实际结算把普通输入、缓存读取/写入、图片、输出分别乘倍率，再乘 model/group ratio 与其他倍率，另外可加工件调用附加费（`service/text_quota.go:285-379`）。
- 音频有独立 AudioRatio/AudioCompletionRatio 或特定输入单价路径（`service/quota.go:49-85`）。

#### 表达式模式

`billing_setting.billing_mode = tiered_expr` 时，`ModelPriceHelper` 编译/执行表达式并冻结 `BillingSnapshot`（`relay/helper/price.go:344-455`）。

- 普通 token 表达式系数是美元/百万 Token，换算为 `rawCost / 1,000,000 × QuotaPerUnit × groupRatio`（`relay/helper/price.go:396-412`）。
- 变量包括 `p/c/len`、缓存 `cr/cc/cc1h/cc12h`、图片 `img/img_o`、音频 `ai/ao`、固定请求价 `fixed()` 和任务 `u("fact")`；`p/c` 只排除表达式显式单独定价的类别（`pkg/billingexpr/expr.md:27-75`）。
- 任务表达式直接返回美元，不再除一百万：`task quota = expression output USD × QuotaPerUnit × groupRatio`（`pkg/billingexpr/expr.md:363-373`）。
- 提交时按估算事实预扣，完成时把实际 facts 覆盖到冻结快照后重算，差额补扣/退款（`service/task_billing.go:437-452`、`service/task_polling.go:692-734`）。
- 多插件共享模型时，价格查找顺序为 `<plugin>::<clientModel>` → 同插件 mapped model → client model 表达式 → mapped model 表达式（`pkg/billingexpr/expr.md:288-302`）。

#### BillingSession 与资金来源

`BillingSession` 封装预扣、追加预留、结算、退款，资金来源和 Token 配额分两步提交，防止重复退款（`service/billing_session.go:28-84`、`service/billing_session.go:87-150`）。资金来源按用户 `BillingPreference` 支持 `subscription_only`、`wallet_only`、`wallet_first`、`subscription_first`；默认订阅优先，无有效订阅用钱包，订阅不足是否溢出到钱包受套餐限制（`service/billing_session.go:388-490`）。普通钱包高余额可有信任旁路，但异步任务和订阅不使用（`service/billing_session.go:328-362`）。

任务提交强制 `ForcePreConsume`，先全额预留；任务持久化成功后结算，失败任务走退款，异步完成可差额结算（`relay/relay_task.go:336-343`、`controller/relay.go:635-760`、`service/task_billing.go:260-377`）。

`controller/channel-billing.go` 是另一条“查询上游余额”的管理路径，不是请求计费本身：支持 OpenAI 兼容、DeepSeek、OpenRouter、Moonshot、Sub2API、New API 等；Task Plugin 与多 Key 渠道不支持该查询（`controller/channel-billing.go:624-735`）。

## 4. 插件系统

> 与 octopus（WASM/Extism）和 magpie（Bun/OpenCode）先行区别一句：本仓库不是外部进程/WASM 插件沙箱，而是把受约束的同步 JavaScript ESM 任务驱动嵌入 Go 进程，由 moejs/moego 执行，并由 Go 宿主掌握路由、凭证、HTTP、数据库和计费。

### 4.1 引擎：编译、执行与 Go 交互

`pkg/jsplugin.Engine` 持有编译后的 `moejs.Module`、runtime pool、并发 semaphore、hook cache 和 exports 快照（`pkg/jsplugin/engine.go:94-139`）。

- 编译期拒绝 `async`、`await`、`import` 和 re-export；运行期禁用动态代码/eval（`pkg/jsplugin/engine.go:92-160`、`pkg/jsplugin/engine.go:803-832`）。
- 默认每次 hook 执行 5 秒、并发 8；runtime 按需创建并池化，watchdog 可中断超时或 context 取消（`pkg/jsplugin/engine.go:24-27`、`pkg/jsplugin/engine.go:715-801`）。
- Go 通过 `Export` 读 `meta`，通过 `Call` / `CallPath` / `CallInto` 调 JS；JS 结果转回 JSON 兼容值或指定 Go 类型（`pkg/jsplugin/engine.go:203-243`、`pkg/jsplugin/engine.go:306-377`）。
- 入参是普通 JSON 值或 `RawJSON`；JS 修改不会回写 Go 对象。runtime 返回池前 `ReleaseCallData`，避免保留下一次请求的数据（`pkg/jsplugin/engine.go:391-518`）。
- `utils` 注入 `fetch`、`json.clone`、`unixNow`、`uuid`、`hmacSHA256`、`jwtSignHS256`、Base64 系列和 `volcSignV4`，另有 `console.log`（`pkg/jsplugin/utils.go:34-228`）。
- `utils.fetch` 只允许 GET/HEAD，单次 hook 最多 4 次、每次 15 秒、总计 30 秒、body 1 MiB；等待网络时暂停 JS 计时并出让执行槽（`pkg/jsplugin/fetch.go:33-42`、`pkg/jsplugin/fetch.go:80-180`）。
- `json_state.go` 实现 SSE delta 所需的 `set`/`append`/`appendText`，限制 1 MiB、32 层、32768 节点、每事件 256 changes；失败不能发布部分结果（`pkg/jsplugin/json_state.go:14-23`、`pkg/jsplugin/json_state.go:256-428`）。
- 图标支持 PNG/SVG sidecar，最大 512 KiB；SVG 禁止脚本、foreignObject、事件属性、DOCTYPE、javascript 和外部 HTTP(S) 引用（`pkg/jsplugin/icon.go:14-58`、`pkg/jsplugin/icon.go:60-126`）。

### 4.2 Manifest / package 结构与能力边界

内置包是 `plugins/tasks/<key>/plugin.js`，可附 `icon.svg` 或 `icon.png`；`plugins/embed.go` 用 `//go:embed tasks` 嵌入全部目录，并在 `init()` 中逐个 `RegisterFactory`（`plugins/embed.go:12-37`）。上传插件不是 zip/npm 包，而是 `source` 字符串加可选独立 `icon` data URI；源码上限 8 MiB，可附 `sourceSha256`（`controller/task_plugin.go:28-67`）。

`Meta` 主要字段如下（`pkg/jsplugin/registry.go:86-110`）：

| 字段 | 作用 |
|---|---|
| `apiVersion` | 当前只能为 1。 |
| `key/name/version/author/description/icon/website/sortPriority` | 标识与展示；key ≤30 且匹配小写规则，version 必须 semver（`pkg/jsplugin/registry.go:39-46`、`pkg/jsplugin/registry.go:1300-1318`）。 |
| `models` | 插件声明的模型，参与 protocol index、Ability 配置和计费；大小写折叠冲突会被拒绝（`pkg/jsplugin/routing.go:1125-1146`）。 |
| `channelTypes` | 兼容的旧 Channel 类型；同一类型不能被两个插件占用，不能声明 60/61（`pkg/jsplugin/registry.go:1323-1338`、`pkg/jsplugin/routing.go:1148-1156`）。 |
| `baseUrl` | type-61 渠道的默认上游 URL，保存渠道时固化；`pkg/jsplugin/registry.go:1952-1999`。 |
| `allowedHosts` | 除 BaseURL host 外允许接收插件请求/凭证的 host[:port]（`pkg/jsplugin/request.go:12-36`）。 |
| `upstreams` | `vendor` 隐含支持；声明 `new_api` 才允许绑定 type-60 New API 渠道（`pkg/jsplugin/registry.go:112-125`）。 |
| `fetchMode` | `per_task` 或 `batch`，决定查询 hook 要求（`pkg/jsplugin/registry.go:351-363`）。 |
| `routes` | 插件自有原生 URL，类型 submit/query/dynamic，含 decode/render、action、taskIdParam、models、retainResult（`pkg/jsplugin/routing.go:19-44`）。 |
| `protocols` | 认领宿主协议，不复制宿主 URL；可限定 models 和 Responses modes（`pkg/jsplugin/routing.go:46-62`）。 |
| `usageSchema/usageProfiles/usageExamples` | 计费事实 schema，数值单位为 second/count/token/credit，另有 boolean/enum；profile 可按模型替换默认 schema（`pkg/jsplugin/registry.go:145-193`、`docs/plugin-api/v1.d.ts:54-61`）。 |
| `auth` | `none` / `api_key` / `oauth2_jwt`（旧 `vertex_oauth` 会归一化）；`pkg/jsplugin/registry.go:1165-1187`。 |
| `submitResponseTypes` / `requiredCapabilities` | JSON/SSE 提交响应与 `json-order@1`、`submit-sse-delta@1` 等宿主能力声明（`pkg/jsplugin/json_state.go:14-28`）。 |

核心 hook 约束：`buildSubmitRequest`、`parseSubmitResponse`、`parseTaskResult` 必须存在；per-task 还要 `buildQueryRequest`，batch 还要 `buildBatchQueryRequest` 和 `parseBatchResult`；`listArtifacts` 与 `buildContentRequest` 必须成对（`pkg/jsplugin/registry.go:351-389`）。协议 hook 会在加载时按 claim 和 supports 双向检查（`pkg/jsplugin/registry.go:404-520`）。

权限边界不是声明式“全能力 ACL”，而是由宿主强制：

1. JS 同步、无模块 import、无动态代码；宿主函数也有执行边界。
2. 请求 URL 只能到 Channel BaseURL host 或 `allowedHosts`，Header 数量和敏感 hop-by-hop 字段受限（`pkg/jsplugin/request.go:12-59`）。
3. 文件字节不直接暴露给 JS；只给 `FileReference{ref,field,filename,mimeType,size}`，descriptor 中的 placeholder 由 Go 替换成 Base64/dataURL 或 multipart part（`relay/channel/task/jsplugin/adaptor.go:342-433`、`relay/channel/task/jsplugin/adaptor.go:444-551`）。
4. 任务快照和插件 state 通常限制 1 MiB；usage facts 有 host-owned 上限，second≤3600、count≤128，token/credit 饱和到 int32 配额边界（`relay/channel/task/jsplugin/adaptor.go:130-133`、`relay/channel/task/jsplugin/adaptor.go:1963-2002`）。
5. HTTP、SSRF、重定向凭证、日志、Task 落库和计费都由 Go 控制。查询重定向一旦越出可信 host，永久剥离后续凭证，HTTPS 不允许降级（`relay/channel/task/jsplugin/adaptor.go:905-965`）。

### 4.3 Registry、版本与路由生成

`Registry` 有 factory、override、activeOverride、disabledFactory 四层状态，另有总开关和不可变 `RoutingGeneration`（`pkg/jsplugin/registry.go:230-290`）。

- factory 来自 embed；override 来自数据库 `task_plugins`；同 key override 覆盖 factory，禁用 factory 可只关闭内置 fallback（`pkg/jsplugin/routing.go:1099-1104`、`pkg/jsplugin/registry.go:958-970`）。
- `TaskPlugin` 行包含 `Key/APIVersion/Version/Source/SourceHash/Icon/Enabled/Active/CreatedAt/Remark`，key+version 唯一；同版本不同源码拒绝（`model/task_plugin.go:78-95`、`model/task_plugin.go:102-133`）。
- Active 版本由激活接口切换，激活同时 enabled；删除 active 版本会提升最新剩余版本（`model/task_plugin.go:244-305`）。
- 后台每 30 秒读取 enabled+active 快照，用 SourceHash 复用或重编译，再 `ReplaceOverrides` 原子发布（`controller/task_plugin.go:839-992`、`controller/task_plugin.go:1001-1005`）。失败可保留旧 incumbent，并记录 partial/error（`pkg/jsplugin/registry.go:764-837`）。
- `RoutingGeneration` 包含 byKey、byModel/modelPlugins、byChannelType、routeIndex、protocolIndex 和运行时 handler，随 atomic pointer 一起发布（`pkg/jsplugin/routing.go:415-436`、`pkg/jsplugin/routing.go:707-714`）。
- 启用前的 preflight 会检查 channel type、大小写模型和 protocol 冲突（`pkg/jsplugin/routing.go:1199-1218`）。上传、启停、激活和删除均走生命周期校验与同步（`controller/task_plugin.go:49-124`、`controller/task_plugin.go:460-730`）。

### 4.4 插件如何参与 HTTP 路由

有两类入口，不能混为一谈：

#### A. `meta.routes`：插件自有原生 URL

- 支持 `submit`（创建任务）、`query`（读取任务）、`dynamic`（按 decode 结果二选一）；submit/dynamic 需要 decode+render，query 只需 render（`pkg/jsplugin/routing.go:886-948`）。
- 厂商路径会再注册一份 `/{key}` 前缀；若路径首段已是 key 或其前缀（如 `/ali`、`/suno`），不再复制（`pkg/jsplugin/routing.go:978-1012`）。
- 同 method+path shape 可由多个插件声明，候选按 key 排序；每个 submit 候选先 decode，第一个接受的模型决定候选集，最终由选中的 Channel 决定执行插件（`middleware/task_plugin.go:197-336`）。query 则由任务所属插件回答（`middleware/task_plugin.go:432-500`）。
- 动态 inner Gin 与插件 generation 同步构建；外层 NoRoute 和少数 handoff 会先尝试插件，再通过 `gatedResponseWriter` 丢弃未命中时产生的输出（`router/plugin-router.go:337-377`、`router/plugin-router.go:438-473`、`router/plugin-router.go:509-562`）。
- 保留 `/v1/tasks`、assets 和控制台命名空间；其他宿主 namespace 只有 built-in 可有限进入，且宿主静态路由优先。被跳过的顶层路径仍可从 `/{key}` 访问（`router/plugin-router.go:256-317`）。
- 两个显式 handoff 是 `GET /v1/videos/:task_id` 与 Gemini `POST /v1beta/models/*path` 的 `:predictLongRunning`（`router/plugin-router.go:63-70`、`router/relay-router.go:203-210`）。

**所以 `routing.go` 较大是合理的：它不仅定义 meta 类型，还实现宿主协议表、路由归一化、前缀复制、不可变 generation、候选索引和冲突检测；插件确实能参与真实 HTTP 路由，而非只做后台任务。**

#### B. `meta.protocols`：宿主已有协议

宿主协议表固定为（`pkg/jsplugin/routing.go:89-110`）：

| 协议 | 操作与 URL | 插件 hook/宿主职责 |
|---|---|---|
| `openai_responses` | `POST /v1/responses`、`GET /v1/responses/:response_id` | create 要 `decodeRequest`；stream 要 `renderEvents`，sync/background 要 `renderFinal`；宿主管理 Responses 状态机与 SSE。 |
| `openai_video` | `POST /v1/videos`、`GET /v1/videos/:task_id`、GET/HEAD `/content` | create `decodeRequest`，retrieve `render`，content 要 artifact hooks。 |
| `openai_image` | `POST /v1/images/generations`、`POST /v1/images/edits` | `decodeRequest` + `render`；宿主在请求内轮询异步任务并输出同步图片响应。 |

共享端点先判断普通 Channel 还是插件 Channel：普通 Relay 与匹配的插件 Channel 仍按 group/priority/weight/affinity/pin 竞争；普通 Channel 胜出则保留原 body 并走 Go Relay，插件 Channel 胜出才执行 decode 和任务桥（`middleware/task_plugin.go:517-570`、`docs/plugin-api/v1.md:71-86`）。路由注册见 `router/task-plugin-protocol-router.go:13-60`。

`controller/plugin_protocol.go` 的 Responses 桥先持久化任务，再按 stream/sync/background 观察；客户端断开不会取消已开始的提交和结算（`controller/plugin_protocol.go:246-391`）。`relay/plugin_protocol.go` 用 `PluginResponsesMachine` 把受限的 progress/output/error 语义事件转换为宿主拥有的 Responses SSE 与最终对象（`relay/plugin_protocol.go:359-443`、`relay/plugin_protocol.go:558-660`）。

### 4.5 内置任务插件

当前 `plugins/tasks` 有 11 个内置 key。下列路径均是声明路径；大多数还会另有 `/{key}` 副本，own-prefix 规则见上节。

| key | 模型/用途与原生路由证据 |
|---|---|
| `sora` | `sora-2`、`sora-2-pro`；兼容 ChannelType 55 与 OpenAI 1；支持 Responses/Video，创建 `/v1/videos`，remix `/v1/videos/:id/remix`；`plugins/tasks/sora/plugin.js:1-36`、`plugins/tasks/sora/plugin.js:95-125`。 |
| `google` | Google Veo（Gemini API），`veo-3.0/3.1`；`POST /v1beta/models/:model`（实际要求 `:predictLongRunning`）、`GET /v1beta/models/:model/operations/:task_id`；`plugins/tasks/google/plugin.js:1-46`、`plugins/tasks/google/plugin.js:193-253`。 |
| `vertex-ai` | Veo + Gemini Omni，ChannelType 41，`oauth2_jwt`；动态接收 v1/v1beta1 Vertex 路径；`plugins/tasks/vertex-ai/plugin.js:31-114`。 |
| `kling` | Kling 视频，`credit` 用量；`/v1/videos/text2video`、`/image2video` 和 Kling 4 专用路径，查询可用 query string `task_ids`；`plugins/tasks/kling/plugin.js:1-45`。 |
| `jimeng` | 即梦视频；`POST /api/common/v3/submit_task_v2`、dynamic `POST /api/common/v3/get_result_v2`；`plugins/tasks/jimeng/plugin.js:1-51`。 |
| `doubao` | Seedance/Seedream，兼容 54/45，支持 JSON+SSE；`/api/v3/contents/generations/tasks[/:task_id]`，图片 `/api/v3/images/generations`（不保留立即结果）；`plugins/tasks/doubao/plugin.js:297-332`。 |
| `alibaba` | 百炼万相/Qwen-Image/Z-Image，支持视频、图片、JSON+SSE 和 usageProfiles；大量 `/ali` 与官方路径，查询 `/api/v1/tasks/:task_id`；`plugins/tasks/alibaba/plugin.js:216-291`、`plugins/tasks/alibaba/plugin.js:292-374`。 |
| `hailuo` | MiniMax/Hailuo 视频；v1/v2 submit/query，`/v1/files/retrieve` 也作为任务产物查询；`plugins/tasks/hailuo/plugin.js:83-168`。 |
| `sunoapi` | `suno_music`、`suno_lyrics`，`fetchMode: batch`；`/suno/submit/:action`、dynamic `/suno/fetch`、query `/suno/fetch/:task_id`；`plugins/tasks/sunoapi/plugin.js:3-38`、`plugins/tasks/sunoapi/plugin.js:143-175`。 |
| `vidu` | Vidu 视频，ChannelType 52，原生凭证可为 `Authorization: Token`；`plugins/tasks/vidu/plugin.js:1-79`、`middleware/task_plugin.go:60-77`。 |
| `xai` | Grok Imagine，BaseURL `https://api.x.ai`，结果 host `vidgen.x.ai`；创建/编辑/延长 `/v1/videos/*`，查询 `/v1/videos/:request_id`；`plugins/tasks/xai/plugin.js:68-129`。 |

补充：Suno 的音频与封面作为不同 artifact 暴露（`plugins/tasks/sunoapi/plugin.js:192-221`）；Alibaba 同步图片路由可用 `retainResult:false`，任务行仍留作计费/日志但不保存结果快照（`plugins/tasks/alibaba/plugin.js:294-314`、`controller/relay.go:697-723`）。

### 4.6 任务数据模型与完整生命周期

`model.Task` 保存公开 `TaskID`、`Platform`、`UserId`、`Group`、`ChannelId`、`Quota`、`Action`、`Status`、`Progress`、`Properties`、上游快照 `Data` 和私有 `PrivateData`（`model/task.go:50-72`）。`PrivateData` 分离真实上游 ID、结果 URL、插件执行快照、计费上下文、PluginState、PollFailures 与 ResultDiscarded（`model/task.go:111-139`）。执行快照只含安全的插件 key/name/version/author/generation，不含源码或凭证（`model/task.go:141-161`）。

生命周期：

1. **提交准备**：通用 `/v1/tasks/:key` 走 `PrepareTaskPluginSubmit`；原生路由走 `PrepareTaskPluginRoute`；共享协议走 `PrepareTaskPluginEndpoint`，解析为统一 submit intent（`router/task-router.go:14-26`、`middleware/task_plugin.go:110-365`、`middleware/task_plugin.go:756-993`）。
2. **选渠道和驱动**：`GetTaskPlatform` 优先 task_plugin_key，再由 generation 找精确 plugin，并构造 `relay/channel/task/jsplugin.TaskAdaptor`（`relay/relay_adaptor.go:130-165`、`relay/relay_adaptor.go:188-234`）。
3. **构造上游请求**：`buildSubmitRequest` 返回 descriptor；Go 校验 URL、responseType、文件和 Header，执行 HTTP（`relay/channel/task/jsplugin/adaptor.go:1628-1710`、`relay/channel/task/jsplugin/adaptor.go:554-590`）。
4. **估算与预扣**：`extractUsage` 生成事实，旧模式转 OtherRatios，表达式模式直接计算 USD；异步任务强制预扣（`relay/relay_task.go:250-343`）。
5. **解析提交**：`parseSubmitResponse` 返回 `{taskId,taskData,immediate?,state?}`；2xx 都接受，clientResponse 被禁止（`relay/channel/task/jsplugin/adaptor.go:625-781`）。SSE 可由 `parseSubmitEvent` 或 delta hook 处理，并由宿主控制帧、状态和结算（`relay/channel/task/jsplugin/submit_stream.go:27-210`）。
6. **持久化屏障**：`executeTaskSubmissionWith` 负责重试、追加预留、插入 Task、最终 SettleBilling 和消费日志；插件 presenter 只在持久化之后运行（`controller/relay.go:497-760`、`controller/relay.go:763-821`）。
7. **后台轮询**：系统任务每 15 秒触发 `RunTaskPollingOnce`，按 platform/channel 分组，per-task 或 batch 查询（`controller/system_task_handlers.go:134-151`、`service/task_polling.go:130-224`）。
8. **状态与失败**：2xx 调 parse；404/410 立即失败退款；401/403 不自动禁渠道，只累计失败；429/5xx/transport 累计失败；默认连续 20 次失败则任务失败，72 小时（4320 分钟）是兜底超时（`service/task_polling.go:68-120`、`service/task_polling.go:737-837`、`common/init.go:209-219`）。状态 CAS 的赢家才做终态结算，避免多节点重复扣退（`service/task_polling.go:614-647`）。
9. **产物**：`listArtifacts` 返回 key/type/mimeType，`buildContentRequest` 产生受限下载 descriptor；通用 URL 为 `/v1/tasks/:key/artifacts/:artifact_key/content`（`relay/channel/task/jsplugin/adaptor.go:1220-1360`、`router/task-router.go:28-36`）。
10. **协议呈现**：原生 `native.render*` 输出厂商 envelope；OpenAI Video 用 `render`，Responses 用 `renderFinal` 或 `renderEvents`；宿主覆盖公开 ID/状态/时间并保留安全扩展（`relay/channel/task/jsplugin/adaptor.go:1173-1217`、`controller/plugin_protocol.go:940-1083`）。
11. **完成结算**：成功按表达式/adaptor/token 重算差额；失败且没有专门结算路径时全额退款（`service/task_polling.go:692-734`、`service/task_billing.go:260-377`）。

### 4.7 插件管理端点与 CLI

管理 API 已列于 `/api/plugin/task`：列表、上传/更新、runtime status、市场源、详情、图标、版本、激活、启停、dry-run、删除版本（`router/api-router.go:263-279`）。

CLI 入口是 `new-api plugin`：

```text
new-api plugin lint <plugin.js>
new-api plugin test <plugin.js> --fixture <fixture.json>
```

`main.go` 在普通服务启动前分流到 `jsplugin.RunCLI`（`main.go:49-52`）；lint 做真实编译/manifest 校验，test 重放 JSON fixture（`pkg/jsplugin/cli.go:11-59`）。

## 5. 关键参数与特点汇总

| 参数/特点 | 当前源码值或语义 |
|---|---|
| `RetryTimes` | 默认 0；每次 retry 选择下一优先级层，超出后钳到最低层；`common/constants.go:143`、`model/channel_cache.go:165-168` |
| 权重 | 内存路径有全零=100和小权重放大；DB 路径是 `Weight+10`；`model/channel_cache.go:188-214`、`model/ability.go:153-167` |
| 自动禁用 | 总开关默认 false，默认状态码仅 401，另支持关键词；`common/constants.go:133-135`、`service/channel.go:60-79` |
| 自动重试状态码 | 默认排除 400/408/504/524 等，详见 `setting/operation_setting/status_code_ranges.go:19-38` |
| 多 Key | Random/Polling，跳过禁用 Key，全禁用则禁用 Channel；`model/channel.go:211-295`、`model/channel.go:678-727` |
| 亲和 | strict/prefer/off，支持 TTL、auto group、成功后切换和参数模板；`service/channel_affinity.go:550-758` |
| JS Engine | 默认 hook 5s、并发 8；`pkg/jsplugin/engine.go:24-27` |
| `utils.fetch` | 每 hook 4 次、15s/次、30s 总计、1 MiB body；`pkg/jsplugin/fetch.go:33-42` |
| 上传源码 | 8 MiB，可附 SHA-256；`controller/task_plugin.go:28-65` |
| 插件图标 | 512 KiB，PNG/SVG sidecar；`pkg/jsplugin/icon.go:14-34` |
| 插件持久化数据 | 通常 taskData/state 1 MiB；`relay/channel/task/jsplugin/adaptor.go:130-133` |
| Responses 协议观察 | 默认 600s，tick 2000ms、jitter 500ms、heartbeat 15s；`common/init.go:214-219` |
| 后台轮询 | 调度间隔 15s；连续失败默认 20；总超时默认 4320 分钟；`controller/system_task_handlers.go:139-150`、`common/init.go:209-213` |
| 定价 | 旧 ratio/per-call 与 tiered_expr 并存；普通表达式与任务 USD 表达式换算不同；`relay/helper/price.go:65-209`、`pkg/billingexpr/expr.md:363-373` |
| 插件绑定 | type-61 单插件 `task_plugin_key`；type-60 还可用 `task_extend_plugin_keys` 扩展多个插件；`relaykit/dto/channel_settings.go:14-28`、`relaykit/dto/channel_settings.go:40-62` |

## 6. 核心证据索引

| 主题 | 文件:行号 |
|---|---|
| 项目定位与能力 | `README.zh_CN.md:59-63`、`README.zh_CN.md:134-157` |
| 技术栈与模块边界 | `AGENTS.md:9-25`、`go.mod:168-178`、`relaykit/README.md:1-5` |
| Web embed / Electron | `main.go:43-47`、`router/web-router.go:21-46`、`electron/README.md:46-66` |
| 管理 API | `router/api-router.go:17-47`、`router/channel-router.go:40-102` |
| Relay API | `router/relay-router.go:64-180`、`router/relay-router.go:182-245` |
| Channel / Ability | `model/channel.go:23-72`、`model/ability.go:18-26`、`model/ability.go:225-339` |
| 选路优先级/权重 | `model/channel_cache.go:117-217`、`model/ability.go:108-173` |
| pin/affinity/filter | `service/channel_select.go:297-396`、`service/channel_affinity.go:550-758`、`model/channel_constraint.go:10-143` |
| 多 Key | `model/channel.go:187-295`、`model/channel.go:678-727` |
| 重试/禁用 | `controller/relay.go:167-239`、`service/relay_error.go:18-80`、`service/channel.go:29-80` |
| 协议适配与 upstream | `relay/channel/adapter.go:18-35`、`relay/relay_adaptor.go:50-128`、`relay/channel/api_request.go:323-349` |
| RelayKit | `relaykit/README.md:1-48` |
| 预扣/结算/订阅 | `relay/request_billing.go:22-82`、`service/billing_session.go:28-84`、`service/billing_session.go:388-490` |
| 定价配置 | `model/model_pricing_config.go:64-75`、`model/pricing.go:326-428`、`pkg/billingexpr/expr.md:279-373` |
| JS 引擎 | `pkg/jsplugin/engine.go:92-200`、`pkg/jsplugin/engine.go:391-518`、`pkg/jsplugin/engine.go:803-832` |
| 插件包与注册 | `plugins/embed.go:12-47`、`pkg/jsplugin/registry.go:292-527` |
| 插件路由 generation | `pkg/jsplugin/routing.go:415-436`、`pkg/jsplugin/routing.go:1021-1197`、`router/plugin-router.go:72-187` |
| 共享宿主协议 | `pkg/jsplugin/routing.go:89-110`、`router/task-plugin-protocol-router.go:13-60`、`middleware/task_plugin.go:517-570` |
| 插件版本同步 | `model/task_plugin.go:78-133`、`controller/task_plugin.go:839-1005` |
| 任务提交/落库/轮询/结算 | `controller/relay.go:497-760`、`service/task_polling.go:130-224`、`service/task_polling.go:614-734` |
| Responses 协议状态机 | `relay/plugin_protocol.go:359-443`、`controller/plugin_protocol.go:162-392` |
| 内置任务插件 | `plugins/tasks/sora/plugin.js:1-36`、`plugins/tasks/google/plugin.js:1-46`、`plugins/tasks/alibaba/plugin.js:216-374` |

## 7. 未确认项与调查边界

1. **未做运行验证**：本报告只读源码，未启动服务、未向真实厂商发请求，未验证各厂商当前线上字段和模型可用性；实际上游 URL、鉴权细节与响应以对应 Channel 配置和厂商为准。
2. **未逐项展开所有厂商适配器**：`relay/relay_adaptor.go` 的 ApiType 分派已核对，但 40+ 厂商每个私有 DTO/错误映射未逐个展开；高级自定义 Channel 只确认了路径过滤和适配入口。
3. **路由表不是每个管理端点的穷尽清单**：`/api` 端点很多，报告按前缀和代表端点归纳；前端路由则按主要页面完整归类。
4. **octopus / magpie 仅作架构层面的先行对照**：本次没有读取这两个仓库，未确认其具体版本、沙箱隔离强度或生命周期细节；只确认 new-api 自身是 moejs/moego 的进程内同步 JS 任务插件模型。
5. **内存与 DB 选路“等价目标”不等于完全同算法**：两者 priority/retry/模型归一化一致，但权重平滑公式不同；线上行为还取决于 `MemoryCacheEnabled`。
6. **市场安装包格式未单独核实**：上传运行时明确是 source + icon；市场 index 的安装下载细节不在本次核心调用链中，未据此推断存在统一 zip 包规范。
