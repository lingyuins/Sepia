# Sepia

[中文](#sepia) · [English](#sepia-english) · [许可证 / License](./LICENSE)

面向个人与小团队的多供应商 AI 网关及运行控制台。Sepia 将不同供应商的模型统一为 OpenAI、Anthropic 和 Gemini 客户端接口，并提供有序故障转移、访问控制、配额、请求观测与费用统计。

当前仓库包含可运行的 React 18 + TypeScript 控制台、Rust/Axum 网关、SQLite 持久化及协议集成测试。没有预置供应商、演示请求或虚构监控数据；WASM 插件运行时尚未实现，控制台会明确显示不可用状态。

## 功能现状

- **供应商与模型**：管理 OpenAI、Anthropic、Gemini 兼容上游，支持多个上游密钥、模型发现、启停和分组。
- **智能路由**：按模型或模式选择路由，保留候选顺序并执行有界故障转移；支持多密钥轮换、失败冷却和最终上游模型记录。
- **统一协议**：原生及跨协议 JSON/SSE 转换，支持文本、函数工具、工具调用/结果以及图像、音频和文件内容的显式映射。
- **OpenAI Responses**：支持函数工具、多模态输入、`previous_response_id`、会话、输入项目分页、后台轮询/取消及流事件回放。
- **客户端密钥**：明文仅在创建时返回一次；支持模型/路由范围、金额与 Token 额度、RPM/TPM、IP/CIDR、分组和供应商限制、有效期及撤销。
- **运行控制台**：管理员初始化与登录、供应商/路由/密钥配置、请求日志、Token/费用分析和运行参数设置；提供中文、English、日本語及明暗主题。
- **安全与持久化**：SQLite 存储配置和观测数据，上游密钥加密，管理员使用 HttpOnly 会话 Cookie，上游重定向不会转发凭据。

当前不支持多候选输出、自动供应商同步、会话亲和、WASM 插件执行或任意供应商内置工具的通用转换。上游 Provider 的 Responses 请求仍通过其 Chat Completions、Anthropic Messages 或 Gemini Generation 接口完成。

## 技术栈

| 模块 | 实现 |
| --- | --- |
| 控制台 | React 18、TypeScript、Vite、Recharts、Headless UI、Lucide |
| 网关 | Rust stable、Axum、Tokio、Reqwest |
| 数据 | SQLite、SQLx、版本化迁移 |
| 安全 | AES-GCM、Argon2、SHA-256、操作系统 Keyring、HttpOnly Cookie |

## 环境要求

- Node.js 22.18+（推荐 Node.js 24）与 npm
- Rust stable 与 Cargo
- Windows 构建：Visual Studio C++ Build Tools，并在 MSVC Developer Command Prompt 中运行 Cargo
- 生产环境：可用的操作系统 Keyring

## 快速开始

### Windows 一键开发启动

在仓库根目录运行：

```bat
start.cmd
```

脚本会在缺少依赖时执行 `npm ci --prefix frontend`，并分别打开网关和前端窗口：

- 控制台：http://localhost:5173
- 网关：http://127.0.0.1:5000

首次启动时，网关窗口会显示一次性初始化令牌。打开控制台，设置至少 12 字节的管理员密码并完成登录。初始化只消费一次令牌；已有管理员时直接登录。

关闭两个服务窗口即可停止开发服务。

### 手动开发启动

在仓库根目录分别执行：

```sh
npm ci --prefix frontend
npm run dev --prefix frontend
cargo run --manifest-path backend/Cargo.toml
```

Vite 将 `/api` 和 `/v1` 代理到 `http://127.0.0.1:5000`。可通过 `SEPIA_GATEWAY_URL` 为 Vite 指定其他网关地址。

### 生产模式

先构建控制台，再从仓库根目录启动网关：

```sh
npm run build --prefix frontend
cargo run --release --manifest-path backend/Cargo.toml
```

网关直接托管 `frontend/dist/`，控制台与管理 API、推理 API 保持同源。生产环境不要设置调试密钥文件开关。

## 环境变量

| 变量 | 用途 |
| --- | --- |
| `SEPIA_DATA_DIR` | SQLite、初始化令牌文件和开发密钥文件所在目录 |
| `SEPIA_LISTEN` | 网关监听地址，默认 `127.0.0.1:5000` |
| `SEPIA_BOOTSTRAP_TOKEN` | 为自动化部署显式指定一次性初始化令牌 |
| `SEPIA_TRUSTED_PROXY=true` | 非回环监听所必需；由受信代理终止 TLS |
| `SEPIA_DEV_INSECURE_KEYSTORE=1` | 仅 debug 构建使用文件测试密钥；禁止用于生产 |
| `SEPIA_GATEWAY_URL` | Vite 开发代理目标，不是浏览器管理 API 的跨域地址 |

## 客户端接入

将客户端基础地址指向 Sepia，而不是上游供应商，并使用控制台创建的客户端密钥。

### OpenAI Chat Completions

```sh
curl http://127.0.0.1:5000/v1/chat/completions \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","messages":[{"role":"user","content":"Hello"}]}'
```

### OpenAI Responses

```sh
curl http://127.0.0.1:5000/v1/responses \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","input":"Hello"}'
```

非后台 `store:false` 响应不能被后续读取；后台响应会保留轮询所需状态。响应 JSON 不包含 SDK 客户端便利属性 `output_text`。

### Anthropic Messages

```sh
curl http://127.0.0.1:5000/v1/messages \
  -H "x-api-key: $SEPIA_CLIENT_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","max_tokens":128,"messages":[{"role":"user","content":"Hello"}]}'
```

`GET /v1/models` 使用 `x-api-key` 时可省略版本头；`POST /v1/messages` 仍要求 `2023-06-01`。

### Gemini

```sh
curl "http://127.0.0.1:5000/v1/models/your-route:generateContent" \
  -H "x-goog-api-key: $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}'
```

同时支持 `/v1beta` 和 `:streamGenerateContent`。共用模型列表入口根据凭据选择格式：`x-goog-api-key` 返回 Gemini 格式，Bearer 返回 OpenAI 格式，`x-api-key` 返回 Anthropic 格式。

## 目录结构

```text
frontend/                 React 控制台、样式、测试与 Vite 配置
backend/                  Rust 网关与集成测试
backend/migrations/       SQLite 版本化迁移
LICENSE                   GNU GPLv3 许可证
start.cmd                 Windows 开发启动脚本
temp/                     本地临时文件，内容不进入 Git
```

## 验证

在仓库根目录执行：

```sh
npm run typecheck --prefix frontend
npm test --prefix frontend
npm run build --prefix frontend
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
```

后端测试覆盖原生/跨协议 JSON 与 SSE、工具与多模态、Responses 状态、故障转移、配额、凭据和管理 API，使用本地可控 HTTP 上游。它们不替代使用真实供应商账户的兼容性验证。

## 安全说明

- 客户端密钥仅在创建时显示一次，数据库只保存哈希；不要将其写入前端存储、日志或提交。
- 管理员密码使用 Argon2 哈希，会话通过 HttpOnly Cookie 保存。
- 上游密钥使用 AES-GCM 加密；生产主密钥由操作系统 Keyring 保护。
- 网关不跟随上游重定向，避免凭据被转发到其他主机。
- 非回环部署应置于启用 TLS 的受信反向代理之后，并显式设置 `SEPIA_TRUSTED_PROXY=true`。

## 许可证

Sepia 依据 [GNU General Public License v3.0](./LICENSE) 发布。完整许可证文本保留在 `LICENSE` 文件中。

---

# Sepia (English) {#sepia-english}

A multi-provider AI gateway and operations console for individuals and small teams. Sepia exposes OpenAI, Anthropic, and Gemini client interfaces over different upstream providers, with ordered failover, access controls, quotas, request observability, and cost analytics.

The repository contains a working React 18 + TypeScript console, a Rust/Axum gateway, SQLite persistence, and protocol integration tests. It does not seed providers, demo requests, or fabricated monitoring data. The WASM plugin runtime is not implemented and is explicitly shown as unavailable in the console.

## Current capabilities

- **Providers and models**: manage OpenAI-, Anthropic-, and Gemini-compatible upstreams, multiple upstream keys, model discovery, enablement, and groups.
- **Routing**: select routes by model or pattern, preserve candidate order, and run bounded ordered failover with multi-key rotation, failure cooldowns, and final upstream-model recording.
- **Unified protocols**: native and cross-protocol JSON/SSE conversion for text, function tools, tool calls/results, and explicit image, audio, and file mappings.
- **OpenAI Responses**: function tools, multimodal input, `previous_response_id`, conversations, input-item pagination, background polling/cancellation, and stream-event replay.
- **Client keys**: plaintext is returned only once at creation. Scope keys by model/route, amount/token budgets, RPM/TPM, IP/CIDR, groups, excluded providers, expiration, and revocation.
- **Operations console**: administrator bootstrap/login, provider/route/key management, request logs, token/cost analytics, and runtime settings, with Chinese, English, Japanese, and light/dark themes.
- **Security and persistence**: SQLite configuration and observability, encrypted upstream keys, HttpOnly administrator sessions, and no credential forwarding through upstream redirects.

Multiple candidate outputs, automatic provider synchronization, session affinity, WASM plugin execution, and generic conversion of arbitrary provider-specific built-in tools are not supported. Upstream Responses requests are still executed through Chat Completions, Anthropic Messages, or Gemini Generation; native upstream Responses is not implied.

## Stack

| Component | Implementation |
| --- | --- |
| Console | React 18, TypeScript, Vite, Recharts, Headless UI, Lucide |
| Gateway | Rust stable, Axum, Tokio, Reqwest |
| Data | SQLite, SQLx, versioned migrations |
| Security | AES-GCM, Argon2, SHA-256, OS keyring, HttpOnly cookies |

## Requirements

- Node.js 22.18+ (Node.js 24 recommended) and npm
- Rust stable and Cargo
- Windows builds: Visual Studio C++ Build Tools and an MSVC Developer Command Prompt
- Production: an available operating-system keyring

## Quick start

### Windows development launcher

From the repository root:

```bat
start.cmd
```

The script installs frontend dependencies if missing, then opens separate gateway and frontend terminals:

- Console: http://localhost:5173
- Gateway: http://127.0.0.1:5000

On first startup, the gateway terminal prints a one-time bootstrap token. Open the console, choose an administrator password of at least 12 bytes, and sign in. Once initialized, subsequent visits use administrator login.

Close both service terminals to stop development.

### Manual development

Run from the repository root in separate terminals:

```sh
npm ci --prefix frontend
npm run dev --prefix frontend
cargo run --manifest-path backend/Cargo.toml
```

Vite proxies `/api` and `/v1` to `http://127.0.0.1:5000`. Set `SEPIA_GATEWAY_URL` to use another gateway for the Vite proxy.

### Production

Build the console, then start the gateway from the repository root:

```sh
npm run build --prefix frontend
cargo run --release --manifest-path backend/Cargo.toml
```

The gateway serves `frontend/dist/`, keeping the console, management API, and inference API same-origin. Never enable the debug file keystore in production.

## Environment variables

| Variable | Purpose |
| --- | --- |
| `SEPIA_DATA_DIR` | Directory for SQLite, bootstrap-token files, and development key files |
| `SEPIA_LISTEN` | Listen address; defaults to `127.0.0.1:5000` |
| `SEPIA_BOOTSTRAP_TOKEN` | Explicit one-time bootstrap token for automation |
| `SEPIA_TRUSTED_PROXY=true` | Required for non-loopback listening; TLS terminates at a trusted proxy |
| `SEPIA_DEV_INSECURE_KEYSTORE=1` | Debug builds only; uses a file test key and must never protect production secrets |
| `SEPIA_GATEWAY_URL` | Vite development proxy target, not a cross-origin browser management API setting |

## Client usage

Point clients at Sepia instead of the upstream provider and use a client key created in the console.

### OpenAI Chat Completions

```sh
curl http://127.0.0.1:5000/v1/chat/completions \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","messages":[{"role":"user","content":"Hello"}]}'
```

### OpenAI Responses

```sh
curl http://127.0.0.1:5000/v1/responses \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","input":"Hello"}'
```

A non-background `store:false` response cannot be retrieved later. Background responses retain the state required for polling. Response JSON does not include the SDK convenience property `output_text`.

### Anthropic Messages

```sh
curl http://127.0.0.1:5000/v1/messages \
  -H "x-api-key: $SEPIA_CLIENT_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-route","max_tokens":128,"messages":[{"role":"user","content":"Hello"}]}'
```

`GET /v1/models` with `x-api-key` may omit the version header. `POST /v1/messages` requires `2023-06-01`.

### Gemini

```sh
curl "http://127.0.0.1:5000/v1/models/your-route:generateContent" \
  -H "x-goog-api-key: $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}'
```

`/v1beta` and `:streamGenerateContent` are also supported. The shared model-list endpoint selects its format from credentials: `x-goog-api-key` returns Gemini, Bearer returns OpenAI, and `x-api-key` returns Anthropic.

## Project layout

```text
frontend/                 React console, styles, tests, and Vite configuration
backend/                  Rust gateway and integration tests
backend/migrations/       Versioned SQLite migrations
LICENSE                   GNU GPLv3 license
start.cmd                 Windows development launcher
temp/                     Local scratch files; contents are excluded from Git
```

## Verification

From the repository root:

```sh
npm run typecheck --prefix frontend
npm test --prefix frontend
npm run build --prefix frontend
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
```

Backend tests cover native/cross-protocol JSON and SSE, tools and multimodal content, Responses state, failover, quotas, credentials, and management APIs using controllable local HTTP upstreams. They do not replace compatibility testing with real provider accounts.

## Security notes

- Client-key plaintext is shown only at creation; only hashes are stored. Never write keys to frontend storage, logs, or commits.
- Administrator passwords use Argon2 hashes; sessions use HttpOnly cookies.
- Upstream keys use AES-GCM encryption, with production master keys protected by the OS keyring.
- Upstream redirects are not followed, preventing credential forwarding to another host.
- Deploy non-loopback instances behind a TLS-enabled trusted reverse proxy and explicitly set `SEPIA_TRUSTED_PROXY=true`.

## License

Sepia is released under the [GNU General Public License v3.0](./LICENSE). The complete license text is preserved in `LICENSE`.
