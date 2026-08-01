# StudyPulse Local-first Desktop Client

This is the Web UI client for StudyPulse. It is packaged as a macOS Tauri
desktop application; the React UI communicates with the existing Rust Core
through Tauri commands. There is no Electron shell and no localhost server in
the production app.

## Requirements

- macOS 15 or newer
- Rust 1.97.1
- Node.js 24+ and npm 11+

## Development

```sh
npm install
npm run tauri:dev
```

The Vite-only command is useful for frontend development and intentionally
shows the desktop-only boundary when no Tauri runtime is present:

```sh
npm run dev
```

## Verification

```sh
npm test
npm run lint
npm run typecheck
npm run build
cargo fmt --manifest-path core/Cargo.toml --all -- --check
cargo test --manifest-path core/Cargo.toml --workspace
cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings
npm run tauri:build
```

The Rust Core is kept under `core/` and uses the same Workspace schema,
Agent event cursor, tool permission model, backup transaction behavior and
optional Cloud AI/BYOK providers as StudyPulse Desktop. Credentials are
stored in the macOS Keychain by the Tauri host and are never sent to the
React layer.
