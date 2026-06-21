# Source

Rust workspace members live here. Build everything from the repo root:

```bash
cargo build --workspace
```

## Crates

- **`app/`** — Tauri shell. Frontend in `app/src/`, Rust backend in `app/src-tauri/`.
- **`verification_engine/`** — DAW-agnostic provenance pipeline. Binary: `gb-verify`.
- **`daw_interfaces/garageband/`** — GarageBand `.band` bundle I/O. Future DAWs get sibling crates under `daw_interfaces/`.

## Dependency graph

```
app (Tauri)
 └── verification_engine
      └── garageband
```
