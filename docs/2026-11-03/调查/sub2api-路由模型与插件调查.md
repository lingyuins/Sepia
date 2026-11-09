# Sub2API 路由模型与插件系统调查

> 调查对象：`D:\CodeWorkspace\sub2api`（Wei-Shaw/sub2api）当前工作树快照。  
> 方法：只读源码与仓库内文档；未修改被调查仓库，未启动服务或连接其 PostgreSQL/Redis。  
> 引用格式均为“仓库相对路径:行号”。运行模式、管理端设置和部署配置可能覆盖文中默认值。

## 1. 概览与技术栈

### 1.1 “订阅配额分发”的实际定位

Sub2API 不是单纯的协议反向代理，而是把上游 AI 产品账号中的 OAuth/API Key 等凭证组织成账号池，再向站内用户签发自己的 API Key。一次调用由平台完成 Key 鉴权、用户/分组授权、订阅或余额准入、限流与并发控制、账号选择、协议适配、转发、失败转移、计费与用量落库。README 对定位的直接描述是“分发和管理 AI 产品订阅的 API 配额”，并明确平台承担鉴权、计费、负载均衡和转发（`README_CN.md:192-205`）。

核心抽象可概括为：

- **Account**：实际上游凭证及其健康、配额、调度属性；是最终被请求占用的资源（`backend/ent/schema/account.go:18-27`）。
- **Group**：API Key 与账号池之间的路由/计费/订阅边界；同时承载平台、倍率、限额、模型白名单、模型路由和 fallback 配置（`backend/ent/schema/group.go:83-103`, `backend/ent/schema/group.go:206-228`, `backend/ent/schema/group.go:273-313`）。
- **APIKey**：站内用户的访问凭证，绑定用户和至多一个 Group；自身还可有过期、总额度、IP ACL 和 5h/1d/7d 金额窗口（`backend/ent/schema/api_key.go:34-118`）。
- **UserSubscription**：用户在某订阅型 Group 上的有效期与日/周/月消费窗口（`backend/ent/schema/user_subscription.go:36-82`）。
- **Channel**：定价、模型映射和模型限制的运营配置层，不是“一个上游账号”或“一次选号节点”；每个 Group 至多关联一个 Channel（`backend/migrations/081_create_channels.sql:21-50`, `backend/internal/service/channel.go:49-72`）。

因此，“订阅”有内外两层：外部是 Account 对应的 ChatGPT/Claude/Gemini 等上游订阅或 API 凭证；内部是 UserSubscription 对用户授予某个 Group 的时限和 USD 限额。二者通过选号与后扣费连接，但数据模型并不相等。

### 1.2 技术栈

| 层 | 实际技术 | 证据 |
|---|---|---|
| 后端 | Go 1.27.3（go.mod 当前值）、Gin 1.9.1、Ent 0.14.5、Wire | `backend/go.mod:1-6`, `backend/go.mod:21-26` |
| 数据与缓存 | PostgreSQL、Redis；JSONB 承载凭证和平台扩展；Redis 承担调度快照、会话、并发、限流和配额缓存 | `README_CN.md:217-225`, `backend/internal/service/concurrency_service.go:21-56` |
| 前端 | Vue 3.5、Vue Router 4、Pinia、TypeScript、Vite 5、TailwindCSS 3、Vitest | `frontend/package.json:18-37`, `frontend/package.json:48-61` |
| 协议/上游 | OpenAI Responses/Chat Completions/WS、Anthropic Messages、Gemini 原生、Grok 媒体/语音、AWS Bedrock、MCP | `backend/internal/server/routes/gateway.go:179-358`, `backend/internal/server/routes/mcp.go:19-56` |
| 插件 | HashiCorp go-plugin、本机 gRPC、gRPC 双向流、原生子进程；不是 WASM | `backend/go.mod:28-29`, `backend/internal/service/plugin_runtime.go:38-78` |
| 支付/扩展 | 内置支付、OAuth 登录、WebAuthn、管理 MCP、提示词安全审计 | `README_CN.md:198-206`, `backend/go.mod:23-24`, `backend/internal/managementmcp/server.go:27-62` |

开发文档采用 Ent schema 修改后重新生成、后端 `go test -tags=unit/integration`、前端 pnpm 的工作流（`DEV_GUIDE.md:59-73`, `DEV_GUIDE.md:199-208`）。README 的 PostgreSQL 15+/Redis 7+ 是部署要求；DEV_GUIDE 记录的是该开发环境的 PostgreSQL 16，不应把后者当成项目最低版本（`DEV_GUIDE.md:5-13`）。

## 2. 页面与 HTTP 路由

### 2.1 前端页面

Vue Router 使用 `createWebHistory`，页面均懒加载；导航守卫处理认证、管理员角色、管理合规确认、支付/订阅/风控开关和简易/后端模式（`frontend/src/router/index.ts:755-766`, `frontend/src/router/index.ts:805-1003`）。

| 类别 | 主要页面路径 | 页面用途/约束 |
|---|---|---|
| 初始化与公开 | `/setup`、`/home`、`/login`、`/register`、`/forgot-password`、`/reset-password` | 设置向导、首页和认证；`/setup` 会查询后端设置状态（`frontend/src/router/index.ts:20-61`, `frontend/src/router/index.ts:830-840`） |
| 公开业务 | `/key-usage`、`/model-plaza`、`/legal/:documentId` | Key 用量、模型广场、法律文档；模型广场受启用和可选登录开关控制（`frontend/src/router/index.ts:160-187`, `frontend/src/router/index.ts:856-886`） |
| 用户核心 | `/dashboard`、`/keys`、`/usage`、`/available-channels`、`/monitor` | 用量看板、Key 管理、调用记录、可用渠道与状态（`frontend/src/router/index.ts:195-241`, `frontend/src/router/index.ts:267-278`, `frontend/src/router/index.ts:517-525`） |
| 用户订阅/支付 | `/subscriptions`、`/purchase`、`/orders`、`/payment/*` | 我的订阅、购买、订单和支付结果；订阅/支付由功能开关门控（`frontend/src/router/index.ts:315-399`） |
| 用户其他 | `/redeem`、`/affiliate`、`/settings/mcp`、`/profile`、`/custom/:id` | 卡密、返利、MCP、个人资料、自定义页面（`frontend/src/router/index.ts:243-313`, `frontend/src/router/index.ts:411-420`） |
| 管理核心 | `/admin/dashboard`、`/admin/ops`、`/admin/users`、`/admin/groups`、`/admin/accounts`、`/admin/subscriptions`、`/admin/usage` | 平台、账号池、分组、订阅、调用和运维管理（`frontend/src/router/index.ts:423-550`, `frontend/src/router/index.ts:649-659`） |
| 管理渠道 | `/admin/channels/pricing`、`/admin/channels/monitor` | 渠道定价/映射与监控（`frontend/src/router/index.ts:488-515`） |
| 管理扩展 | `/admin/plugins`、`/admin/risk-control`、`/admin/prompt-audit`、`/admin/audit-logs` | 本地插件、风控、提示词审计和操作审计（`frontend/src/router/index.ts:452-463`, `frontend/src/router/index.ts:551-562`, `frontend/src/router/index.ts:623-648`） |
| 管理其他 | `/admin/proxies`、`/admin/redeem`、`/admin/promo-codes`、`/admin/settings`、`/admin/orders*`、`/admin/affiliates/*` | 代理、卡密、优惠码、系统设置、支付订单/套餐和返利（`frontend/src/router/index.ts:563-620`, `frontend/src/router/index.ts:661-738`） |

**是否内嵌：是，但由 build tag 决定。** `embed` 构建通过 `//go:embed all:dist` 把前端打包进 Go 二进制；Gin 中间件为 SPA 路由回退到 `index.html`，并注入公开设置、CSP nonce，支持 `data/public` 覆盖文件（`backend/internal/web/embed_on.go:1-30`, `backend/internal/web/embed_on.go:37-76`, `backend/internal/web/embed_on.go:86-117`）。非 embed 构建只提供 404 stub（`backend/internal/web/embed_off.go:1-46`）。

### 2.2 后端路由注册与全局中间件

`SetupRouter` 依次安装请求日志、IP/UA 会话绑定上下文、Logger、CORS、安全头、ServerTiming 和内嵌前端中间件，再注册业务路由（`backend/internal/server/router.go:58-93`）。面板 API 统一位于 `/api/v1`，数据面网关主要直接挂在根路径（`backend/internal/server/router.go:116-137`）。

| 路由域 | 代表路径 | Handler/说明 | 鉴权 |
|---|---|---|---|
| 通用 | `GET /health`、`GET /setup/status`、`POST /api/event_logging/batch` | 健康、设置状态、忽略 Claude 遥测 | 公开（`backend/internal/server/routes/common.go:9-31`） |
| 认证 | `POST /api/v1/auth/register`、`/login`、`/login/2fa`、`/refresh`、多种 `/oauth/*` | 登录注册、2FA、Passkey、OAuth | 公开接口多有 Redis 限流且 fail-close（`backend/internal/server/routes/auth.go:28-74`） |
| 用户面板 | `/api/v1/user/*`、`/keys`、`/groups/*`、`/channels/available`、`/usage/*`、`/subscriptions/*` | 资料、Key、用量、订阅等 | JWT + 用户模式守卫 + 面板限流 + 审计（`backend/internal/server/routes/user.go:20-27`, `backend/internal/server/routes/user.go:82-143`） |
| 管理面板 | `/api/v1/admin/{users,groups,accounts,subscriptions,usage,ops,settings,channels,...}` | 全量运营管理 | AdminAuth + 面板限流 + 审计 + 合规（`backend/internal/server/routes/admin.go:26-35`） |
| 插件管理 | `/api/v1/admin/plugins*` | 上传、启停、配置、测试、状态和 UI 会话 | 管理员；写操作多有 step-up（`backend/internal/server/routes/admin.go:766-780`） |
| 插件 UI | `GET /api/v1/plugin-ui/:token/*path` | 仅提供已安装且清单声明的静态资源 | 短时能力 URL，不经过 AdminAuth（`backend/internal/server/routes/admin.go:23-24`） |
| 模型广场 | `GET /api/v1/model-plaza` | 模型/分组展示 | OptionalJWT + 开关（`backend/internal/server/routes/model_plaza.go:11-29`） |
| 自定义页 | `GET /api/v1/pages/:slug`、`/pages/:slug/images/*filename` | 页面内容/图片 | 内容需 JWT，图片做可见性检查；管理列表需 AdminAuth（`backend/internal/handler/page_handler.go:260-283`） |
| 支付 | `/api/v1/payment/*`、`/payment/webhook/*`、`/admin/payment/*` | 套餐、订单、回调、退款 | 分别使用 JWT、公开回调或 AdminAuth（`backend/internal/server/routes/payment.go:39-133`） |
| 管理 MCP | `/api/v1/mcp`、`/api/v1/mcp/oauth/*`、OAuth discovery | 将白名单管理操作暴露为 MCP 工具 | OAuth token + 授权 scope + 限流（`backend/internal/server/routes/mcp.go:19-56`） |

### 2.3 数据面网关入口

`/v1` 中间件顺序包含请求体限制、ClientRequestID、Ops 错误日志、入口标准化、APIKeyAuth、分组模型白名单、Composite 目标解析和未分组 Key 拦截（`backend/internal/server/routes/gateway.go:179-190`）。`GET /v1/sub2api/billing` 在白名单/Composite 之前注册，用于 Key 倍率自省（`backend/internal/server/routes/gateway.go:185-189`）。

| 入口族 | 代表端点 | 分发规则 |
|---|---|---|
| Anthropic/OpenAI 兼容 `/v1` | `POST /v1/messages`、`/responses`、`/chat/completions`、`/messages/count_tokens`；`GET /v1/models`、`/usage` | 根据 Group/Composite 解析后的平台选择 `GatewayHandler` 或 `OpenAIGatewayHandler`（`backend/internal/server/routes/gateway.go:191-239`） |
| OpenAI 其他能力 | `/v1/embeddings`、`/images/*`、`/videos/*`、`/tts`、`/stt`、`/realtime`、`/web_search`、`/x_search` | 按平台限制能力；Grok 有媒体、语音、实时和搜索专用转发（`backend/internal/server/routes/gateway.go:240-334`） |
| 图片异步/批处理 | `/v1/images/generations/async`、`/images/tasks/:task_id`、`/images/batches*` | 独立任务处理器（`backend/internal/server/routes/gateway.go:253-267`） |
| Live/Codex | `/v1/live`、`/v1/live/:call_id`、`/backend-api/codex/responses`、`/backend-api/codex/models` | Codex 专用别名与 Live（`backend/internal/server/routes/gateway.go:211-212`, `backend/internal/server/routes/gateway.go:387-399`） |
| Gemini 原生 | `/v1beta/models*`、`/v1beta/cachedContents*` | Gemini SDK/CLI 直连及显式缓存（`backend/internal/server/routes/gateway.go:337-358`） |
| 根路径别名 | `/responses`、`/chat/completions`、`/embeddings`、`/models`、图片/视频/语音等 | 复用与 `/v1` 相同的认证、白名单和 Composite 链（`backend/internal/server/routes/gateway.go:360-488`） |
| Seedance | `/{api/v3,v3,v1,}/contents/generations/tasks*` | 异步任务创建、查询、删除（`backend/internal/server/routes/gateway.go:373-377`） |
| Antigravity 专用 | `/antigravity/v1/messages`、`/antigravity/v1beta/models/*modelAction` | `ForcePlatform(antigravity)`，不与普通分组混选（`backend/internal/server/routes/gateway.go:490-522`） |

### 2.4 鉴权中间件的实际职责

- 网关 Key 可来自 `Authorization: Bearer`、`x-api-key` 或 `x-goog-api-key`；查询参数 Key 被明确拒绝（`backend/internal/server/middleware/api_key_auth.go:49-95`）。
- 鉴权先检查 Key、用户、IP ACL、Group 可用性与专属组授权；订阅型 Group 再加载 `(user_id, group_id)` 的活跃订阅并同步维护窗口（`backend/internal/server/middleware/api_key_auth.go:98-167`, `backend/internal/server/middleware/api_key_auth.go:191-267`）。
- `/v1/usage`、`/v1/sub2api/billing` 和异步图片任务读取可跳过计费执行，但仍需鉴权（`backend/internal/server/middleware/api_key_auth.go:25-34`, `backend/internal/server/middleware/api_key_auth.go:168-173`）。
- 管理端支持管理员 JWT、全局 Admin API Key、`adm_sc_` 范围管理密钥；JWT 会检查用户状态、TokenVersion 和 IP/UA 会话绑定（`backend/internal/server/middleware/admin_auth.go:25-90`, `backend/internal/server/middleware/admin_auth.go:167-230`, `backend/internal/server/middleware/admin_auth.go:233-263`）。
- 分组模型白名单位于 APIKeyAuth 之后、Composite 改写之前，只看客户端公开模型；Responses WS 改由首帧/逐 turn 校验（`backend/internal/server/middleware/group_model_allowlist.go:16-31`, `backend/internal/server/middleware/group_model_allowlist.go:45-88`）。

## 3. 数据模型

### 3.1 核心关系

```text
User ──< APIKey >── Group ──< AccountGroup >── Account >── Proxy(可选)
 │                  │  │                            │
 │                  │  └──< UserSubscription        └──< UsageLog
 │                  │         (user + group)              ▲
 │                  └────────── Channel(至多一个)           │
 └──────────────────< UsageLog >────────────────────────────┘

Composite Group ──< CompositeModelRoute（公开模型/端点 → 具体平台/上游模型）
```

Account 与 Group 是多对多，`account_groups` 另有关系级 priority；Account 自身的 priority 也是独立字段（`backend/ent/schema/account_group.go:15-39`, `backend/ent/schema/account.go:103-108`, `backend/ent/schema/account.go:207-230`）。

### 3.2 主要实体及调度/计费字段

| 实体 | 关键字段与语义 |
|---|---|
| Account / `accounts` | `platform/type/credentials/extra/proxy_id`；`concurrency`、可选 `load_factor`、`priority`、`rate_multiplier`；`status/schedulable/expires_at`；`rate_limit_reset_at/overload_until/temp_unschedulable_until`；会话窗口；Spark 母子关系与 `quota_dimension`（`backend/ent/schema/account.go:63-204`） |
| Group / `groups` | `platform`、standard/subscription、日周月 USD 限额、默认有效期；分组/高峰/图片/视频/搜索定价；Claude Code fallback、模型路由、白名单、OpenAI 调度策略、RPM、利润门（`backend/ent/schema/group.go:52-103`, `backend/ent/schema/group.go:198-313`） |
| APIKey / `api_keys` | `user_id/group_id/key/status`、IP 黑白名单、总 quota 与已用量、过期时间、5h/1d/7d 金额窗口及起点（`backend/ent/schema/api_key.go:34-118`） |
| User / `users` | 角色、余额/冻结余额、用户并发、状态、2FA、公开分组限制、全局 RPM（`backend/ent/schema/user.go:36-123`） |
| UserSubscription / `user_subscriptions` | 用户+分组、起止时间、active/expired/suspended、日周月窗口起点与 USD 已用量、分配者（`backend/ent/schema/user_subscription.go:36-82`） |
| SubscriptionPlan / `subscription_plans` | 商品配置：Group、价格/币种、有效期、是否在售；购买后实际 entitlement 仍在 UserSubscription（`backend/ent/schema/subscription_plan.go:14-72`） |
| UserPlatformQuota | standard 模式下额外的 user×platform 日/周/月 USD 限额和窗口；`nil` 为无限制，`0` 为禁用该平台（`backend/ent/schema/user_platform_quota.go:35-90`） |
| Proxy / `proxies` | 协议、地址、认证、状态、过期时间；过期可转备用代理或直连（`backend/ent/schema/proxy.go:32-67`） |
| UsageLog / `usage_logs` | 用户、Key、账号、Group、订阅、请求/上游模型、渠道、token、缓存、总成本/实际成本、分组及账号倍率、流式耗时、图片/视频（`backend/ent/schema/usage_log.go:31-189`） |
| CompositeModelRoute | `public_model`、exact/prefix、具体 `target_platform`、`upstream_model`、端点范围、priority、enabled（`backend/ent/schema/composite_model_route.go:36-75`） |
| Channel | 非 Ent schema；由 SQL migration + service model 定义，关联 Group，并含定价、映射、计费模型来源和账号统计价格规则（`backend/internal/service/channel.go:49-111`, `backend/migrations/081_create_channels.sql:8-50`） |

**JSONB 是重要扩展面。** Account 的 `credentials` 不只存密钥，还含 `model_mapping`、临时不可调度规则、池模式等；`extra` 存上游配额快照、窗口成本、会话/RPM 限制等。账号配额适用于 `apikey`、`bedrock`、`service_account`，总额/日/周任一达到上限即不可调度（`backend/internal/service/account.go:1429-1447`, `backend/internal/service/account.go:3042-3105`）。

## 4. 路由模型：账号调度、订阅配额、失败转移与多平台适配

### 4.1 选路全流程（从网关请求到 upstream Account）

下表以普通文本生成为主线；图片、视频、实时 WS 等会增加独立能力与连接约束。

| 步骤 | 动作 | 具体实现/证据 |
|---:|---|---|
| 1 | 进入协议入口，执行 body limit、请求 ID、Ops 记录和 APIKeyAuth | `/v1` 链见 `backend/internal/server/routes/gateway.go:179-190`；Key/用户/Group/IP/订阅检查见 `backend/internal/server/middleware/api_key_auth.go:98-167` |
| 2 | 若 Group 为订阅型，读取活跃订阅，激活/滚动窗口并检查日周月限额 | `backend/internal/server/middleware/api_key_auth.go:191-260`；窗口维护见 `backend/internal/service/subscription_service.go:961-1048` |
| 3 | 对客户端原始公开模型做 Group 白名单准入 | 必须在 Composite 改写之前：`backend/internal/server/middleware/group_model_allowlist.go:16-31` |
| 4 | Composite 分组按“显式路由→账号模型归属→内置模型识别”解析具体平台和上游模型，并注入请求 context | `backend/internal/service/composite_route_resolver.go:97-179`；中间件见 `backend/internal/server/routes/gateway.go:549-610` |
| 5 | Handler 解析 body/model/stream，执行客户端限制、reasoning 策略、安全审计、端点能力及渠道映射 | Anthropic 主线：`backend/internal/handler/gateway_handler.go:160-247`；OpenAI 主线：`backend/internal/handler/openai_gateway_handler.go:422-589` |
| 6 | 获取用户并发槽；等待后再次做余额/订阅、user×platform quota、Key 窗口和 RPM 准入 | `backend/internal/handler/gateway_handler.go:249-287`；统一检查见 `backend/internal/service/billing_cache_service.go:740-790` |
| 7 | 可选登记在途余额预留，降低并发请求看到同一余额造成的透支 | `backend/internal/service/billing_inflight_reservation.go:45-65`, `backend/internal/service/billing_inflight_reservation.go:248-347` |
| 8 | 从显式会话头、`prompt_cache_key`、内容或 previous_response_id 生成粘性锚点 | OpenAI 信号顺序：`backend/internal/service/openai_gateway_scheduling.go:31-55`, `backend/internal/service/openai_gateway_scheduling.go:147-184` |
| 9 | 解析实际 Group/平台：Claude Code 限制可沿 fallback Group 链移动；Composite 使用步骤 4 的具体平台 | `backend/internal/service/gateway_scheduling.go:922-1011` |
| 10 | 从 Redis 调度快照按 Group+平台取候选，未命中时受控回源 DB；选中后重新水合完整账号 | `backend/internal/service/scheduler_snapshot_service.go:211-299`；DB 路径见 `backend/internal/service/gateway_scheduling.go:1014-1122` |
| 11 | 过滤已排除、状态/手动停调、过期、429/529/临时封禁、模型映射/模型级冷却、配额、窗口费用、RPM、平台/能力/传输、隐私及利润不合格账号 | 通用过滤：`backend/internal/service/gateway_scheduling.go:645-694`；OpenAI 过滤：`backend/internal/service/openai_gateway_scheduling.go:1273-1303` |
| 12 | 按引擎选择：通用侧“模型路由→粘性→优先级/负载/LRU”；OpenAI 高级侧“previous response→母账号→会话粘性→加权 Top-K” | `backend/internal/service/gateway_scheduling.go:276-330`, `backend/internal/service/gateway_scheduling.go:523-568`, `backend/internal/service/gateway_scheduling.go:645-793`；`backend/internal/service/openai_account_scheduler.go:382-493` |
| 13 | 获取账号 Redis 并发槽；未立即得到则生成有界 WaitPlan，由 Handler 入队等待；准入前再次核验最新账号和利润门 | `backend/internal/service/concurrency_service.go:339-415`；`backend/internal/handler/gateway_handler.go:755-838` |
| 14 | 依据 Account 选择协议转换、凭证、代理、HTTP/WS/TLS 路径；OpenAI OAuth 命中插件时改由插件建立真实上游 HTTP 连接 | `backend/internal/service/openai_gateway_forward.go:180-195`；`backend/internal/service/openai_plugin_transport.go:9-18` |
| 15 | 错误时先按错误元数据决定同账号重试或换号；排除失败账号并重新从步骤 10 选号；已写出语义流内容或客户端断开则停止 | `backend/internal/handler/failover_loop.go:191-267`；OpenAI 循环见 `backend/internal/handler/openai_gateway_handler.go:861-965` |
| 16 | 成功后解析 usage，异步提交有界 worker；原子扣减订阅/余额、Key/账号/平台额度并落 UsageLog | `backend/internal/service/gateway_usage_billing.go:289-392`, `backend/internal/service/gateway_usage_billing.go:414-465` |

### 4.2 通用 GatewayService 调度引擎

`GatewayService.SelectAccountWithLoadAwareness` 服务 Anthropic/Gemini 及相关兼容路径。主要顺序是：

1. **Group 和客户端约束**：Claude Code-only Group 无权限时沿 `fallback_group_id` 链下降，检测循环；随后做渠道定价限制（`backend/internal/service/gateway_scheduling.go:112-129`, `backend/internal/service/gateway_scheduling.go:922-970`）。
2. **Layer 1 模型路由**：Group 的 `model_routing` 给出优先 Account ID 列表，优先于普通粘性；目前模型路由只适用于目标 Anthropic/OpenAI（含 Composite 解析到这两类）（`backend/ent/schema/group.go:219-228`, `backend/internal/service/gateway_scheduling.go:253-330`, `backend/internal/service/gateway_scheduling.go:877-919`）。列表内再做粘性、负载与抢槽。
3. **Layer 1.5 粘性**：从 context 预取或 Redis 读取 session→Account；即使命中也必须通过当前平台、模型、配额、窗口、RPM 和健康门（`backend/internal/service/gateway_scheduling.go:131-141`, `backend/internal/service/gateway_scheduling.go:523-620`）。
4. **Layer 2 负载**：批量读取账号当前并发和等待数；负载分母用 `EffectiveLoadFactor()`，即正数 `load_factor`，否则 concurrency，再否则 1（`backend/internal/service/account.go:181-192`, `backend/internal/service/gateway_scheduling.go:708-736`）。筛选顺序为最低 priority 数值→可选最早 reset→最低负载→LRU，再抢真实 concurrency 槽（`backend/internal/service/gateway_scheduling.go:738-776`）。
5. **Layer 3 排队**：无可立即抢槽的账号时按 fallback 策略生成等待计划，不是直接创建无限 goroutine（`backend/internal/service/gateway_scheduling.go:779-793`）。

**混合调度**：Anthropic/Gemini Group 可纳入启用了 `mixed_scheduling` 的 Antigravity 账号；强制 Antigravity 路由不走混合，Group 内无账号也不会回退到全平台池（`backend/internal/service/gateway_scheduling.go:78-94`, `backend/internal/service/gateway_scheduling.go:1041-1095`, `backend/internal/service/gateway_scheduling.go:1148-1158`）。

**快照一致性**：调度快照启动时重建，并通过 outbox 与周期全量重建维护；缓存 miss 回源 DB，可配置回源超时/QPS（`backend/internal/service/scheduler_snapshot_service.go:171-199`, `backend/internal/service/scheduler_snapshot_service.go:211-272`）。候选过滤会再次调用 `Account.IsSchedulable()` 抵消快照延迟，最终选中还会水合完整 Account，避免快照精简模型导致凭证缺失（`backend/internal/service/gateway_scheduling.go:659-663`, `backend/internal/service/gateway_scheduling.go:1573-1600`）。

### 4.3 OpenAI 兼容调度引擎

OpenAI、Grok、Kimi、Zhipu、DeepSeek、MiniMax、OpenCode、Command Code、Cline 等由 OpenAI 网关族处理，但平台匹配保持精确，不会因共用协议而把不同平台账号混在一个候选池（`backend/internal/domain/platforms.go:37-51`, `backend/internal/service/openai_gateway_scheduling.go:289-299`）。

高级调度器由 DB 设置 `openai_advanced_scheduler_enabled` 控制；未启用时走 legacy sticky + 负载路径。设置读取有 5 秒缓存和 singleflight；粘性加权、订阅优先、Top-K 和权重覆盖均受总开关门控（`backend/internal/service/openai_account_scheduler.go:22-35`, `backend/internal/service/openai_account_scheduler.go:1886-2001`, `backend/internal/service/openai_account_scheduler.go:2579-2590`）。

高级调度 `defaultOpenAIAccountScheduler.Select` 的顺序为：

1. `previous_response_id` 硬粘连（满足平台、分组、能力和传输约束时）；
2. guardian parent Account；
3. `session_hash` 粘性，含 DB 复核、Grok 免费额度/Team+model 冷却、健康逃逸和并发槽；
4. 负载均衡（`backend/internal/service/openai_account_scheduler.go:382-493`）。

负载评分不是单纯“最小并发”。每个候选计算：

```text
score = Wpriority*priorityFactor + Wload*(1-loadRate/100)
      + Wqueue*queueFactor + WerrorRate*(1-errorEWMA)
      + Wttft*ttftFactor + Wreset*resetFactor
      + WquotaHeadroom*quotaFactor + WupstreamCost*(costFactor-neutral)
      + [可选] previous/session sticky 加分
```

具体归一化和公式见 `backend/internal/service/openai_account_scheduler.go:910-1038`。错误率与 TTFT 使用 alpha=0.2 的进程内 EWMA（`backend/internal/service/openai_account_scheduler.go:191-269`）。选 Top-K 后按“分值相对最低分+1”做无放回加权排序；有会话锚点时随机种子稳定，无锚点加入时间熵（`backend/internal/service/openai_account_scheduler.go:698-746`, `backend/internal/service/openai_account_scheduler.go:775-856`）。单次选号探测有 64 次上限，防止大池无限抢槽/复核（`backend/internal/service/openai_account_scheduler.go:30-36`, `backend/internal/service/openai_account_scheduler.go:305-351`）。

粘性并非永不逃逸：TTFT 或错误率 EWMA 超过阈值可逃离；默认有效阈值为 15 秒和 0.5，sticky escape 默认开启（`backend/internal/service/openai_account_scheduler.go:642-653`, `backend/internal/service/openai_account_scheduler.go:2593-2621`, `backend/internal/config/config.go:2710-2717`）。

### 4.4 Composite、渠道映射与账号映射是三层不同路由

- **Composite 层**：决定“公开模型→具体平台+上游模型”。显式路由内部按 exact 优先于 prefix、端点专用优先于 any、长前缀、较低 priority、较小 route ID 排序（`backend/internal/service/composite_route_resolver.go:182-252`）。未识别或有危险 OAuth 跨平台歧义时 fail-closed，而不是猜测平台（`backend/internal/server/routes/gateway.go:583-606`）。
- **Channel 层**：决定渠道级模型映射、定价与允许模型；`billing_model_source=upstream` 时需对每个 Account 的映射结果再检查限制（`backend/internal/service/gateway_forward.go:936-1005`）。
- **Account 层**：`model_mapping` 决定该账号是否支持公开/渠道模型及真正发往上游的模型；Composite 的 account-model 路由还要求只有显式映射声明方才拥有该别名（`backend/internal/service/openai_gateway_scheduling.go:389-445`）。

Composite 不负责自动比较多个平台的时延或价格，也不生成定价/能力记录；它主要是确定性的模型路由层（`docs/COMPOSITE_GROUPS.md:135-146`）。

### 4.5 订阅配额、标准余额与账号成本

#### 准入层次

`BillingCacheService.CheckBillingEligibility` 的顺序为：

1. standard 模式检查余额，subscription 模式检查订阅缓存；
2. 仅 standard 模式执行 user×platform quota；
3. 两种模式都执行 API Key 5h/1d/7d 金额窗口；
4. 最后执行 RPM（`backend/internal/service/billing_cache_service.go:740-790`）。

这意味着 **UserPlatformQuota 并不叠加到订阅模式**，不能把两种模式的所有限额误解为全部同时生效。API Key 的终身 quota/过期则由更早的 APIKeyAuth 检查（`backend/internal/server/middleware/api_key_auth.go:214-267`）。

RPM 的 user-group override 替代 Group 限额，但用户全局 RPM 始终同时生效；Redis 故障在该路径 fail-open（`backend/internal/service/billing_cache_service.go:925-1006`）。

#### 窗口语义

- 订阅日窗口按配置时区的日历日 0 点刷新；周窗口 7×24h、月窗口 30×24h，与订阅期限锚点对齐；最后一个不完整周期不重复发放（`backend/internal/service/user_subscription.go:113-175`）。
- 首次使用激活窗口；中间件需要维护时同步推进 DB 并回读，避免用缓存中的过期值放行（`backend/internal/service/subscription_service.go:871-984`）。
- 订阅分配/续期有行锁、事务和缓存失效；撤销/恢复、重置限额通过管理 API 操作（`backend/internal/service/subscription_service.go:290-379`, `backend/internal/server/routes/admin.go:684-705`）。
- 到期状态另有周期任务和 7/3/1 天提醒，多实例用 leader lock（`backend/internal/service/subscription_expiry_service.go:74-118`, `backend/internal/service/subscription_expiry_service.go:121-214`）。

#### 后扣费与成本口径

`CostBreakdown.TotalCost` 是原始成本，`ActualCost` 是应用分组/用户分组等倍率后的用户侧金额（`backend/internal/service/billing_service.go:200-225`）。定价解析链为 **Group→Channel→LiteLLM→Fallback**，支持 token、按次、图片、视频、长上下文、服务档和 reasoning 倍率（`backend/internal/service/model_pricing_resolver.go:47-72`, `backend/internal/service/billing_service.go:1556-1608`）。

后扣费时：

- 订阅消耗与用户余额扣减使用 `ActualCost`；
- API Key 总 quota 和 5h/1d/7d 窗口也使用 `ActualCost`；
- Account 本地配额使用 `TotalCost × account.rate_multiplier`，与用户售价分离；
- user×platform quota 仅 standard 模式累加 ActualCost（`backend/internal/service/gateway_usage_billing.go:329-350`, `backend/internal/service/gateway_usage_billing.go:428-459`）。

生产主路径通过 `UsageBillingRepository.Apply` 将 usage 和多种扣费作为原子命令执行，并用 request ID/请求体指纹去重；旧路径只作降级/测试 fallback（`backend/internal/service/gateway_usage_billing.go:289-392`）。账号的 `rate_multiplier` 不直接改变用户/API Key 扣费，这一语义也直接写在 Ent 字段注释中（`backend/ent/schema/account.go:110-114`）。

### 4.6 失败转移、限流与错误透传

#### 错误分类与转移循环

`UpstreamFailoverError` 不只携带状态码，还携带响应体/头、失败阶段、作用域、是否允许下一账号、同账号重试预算/退避、是否可安全在写出后转移、客户端状态和消息（`backend/internal/service/gateway_service.go:663-740`）。

通用 `FailoverState.HandleFailoverError` 的流程是：

1. context 已取消或 `NextAccountStop` 则终止；
2. `RetryableOnSameAccount` 时优先同账号重试，次数来自 Account 池模式配置，错误自身可进一步收紧；
3. 重试耗尽后按错误类型临时停调；
4. 把 Account 放入本次请求排除集，检查 `MaxSwitches`，再重新选号；
5. Antigravity 换号有递增延迟（`backend/internal/handler/failover_loop.go:191-267`）。

池模式只适用于 API Key/Bedrock 类账号，默认同账号重试 3 次、最大 10，默认状态码 401/403/429，可自定义列表；池模式默认不把上游错误写成账号永久状态（`backend/internal/service/account.go:1228-1369`, `backend/internal/service/ratelimit_service.go:331-349`）。

账号级冷却包括：

- `Account.IsSchedulable` 统一拦截非 active、手动停调、过期、429 reset、529 overload、临时封禁和账号配额超限（`backend/internal/service/account.go:194-215`）。
- 429 优先解析上游窗口/reset；无可靠值时使用可配置短冷却，源码默认 5 秒，范围 1–7200 秒（`backend/internal/service/ratelimit_service.go:75-78`, `backend/internal/service/ratelimit_service.go:1407-1432`）。
- 529 默认冷却 10 分钟，可被 DB 设置覆盖或关闭（`backend/internal/service/ratelimit_service.go:2018-2056`）。
- 临时不可调度规则支持状态码+关键词、时长、计数阈值和 account/model 两种隔离粒度（`backend/internal/service/account.go:135-149`, `backend/internal/service/temp_unsched.go:8-27`）。
- 配额阈值停调覆盖 OpenAI/Anthropic/Grok 及部分供应商窗口；Gemini/Kiro/Antigravity 明确不走该阈值停调器（`backend/internal/service/account_scheduling_threshold_eval.go:31-76`, `backend/internal/service/account_scheduling_threshold_eval.go:329-360`）。

#### 流式与副作用边界

- 已写出语义响应后通常不能换号拼接流；仅 SSE 注释等非语义字节可例外。客户端断开后不再发起新账号尝试（`backend/internal/handler/gateway_handler.go:1056-1076`, `backend/internal/handler/failover_loop.go:327-346`）。
- 插件返回 `request_sent=true` 时禁止自动重放，避免重复扣费/重复执行；只有确定未交给上游才允许 failover（`backend/internal/service/openai_upstream_transport_error.go:147-160`, `backend/internal/service/plugin_security_regression_test.go:182-223`）。
- 代理故障会临时停调关联账号；代理过期还可按 fallback 链改投可用备用代理或直连，检测环路/无效链（`backend/internal/service/openai_upstream_transport_error.go:153-185`, `backend/internal/service/proxy_fallback.go:5-48`）。
- OpenAI 代理流熔断若导致整池无候选，会再执行一次绕过隔离的选号，使隔离从硬拒绝退化为偏好（`backend/internal/service/openai_account_scheduler.go:2156-2195`）。

#### 错误透传不是 failover 本身

ErrorPassthroughRule 按平台、状态码、关键词和 any/all 匹配，可透传/改写客户端状态与消息、跳过监控；规则按 priority 排序，有 Redis+本地缓存和跨实例通知（`backend/ent/schema/error_passthrough_rule.go:40-118`, `backend/internal/service/error_passthrough_service.go:155-179`, `backend/internal/service/error_passthrough_service.go:182-307`）。命中后客户端错误类型统一为 `upstream_error`，但是否继续转移仍由 failover 错误元数据决定（`backend/internal/service/error_passthrough_runtime.go:30-71`）。

### 4.7 多平台适配

| 平台/形态 | 适配方式 |
|---|---|
| 平台登记表 | `domain.PlatformSpec` 统一声明 ID、显示名、网关族、国产供应商标识和 LiteLLM provider；Composite 遍历/定价顺序也由此派生（`backend/internal/domain/platforms.go:24-51`, `backend/internal/domain/platforms.go:104-137`） |
| OpenAI/Codex | OAuth、SetupToken、API Key、upstream 等凭证；支持 Responses、Chat Completions、HTTP/WS bridge、模型目录、Fast/Flex 和 Codex 身份/转换逻辑（`backend/internal/domain/constants.go:63-71`, `backend/internal/service/openai_gateway_forward.go:20-195`） |
| OpenAI 兼容供应商 | 入站与上游协议分别判定；可按模型目录/账号 `protocol_rules` 选择 Responses、Chat Completions 或 Anthropic，支持 adaptive 和原生同协议直通（`backend/internal/service/upstream_protocol_routing.go:11-49`, `backend/internal/service/upstream_protocol_routing.go:72-118`） |
| Anthropic/Bedrock | Bedrock 是 `platform=anthropic + type=bedrock`，不是独立 Platform；支持 SigV4 或 API Key，Claude 模型映射为区域化 Bedrock Model ID，请求转 InvokeModel/流式端点（`backend/internal/service/account.go:1421-1431`, `backend/internal/service/bedrock_request.go:150-187`, `backend/internal/service/gateway_bedrock.go:58-175`） |
| Gemini/Vertex | AI Studio 兼容入口统一 API Key/OAuth/Vertex Service Account；模型列表也会按账号类型转换或回退内置目录（`backend/internal/service/gemini_models_endpoints.go:20-22`, `backend/internal/service/gemini_models_endpoints.go:68-80`, `backend/internal/service/gemini_models_endpoints.go:162-187`） |
| Antigravity | 独立转发服务和配额探测；账号映射/默认映射、Claude/Gemini 兼容、模型级冷却和容量重试（`backend/internal/service/antigravity_gateway_service.go:22-65`, `backend/internal/service/antigravity_gateway_service.go:122-169`, `backend/internal/service/antigravity_quota_fetcher.go:29-92`） |
| Grok/xAI | OpenAI 网关族中的专用 Responses/Chat/媒体/语音/搜索路径；有会话缓存身份、Team+model 冷却、免费额度软门及主动配额探测（`backend/internal/service/openai_gateway_grok.go:34-118`, `backend/internal/service/grok_quota_service.go:91-128`, `backend/internal/service/grok_quota_service.go:140-220`） |
| OpenCode/Ollama 等 | 通过账号凭证中的 base URL、模式、协议和能力做专门规整/探测；例如 Ollama Cloud 的 429 会触发异步真实窗口探测（`backend/internal/service/ratelimit_service.go:33-38`） |
| Live attestation | 独立 `internal/platform/liveattestation` 抽象为 Live 请求生成 DeviceCheck；当前接口说明仅 macOS 实现，Windows 未实现（`backend/internal/platform/liveattestation/attestation.go:8-17`） |

## 5. 插件系统

### 5.1 形态：原生独立子进程，不是 WASM/JS

`.s2plugin` 本质是 ZIP，包含 `manifest.json`、生产必需的 `signature.json`、按 `goos-goarch` 选择的原生可执行文件和静态 UI。启用后由宿主 `exec.CommandContext` 拉起，使用 HashiCorp go-plugin 握手、本机 gRPC 与双向流传递 HTTP 请求/响应（`backend/internal/service/plugin_package.go:60-183`, `backend/internal/service/plugin_runtime.go:38-108`, `backend/pkg/pluginapi/v1/runtime.go:29-85`）。

**与 octopus(WASM Extism)/magpie(Bun+Go builtin) 的差别：Sub2API 选择“原生可执行子进程 + go-plugin/gRPC”，无共享内存 ABI，也没有 WASM 或内嵌 JS 运行时。** 更重要：子进程只是代码/发布边界，**不是操作系统安全沙箱**，它继承 Sub2API 服务用户可访问的文件、网络等权限，因此只能安装可信发布者包（`backend/pkg/pluginapi/README.md:12-20`, `backend/pkg/pluginapi/docs/security.md:11-25`）。

### 5.2 包规范与 manifest

`PluginManifest` 字段如下（`backend/internal/service/plugin_manifest.go:32-74`；JSON Schema 见 `backend/pkg/pluginapi/v1/manifest.schema.json:5-75`）：

| 字段 | 语义 |
|---|---|
| `schema_version` | 当前固定 1 |
| `id/name/version/description/author` | 小写命名空间 ID、展示信息、语义化版本 |
| `requires.sub2api` | 宿主硬兼容版本范围 |
| `recommended_sub2api_version` / `tested_sub2api_versions` | 建议版本与实际测试版本；兼容但未测试时需管理员确认 |
| `plugin_protocol` / `transport_api` / `ui_bridge` | 三项独立协议版本，当前均为 1 |
| `capabilities[]` | 每项含 `id/platform/account_type`；当前仅允许 `openai.oauth.outbound_transport.v1 + openai + oauth` |
| `runtimes` | `goos-goarch → 可执行文件相对路径` |
| `ui.entrypoint` | 必须位于 `ui/` |
| `files` | 每个运行时/UI 文件的小写 SHA-256；清单和签名自身不列入 |

安装器会限制上传大小、解压大小和文件数（默认 128 MiB/256 MiB/512 文件），拒绝路径穿越、绝对路径、重复路径、符号链接、未声明/缺失文件，并逐一校验实际 SHA-256（`backend/internal/config/config.go:2411-2417`, `backend/internal/service/plugin_package.go:186-252`, `backend/internal/service/plugin_package.go:302-379`）。

`signature.json` 含 `algorithm=ed25519`、`key_id`、Base64 signature；签名对象是 manifest 原始字节。官方 OpenAI Transport 公钥内置且绑定固定插件 ID，第三方公钥来自 `plugins.trusted_publishers`；生产默认不允许未签名包（`backend/internal/service/plugin_package.go:255-300`, `backend/pkg/pluginapi/docs/package-format.md:31-45`）。

兼容性同时检查宿主 semver 范围和三个协议版本；未列入 tested 版本并不等同不兼容，但启用需 `accept_untested`（`backend/internal/service/plugin_compatibility.go:16-51`, `backend/internal/service/plugin_manager.go:557-600`）。

### 5.3 能力、权限与 Host Services

#### 传输 RPC

`TransportPlugin` 暴露 `GetInfo`、`Health`、`ValidateConfig`、`ApplyConfig`、`TestConfig`、双向流 `Forward` 和可选 `InitHostServices`（`backend/pkg/pluginapi/v1/plugin.proto:7-19`）。Forward start 帧含 method、完整 URL、Host、headers、proxy URL、Account ID/并发/平台/类型、Content-Length 和是否有 body；随后传 body chunk/end，响应同样流式返回（`backend/pkg/pluginapi/v1/plugin.proto:100-151`）。

#### 反向 HostService

宿主通过 go-plugin broker 启动 HostService，插件在 `InitHostServices` 中取得 broker ID 后反拨；老插件返回 Unimplemented 会静默跳过，不影响传输（`backend/internal/service/plugin_runtime.go:105-153`）。当前 HostService API 版本为 2（`backend/pkg/pluginapi/v1/runtime.go:17-26`）。

| 能力 | 权限边界与资源限制 |
|---|---|
| `KVGet/KVSet/KVDelete/KVList` | Redis 支撑，跨请求/副本/重启；宿主按运行时注入 pluginKey，插件不能伪造；namespace 128、key 256、值 256 KiB、List 默认 100/最大 1000、TTL 最大 90 天（`backend/internal/service/plugin_host_services.go:14-37`, `backend/internal/service/plugin_host_services.go:138-253`, `backend/internal/service/plugin_host_services.go:317-370`） |
| `ListAccounts` | 仅返回能力范围内的活跃账号非机密视图，含 `schedulable` 和 metadata JSON；排除影子账号；不含 Credentials（`backend/internal/service/openai_plugin_account_directory.go:23-85`, `backend/internal/service/openai_plugin_account_directory.go:88-119`） |
| `ResolveOutboundIdentity` | 敏感能力：仅在能力范围内解析短期 access token、出站 headers 和 proxy URL；不提供 refresh token（`backend/internal/service/plugin_host_services.go:39-48`, `backend/internal/service/openai_plugin_account_directory.go:121-166`） |

账号可见范围由 **宿主固定的能力→scope 映射** 推导，而不是信任插件在 manifest 中任意扩大 platform/account_type；当前映射只有 OpenAI OAuth（`backend/internal/service/plugin_manager.go:1058-1095`）。

### 5.4 生命周期与多实例

状态包括 `disabled/starting/enabled/error/incompatible`（`backend/internal/service/plugin_manifest.go:17-26`）。主要流程：

1. **上传安装**：验签、验哈希、解包到受管目录，保存原始包和 DB 安装记录，默认停用并生成未启用 binding（`backend/internal/service/plugin_manager.go:183-234`）。
2. **启用**：校验兼容性、tested 确认、灰度 1–100；DB CAS 进入 starting，启动 runtime，GetInfo/Health/配置校验通过后写 enabled 并发布路由（`backend/internal/service/plugin_manager.go:557-631`）。
3. **运行协调**：每 1 秒以 DB 为权威对齐各实例，检查进程健康、版本哈希、加密配置和灰度；同一 OpenAI OAuth 能力只允许一个启用插件（`backend/internal/service/plugin_manager.go:250-388`；DB 部分唯一索引见 `backend/migrations/229_plugins.sql:31-54`）。
4. **缺文件恢复**：本地安装缺失或哈希不符时从 DB 读取原始包，重新验签解包；多实例不要求共享插件目录，但须共享 DB 与加密密钥（`backend/internal/service/plugin_manager.go:433-472`, `backend/pkg/pluginapi/README.md:18-20`）。
5. **配置**：先由插件 ValidateConfig/ApplyConfig，再加密写 DB；DB 写失败尝试恢复旧运行时配置（`backend/internal/service/plugin_manager.go:697-783`）。
6. **停用/删除/停止**：先停止接收新请求，drain 在途请求最多 10 秒再 Kill；启用中的插件须先停用才能删除（`backend/internal/service/plugin_runtime.go:233-271`, `backend/internal/service/plugin_manager.go:634-686`）。
7. **故障**：已绑定插件不可用时不会静默回退到内置 OAuth 传输，而是 fail-closed；避免同一账号网络行为悄悄改变（`backend/internal/service/plugin_manager.go:947-978`, `backend/pkg/pluginapi/docs/security.md:3-16`）。

安装与 binding 使用 `sub2api_plugin_installations` / `sub2api_plugin_bindings`，不是 Ent schema；状态更新带 expected state/binary hash/config 条件以防多实例覆盖（`backend/migrations/229_plugins.sql:5-54`, `backend/internal/repository/plugin_repo.go:150-260`）。

### 5.5 与网关路由/账号选择的真实关系

当前唯一能力是 **OpenAI OAuth 出站 HTTP Transport**，不是通用路由决策插件：

- 核心调度器先选出 Account；`doOpenAIUpstream` 再把该账号对应的已构造 HTTP 请求交给 `PluginManager.RoundTripOpenAIOAuth`，未命中才走内置 `HTTPUpstream`（`backend/internal/service/openai_plugin_transport.go:9-18`）。
- 命中条件仅为 `platform=openai`、`type=oauth`、存在启用 route，且 `stablePluginBucket(account.ID) < rolloutPercent`。灰度以账号 ID 稳定分桶，不是每请求随机（`backend/internal/service/plugin_manager.go:981-989`, `backend/internal/service/plugin_manager.go:1159-1165`）。
- API Key 账号、其他 Provider、OAuth 登录与 token 刷新不进入插件（`backend/pkg/pluginapi/README.md:22-31`；路由回归测试见 `backend/internal/service/plugin_manager_routing_test.go:40-106`）。
- 命中插件的 OAuth WebSocket 账号强制使用 HTTP Bridge，不直接建立上游 WS，因为 v1 插件只实现 HTTP（`backend/internal/service/openai_ws_forwarder_ingress.go:117-123`）。
- 插件建立真实 HTTP/TLS 连接；宿主仍负责响应状态、SSE 解析、错误映射、用量、计费和下游输出（`backend/pkg/pluginapi/docs/development.md:3-7`）。

因此，`plugin_manager_routing_test.go` 中的“routing”应理解为 **选中账号后的传输路由/灰度**，不能据此宣称插件能替换 `OpenAIAccountScheduler`。HostService 虽允许插件读取账号目录并自行做内部决策，但核心请求链没有看到“由插件返回 Account ID 来替代核心选号”的接口。

### 5.6 插件 UI 与管理 API

管理前端 `/admin/plugins` 对应 `PluginsView.vue`；配置 UI 在 `sandbox="allow-scripts"` 且无 `allow-same-origin` 的 iframe 中运行（`frontend/src/router/index.ts:551-561`, `frontend/src/views/admin/PluginsView.vue:315-319`）。

每次打开创建 30 分钟 UI 会话，资源 URL 带加密短时 asset token，Bridge Token 只放在 fragment；CSP 禁止外部连接、表单和外部 frame，静态资源还必须属于已验签清单并再次核对哈希（`backend/internal/handler/admin/plugin_handler.go:24-25`, `backend/internal/handler/admin/plugin_handler.go:202-255`, `backend/internal/service/plugin_manager.go:842-944`）。Bridge 消息同时校验窗口来源、来源标识、Bridge Token 和 request ID；读状态免二次验证，配置写入/主动测试需要 step-up（`backend/pkg/pluginapi/docs/ui-bridge.md:3-13`, `backend/pkg/pluginapi/docs/ui-bridge.md:39-59`）。

管理 API（统一前缀 `/api/v1`）：

| 方法与路径 | 用途 | Step-up |
|---|---|---|
| `GET /admin/plugins`、`GET /admin/plugins/:id` | 列表/详情 | 否 |
| `POST /admin/plugins/upload` | multipart 字段 `plugin` 上传 `.s2plugin` | 是 |
| `POST /admin/plugins/:id/enable` | `accept_untested`、`rollout_percent` | 是 |
| `POST /admin/plugins/:id/disable`、`DELETE /admin/plugins/:id` | 停用/卸载 | 是 |
| `GET /admin/plugins/:id/config`、`GET /admin/plugins/:id/status` | 读配置/被动运行状态 | 否 |
| `PUT /admin/plugins/:id/config`、`POST /admin/plugins/:id/test` | 保存/测试配置 | 是 |
| `POST /admin/plugins/:id/ui-session` | 创建隔离 UI 会话 | 路由本身否，但后续写操作需二次验证 |

路由注册证据：`backend/internal/server/routes/admin.go:766-780`；请求 DTO 与 UI URL 生成：`backend/internal/handler/admin/plugin_handler.go:57-103`, `backend/internal/handler/admin/plugin_handler.go:202-223`。

### 5.7 其他扩展面：managementmcp 与 securityaudit

- **managementmcp**：不是另一套业务 API，而是把白名单 ToolSpec 映射到私有 Gin router 中已有的用户/管理 HTTP 路由，复用所有权、守卫和审计；按 user/admin scope 与 readOnly 组装四套不可变工具集（`backend/internal/server/routes/mcp.go:19-42`, `backend/internal/managementmcp/server.go:27-62`）。工具目录覆盖 Key、Group、Account、User、Subscription、Usage、Ops 等代表操作，参数 schema 禁止任意 URL/method/凭证（`backend/internal/managementmcp/catalog.go:6-23`, `backend/internal/managementmcp/catalog.go:85-151`）。
- **securityaudit**：Coordinator 组合 legacy 内容审核与 PromptEngine；Prompt 可异步记录或 blocking 同步评估，blocking 模式并行运行两者并统一决策（`backend/internal/securityaudit/coordinator.go:20-49`, `backend/internal/securityaudit/coordinator.go:52-126`）。它在选号前执行，能拒绝请求，但不是账号选择插件（`backend/internal/handler/gateway_handler.go:236-239`）。

## 6. 关键参数与特点

### 6.1 默认参数速查

| 参数 | 源码默认/语义 |
|---|---|
| `gateway.max_account_switches` | 10；Gemini 单独为 3，Gemini 同账号重试默认 4（`backend/internal/config/config.go:2488-2491`） |
| `gateway.scheduling.sticky_session_max_waiting` / `sticky_session_wait_timeout` | 3 / 120 秒（`backend/internal/config/config.go:2608-2609`） |
| `fallback_wait_timeout` / `fallback_max_waiting` | 30 秒 / 100（`backend/internal/config/config.go:2610-2611`） |
| `load_batch_enabled` / `load_batch_cache_ttl_ms` | true / 200 ms（`backend/internal/config/config.go:2614-2615`） |
| 调度 DB fallback | 默认开启，额外 timeout=0、QPS=0；快照 outbox 1 秒、全量重建 300 秒（`backend/internal/config/config.go:2619-2627`） |
| OpenAI `lb_top_k` | 7；sticky session/response ID TTL 默认 3600 秒（`backend/internal/config/config.go:2539-2545`） |
| OpenAI 调度权重 | priority=1、load=1、queue=0.7、error_rate=0.8、TTFT=0.5；reset/quota_headroom/upstream_cost 默认 0；previous=5、session sticky=3（`backend/internal/config/config.go:2546-2555`） |
| OpenAI sticky escape | 默认开启；运行时零值归一为 TTFT 15000 ms、error rate 0.5（`backend/internal/service/openai_account_scheduler.go:2593-2621`） |
| 池模式 | 默认同账号重试 3，最大 10；默认 401/403/429（`backend/internal/service/account.go:1242-1289`） |
| 通用上游内部重试 | 最多 5 次（含首次），退避 300 ms 起、最大 3 秒，总预算 10 秒；与外层换号预算是两层（`backend/internal/service/gateway_forward.go:22-65`） |
| 429 fallback / 529 | 5 秒 / 10 分钟，均可由运行设置改变（`backend/internal/service/ratelimit_service.go:75-78`, `backend/internal/config/config.go:2398-2400`） |
| 订阅 L1 | 16384 条、10 秒、10% TTL 抖动（`backend/internal/config/config.go:2438-2441`） |
| 在途余额预留 | 默认开启，TTL 900 秒；费用估算默认 `default_max_tokens=8192`，`max_output_tokens=128000`；无法估价默认 fail-open（`backend/internal/config/config.go:2187-2195`） |
| 用量 worker | 128 workers、队列 16384、5 秒任务超时，溢出默认 sync；自动扩至 512（`backend/internal/config/config.go:2628-2644`） |
| 插件包 | 默认拒绝未签名；上传 128 MiB、解压 256 MiB、启动 15 秒（`backend/internal/config/config.go:2411-2417`） |

### 6.2 架构特点与边界

1. **Group 是核心路由边界**：Key→Group→AccountPool，不会因分组内无账号而任意泄漏到全平台账号池（`backend/internal/service/gateway_scheduling.go:1086-1095`）。
2. **粘性与健康不是二选一**：粘性账号每次仍需通过健康、模型、配额、渠道和 DB 复核；OpenAI 还可按 EWMA 逃逸（`backend/internal/service/openai_account_scheduler.go:526-613`）。
3. **负载分母与真实槽位分离**：`load_factor` 可影响排序，但实际等待/抢槽使用 `concurrency`（`backend/internal/service/account.go:181-192`, `backend/internal/service/openai_account_scheduler.go:589-610`）。
4. **计费是选后、异步但有界**：成功的上游消耗和流中断前已观察到的部分 usage 都会进入记录链；worker 溢出默认 sync，避免静默丢账（`backend/internal/handler/openai_gateway_handler.go:815-859`, `backend/internal/config/config.go:2628-2634`）。
5. **失败转移有多重安全闸**：账号排除、切换预算、同账号预算、语义流写出、request_sent、客户端取消和幂等后扣费共同限制重放（`backend/internal/handler/failover_loop.go:191-267`, `backend/internal/service/openai_upstream_transport_error.go:147-160`）。
6. **Redis 故障策略并不统一**：面板认证/登录部分限流 fail-close，RPM/部分软配额/在途预留则 fail-open；不能笼统说“所有限制均 fail-open”或“均 fail-close”（`backend/internal/server/routes/auth.go:34-56`, `backend/internal/service/billing_cache_service.go:925-934`, `backend/internal/service/billing_inflight_reservation.go:248-256`）。
7. **插件面目前很窄但权限很深**：只接管 OpenAI OAuth 出站传输，却能处理真实 Authorization 和账号身份；包签名+进程权限+UI 沙箱共同构成边界，而非 WASM 能力沙箱（`backend/pkg/pluginapi/docs/security.md:27-29`）。

## 7. 证据索引（按主题）

| 主题 | 首选入口 |
|---|---|
| 项目定位/功能 | `README_CN.md:192-225` |
| 开发结构与依赖 | `DEV_GUIDE.md:313-339`, `backend/go.mod:1-65`, `frontend/package.json:18-61` |
| 路由总装 | `backend/internal/server/router.go:98-137` |
| 数据面路由 | `backend/internal/server/routes/gateway.go:179-358`, `backend/internal/server/routes/gateway.go:360-522` |
| 管理/用户路由 | `backend/internal/server/routes/admin.go:13-135`, `backend/internal/server/routes/user.go:11-165` |
| 前端页面与守卫 | `frontend/src/router/index.ts:20-750`, `frontend/src/router/index.ts:805-1003` |
| 内嵌前端 | `backend/internal/web/embed_on.go:29-117` |
| 核心 Ent 模型 | `backend/ent/schema/account.go:50-230`, `backend/ent/schema/group.go:36-331`, `backend/ent/schema/api_key.go:34-133`, `backend/ent/schema/user_subscription.go:36-118`, `backend/ent/schema/usage_log.go:32-218` |
| 通用选号 | `backend/internal/service/gateway_scheduling.go:100-793` |
| OpenAI 选号 | `backend/internal/service/openai_account_scheduler.go:382-493`, `backend/internal/service/openai_account_scheduler.go:859-1117`, `backend/internal/service/openai_account_scheduler.go:2298-2471` |
| 并发与快照 | `backend/internal/service/concurrency_service.go:21-56`, `backend/internal/service/concurrency_service.go:309-415`, `backend/internal/service/scheduler_snapshot_service.go:171-299` |
| Composite | `backend/internal/service/composite_route_resolver.go:97-253`, `backend/internal/server/routes/gateway.go:549-638` |
| 配额/扣费 | `backend/internal/service/billing_cache_service.go:740-790`, `backend/internal/service/subscription_service.go:961-1095`, `backend/internal/service/gateway_usage_billing.go:289-465` |
| 失败转移 | `backend/internal/handler/failover_loop.go:124-318`, `backend/internal/service/ratelimit_service.go:300-395` |
| 插件包/运行时 | `backend/internal/service/plugin_package.go:60-396`, `backend/internal/service/plugin_runtime.go:38-153`, `backend/internal/service/plugin_runtime.go:274-448` |
| 插件生命周期/路由 | `backend/internal/service/plugin_manager.go:557-686`, `backend/internal/service/plugin_manager.go:947-989`, `backend/internal/service/plugin_manager.go:1011-1095` |
| 插件权限/协议 | `backend/internal/service/plugin_host_services.go:14-370`, `backend/pkg/pluginapi/v1/plugin.proto:7-39`, `backend/pkg/pluginapi/v1/plugin.proto:100-274` |
| 插件管理 API/UI | `backend/internal/server/routes/admin.go:766-780`, `backend/internal/handler/admin/plugin_handler.go:24-273`, `backend/pkg/pluginapi/docs/ui-bridge.md:3-59` |
| 管理 MCP/安全审计 | `backend/internal/managementmcp/server.go:27-197`, `backend/internal/securityaudit/coordinator.go:20-139` |

## 8. 未确认项与调查限制

1. **未确认运行时实际配置**：本报告默认值来自源码，未连接实际 PostgreSQL/Redis，未读取部署环境变量或数据库中的 OpenAI 高级调度、429/529、订阅等覆盖值。
2. **未确认工作树对应的发布版本**：本次只读当前文件内容，没有以 Git 提交或 Release 对照核验版本；`PluginHostInfo.Version` 也是运行时注入值。
3. **未执行真实插件集成测试**：`plugin_runtime_integration_test.go` 需要 `SUB2API_TEST_PLUGIN_PACKAGE` 指向真实包，否则主动跳过（`backend/internal/service/plugin_runtime_integration_test.go:26-38`）。本次没有该包，也未运行测试；关于子进程行为的结论来自实现和测试源码。
4. **未对 octopus/magpie 源码做本轮交叉核验**：二者名称与技术形态按任务给定背景作一句架构对照；Sub2API 自身结论均来自本仓库。
5. **未发现通用“插件替换账号选择器”接口**：当前 manifest 和 Manager 只支持 OpenAI OAuth 出站传输；HostService 可读账号目录不等于核心 scheduler 委派。若后续版本新增路由插件，需要重新核对 manifest、scheduler 和 gateway forward 的调用链。
6. **未穷尽逐端点差异**：报告列出路由组与代表端点；WebSocket 多轮、图片批处理、支付回调、各供应商专有错误文案仍有大量细粒度分支，应以相应 Handler/Service 为准。
