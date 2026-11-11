---
name: sepia-frontend-direction
description: Sepia 前端使用 React 18/Vite/Tailwind/Headless UI，视觉为深绿玻璃拟态、薄荷绿渐变与克制的 Three.js 效果。
metadata:
  type: project
---

用户指定 Sepia 前端技术栈为 React 18 + TypeScript、Vite、Tailwind CSS + Headless UI，完全自定义样式；使用 backdrop-filter 玻璃拟态、CSS/Mesh Gradient，Three.js / React Three Fiber 作为可选视觉冲击。界面采用深绿黑底、薄荷绿主色、细边框玻璃面板，现代、高级，避免普通模板感。主架构后续若实现后端，用户接受 Rust + Axum + Tokio 的方向。

**Why:** 用户明确提出这套技术与审美要求，强调玻璃拟态和高级感；在后续前端改造中也要求保留已经看过的整体视觉语言。

**How to apply:** 新页面与组件延续此风格和技术栈，优先清晰的网关信息层次；3D 应服务路由拓扑表达，不牺牲可读性与移动端体验。不要改用其他 UI 框架或套用通用后台模板。实施顺序遵守 [[frontend-approval-gate]]。

2026-11-11 Provider 模型管理新增“拉取模型”主按钮，与“重新获取模型”共用真实 `POST /api/v1/providers/:id/test`；拉取期间禁用模型变更，成功仅替换弹窗草稿，点击“保存供应商”后持久化，失败保留原草稿。
