# Repository Guidelines

## Project Overview

Sepia combines a React control-plane preview and an independent Rust AI-gateway service. The React UI still uses seeded demo data and browser persistence; its connection tests, refreshes, and installs remain simulated until the explicit frontend-integration change. The Rust service implements real configuration, authentication, protocol adaptation, routed HTTP inference, and observation. Do not imply the UI already consumes it.

## Architecture & Data Flow

- `index.html` loads `src/main.tsx`, which selects the first-run `Onboarding` flow or `<App />` inside `WorkspaceProvider` and `React.StrictMode`. Completion is stored under `localStorage['sepia.onboarding.v1']`; skipping also marks it complete.
- `src/App.tsx` owns the shell, hash-based navigation (`#Providers`, `#Routing`, etc.), Overview, command search, notifications, and cross-page dialogs. Navigation is hand-rolled with `window.location.hash`; there is no router library.
- `src/workspace.tsx` is the central data layer: domain types, demo defaults, React Context, persistence, translations hook, and file downloads. `useWorkspace()` exposes state and setters to views.
- State flow: view interaction → Context setter → component render → one effect serializes the workspace to `localStorage['sepia.workspace.v1']`. Loading merges stored settings over defaults and falls back to demo data on storage/parse failure.
- Settings effects update the root element's theme, accent, language, reduced-motion, and compact-table attributes/classes.
- `src/gateway.ts` only formats endpoint URLs. Local deployment always forces HTTP; cloud honors the configured protocol.
- `src/components/RoutingOrb.tsx` is lazy-loaded and omitted when reduced motion is enabled. Three.js and chart dependencies are split into separate production chunks.
- `backend/` is a Rust 2024 Axum/Tokio service: `server.rs` owns HTTP inference, `admin.rs` session-protected management, `protocol.rs` OpenAI/Anthropic/Gemini text and SSE codecs, `relay.rs` upstream HTTP/streaming, `store.rs` + migrations durable configuration, `crypto.rs` key protection, and `quota.rs` transactional quota reservations. `observability.rs` and `attempts.rs` persist request/attempt traces; `discovery.rs` performs real model-list connection tests.
- Backend supports the three native text protocols in both directions and cross-protocol JSON/SSE; ordered failover only. Unsupported media/tools/multi-candidate features must fail explicitly, never silently drop content. The UI and service are not integrated yet; plugins remain a later WASM deliverable.

## Key Directories

- `src/views/`: page-level Providers, Routing, Plugins, Requests, and Analytics views.
- `src/components/`: provider editing, settings, shell dialogs, dropdowns, shared UI primitives, and the 3D orb.
- `public/`: static assets copied to the build; currently the favicon.
- `docs/2026-11-03/调查/`: historical research about other gateway projects, not Sepia's executable specification or current architecture. `docs/2026-11-03/review/` is empty.
- `dist/` and `tsconfig.tsbuildinfo`: generated build output; do not edit them. There is no repository `scripts/` directory or project README.

## Development Commands

```sh
npm ci                 # reproducible install from package-lock.json
npm run dev            # Vite on 0.0.0.0:5173
npm run typecheck      # tsc --noEmit for src/
npm test               # Node tests matching src/*.test.js
npm run build          # tsc -b && vite build; writes dist/
npm run preview        # serve the production build with Vite
cargo test --manifest-path backend/Cargo.toml
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path backend/Cargo.toml --check
cargo run --manifest-path backend/Cargo.toml
```

On Windows, install Rust stable plus Visual Studio Build Tools' C++ workload. Run Cargo from a Visual Studio Developer Command Prompt; plain shells may resolve Git's `link.exe` instead of MSVC's linker. Vite ignores `backend/target` to avoid Windows watcher locks. Backend defaults to `127.0.0.1:3425`; use `SEPIA_DATA_DIR`, `SEPIA_LISTEN`, and `SEPIA_BOOTSTRAP_TOKEN` for local configuration. Production uses the OS keyring. `SEPIA_DEV_INSECURE_KEYSTORE=1` is debug-only and must never be used for production secrets.

Run `npm run typecheck` and `npm test` after source changes; run `npm run build` when bundling or lazy chunks are affected. No lint, formatting, coverage, end-to-end, or CI command is configured—do not claim one exists.

## Change Completion & Commits

- A complete change is an end-to-end, verified deliverable, not each intermediate file edit or isolated research step.
- Before declaring a complete change done: update `AGENTS.md` with durable repository conventions, architecture, or commands affected by it; update `.memory/MEMORY.md` and the relevant linked memory file with current product/architecture state and decisions. Remove obsolete status statements instead of accumulating contradictory snapshots.
- Commit each complete change locally with `git commit`; never push unless the user explicitly asks. Review `git status --short` and stage only the files belonging to that change. Never mix unrelated user changes into its commit.
- The commit must include the relevant `AGENTS.md` and memory updates, even when `.memory/` is ignored; use explicit tracked paths or `git add -f .memory/<file>` for memory files. Do not force-add the entire directory.
- Report the commit hash and exercised verification in the completion message. Blocked or partial work, unanswered design proposals, and investigation-only research do not qualify for this commit rule.

## Code Conventions & Common Patterns

- Match existing TypeScript/TSX style: 2-space indentation, single quotes, no semicolons, and roughly React → third-party → local imports. Use inline `type` imports and strict typing; `noUnusedLocals` and `noUnusedParameters` are enabled.
- Use relative imports; no path aliases are configured. Components generally use named exports and inline prop type literals; retain existing default exports for `App`, `SettingsView`, and `RoutingOrb`.
- Reuse `useWorkspace()` for shared state instead of adding another store. Prefer functional setters (`setProviders(current => ...)`) for updates based on prior state.
- Extend shared domain types and defaults in `workspace.tsx` together. Normalize provider protocols with `protocolOf()`/`protocolModels()`; use canonical `openai`, `anthropic`, or `gemini` values for new providers.
- Route rules have both legacy primary/fallback fields and optional `candidates`; route editing derives the former from the first two enabled candidates. Keep these representations synchronized.
- Reuse `Modal`, `Field`, `Toggle`, and `DialogActions` from `components/UI.tsx`, Headless UI dialogs, Lucide icons, translated labels, and existing accessible roles (`switch`, `listbox`, `option`).
- Add user-facing strings to `src/i18n.ts` as `[English, 简体中文, 日本語]`, then call `useTranslation()`. Language values are display strings; update every language-to-locale/index mapping when adding a language. Dynamic translation-key casts can hide missing keys, so verify new navigation/entity labels at runtime.
- Styling is primarily semantic CSS in `src/styles.css` (`glass-panel`, `button primary`, `settings-card`, state modifiers), not Tailwind utility JSX. Reuse CSS variables, especially `--accent`, and account for explicit light-theme and responsive rules. Light surfaces use the `--light-*` semantic tokens; charts use `--chart-accent` and `--chart-series-*`, never hardcoded mint colors that disappear on white.
- Async-looking actions are timer-driven previews; no shared async service layer or dependency-injection framework exists. Storage/clipboard failures are handled locally, and `useWorkspace()` throws if used outside its provider.
- Several UI details are coupled to demo IDs (route/plugin/notification translations) or duplicated in `App.tsx` and `views/ProvidersView.tsx` (`Page`, `SectionHeading`, `StatusDot`, `ProviderCell`). Check these when changing shared behavior rather than assuming a single source of truth.

## Important Files

- `package.json`: npm scripts and React/Vite/TypeScript dependencies.
- `src/main.tsx`: browser entry point, first-run completion gate, and provider composition.
- `src/components/Onboarding.tsx`: three-step workspace initialization with local copy for English, Simplified Chinese, and Japanese; `src/onboarding.ts` owns its persistence key and settings normalization.
- `src/App.tsx`: navigation, shell, Overview, and global dialogs.
- `src/workspace.tsx`: state schema, defaults, persistence, i18n hook, and download helper.
- `src/i18n.ts`: complete translation-key table.
- `src/gateway.ts`: pure gateway URL builder.
- `src/components/SettingsView.tsx`: settings validation and separate preview-key storage (`sepia.preview.keys`).
- `src/components/KeyPolicyDialog.tsx` and `src/key-policy.ts`: preview-key policy editor, model/provider/group scopes, browser-safe IP/CIDR validation, quota JSON, token abbreviations, and custom-key handling. Optional amount/token limits are `null`; zero RPM/TPM means unlimited.
- `src/styles.css`: design tokens, component styling, themes, and responsive behavior.
- `vite.config.ts`: dev server and manual production chunks.
- `tsconfig.json`, `tailwind.config.js`, `postcss.config.js`: source compilation and styling pipeline.

## Runtime/Tooling Preferences

- Use **npm** with `package-lock.json`; no alternative package-manager lockfile or `packageManager` field exists.
- Use **Node.js with native TypeScript type stripping** (Node 22.18+ or newer; Node 24 is a safe current choice). `npm test` directly imports `.ts` from JavaScript without a loader, and the repository has no `engines` or `.nvmrc` pin.
- The package is ESM (`"type": "module"`). Source targets ES2020 with bundler resolution and automatic React JSX runtime.
- Vite exposes the dev server on all interfaces at port 5173. There is no dev proxy, environment-variable configuration, or backend process to start.
- Do not introduce a formatter/linter as though it were already enforced. Follow nearby formatting and run the existing checks.

## Testing & QA

- Tests use Node's built-in `node:test` and `node:assert/strict`, with behavior-focused names and deterministic public-output assertions.
- Co-locate pure TypeScript tests as `src/<module>.test.js` and import the source explicitly (`import { gatewayUrl } from './gateway.ts'`). The current script discovers only top-level `src/*.test.js`; nested component/view tests will not run, and no JSX/TSX test loader is configured.
- `src/gateway.test.js` covers local/cloud protocols, default/custom ports, and default/custom API paths. Extend this pattern for pure logic; component testing would require intentionally adding a browser/React test toolchain.
- No coverage threshold, browser test suite, or CI workflow is configured. For UI changes, supplement typecheck/tests with `npm run dev` and exercise the affected hash route, persistence after reload, dialog/keyboard behavior, responsive layout, and light/dark or reduced-motion settings where relevant.
