# Sepia 后端基础设计

## 1. 目标与范围

为个人与小团队交付可实际调用的本地 AI 网关。首个完整变更贯通持久化配置、管理员鉴权、供应商密钥保护、OpenAI/Anthropic/Gemini 原生入站与出站、跨协议文本转换、JSON/SSE、候选路由、重试和请求/尝试观测。使用本地可控 HTTP 上游做端到端验收；真实第三方账户与插件执行不在本项验收中。

后续依次为管理界面联调、WASM 插件闭环和完整首版验收。插件采用 Wasmtime、版本化清单与能力授权；当前只保留边界，不创建插件桩代码。

## 2. 已确认决策

- 真实推理纵向闭环优先，不先做纯 CRUD。
- 首批支持三种原生协议入站/出站及跨协议 JSON/SSE 文本转换。
- 操作系统钥匙串保存主密钥；SQLite 保存加密供应商密钥；客户端 Key 只保存哈希，创建时仅返回一次明文。
- 管理员会话与推理 Bearer Key 分离；局域网不绕过鉴权。
- 每项完整变更更新 AGENTS、memory 并本地提交，不推送。

## 3. 技术与模块

Rust 稳定工具链、Axum/Tokio 单进程、Reqwest 连接池、SQLite/SQLx。配置事务提交后发布不可变快照；每个请求读取一次快照，避免全局锁串联上游请求。

- `server/`：HTTP、鉴权、DTO 校验、错误映射。
- `relay/`：请求状态机、重试边界、取消传播、SSE。
- `routing/`：候选排序、健康、Key 策略与选择。
- `providers/`：统一模型、协议转换、模型发现。
- `store/`：迁移、事务、密文/哈希、观测查询。

## 4. 协议契约

| 协议 | 入站 | 出站 |
|---|---|---|
| OpenAI | `POST /v1/chat/completions`；`GET /v1/models` | `/chat/completions`；`/models` |
| Anthropic | `POST /v1/messages` | `/v1/messages`；`/v1/models` |
| Gemini | `POST /v1beta/models/{model}:generateContent`、`:streamGenerateContent` | 同路径与模型列表 |

统一模型保存 system/developer 指令、user/assistant 多轮文本、model、max_tokens、temperature、top_p、stop、stream 和客户端请求 ID。不能无损转换的 image/file/tool/function 内容返回 400 `unsupported_feature`，不静默丢弃。未知扩展字段不保证透传。

非流式统一 finish_reason、文本与 input/output token；流式统一 message_start、content_delta、usage、message_stop 和错误事件，再映射回客户端协议。保留协议 SSE event 名、字段与结束标记（含 OpenAI `[DONE]`）。Gemini `alt=sse` 由服务端固定。

## 5. 配置、密钥与管理 API

一次性本机引导令牌用于设置管理员用户名和密码。密码 Argon2id；HttpOnly、SameSite=Strict 随机会话 Cookie，数据库只存会话哈希。生产非环回监听要求 TLS 或显式可信代理。开发测试可用测试密钥库，生产禁止禁用密钥保护。

管理接口包含：引导/登录/退出/当前会话；业务设置；Provider、渠道组和多个上游 Key；路由和有序候选；客户端 Key 创建/撤销/启停/额度与策略；请求/尝试/聚合观测；真实模型发现与连接测试。浏览器主题和语言留在客户端。

客户端 Key 的模型/分组/额度/IP/过期策略在鉴权后、路由前求值。上游密钥只入不出；请求日志默认不记正文。开启正文记录时结构化脱敏 Authorization/Cookie/API Key，限制大小与保留期。管理写操作需要 CSRF 或等效同源校验，CORS 默认关闭。

## 6. 数据、路由与重试

核心表：管理员/会话、设置、Provider、渠道组、上游 Key、模型、路由、候选、客户端 Key/策略、请求、尝试、使用事件。金额以最小货币单位整数存储，token/延迟用整数，成功率从记录派生。

模型先匹配路由再生成有序候选；无匹配返回 404。每轮记录 Provider、Key ID、上游模型、开始/结束、状态、重试原因、token、首字节延迟，不记录 Authorization。

- 无效输入或不支持特性：400，不调用上游。
- 鉴权/额度/策略失败：401/403/429，不调用上游。
- 连接失败、连接前超时、408/429/可重试 5xx：仅在未向客户端输出时按策略重试，遵守有限 Retry-After 和总预算。
- 其他上游 4xx：映射安全错误，不盲目重试。
- 已输出响应或流已开始：不换供应商重放，发送协议错误或终止流。
- 所有候选不可用：最后一个安全错误或 503，请求和尝试均可查询。
- 客户端断开：取消上游与重试，状态 cancelled。

## 7. 验收

1. 三种入站到三种出站的非流式/SSE 矩阵，覆盖 system、多轮、参数、用量、finish_reason 与不支持内容。
2. 本地 HTTP 上游校验路径、鉴权头、请求体、模型发现和连接错误。
3. 路由匹配、候选顺序、失败重试、无候选、输出后失败与客户端取消。
4. 会话、Bearer Key、额度/模型/IP/过期策略、无明文列表、日志脱敏。
5. 迁移、重启恢复、事务回滚、并发请求一致快照。
6. 启动 Axum 服务，以三种客户端协议调用三种模拟上游，读取 JSON/SSE 并查询请求与尝试。
7. `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。

## 8. 非目标与制品记录

首项不含完整 React 界面替换、插件执行、公共市场、支付、多租户 RBAC、多媒体、工具调用或自动更新。插件阶段另行设计 WASM ABI、资源限制、请求/流钩子与 UI 沙箱。

决策来源：`.ryugi/artifacts/document/backend-foundation/`。最终工程制品：`.ryugi/artifacts/document/backend-foundation-design/`。用户选择真实纵向闭环、三种原生协议、WASM、OS 钥匙串、本地管理员会话；其余为本文明确的工程选择。

## 9. 构建契约（设计审查后补充）

以下为构建者的工程决定，不改变已批准的产品范围。

### 模型、路由与策略

- 客户端原始 `model` 按路由 `patterns` 的 `*` glob 匹配；先匹配显式 slug，再按路由创建顺序匹配。仅显式 `is_default` 路由处理未匹配模型；没有默认路由则 404。公开模型列表为当前客户端 Key 可访问的路由公开模型，未启用 `list_members` 时仅暴露 slug；不泄露全部上游模型。
- 首项路由只实现 ordered failover。管理 API 拒绝其他 strategy、权重、会话亲和和熔断配置，不接受但忽略。Provider 多 Key 采用起点轮转；单 Provider 401/403 尝试下一 Key，429/连接失败冷却该 Key 60 秒后尝试下一候选。健康状态只在进程内，重启清零。
- 模型与分组按 ID 授权。额度包含 lifetime tokenLimit、UTC 自然日 maxAmount、每分钟 RPM/TPM 与每模型覆盖。模型必须配置 input/output 每百万 token 的美元单价才能启用金额限额；服务端按统一用量整数计算费用。SQLite 事务原子预占请求数、token 与金额预算，完成时结算/释放；没有全局限额互斥锁。

### 原生客户端与协议

- OpenAI 接受 `Authorization: Bearer`；Anthropic 接受 `x-api-key`，要求受支持的 `anthropic-version`；Gemini 接受 `x-goog-api-key` 或标准 `?key=`，认证成功后从日志与上游 URL 移除 query key。入站与出站的这些传输分别遵循对应 SDK 习惯。
- Provider `endpoint` 保留版本化 base URL：OpenAI 默认 `https://api.openai.com/v1`，Anthropic `https://api.anthropic.com`，Gemini `https://generativelanguage.googleapis.com/v1beta`。追加表中的相对路径，移除末尾斜杠；路径模型段逐个 percent-encode，不把 `/` 当模型名内容。
- 接受空 `tools: []`，非空工具、多候选、思考/推理、logprobs、response_format、候选数与安全设置等改变语义的字段显式 400。空可选值不拒绝，绝不静默将多候选降为单候选。
- 出站 OpenAI 路径为 `/chat/completions`，Anthropic 为 `/v1/messages`，Gemini 为 `/models/{encoded-model}:generateContent` 或 `:streamGenerateContent?alt=sse`。

### 快照、额度与观测

- 不可变路由快照只包含非密文 Provider/模型/路由元数据，由相关管理写事务重发。上游 Key 按选中 ID 在单次尝试内解密。客户端 Key 哈希与策略从 SQLite 鉴权/预占，不进入快照；在途请求按开始时授权完成，吊销影响后续请求。
- 400/404 合法请求进入观测；401/403 不创建未鉴权请求记录；429 预占拒绝创建已识别客户端请求。观测写入同步完成才发最终响应，流式在完成时落库；失败返回安全 500。崩溃时未提交事务回滚，不宣称零丢失异步队列。默认永久保留元数据，正文默认关闭；后续设置正文时单条上限 64 KiB，保留期由设置控制。

### 安全、运行与预算

- 默认 `127.0.0.1:3425`；默认数据库为用户数据目录下 `sepia/sepia.db`，环境变量仅覆盖地址、数据目录和明确的开发测试模式。首次启动在 stdout 与权限为 0600 的数据目录文件中输出一次性 bootstrap token，初始化后删除文件；自动化可传入预设 token。
- 非环回监听时无 `SEPIA_TRUSTED_PROXY=true` 拒绝启动；TLS 由可信反向代理终止，首项不内嵌 rustls。loopback Cookie 不强制 Secure，非环回强制 Secure。Vite 开发只允许 `http://localhost:5173` 与 `http://127.0.0.1:5173` 显式 Origin，生产最终由 Rust 托管同源 SPA；首项提供静态文件挂载边界，不联调 UI。
- 全局超时/重试来自设置：连接 10 秒、首字节 60 秒、流空闲 60 秒、默认额外尝试 2 次、Retry-After 最多 5 秒；测试可注入毫秒级预算，不依赖真实长等待。
- `SEPIA_DEV_INSECURE_KEYSTORE` 仅在 `cfg(debug_assertions)` 且显式设置时允许文件测试密钥；release 构建忽略此开关，操作系统钥匙串不可用即启动失败。测试与开发不记录密钥或客户端 query key。

### 实施切片

1. 配置、数据库迁移、密钥库与管理鉴权。
2. 统一协议模型、三协议请求/响应/SSE 转换及模型列表。
3. 路由、额度预占、上游传输、重试和端到端推理。
4. 请求/尝试观测、运行配置、完整验收、文档和独立审查。

每片先有失败测试，再实现、运行完整 Rust 检查；首项全部完成后更新 AGENTS/memory 并提交本地 Git，不推送。
