# Sepia gateway backend

Rust/Axum service for native OpenAI, Anthropic and Gemini text APIs. The React app remains a separate preview until its API integration change.

## Requirements

- Rust stable (verified with 1.99.1)
- Windows: Visual Studio Build Tools with the C++ workload; run Cargo from a Visual Studio Developer Command Prompt
- Production: an available operating-system keyring

## Run

```sh
cargo run --manifest-path backend/Cargo.toml
```

Defaults: `127.0.0.1:5000`, SQLite under the user data directory. On first start, a one-time bootstrap token is printed and written in that directory. `POST /api/v1/auth/bootstrap` with `{username,password,token}` initializes the administrator and revokes the token file. Passwords must contain at least 12 characters.

Useful environment variables:

- `SEPIA_DATA_DIR`: database and bootstrap-token directory
- `SEPIA_LISTEN`: listen address
- `SEPIA_BOOTSTRAP_TOKEN`: explicit token for automation
- `SEPIA_TRUSTED_PROXY=true`: required for non-loopback listening; TLS is terminated by a trusted proxy
- `SEPIA_DEV_INSECURE_KEYSTORE=1`: debug builds only; uses a file test key and must not protect production secrets

The service serves `frontend/dist/` as a same-origin SPA when built. Run `npm run build --prefix frontend` and start the gateway from the repository root before using the production UI.

## API surfaces

- OpenAI: `GET /v1/models`, `POST /v1/chat/completions` with Bearer Key
- Anthropic: `GET /v1/models` with `x-api-key`; `anthropic-version: 2023-06-01` is optional for model listing but required by `POST /v1/messages`
- Gemini: `GET /v1beta/models`, `POST /v1beta/models/{model}:generateContent` and `:streamGenerateContent` with `x-goog-api-key` or `?key=`
- Management: `/api/v1/auth/*`, `/providers`, `/routes`, `/keys`, `/groups`, `/settings`, `/requests`, `/analytics`; session cookie required except bootstrap/login

Provider keys are AES-GCM encrypted with an OS-keyring master key. Redirects are not followed. Client keys are returned only once at creation and stored as SHA-256 hashes. Routing is ordered failover, with bounded attempts, key cooldowns and no retry after client output.

## Verification

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
npm run build --prefix frontend
npm test --prefix frontend
```

Tests use local mock HTTP upstreams. They cover native/cross-protocol JSON and SSE, failover, secrets, quotas, lifecycle cleanup and management. They do not claim live third-party provider verification; that requires separately supplied credentials.
