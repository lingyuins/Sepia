# Sepia

[中文](#sepia) · [English](#sepia-english) · [许可证 / License](./LICENSE)

面向个人与小团队的多供应商 AI 网关及运行控制台。Sepia 将不同供应商的模型统一为 OpenAI、Anthropic 和 Gemini 客户端接口，并提供有序故障转移、访问控制、配额、请求观测与费用统计。

当前仓库包含可运行的 React 18 + TypeScript 控制台、Rust/Axum 网关、SQLite 持久化及协议集成测试。没有预置供应商、演示请求或虚构监控数据。本地 WASM 插件运行时已实现（wasmtime 沙箱），插件通过本地 `.zip` 包安装；没有公共插件市场或自动更新。

## 功能现状

- **供应商与模型**：管理 OpenAI、Anthropic、Gemini 兼容上游，支持多个上游密钥、逐密钥模型范围限定、模型发现、启停和分组。OpenAI 上游为通用双端点：按入站协议优先选择 Chat Completions 或 Responses，首腿失败在同一供应商内换另一端点一次后再推进候选。
- **模型广场**：`#Plaza` 按变体家族聚合各供应商模型，展示真实渠道、可用性与已配置价格；可一键同步 OpenAI 风格模型元数据（默认 OpenRouter，`SEPIA_METADATA_SOURCE` 可覆盖），参考价与配置价分离标注，并支持逐模型改价。
- **智能路由**：按模型或模式选择路由，保留候选顺序并执行有界故障转移；支持多密钥轮换、失败冷却和最终上游模型记录。路由测试对每个候选发送一次真实最小推理探测并逐候选报告结果。
- **统一协议**：原生及跨协议 JSON/SSE 转换，支持文本、函数工具、工具调用/结果以及图像、音频和文件内容的显式映射。
- **OpenAI Responses**：支持函数工具、多模态输入、`previous_response_id`、会话、输入项目分页、后台轮询/取消及流事件回放。
- **辅助端点**：OpenAI 兼容的 embeddings、rerank、moderations、图像生成/编辑、语音合成/转写/翻译、视频生成与轮询、搜索（含 `alpha/search`、`tools/search`、`tools/fetch` 别名）及 Suno 风格代理，原样透传至 OpenAI 上游并复用鉴权、路由、配额与故障转移；异步任务按客户端密钥隔离。
- **客户端密钥**：明文仅在创建时返回一次；支持模型/路由范围、金额与 Token 额度、RPM/TPM、IP/CIDR、分组和供应商限制、有效期及撤销。
- **请求观测**：记录客户端 IP、首字延迟、缓存输入与推理输出 Token 明细和真实总延迟；可选正文记录（默认关闭，1 MiB 截断）；Token/费用分析按真实请求日志聚合。
- **插件系统**：本地 `.zip` 包安装（manifest 权限校验、SHA256 记录、semver 拒降级），wasmtime 沙箱执行（fuel/epoch 超时、内存上限、无 WASI），提供请求改写、路由过滤/策略、响应改写、观测、插件供应商与 RPC 钩子及 kv/secret/http/log 宿主能力；控制台支持启停、配置编辑、沙箱 iframe 插件 UI 与卸载。
- **运行控制台**：管理员初始化与登录、供应商/路由/密钥/插件配置、模型广场、请求日志、Token/费用分析和运行参数设置；提供中文、English、日本語及明暗主题。
- **安全与持久化**：SQLite 存储配置和观测数据，上游密钥与插件 secret 加密，管理员使用 HttpOnly 会话 Cookie，上游重定向不会转发凭据。

当前不支持多候选输出、自动供应商同步、会话亲和、公共插件市场/自动更新或任意供应商内置工具的通用转换；内置路由策略仅有有序故障转移（插件可通过策略钩子重排/排除候选）。

## 技术栈

| 模块 | 实现 |
| --- | --- |
| 控制台 | React 18、TypeScript、Vite、Recharts、Headless UI、Lucide |
| 网关 | Rust stable、Axum、Tokio、Reqwest、wasmtime |
| 数据 | SQLite、SQLx、版本化迁移 |
| 安全 | AES-GCM、Argon2、SHA-256、操作系统 Keyring、HttpOnly Cookie、WASM 沙箱 |

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

### 桌面发布包

GitHub Release 为 Windows x64、macOS Intel、macOS Apple Silicon、Linux x64 和 Linux ARM64 提供 `.tar.gz` 及对应 `.sha256`。下载并校验后解压，从包根目录启动；数据库迁移已编译进二进制，无需另附 SQL 文件：

```sh
# Linux / macOS
./bin/sepia-gateway

# Windows（PowerShell）
.\bin\sepia-gateway.exe
```

包内包含 `README.md`、`LICENSE`、`bin/sepia-gateway[.exe]` 和 `frontend/dist/`。可从任意工作目录运行，运行数据默认保存在用户数据目录，也可用 `SEPIA_DATA_DIR` 指定。桌面包默认使用操作系统 Keyring；设置 `SEPIA_MASTER_KEY` 时会优先使用该显式主密钥。macOS 包未签名或公证，首次运行可能出现 Gatekeeper 提示，请在 macOS 隐私与安全性设置中确认是否允许运行。

### Docker

镜像支持 `linux/amd64` 与 `linux/arm64`，同时发布到 GHCR 和 Docker Hub：

```sh
docker pull ghcr.io/lingyuins/sepia:latest
docker pull lingyuins/sepia:latest
# 版本标签示例：ghcr.io/lingyuins/sepia:v0.1.0、lingyuins/sepia:0.1.0
```

生成一次主密钥，将其写入权限受限且不进入 Git 的 `.env`，然后使用仓库中的 `compose.yaml`：

```sh
umask 077
printf 'SEPIA_MASTER_KEY=%s\n' "$(openssl rand -base64 32)" > .env
docker compose --env-file .env up -d
curl http://127.0.0.1:5000/api/v1/auth/status
```

控制台地址为 `http://127.0.0.1:5000`，首次打开按页面显示的初始化令牌完成管理员初始化。Compose 将端口仅绑定到本机回环地址，缺少 `SEPIA_MASTER_KEY` 时会立即报错。命名卷 `sepia-data` 保存数据库与初始化状态；必须同时备份该卷和同一个主密钥，丢失或轮换主密钥会使已有上游凭据无法解密。已有桌面条目和 OS Keyring 密钥不会自动迁入容器。

远程或局域网部署必须使用 HTTPS 反向代理，设置 `SEPIA_TRUSTED_PROXY=true`，并在容器环境中移除 `SEPIA_HTTP_LOOPBACK_COOKIE`，使管理员 Cookie 保持 `Secure`。不要把容器的 5000 端口直接暴露到不受信网络。镜像不内置主密钥、管理员密码、初始化令牌或供应商凭据。

### 发布流程

`.github/workflows/release.yml` 是唯一的发布 workflow。推送 `v*` 标签会验证代码、构建五种原生包、构建双架构镜像并执行容器启动/持久化/错误主密钥测试，然后发布 GHCR、Docker Hub 和 GitHub Release。手动 **Run workflow** 只构建和测试，不登录 registry、不推送镜像，也不创建 Release；可下载五组原生包及校验和 artifact。

发布前先在 GitHub 仓库 **Settings → Secrets and variables → Actions** 配置 variable `DOCKERHUB_USERNAME=lingyuins` 和 secret `DOCKERHUB_TOKEN`。`backend/Cargo.toml` 的 `version` 必须与标签去掉 `v` 后完全一致；不匹配会在 `verify` 失败，不产生公开产物。确认后由维护者执行：

```sh
git tag v<version>
git push origin v<version>
```

Release job 依赖原生包与镜像全部成功。若最后仅 Release API 失败，保留已推送镜像，不删除或重建标签；修复后重跑同一标签的 workflow。

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
| `SEPIA_MASTER_KEY` | 可选的 Base64 编码 32 字节生产主密钥；优先于 OS Keyring，不能轮换 |
| `SEPIA_FRONTEND_DIR` | 可选的控制台构建目录绝对路径；容器已设置 |
| `SEPIA_HTTP_LOOPBACK_COOKIE=true` | 仅允许 `0.0.0.0:5000` 的本机 HTTP 容器映射省略 `Secure`；远程部署必须移除 |
| `SEPIA_TRUSTED_PROXY=true` | 非回环监听所必需；由受信代理终止 TLS |
| `SEPIA_DEV_INSECURE_KEYSTORE=1` | 仅 debug 构建使用文件测试密钥；禁止用于生产 |
| `SEPIA_GATEWAY_URL` | Vite 开发代理目标，不是浏览器管理 API 的跨域地址 |
| `SEPIA_METADATA_SOURCE` | 模型广场元数据同步源（OpenAI 风格模型列表），默认 `https://openrouter.ai/api/v1/models` |

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

### 辅助端点（OpenAI 兼容透传）

以下端点将请求体原样转发到 OpenAI/OpenAI Responses 上游（Anthropic/Gemini 供应商不参与），复用客户端密钥鉴权、模型范围、路由匹配、配额与故障转移；上游终态错误按原状态码和正文返回，二进制响应保留上游 Content-Type：

- `POST /v1/embeddings`、`/v1/rerank`、`/v1/moderations`
- `POST /v1/images/generations`、`/v1/images/edits`（multipart）
- `POST /v1/audio/speech`、`/v1/audio/transcriptions`、`/v1/audio/translations`（后两者 multipart）
- `POST /v1/videos`、`/v1/video/generations`；轮询 `GET /v1/videos/:task_id`、`GET /v1/video/generations/:task_id`
- `POST /v1/search` 及别名 `/v1/alpha/search`、`/v1/tools/search`、`/v1/tools/fetch`
- Suno 风格代理：`POST /suno/submit/:action`、`POST /suno/fetch`、`GET /suno/fetch/:task_id`

```sh
curl http://127.0.0.1:5000/v1/embeddings \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-embedding-model","input":"Hello"}'
```

异步提交任务按客户端密钥隔离：视频轮询与 Suno fetch 固定使用提交时的供应商/密钥，其他密钥不可见。

## 目录结构

```text
frontend/                 React 控制台、样式、测试与 Vite 配置
backend/                  Rust 网关与集成测试
backend/migrations/       SQLite 版本化迁移（编译进发布二进制）
backend/plugin-sdk/       no_std WASM 插件 SDK（wasm32-unknown-unknown）
backend/plugin-examples/  示例插件源码
Dockerfile                双架构、非 root 容器构建
compose.yaml              本机回环端口与命名卷部署
scripts/                  原生包打包与解压启动检查
.github/workflows/        唯一验证、构建与发布 workflow
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

后端测试覆盖原生/跨协议 JSON 与 SSE、工具与多模态、Responses 状态、辅助端点、故障转移、配额、凭据、管理 API、模型元数据与 WASM 插件运行时，使用本地可控 HTTP 上游。它们不替代使用真实供应商账户的兼容性验证。

## 安全说明

- 客户端密钥仅在创建时显示一次，数据库只保存哈希；不要将其写入前端存储、日志或提交。
- 管理员密码使用 Argon2 哈希，会话通过 HttpOnly Cookie 保存。
- 上游密钥使用 AES-GCM 加密；桌面条目的生产主密钥默认由操作系统 Keyring 保护，容器使用显式 `SEPIA_MASTER_KEY`。数据库中的加密校验标记会在监听前拒绝更换后的主密钥。
- 网关不跟随上游重定向，避免凭据被转发到其他主机。
- WASM 插件在 wasmtime 沙箱内执行：无 WASI，出站 HTTP 仅允许 manifest 声明的 origin，敏感请求头对插件隐藏且不可写；插件 secret 与上游密钥同样加密存储。
- 非回环部署应置于启用 TLS 的受信反向代理之后，并显式设置 `SEPIA_TRUSTED_PROXY=true`。

## 许可证

Sepia 依据 [GNU General Public License v3.0](./LICENSE) 发布。完整许可证文本保留在 `LICENSE` 文件中。

---

# Sepia (English) {#sepia-english}

A multi-provider AI gateway and operations console for individuals and small teams. Sepia exposes OpenAI, Anthropic, and Gemini client interfaces over different upstream providers, with ordered failover, access controls, quotas, request observability, and cost analytics.

The repository contains a working React 18 + TypeScript console, a Rust/Axum gateway, SQLite persistence, and protocol integration tests. It does not seed providers, demo requests, or fabricated monitoring data. A local WASM plugin runtime is implemented (wasmtime sandbox); plugins install from local `.zip` packages. There is no public plugin marketplace or auto-update.

## Current capabilities

- **Providers and models**: manage OpenAI-, Anthropic-, and Gemini-compatible upstreams with multiple upstream keys, per-key model scopes, model discovery, enablement, and groups. OpenAI upstreams are universal dual-endpoint: the primary API is chosen from the inbound protocol (Chat Completions or Responses), and a failed first leg tries the other endpoint once within the same provider before advancing candidates.
- **Model Plaza**: `#Plaza` groups provider models into variant families with real channels, availability, and configured prices. One action syncs OpenAI-style model metadata (OpenRouter by default, overridable via `SEPIA_METADATA_SOURCE`); reference prices are labeled separately from configured prices, which are editable per model.
- **Routing**: select routes by model or pattern, preserve candidate order, and run bounded ordered failover with multi-key rotation, failure cooldowns, and final upstream-model recording. Route tests send one real minimal inference probe per candidate and report per-candidate results.
- **Unified protocols**: native and cross-protocol JSON/SSE conversion for text, function tools, tool calls/results, and explicit image, audio, and file mappings.
- **OpenAI Responses**: function tools, multimodal input, `previous_response_id`, conversations, input-item pagination, background polling/cancellation, and stream-event replay.
- **Auxiliary endpoints**: OpenAI-compatible embeddings, rerank, moderations, image generation/edits, speech/transcription/translation, video generation with polling, search (including `alpha/search`, `tools/search`, `tools/fetch` aliases), and a Suno-style proxy pass bodies through verbatim to OpenAI upstreams, reusing auth, routing, quotas, and failover; async tasks are scoped to the submitting client key.
- **Client keys**: plaintext is returned only once at creation. Scope keys by model/route, amount/token budgets, RPM/TPM, IP/CIDR, groups, excluded providers, expiration, and revocation.
- **Request observation**: client IP, time-to-first-byte, cached-input and reasoning-output token detail, and real total latency are recorded per request. Optional body capture (off by default, capped at 1 MiB); token/cost analytics aggregate real request logs.
- **Plugin system**: local `.zip` install with manifest-validated permissions, SHA256 recording, and semver downgrade rejection; wasmtime sandboxed execution (fuel/epoch timeouts, memory caps, no WASI); relay rewrite, route filter/strategy, response rewrite, observation, plugin-backed provider, and RPC hooks plus kv/secret/http/log host capabilities; console enable/disable, config editing, sandboxed iframe plugin UI, and uninstall.
- **Operations console**: administrator bootstrap/login, provider/route/key/plugin management, Model Plaza, request logs, token/cost analytics, and runtime settings, with Chinese, English, Japanese, and light/dark themes.
- **Security and persistence**: SQLite configuration and observability, encrypted upstream keys and plugin secrets, HttpOnly administrator sessions, and no credential forwarding through upstream redirects.

Multiple candidate outputs, automatic provider synchronization, session affinity, a public plugin marketplace/auto-update, and generic conversion of arbitrary provider-specific built-in tools are not supported. Ordered failover is the only built-in routing strategy; plugins may reorder or exclude candidates through the strategy hook.

## Stack

| Component | Implementation |
| --- | --- |
| Console | React 18, TypeScript, Vite, Recharts, Headless UI, Lucide |
| Gateway | Rust stable, Axum, Tokio, Reqwest, wasmtime |
| Data | SQLite, SQLx, versioned migrations |
| Security | AES-GCM, Argon2, SHA-256, OS keyring, HttpOnly cookies, WASM sandbox |

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

### Desktop release packages

GitHub Releases provide `.tar.gz` archives and `.sha256` checksums for Windows x64, macOS Intel, macOS Apple Silicon, Linux x64, and Linux ARM64. Download, verify, and extract the matching archive, then run from its package root. Database migrations are embedded in the binary; separate SQL files are not required:

```sh
# Linux / macOS
./bin/sepia-gateway

# Windows (PowerShell)
.\bin\sepia-gateway.exe
```

Each package contains `README.md`, `LICENSE`, `bin/sepia-gateway[.exe]`, and `frontend/dist/`. It can be launched from any working directory. Runtime data uses the user data directory by default, or `SEPIA_DATA_DIR` when set. Desktop packages use the OS keyring by default; an explicit `SEPIA_MASTER_KEY` takes precedence. macOS archives are unsigned and not notarized, so Gatekeeper may prompt on first launch; use macOS Privacy & Security settings to decide whether to allow it.

### Docker

Images support `linux/amd64` and `linux/arm64` and are published to both GHCR and Docker Hub:

```sh
docker pull ghcr.io/lingyuins/sepia:latest
docker pull lingyuins/sepia:latest
# Version examples: ghcr.io/lingyuins/sepia:v0.1.0, lingyuins/sepia:0.1.0
```

Generate the master key once, store it in a permission-restricted `.env` excluded from Git, and use the repository's `compose.yaml`:

```sh
umask 077
printf 'SEPIA_MASTER_KEY=%s\n' "$(openssl rand -base64 32)" > .env
docker compose --env-file .env up -d
curl http://127.0.0.1:5000/api/v1/auth/status
```

Open `http://127.0.0.1:5000` and use the bootstrap token shown on the page to initialize the administrator. Compose binds the port only to the local loopback address and fails immediately when `SEPIA_MASTER_KEY` is missing. The `sepia-data` named volume stores the database and initialization state. Back up that volume and the same master key together; losing or rotating the key makes existing upstream credentials undecryptable. Existing desktop data and OS-keyring keys are not imported automatically.

For remote or LAN deployment, use an HTTPS reverse proxy, set `SEPIA_TRUSTED_PROXY=true`, and remove `SEPIA_HTTP_LOOPBACK_COOKIE` from the container environment so administrator cookies remain `Secure`. Do not expose port 5000 directly to untrusted networks. Images contain no master key, administrator password, bootstrap token, or provider credentials.

### Release process

`.github/workflows/release.yml` is the only release workflow. Pushing a `v*` tag verifies the code, builds five native packages and both container architectures, runs container startup/persistence/wrong-key checks, then publishes GHCR, Docker Hub, and the GitHub Release. A manual **Run workflow** only builds and tests: it does not log in to registries, push images, or create a Release. Five native archive/checksum artifacts remain downloadable.

Before tagging, configure the GitHub repository Actions variable `DOCKERHUB_USERNAME=lingyuins` and secret `DOCKERHUB_TOKEN` under **Settings → Secrets and variables → Actions**. The `version` in `backend/Cargo.toml` must exactly match the tag without its `v`; a mismatch fails `verify` before any public artifacts are produced. A maintainer can then run:

```sh
git tag v<version>
git push origin v<version>
```

The Release job requires both native packages and image publication to succeed. If only the final Release API call fails, keep the published images and do not delete or recreate the tag; fix the issue and rerun the workflow for that tag.

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
| `SEPIA_MASTER_KEY` | Optional Base64-encoded 32-byte production master key; takes precedence over the OS keyring and cannot be rotated |
| `SEPIA_FRONTEND_DIR` | Optional absolute path to the built console; set in the container image |
| `SEPIA_HTTP_LOOPBACK_COOKIE=true` | Omits `Secure` only for a local HTTP container mapping on `0.0.0.0:5000`; must be removed for remote deployments |
| `SEPIA_TRUSTED_PROXY=true` | Required for non-loopback listening; TLS terminates at a trusted proxy |
| `SEPIA_DEV_INSECURE_KEYSTORE=1` | Debug builds only; uses a file test key and must never protect production secrets |
| `SEPIA_GATEWAY_URL` | Vite development proxy target, not a cross-origin browser management API setting |
| `SEPIA_METADATA_SOURCE` | Model Plaza metadata sync source (OpenAI-style model list); defaults to `https://openrouter.ai/api/v1/models` |

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

### Auxiliary endpoints (OpenAI-compatible passthrough)

These endpoints forward request bodies verbatim to OpenAI/OpenAI Responses upstreams (Anthropic/Gemini providers are skipped), reusing client-key auth, model scopes, route matching, quotas, and failover. Terminal upstream errors are returned with the upstream status and body; binary responses keep their upstream Content-Type:

- `POST /v1/embeddings`, `/v1/rerank`, `/v1/moderations`
- `POST /v1/images/generations`, `/v1/images/edits` (multipart)
- `POST /v1/audio/speech`, `/v1/audio/transcriptions`, `/v1/audio/translations` (the latter two multipart)
- `POST /v1/videos`, `/v1/video/generations`; polling via `GET /v1/videos/:task_id`, `GET /v1/video/generations/:task_id`
- `POST /v1/search` plus aliases `/v1/alpha/search`, `/v1/tools/search`, `/v1/tools/fetch`
- Suno-style proxy: `POST /suno/submit/:action`, `POST /suno/fetch`, `GET /suno/fetch/:task_id`

```sh
curl http://127.0.0.1:5000/v1/embeddings \
  -H "Authorization: Bearer $SEPIA_CLIENT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-embedding-model","input":"Hello"}'
```

Async submitted tasks are scoped to the submitting client key: video polls and Suno fetches pin the submitting provider/key and are invisible to other keys.

## Project layout

```text
frontend/                 React console, styles, tests, and Vite configuration
backend/                  Rust gateway and integration tests
backend/migrations/       Versioned SQLite migrations embedded in release binaries
backend/plugin-sdk/       no_std WASM plugin SDK (wasm32-unknown-unknown)
backend/plugin-examples/  Example plugin sources
Dockerfile                Multi-architecture non-root container build
compose.yaml              Loopback-only port and named-volume deployment
scripts/                  Native archive packaging and extracted-startup checks
.github/workflows/        Single verification, build, and release workflow
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

Backend tests cover native/cross-protocol JSON and SSE, tools and multimodal content, Responses state, auxiliary endpoints, failover, quotas, credentials, management APIs, model metadata, and the WASM plugin runtime using controllable local HTTP upstreams. They do not replace compatibility testing with real provider accounts.

## Security notes

- Client-key plaintext is shown only at creation; only hashes are stored. Never write keys to frontend storage, logs, or commits.
- Administrator passwords use Argon2 hashes; sessions use HttpOnly cookies.
- Upstream keys use AES-GCM encryption. Desktop production master keys use the OS keyring by default; containers use an explicit `SEPIA_MASTER_KEY`. An encrypted database marker rejects a changed master key before the gateway starts listening.
- Upstream redirects are not followed, preventing credential forwarding to another host.
- WASM plugins run in a wasmtime sandbox: no WASI, outbound HTTP restricted to manifest-declared origins, sensitive request headers hidden and unwritable; plugin secrets are encrypted like upstream keys.
- Deploy non-loopback instances behind a TLS-enabled trusted reverse proxy and explicitly set `SEPIA_TRUSTED_PROXY=true`.

## License

Sepia is released under the [GNU General Public License v3.0](./LICENSE). The complete license text is preserved in `LICENSE`.
