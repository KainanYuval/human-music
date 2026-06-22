# Human Music

Verify that released audio was produced from a GarageBand `.band` project.

## Roadmap

```
Step 1  ████████████████░░  Single-DAW local verifier (GarageBand) — POC          ← now
Step 2  ░░░░░░░░░░░░░░░░░░  AI detection on single tracks
Step 3  ░░░░░░░░░░░░░░░░░░  Online registry (proof of humanity)
Step 4  ░░░░░░░░░░░░░░░░░░  Multiple DAW support
```

| Step | Goal | Status |
|------|------|--------|
| **1** | Local desktop app: one `.band` + one release → verification report | **POC shipped** |
| **2** | Detect AI-generated audio on individual tracks | Planned |
| **3** | Publish verified reports to a public registry | Planned |
| **4** | Ableton, Logic, and other DAW adapters | Planned |

## Download (macOS)

CI builds on every push to `main` and publishes installable artifacts to **GitHub Container Registry**:

**Package:** `ghcr.io/kainanyuval/human-music:macos-latest`

Requires [ORAS](https://oras.land/docs/installation) (e.g. `brew install oras`):

```bash
mkdir -p ~/Downloads/human-music && cd ~/Downloads/human-music
oras pull ghcr.io/kainanyuval/human-music:macos-latest
open Human-Music.dmg
```

Install **ffmpeg** separately (`brew install ffmpeg`) — the verifier uses it at runtime to normalize audio.

Tagged releases use `ghcr.io/kainanyuval/human-music:macos-v0.1.0` (match the git tag).

The same package includes the `gb-verify` CLI binary for terminal use.

> **First launch:** macOS may block unsigned builds. Right-click the app → **Open**, or allow in **System Settings → Privacy & Security**.

## Layout

```
src/
├── app/                  # Tauri desktop UI (“Human Music”)
├── verification_engine/  # gb-verify CLI + library
├── shazam_engine/        # fingerprint matcher
└── daw_interfaces/
    └── garageband/       # .band scanner
```

## Build from source

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
