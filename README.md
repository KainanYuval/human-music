# Human Music

Verify that released audio was produced from a GarageBand `.band` project.

## Layout

```
src/
├── app/                  # Tauri desktop UI (“Human Music”)
├── verification_engine/  # gb-verify CLI + library
├── shazam_engine/        # fingerprint matcher
└── daw_interfaces/
    └── garageband/       # .band scanner
```

## Quick start

**CLI:**

```bash
cargo run -p verification_engine --bin gb-verify --release -- \
  --project "/path/to/session.band" \
  --audio   "/path/to/release.mp3" \
  --out     /tmp/gb-report
```

**Desktop app:**

```bash
cd src/app && npm install && npm run dev
```

## Pipeline

Scan `.band` stems → normalize audio → Shazam-style fingerprint match → timeline coverage → PASS/FAIL + JSON/HTML report.

## Crates

| Crate | Role |
|-------|------|
| `garageband` | Parse GarageBand bundles |
| `shazam_engine` | Landmark fingerprints |
| `verification_engine` | Full verification pipeline (`gb-verify`) |
| `gb-verify-app` | Desktop shell |

Build everything from the repo root: `cargo build --workspace`.
