# Sepia

Sepia is a React control plane for a Rust AI gateway.

## Start on Windows

Run `start.cmd` from the repository root. It installs frontend dependencies when needed, then opens separate Gateway and Frontend terminal windows.

- Frontend: http://localhost:5173
- Gateway: http://127.0.0.1:3425

Close both service windows to stop the development servers. First-run gateway initialization uses the bootstrap token shown in the Gateway window.

## Project layout

- `frontend/`: React, TypeScript and Vite application, including its npm dependencies and build output.
- `backend/`: Rust gateway, migrations and integration tests.
- `docs/`: design documents and historical research.
- `temp/`: local scratch files; contents are ignored by Git.

## Manual development commands

Run commands from the repository root:

```sh
npm ci --prefix frontend
npm run dev --prefix frontend
cargo run --manifest-path backend/Cargo.toml
```

## Verification

```sh
npm run typecheck --prefix frontend
npm test --prefix frontend
npm run build --prefix frontend
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path backend/Cargo.toml
```

For production, build the frontend first and start the gateway from the repository root. It serves `frontend/dist/` as the same-origin SPA.
