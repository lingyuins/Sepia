---
name: backend-foundation
description: Sepia Rust 后端已具备三协议真实推理闭环；React 演示 UI 尚未联调，插件尚未实现。
metadata:
  type: project
---

2026-11-10 已批准 `docs/ryugi/specs/2026-11-10-backend-foundation-design.md`，并实现独立 Rust/Axum 服务 `backend/`。支持 OpenAI、Anthropic、Gemini 原生文本入站与出站、跨协议 JSON/SSE、SQLite 配置、管理员会话、加密供应商 Key、客户端 Key 哈希与撤销、模型/额度策略、ordered failover、真实模型发现和请求/尝试记录。

**Why:** 用户选择优先完成真实推理纵向闭环，再联调前端和实现插件。首项不要求真实第三方账户，使用本地可控 HTTP 上游验证。

**How to apply:** 后续管理前端联调直接复用 `/api/v1` 与 `/v1` 契约，不新建第二套路由或协议逻辑。不得声称 React 已接真实后端，也不得声称插件已经可执行。首项仅 ordered failover，多媒体、工具调用、会话亲和、熔断等未实现字段必须显式拒绝。插件后续采用 Wasmtime、版本化清单、能力授权和 UI 沙箱。

Windows Rust MSVC 构建需要 Visual Studio C++ Build Tools；Vite 必须忽略 `backend/target`，否则 watcher 可能因编译中的 EXE 返回 EBUSY。
