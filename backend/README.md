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
- `SEPIA_MASTER_KEY`: optional Base64-encoded 32-byte master key; takes precedence over the OS keyring and cannot be rotated
- `SEPIA_FRONTEND_DIR`: optional absolute console build directory; containers use `/opt/sepia/frontend/dist`
- `SEPIA_HTTP_LOOPBACK_COOKIE=true`: only for local HTTP access to a container published on loopback at `0.0.0.0:5000`; remove for remote/LAN deployment
- `SEPIA_TRUSTED_PROXY=true`: required for non-loopback listening; TLS is terminated by a trusted proxy
- `SEPIA_DEV_INSECURE_KEYSTORE=1`: debug builds only; uses a file test key and must not protect production secrets

The service serves `frontend/dist/` as a same-origin SPA when built. Run `npm run build --prefix frontend` and start the gateway from the repository root before using the production UI. At startup the console path is resolved before the process changes to `SEPIA_DATA_DIR`: an existing working-directory `frontend/dist` wins, otherwise packaged `bin/sepia-gateway` resolves the sibling package-root `frontend/dist`. `SEPIA_FRONTEND_DIR` overrides both. The root `Dockerfile` embeds migrations in the release binary, serves `/opt/sepia/frontend/dist`, and stores runtime state in `/data`.

Provider keys are AES-GCM encrypted with an OS-keyring master key, or with explicit `SEPIA_MASTER_KEY` when supplied. Migration `0008_keystore_metadata.sql` stores an encrypted check value on first startup; later startups reject a different master key before listening rather than failing later during upstream-key decryption. Invalid Base64 and non-32-byte keys fail immediately.

## API surfaces

- OpenAI: `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/responses` with Bearer Key. Responses accepts string/message-array text input, `input_text`, assistant `output_text` history, `instructions` and `max_output_tokens`; it returns native response JSON and lifecycle SSE. Tools, media, file input, conversation chaining and background execution are rejected rather than silently discarded. Existing Provider protocols continue to forward through Chat Completions, Anthropic Messages or Gemini generation.
- Anthropic: `GET /v1/models` with `x-api-key`; `anthropic-version: 2023-06-01` is optional for model listing but required by `POST /v1/messages`
- Gemini: `GET /v1/models` or `/v1beta/models`, `POST /v1/models/{model}:generateContent` or `/v1beta/models/{model}:generateContent`, and `:streamGenerateContent` with `x-goog-api-key` or `?key=`. Both stable and beta paths share protocol conversion; `/v1/models` selects Gemini listing when `x-goog-api-key` is supplied. Current `@google/genai` default stable-v1 listing, JSON and streaming are covered by local SDK verification.
- Management: `/api/v1/auth/*`, `/providers`, `/routes`, `/keys`, `/groups`, `/settings`, `/requests`, `/analytics`; session cookie required except bootstrap/login

Redirects are not followed. Client keys are returned only once at creation and stored as SHA-256 hashes. Routing is ordered failover, with bounded attempts, key cooldowns and no retry after client output.

## Verification

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
npm run build --prefix frontend
npm test --prefix frontend
```

Tests use local mock HTTP upstreams. They cover native/cross-protocol JSON and SSE, failover, secrets, quotas, lifecycle cleanup and management. They do not claim live third-party provider verification; that requires separately supplied credentials.
