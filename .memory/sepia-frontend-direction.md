---
name: sepia-frontend-direction
description: Sepia 前端使用 React 18/Vite/Tailwind/Headless UI，视觉为深绿玻璃拟态、薄荷绿渐变与克制的 Three.js 效果。
metadata:
  type: project
---

用户指定 Sepia 前端技术栈为 React 18 + TypeScript、Vite、Tailwind CSS + Headless UI，完全自定义样式；使用 backdrop-filter 玻璃拟态、CSS/Mesh Gradient，Three.js / React Three Fiber 作为可选视觉冲击。界面采用深绿黑底、薄荷绿主色、细边框玻璃面板，现代、高级，避免普通模板感。主架构后续若实现后端，用户接受 Rust + Axum + Tokio 的方向。

**Why:** 用户明确提出这套技术与审美要求，强调玻璃拟态和高级感；在后续前端改造中也要求保留已经看过的整体视觉语言。

**How to apply:** 新页面与组件延续此风格和技术栈，优先清晰的网关信息层次；3D 应服务路由拓扑表达，不牺牲可读性与移动端体验。不要改用其他 UI 框架或套用通用后台模板。实施顺序遵守 [[frontend-approval-gate]]。

2026-11-12 模型拉取合并为单一按钮，新建使用无持久化 `POST /api/v1/providers/test` 预览端点；首次拉取只回填模型并保持弹窗，点击“连接供应商”后才创建。编辑仍需显式保存。真实 UI 验证拉取前后 Provider 数不变，保存后模型正确持久化。

2026-11-12 修复新建 Provider 自动发现 401：前端此前只提交 `secrets` 数组，`secret` 为空，后端向上游发送空 Bearer。现在首个 Key 同时写入 `secret` 与 `secrets`，后端发现支持数组兜底，保存去重保证留空测试可回退。真实 UI 验证创建成功，前端 21 项测试和 Rust 全量测试通过。

2026-11-11 修复编辑 Provider 拉取模型忽略新 Key：测试端点接受当前协议、地址和首个新 Key，留空才回退已保存 Key；401 等上游错误返回清理后的认证提示。真实浏览器验证当前 Key 成功重新获取 32 个模型。

2026-11-11 共享 Dropdown 的普通和紧凑菜单统一为各自触发器的 `width: 100%`，不再使用 310px/220px 固定宽度。长选项省略显示并通过 `title` 提供完整名称；已验证桌面 Provider 表单与 390px 移动端等宽且不溢出。

2026-11-11 Provider 模型管理新增“拉取模型”主按钮。已保存 Provider 的拉取/刷新/测试使用真实 `POST /api/v1/providers/:id/test`，只更新弹窗草稿，点击“保存供应商”后持久化；新建 Provider 的拉取/测试提交当前表单到创建端点，后端在模型为空时先发现再创建，成功自动关闭弹窗，失败不创建 Provider。加载期间禁用模型变更，编辑态失败保留原草稿。
