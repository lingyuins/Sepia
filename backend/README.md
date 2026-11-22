# Sepia gateway backend

Rust/Axum service for native OpenAI, Anthropic and Gemini text APIs plus OpenAI-compatible auxiliary endpoints. The React console is fully integrated through the same-origin `/api/v1` management API.

## Requirements

- Rust stable (verified with 1.99.1)
- Windows: Visual Studio Build Tools with the C++ workload; run Cargo from a Visual Studio Developer Command Prompt
- Production: an operating-system keyring (Windows/macOS), an explicit `SEPIA_MASTER_KEY`, or the auto-generated `master-key` file in the data directory (platforms without a persistent keyring, e.g. Linux/containers)

## Run

```sh
cargo run --manifest-path backend/Cargo.toml
```

Defaults: `127.0.0.1:5000`, SQLite under the user data directory. On first start, a one-time bootstrap token is printed and written in that directory. `POST /api/v1/auth/bootstrap` with `{username,password,token}` initializes the administrator and revokes the token file. Passwords must contain at least 8 characters.

Useful environment variables:

- `SEPIA_DATA_DIR`: database and bootstrap-token directory
- `SEPIA_LISTEN`: listen address
- `SEPIA_BOOTSTRAP_TOKEN`: explicit token for automation
- `SEPIA_MASTER_KEY`: Base64-encoded 32-byte master key; takes precedence over the OS keyring and the data-directory key file and cannot be rotated; optional (without a persistent keyring the key is auto-generated and persisted to `<data dir>/master-key`)
- `SEPIA_FRONTEND_DIR`: optional absolute console build directory; containers use `/opt/sepia/frontend/dist`
- `SEPIA_HTTP_LOOPBACK_COOKIE=true`: only for local HTTP access to a container published on loopback at `0.0.0.0:5000`; remove for remote/LAN deployment
- `SEPIA_TRUSTED_PROXY=true`: required for non-loopback listening; TLS is terminated by a trusted proxy
- `SEPIA_DEV_INSECURE_KEYSTORE=1`: debug builds only; uses a file test key and must not protect production secrets
- `SEPIA_METADATA_SOURCE`: Model Plaza metadata sync source (OpenAI-style model list); defaults to `https://openrouter.ai/api/v1/models`

The service serves `frontend/dist/` as a same-origin SPA when built. Run `npm run build --prefix frontend` and start the gateway from the repository root before using the production UI. At startup the console path is resolved before the process changes to `SEPIA_DATA_DIR`: an existing working-directory `frontend/dist` wins, otherwise packaged `bin/sepia-gateway` resolves the sibling package-root `frontend/dist`. `SEPIA_FRONTEND_DIR` overrides both. The root `Dockerfile` embeds migrations in the release binary, serves `/opt/sepia/frontend/dist`, and stores runtime state in `/data`.

Provider keys are AES-GCM encrypted with an OS-keyring master key (Windows Credential Manager / macOS Keychain via the keyring crate's native backends), or with explicit `SEPIA_MASTER_KEY` when supplied. When no persistent keyring backend exists (e.g. Linux containers, where the keyring crate would silently fall back to an in-memory mock store) and `SEPIA_MASTER_KEY` is unset, the master key is generated once and persisted to `<data dir>/master-key` (0600 on Unix) so restarts keep working; key and data then share one trust boundary. Migration `0008_keystore_metadata.sql` stores an encrypted check value on first startup; later startups reject a different master key before listening rather than failing later during upstream-key decryption. Invalid Base64 and non-32-byte keys fail immediately.

## API surfaces

- OpenAI: `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/responses` with Bearer key. Responses accepts string/message-array input, `input_text`, assistant `output_text` history, function tools and tool outputs, images/audio/files, `instructions` and `max_output_tokens`, response retrieval, input-item listing, `previous_response_id` history, background polling/cancellation and stream-event replay (migration `0007_responses.sql`). Non-background `store:false` responses cannot be retrieved.
- Anthropic: `GET /v1/models` with `x-api-key`; `anthropic-version: 2023-06-01` is optional for model listing but required by `POST /v1/messages`
- Gemini: `GET /v1/models` or `/v1beta/models`, `POST /v1/models/{model}:generateContent` or `/v1beta/models/{model}:generateContent`, and `:streamGenerateContent` with `x-goog-api-key` or `?key=`. Both stable and beta paths share protocol conversion; `/v1/models` selects Gemini listing when `x-goog-api-key` is supplied. Current `@google/genai` default stable-v1 listing, JSON and streaming are covered by local SDK verification.
- Auxiliary passthrough (`auxiliary.rs`, OpenAI/OpenAI Responses providers only): `POST /v1/embeddings|rerank|moderations|images/generations|audio/speech|videos|video/generations|search|alpha/search|tools/search|tools/fetch`, multipart `POST /v1/images/edits|audio/transcriptions|audio/translations`, `GET /v1/videos/:task_id` and `GET /v1/video/generations/:task_id`, and the Suno-style proxy `POST /suno/submit/:action`, `POST /suno/fetch`, `GET /suno/fetch/:task_id`. Bodies pass through verbatim; terminal upstream errors return the upstream status/body and binary responses keep their content type. Client-key auth, model scopes, routing, quotas (1024-token reservation settled from usage) and `max_attempts` failover are shared with inference. Async tasks (`0011_auxiliary_tasks.sql`) are client-key scoped; polls/fetches pin the submitting provider/key.
- Management: `/api/v1/auth/*`, `/providers` (incl. per-key model scopes and `/providers/:id/test` catalog discovery), `/routes` (incl. `POST /routes/:id/test` real per-candidate probes), `/keys`, `/groups`, `/settings`, `/requests` (incl. `/requests/:id/bodies`), `/analytics`, `/models/catalog`, `/models/metadata/sync`, `/models/pricing`, `/plugins/*`; session cookie required except bootstrap/login
- Plugins: local `.zip` install (`POST /api/v1/plugins/install`, multipart), enable/disable, config, uninstall, sandboxed UI bundle and RPC. Runtime is `wasmtime` (`plugin.rs`): fuel + epoch timeouts, memory caps, no WASI; manifest-validated permissions gate relay/route/strategy/response/observation/provider/RPC hooks and kv/secret/http/log host capabilities. Plugin-backed providers (`providers.plugin_id`) discover models via `provider.models` and serve inference via `provider.fetch`. Guest SDK: `backend/plugin-sdk` (no_std, wasm32-unknown-unknown); examples in the standalone `plugin-examples/` crate at the repository root.

Management API responses use gzip when the client advertises support; this includes the full `/models/catalog` payload and does not truncate model records. Plugin UI RPC methods are encoded per path segment by the console, and the gateway rejects decoded traversal, empty segments, backslashes, percent escapes and control characters before invoking a plugin.

OpenAI providers are universal dual-endpoint: `server.rs` `candidate_protocols` picks Chat Completions or Responses first from the inbound request and tries the other endpoint once per provider before advancing candidates; protocol `openai-responses` pins Responses-first with Chat fallback. 401/403 rotate provider keys within one leg (bounded by the eligible key count for the requested model); 404/405/501 mark the endpoint unsupported and advance. Redirects are not followed. Outbound requests default to `User-Agent: Sepia/<crate version>` (`relay.rs` `protected_http_client`); an explicitly set request header takes precedence. Client keys are returned only once at creation and stored as SHA-256 hashes.

Routing strategies (`strategy.rs`, migration `0017_route_strategies.sql`): each route stores one of `priority` (saved order, default), `weighted` (per-candidate weights 1–100, sampling without replacement), `round-robin` (per-route rotating cursor), `random`, `p2c` (power of two choices by in-flight load), `least-used` (in-flight load ascending), `cost-optimized` (configured channel price ascending, unpriced last), `latency` (EWMA of time-to-first-byte, failures penalized, unknown first), `context-fit` (estimated request tokens vs. synced model context lengths) or `sticky` (last successful candidate first). Every strategy only reorders candidates — bounded attempts, key cooldowns, dual-endpoint legs and no retry after client output are unchanged. In-flight counters, latency EWMA, cursors and last-success memory live in a process-wide `RoutingStats` shared by inference and auxiliary forwarding (pinned async polls keep their provider). Prices come from configured channel pricing; context lengths from Model Plaza metadata sync (`bare_id` matched). Plugin route-filter/strategy hooks still run first; `priority` preserves plugin ordering, other strategies reorder the filtered list. Route create/update accept optional `strategy` and `weights` (one weight per candidate, clamped 1–100); `GET /routes` and the model catalog's owning-route entries expose the strategy.

Request logs record client IP, time-to-first-byte, cached/reasoning token detail and real total latency (`0010_request_details.sql`). Body capture is opt-in via the `log_bodies` workspace setting (default off), stored in `request_bodies` with a 1 MiB cap and cascade-deleted with history.

`GET /requests` returns the latest 200 records. The console discloses that search, pagination and exports operate on this window; it does not claim to export the complete retained history.

## Verification

```sh
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
npm run build --prefix frontend
npm test --prefix frontend
```

Tests use local mock HTTP upstreams. They cover native/cross-protocol JSON and SSE, auxiliary passthrough endpoints, failover, secrets, per-key model scopes, quotas, Responses lifecycle, model metadata, the WASM plugin runtime and management. They do not claim live third-party provider verification; that requires separately supplied credentials.
