---
name: backend-foundation
description: Sepia Rust 后端已具备三协议真实推理闭环；React 已联调真实管理 API，插件尚未实现。
metadata:
  type: project
---

2026-11-10 已批准 `docs/ryugi/specs/2026-11-10-backend-foundation-design.md`，并实现独立 Rust/Axum 服务 `backend/`。支持 OpenAI、Anthropic、Gemini 原生文本入站与出站、跨协议 JSON/SSE、SQLite 配置、管理员会话、加密供应商 Key、客户端 Key 哈希与撤销、模型/额度策略、ordered failover、真实模型发现和请求/尝试记录。

**Why:** 用户选择优先完成真实推理纵向闭环，再联调前端和实现插件。首项不要求真实第三方账户，使用本地可控 HTTP 上游验证。

**How to apply:** 后续管理前端联调直接复用 `/api/v1` 与 `/v1` 契约，不新建第二套路由或协议逻辑。React 通过 `src/gateway-workspace.ts` 与 `src/api.ts` 调用真实管理 API；浏览器只保存界面偏好，Provider/路由/密钥/请求数据由网关持久化。不得声称插件已经可执行。首项仅 ordered failover，多媒体、工具调用、会话亲和、熔断等未实现字段必须显式拒绝。插件后续采用 Wasmtime、版本化清单、能力授权和 UI 沙箱。

2026-11-11 独立安全审查后补齐：禁止上游重定向防 Key 泄漏、真实金额定价与日额度、请求断连 guard、400/404/429 状态与配额释放、原生 SSE 生命周期和 UTF-8 分帧、不可变配置快照、多 Key 轮换/冷却、Retry-After/尝试上限、IP/CIDR/分组/模型额度、管理 Origin/Secure Cookie 与生产 SPA 托管。`backend/README.md` 记录运行方式。31 项 Rust 测试、Clippy、前端构建与 16 项前端测试通过；实际启动服务验证初始化/登录/Key 创建撤销/模型接口/SPA，未使用真实第三方供应商账户。

2026-11-11 React 联调完成：真实管理员初始化/登录、Provider 创建与启停、路由名称/描述持久化、客户端 Key 创建/撤销、请求历史刷新与周期分析均通过浏览器验证；本地 OpenAI 上游返回真实 `pong`，页面显示成功请求、Token 和 Provider 统计。路由元数据使用 `0005_route_metadata.sql`，分析 API 使用统一 period 窗口。插件仍仅展示未实现空状态。

2026-11-12 项目检查修复客户端 Key 过期时间：前端不再把 `datetime-local` 按 UTC 重新解释，而是原样传递所选时间；网关将无时区时间规范化为 UTC，并在创建时拒绝无效或已过期日期，避免生成无法鉴权的 Key 或返回 500。34 项 Rust 测试、22 项前端测试、Clippy、格式检查与生产构建通过，隔离网关验证了保存时间、鉴权和 400 错误。

Windows Rust MSVC 构建需要 Visual Studio C++ Build Tools；Vite 必须忽略 `backend/target`，否则 watcher 可能因编译中的 EXE 返回 EBUSY。

2026-11-12 项目检查修复两项管理与流式边界：编辑 Provider 时协议与其它配置在同一更新事务中持久化；上游 SSE 分帧同时接受 LF 和 CRLF，避免标准 CRLF 响应丢失内容、用量和终止事件。新增管理 API、存储更新和流式回归测试；35 项 Rust 测试、22 项前端测试、Clippy、格式检查、类型检查与生产构建通过。构建仍提示 `charts` 与 `three` chunk 超过 500 KB。
