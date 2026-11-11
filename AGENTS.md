# Repository Guidelines

## Project Overview

Sepia is a React 18 + TypeScript control plane connected to a Rust AI gateway. Provider, route, client-key, request and analytics operations use the real `/api/v1` service; there is no seeded provider/request dataset. The gateway implements OpenAI, Anthropic and Gemini text protocols, cross-protocol JSON/SSE, ordered failover, quotas and observation. WASM plugins are not implemented: the Plugins page explicitly reports an unavailable runtime.

## Architecture & Data Flow

- `src/main.tsx` composes `WorkspaceProvider`, first-run `Onboarding`, `GatewayGate` and `App`. Onboarding completion is stored in `localStorage['sepia.onboarding.v1']`.
- `src/workspace.tsx` owns domain types, Context, translations and download helpers. `src/gateway-workspace.ts` loads providers, routes, requests, analytics, client keys, groups and runtime settings from the gateway and exposes `refresh`, `run`, `login` and `logout`.
- `src/api.ts` is the same-origin API client and boundary mapper. It serializes Provider/route/key inputs, maps backend records to UI types, and preserves request timestamps for period filtering. Mutations call `run()`, which executes the request then refreshes the workspace; do not reintroduce local-only Provider/route/request setters.
- `GatewayGate` checks the session and public initialization status. First startup accepts the one-time bootstrap token and administrator credentials, then logs in. Administrator sessions use HttpOnly cookies and are separate from inference Bearer keys.
- `src/App.tsx` owns hash navigation (`#Providers`, `#Routing`, etc.), Overview, command search and global dialogs. Views call the real management API through Context.
- Browser storage is limited to interface preferences and onboarding state. Provider configuration, encrypted upstream credentials, hashed client keys, routes, requests and runtime retry settings live in the gateway.
- Backend `admin.rs` owns management endpoints; `server.rs` owns inference; `store.rs`, `updates.rs`, `policy.rs`, `observability.rs` and migrations own durable state; `snapshot.rs` publishes enabled routes/providers and `max_attempts` to the runtime.
- Analytics accepts `period=24 hours|7 days|30 days` and uses one SQL window for totals, series and Provider aggregates. Frontend route/model breakdowns filter the same period locally. Route name/description are persisted by migration `0005_route_metadata.sql`.
- Only ordered failover is exposed. Do not add UI controls for unimplemented routing strategies, session affinity, automatic Provider sync, plugin installation or body logging unless the runtime supports them.

## Key Directories

- `src/views/`: Providers, Routing, Plugins, Requests and Analytics pages.
- `src/components/`: Provider/key editors, settings, onboarding, authentication gate and shared UI.
- `backend/src/`: Rust gateway modules; `backend/migrations/`: SQL migrations; `backend/tests/`: protocol, security and integration tests.
- `public/`: static assets. `dist/` and `*.tsbuildinfo`: generated output; do not edit.
- `docs/2026-11-03/调查/`: historical research, not the executable specification.

## Development Commands

```sh
npm ci
npm run dev
npm run typecheck
npm test
npm run build
npm run preview
cargo test --manifest-path backend/Cargo.toml
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path backend/Cargo.toml --check
cargo run --manifest-path backend/Cargo.toml
```

Vite listens on `0.0.0.0:5173` and proxies `/api` and `/v1` to `http://127.0.0.1:3425`. Set `SEPIA_GATEWAY_URL` to override the proxy target for an isolated gateway. Backend configuration uses `SEPIA_DATA_DIR`, `SEPIA_LISTEN` and `SEPIA_BOOTSTRAP_TOKEN`. Production serves `dist/` through the gateway; run `npm run build` before production smoke tests.

On Windows, use Rust stable with Visual Studio C++ Build Tools and an MSVC Developer Command Prompt. Stop a running development gateway before rebuilding its executable. `SEPIA_DEV_INSECURE_KEYSTORE=1` is debug-only and must never protect production secrets. There is no lint/coverage/CI command configured.

## Code Conventions & Common Patterns

- TypeScript/TSX: 2-space indentation, single quotes, no semicolons, strict types, relative imports. `noUnusedLocals` and `noUnusedParameters` are enabled.
- Keep the existing React + semantic CSS design: `glass-panel`, `button`, `settings-card`, theme variables and responsive rules. Reuse `Modal`, `Field`, `Toggle`, `DialogActions`, Headless UI and Lucide icons.
- Dropdown menus use `width: 100%` of their own trigger for both field and compact variants. Do not restore fixed menu widths. Long option labels ellipsize and expose their full text through `title`; verify narrow viewports do not overflow.
- Add user-facing strings to `src/i18n.ts` as `[English, 简体中文, 日本語]`; use `useTranslation()`. Verify dynamic labels in the running UI.
- Keep backend snake_case and frontend camelCase conversion in `api.ts`. Route candidates use Provider IDs in API tuples and Provider names in UI state. Route names/descriptions must survive refresh.
- Provider model discovery uses `POST /api/v1/providers/:id/test` for saved Providers. Fetch/refetch and test replace only the dialog draft and require “Save provider” to persist; failures keep the existing draft. For new Providers, fetch/test submits the current name, protocol, endpoint and keys to `POST /api/v1/providers`; when models are empty, the backend discovers before creating and returns the model list. Success closes the dialog; failure creates nothing. Loading disables model mutations.
- Client-key plaintext is returned only by key creation and shown once in the current page. Never persist it in localStorage or claim an existing key can be revealed later. Do not accept a browser-generated client secret.
- Use real empty states and real connection errors. Do not fabricate metrics, health, savings, request records, plugin capabilities or successful upstream calls.
- Keep pure logic tests in top-level `src/*.test.js` using `node:test` and `node:assert/strict`. Rust integration coverage belongs in `backend/tests/`.

## Important Files

- `src/gateway-workspace.ts`: real workspace state and session lifecycle.
- `src/api.ts`: API client, serializers and record mappers.
- `src/components/GatewayGate.tsx`: login and first-run administrator initialization.
- `src/components/SettingsView.tsx`: client-key management and supported runtime settings.
- `src/components/KeyPolicyDialog.tsx` + `src/key-policy.ts`: scoped client-key policy editor and validation.
- `src/components/Onboarding.tsx` + `src/onboarding.ts`: workspace preferences and first-run flow.
- `src/App.tsx`: shell, Overview and global dialogs.
- `vite.config.ts`: dev proxy, watcher exclusions and production chunks.
- `backend/src/admin.rs`, `server.rs`, `snapshot.rs`, `settings.rs`: management, inference and live runtime configuration.

## Runtime/Tooling Preferences

- Use npm with `package-lock.json`; ESM throughout. Node 22.18+ is required for native TypeScript stripping in tests; Node 24 is suitable.
- TypeScript targets ES2020 with bundler resolution and automatic JSX. No path aliases, Tailwind utility rewrite, formatter or alternative package manager is configured.
- Run `npm run typecheck` and `npm test` after frontend changes, `npm run build` for bundle changes, and Rust tests/format checks for backend changes. UI changes require exercising the actual hash routes and affected dialogs against a real gateway.

## Testing & QA

- Frontend tests are deterministic pure-logic tests; `npm test` discovers only top-level `src/*.test.js`.
- Backend tests cover native/cross-protocol JSON/SSE, failover, quotas, credentials, management, settings and route metadata. Use local controllable HTTP upstreams for end-to-end inference checks; never substitute UI fixture data for real responses.
- Before a complete change is committed: update this file and `.memory/MEMORY.md` plus the relevant linked memory file; stage only related files. Force-add only specific ignored memory files when necessary. Commit locally and report the hash and exercised checks; do not push without an explicit request.
