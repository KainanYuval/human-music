### 2026-06-21 — repo layout: project / data / src
- **Decision:** Top-level dirs are `project/` (meta), `data/` (fixtures), `src/` (code). Under `src/`: `app/` (Tauri UI), `verification_engine/` (pipeline + `gb-verify` CLI), `daw_interfaces/garageband/` (`.band` scanner + metadata). Root `Cargo.toml` is a workspace.
- **Rationale:** Clear separation of product code, DAW adapters, and test data; room for more DAW crates without bloating the engine.

### 2026-06-21 — gb-verify desktop shell
- **Decision:** Super-simple Tauri app under `src/app/` calls `verification_engine` in-process via `run_verify` with structured progress events; user picks `.band` folder + released audio, sees overall + step progress bars and activity log.
- **Rationale:** GUI for non-CLI users; pure Rust stack — no Python runtime.

### 2026-06-21 — duration-weighted progress budget
- **Decision:** After scan, ffprobe target + asset durations; allocate work units (normalize ∝ seconds, chroma ∝ seconds, coverage ∝ song_length × asset_count); overall percent = completed_units / total_units (~28 units/sec).
- **Rationale:** Fixed stage slices mis-track wall time on large projects; bar + ETA now scale with song length and sample count.

- **Decision:** Remove Python `gb_verify/` package; CLI binary is `gb-verify` from `src/verification_engine/`; Tauri links `verification_engine` as path dependency.
- **Rationale:** Rust verifier is faster (~1.6s vs ~2.5s) and eliminates venv/Python maintenance.

- **Decision:** Mixed-export verification uses pitch-class (chroma) similarity and 2s sliding timeline coverage, not raw waveform Pearson alone; PASS when timeline coverage ≥70% plus strong/possible asset match.
- **Rationale:** Ground-truth `.band`→MP3 export destroys waveform correlation on individual stems; chroma survives EQ/limiting and explains ~98% of target timeline.

### 2026-06-21 — gb-verify speed: FFT chroma xcorr
- **Decision:** Replace Python offset loops with scipy FFT cross-correlation on cached chroma matrices; drop waveform refinement from hot path.
- **Rationale:** Same scores as brute-force chroma (~150× faster on ground truth); matching drops from minutes to sub-second.

### 2026-06-21 — gb-verify-rs Rust port
- **Decision:** Rust implementation at `src/verification_engine/` with binary `gb-verify`; GarageBand I/O in `src/daw_interfaces/garageband/`; same pipeline (scan → ffmpeg normalize → FFT chroma match → timeline coverage → JSON/HTML report); `--progress-jsonl` for CLI subprocess use.
- **Rationale:** Native speed (~1.6s on ground truth); single language for CLI and Tauri.

### 2026-06-21 — canonical src layout enforced
- **Decision:** All product code lives under `src/` only: `verification_engine/` (CLI + discrimination pipeline), `app/` (Tauri), `daw_interfaces/garageband/`. Removed root-level `gb_verify_rs/` and `app/` duplicates; workspace members unchanged.
- **Rationale:** README layout was correct but implementation had drifted to parallel root crates during discrimination work.
