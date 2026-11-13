# Repository Guidelines

## Project Overview

Sepia is a React 18 + TypeScript control plane connected to a Rust AI gateway. Provider, route, client-key, request and analytics operations use the real `/api/v1` service; there is no seeded provider/request dataset. The gateway implements OpenAI, Anthropic and Gemini text protocols, cross-protocol JSON/SSE, ordered failover, quotas and observation. WASM plugins are not implemented: the Plugins page explicitly reports an unavailable runtime.

## Architecture & Data Flow

- `frontend/src/main.tsx` composes `WorkspaceProvider`, first-run `Onboarding`, `GatewayGate` and `App`. Onboarding completion is stored in `localStorage['sepia.onboarding.v1']`.
- `frontend/src/workspace.tsx` owns domain types, Context, translations and download helpers. `frontend/src/gateway-workspace.ts` loads providers, routes, requests, analytics, client keys, groups and runtime settings from the gateway and exposes `refresh`, `run`, `login` and `logout`.
- `frontend/src/api.ts` is the same-origin API client and boundary mapper. It serializes Provider/route/key inputs, maps backend records to UI types, and preserves request timestamps for period filtering. Mutations call `run()`, which executes the request then refreshes the workspace; do not reintroduce local-only Provider/route/request setters.
- `GatewayGate` checks the session and public initialization status. First startup accepts the one-time bootstrap token and administrator credentials, then logs in. Administrator sessions use HttpOnly cookies and are separate from inference Bearer keys.
- `frontend/src/App.tsx` owns hash navigation (`#Providers`, `#Routing`, etc.), the operational Overview, route preview selection, command search and global dialogs. Views call the real management API through Context.
- The console uses ink-neutral surfaces, semantic theme tokens and mint route connectors. `theme.ts` maps each accent to distinct dark/light foreground variants; chart graphics use `--chart-*`. The legacy marketing Hero, Three.js orb, fake uptime bar and source-less notification center are removed.
- `route-display.ts` selects the latest enabled route with a request, ignoring newer unrouted/disabled-route records. `RouteChain` preserves saved candidate order and Provider identity for same-model channels, and labels disabled Providers unavailable. The Overview previews the latest successful request for the selected route without inventing per-hop telemetry.
- Routing uses a compact searchable route list and one selected detail, not one large topology card per model. The route editor retains ordered selected-model cards, Provider/model catalog search, drag reordering and accessible up/down controls. Route tests only describe matching/configuration and explicitly do not call upstreams or infer health.
- Browser storage is limited to interface preferences and onboarding state. Provider configuration, encrypted upstream credentials, hashed client keys, routes, requests and runtime retry settings live in the gateway.
- Backend `admin.rs` owns management endpoints; `server.rs` owns inference; `store.rs`, `updates.rs`, `policy.rs`, `observability.rs` and migrations own durable state; `snapshot.rs` publishes enabled routes/providers and `max_attempts` to the runtime.
- Analytics accepts `period=24 hours|7 days|30 days` and uses one SQL window for totals, series and Provider aggregates. Frontend route/model breakdowns filter at most the latest 200 request records to the same period and disclose that limit in the UI. Request logs persist the matched route slug and expose the final successful upstream model, including streaming and failover; route charts use that stored route instead of inferring from the model. Route name/description are persisted by migration `0005_route_metadata.sql`.
- Only ordered failover is exposed. Do not add UI controls for unimplemented routing strategies, session affinity, automatic Provider sync, plugin installation or body logging unless the runtime supports them.
- External Anthropic clients may omit `anthropic-version` for `GET /v1/models`; if present, it must be `2023-06-01`. `POST /v1/messages` still requires that exact version. A client key grants a route when it matches either the route slug or an enabled candidate model. The local gateway and Vite proxy default to `127.0.0.1:5000`; existing `3425` browser preferences migrate to this default.
- The route editor uses a compact searchable Provider/model catalog and ordered selected-model cards. Only enabled candidates are persisted and exposed; the editor supports accessible up/down controls and drag reordering. Key creation lists enabled smart route slugs separately from Provider models. Model authorization reuses route selection: a candidate-scoped key may call its route slug, and an exact candidate request selects its owning route before pattern/default fallback.

## Key Directories

- `frontend/src/views/`: Providers, Routing, Plugins, Requests and Analytics pages.
- `frontend/src/components/`: Provider/key editors, settings, onboarding, authentication gate and shared UI.
- `backend/src/`: Rust gateway modules; `backend/migrations/`: SQL migrations; `backend/tests/`: protocol, security and integration tests.
- `frontend/public/`: static assets. `frontend/dist/` and `*.tsbuildinfo`: generated output; do not edit.
- `docs/2026-11-03/调查/`: historical research, not the executable specification.
- `temp/`: ignored local scratch files; keep temporary investigation output here instead of the repository root.

## Development Commands

On Windows, run `start.cmd` from the repository root to install missing frontend dependencies and open the gateway and Vite in separate terminal windows. Run individual commands from the repository root:

```sh
npm ci --prefix frontend
npm run dev --prefix frontend
npm run typecheck --prefix frontend
npm test --prefix frontend
npm run build --prefix frontend
npm run preview --prefix frontend
cargo test --manifest-path backend/Cargo.toml
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path backend/Cargo.toml --check
cargo run --manifest-path backend/Cargo.toml
```

Vite listens on `0.0.0.0:5173` and proxies `/api` and `/v1` to `http://127.0.0.1:5000`. Set `SEPIA_GATEWAY_URL` to override the proxy target for an isolated gateway. Backend configuration uses `SEPIA_DATA_DIR`, `SEPIA_LISTEN` and `SEPIA_BOOTSTRAP_TOKEN`. Production serves `frontend/dist/` through the gateway; run `npm run build --prefix frontend` and start the gateway from the repository root before production smoke tests.

On Windows, use Rust stable with Visual Studio C++ Build Tools and an MSVC Developer Command Prompt. Stop a running development gateway before rebuilding its executable. `SEPIA_DEV_INSECURE_KEYSTORE=1` is debug-only and must never protect production secrets. There is no lint/coverage/CI command configured.

## Code Conventions & Common Patterns

- TypeScript/TSX: 2-space indentation, single quotes, no semicolons, strict types, relative imports. `noUnusedLocals` and `noUnusedParameters` are enabled.
- Keep the React + semantic CSS design: `glass-panel`, `button`, `settings-card`, theme variables and responsive rules. Reuse `SectionHeading`, `StatusDot`, `ProviderCell`, `RouteChain`, `Modal`, `Field`, `Toggle`, `DialogActions`, Headless UI and Lucide icons from `components/UI.tsx`/`RouteChain.tsx`; do not recreate them inside individual views.
- Use `--bg`, `--surface`, `--text`, `--text-secondary`, `--border`, `--accent`, `--focus`, `--connection` and `--chart-*` instead of hard-coded green light-theme overrides. Main text is at least 14px, supporting labels 12px, controls 40–44px, and route chains collapse vertically on narrow screens.
- Dropdown menus use `width: 100%` of their own trigger for both field and compact variants. Do not restore fixed menu widths. Long option labels ellipsize and expose their full text through `title`; verify narrow viewports do not overflow.
- Add user-facing strings to `frontend/src/i18n.ts` as `[English, 简体中文, 日本語]`; use `useTranslation()`. Verify dynamic labels in the running UI.
- Keep backend snake_case and frontend camelCase conversion in `api.ts`. Route candidates use Provider IDs in API tuples and Provider names in UI state. Route names/descriptions must survive refresh.
- Provider model discovery is one “Fetch models” action. New Providers use `POST /api/v1/providers/test`; saved Providers use `POST /api/v1/providers/:id/test` with the current protocol, endpoint and first newly entered key (blank falls back to saved). Both return models without persisting. Fetch/test only replaces the dialog draft; the explicit connect/save action persists. Failures keep the draft and show a sanitized upstream error. Loading disables model mutations.
- Provider edits persist the selected protocol, endpoint, group, enabled state and model list together; streamed upstream SSE accepts both LF and CRLF frame separators before protocol conversion.
- Client-key plaintext is returned only by key creation and shown once in the current page. Never persist it in localStorage or claim an existing key can be revealed later. Do not accept a browser-generated client secret.
- Provider creation sends both `secret` (the first entered upstream key) and `secrets` (all entered keys). The backend deduplicates them for storage and may use either for discovery; never send only `secrets` with an empty `secret`, which would break saved-key fallback.
- Use real empty states and real connection errors. Do not fabricate metrics, health, savings, request records, plugin capabilities or successful upstream calls.
- Keep pure logic tests in top-level `frontend/src/*.test.js` using `node:test` and `node:assert/strict`. Rust integration coverage belongs in `backend/tests/`.

## Important Files

- `frontend/src/gateway-workspace.ts`: real workspace state and session lifecycle.
- `frontend/src/api.ts`: API client, serializers and record mappers.
- `frontend/src/components/GatewayGate.tsx`: login and first-run administrator initialization.
- `frontend/src/components/SettingsView.tsx`: client-key management and supported runtime settings.
- `frontend/src/components/KeyPolicyDialog.tsx` + `frontend/src/key-policy.ts`: scoped client-key policy editor and validation.
- `frontend/src/components/Onboarding.tsx` + `frontend/src/onboarding.ts`: workspace preferences and first-run flow.
- `frontend/src/App.tsx`: shell, Overview and global dialogs.
- `frontend/vite.config.ts`: dev proxy, watcher exclusions and production chunks.
- `backend/src/admin.rs`, `server.rs`, `snapshot.rs`, `settings.rs`: management, inference and live runtime configuration.

## Runtime/Tooling Preferences

- Use npm with `frontend/package-lock.json`; ESM throughout. Node 22.18+ is required for native TypeScript stripping in tests; Node 24 is suitable.
- TypeScript targets ES2020 with bundler resolution and automatic JSX. No path aliases, Tailwind utility rewrite, formatter or alternative package manager is configured.
- Run `npm run typecheck --prefix frontend` and `npm test --prefix frontend` after frontend changes, `npm run build --prefix frontend` for bundle changes, and Rust tests/format checks for backend changes. UI changes require exercising the actual hash routes and affected dialogs against a real gateway.

## Testing & QA

- Frontend tests are deterministic pure-logic tests; `npm test --prefix frontend` discovers only top-level `frontend/src/*.test.js`.
- Backend tests cover native/cross-protocol JSON/SSE, failover, quotas, credentials, management, settings and route metadata. Use local controllable HTTP upstreams for end-to-end inference checks; never substitute UI fixture data for real responses.
- Keep regression coverage for both protocol persistence through the management API and CRLF-formatted upstream SSE streams.
- Before a complete change is committed: update this file and `.memory/MEMORY.md` plus the relevant linked memory file; stage only related files. Force-add only specific ignored memory files when necessary. Commit locally and report the hash and exercised checks; do not push without an explicit request.
