# Source

Rust workspace — build from repo root:

```bash
cargo build --workspace
```

## Crates

| Crate | Role |
|-------|------|
| **`app/`** | Tauri desktop (“Human Music”). Frontend in `app/src/`, backend in `app/src-tauri/`. |
| **`verification_engine/`** | DAW-agnostic provenance pipeline. Binary: `gb-verify`. |
| **`shazam_engine/`** | Landmark fingerprinting + match (production hot path). |
| **`daw_interfaces/garageband/`** | GarageBand `.band` I/O. Future DAWs get sibling crates. |

## Dependency graph

```
app (Tauri)
 └── verification_engine
      ├── garageband
      └── shazam_engine
```

Python benchmarks live in [`../benchmarks/`](../benchmarks/) — not part of this workspace.
