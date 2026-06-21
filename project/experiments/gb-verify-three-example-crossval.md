# gb-verify CLI — three-example PASS cross-validation

## Context

Ground-truth check: all three `data/example_*` pairs (.band + released export) should **PASS**. Two consecutive CLI runs per pair to confirm deterministic verdicts and score stability.

## Environment

- Date: 2026-06-21
- Binary: `cargo run -p verification_engine --release` (or `target/release/gb-verify`)
- Host: macOS (darwin), ffmpeg/ffprobe on PATH

## Method

```bash
/usr/bin/time -p gb-verify \
  --project "<.band>" \
  --audio "<export>" \
  --out "data/example_N/cli_bench/<run>"
```

Each pair run twice (clean `--out` each time). Exit code 0 = PASS, 2 = FAIL.

## Dataset shape

| Example | Assets | Target (s) | Asset audio total (s) |
|---------|--------|------------|------------------------|
| example_1 (כל מה) | 15 | 19.3 (MP3) | 345.2 |
| example_2 (השיר הלbm) | 7 | 50.5 | 272.0 |
| example_3 (nobodys_listening_anyway) | 50 | 206.5 | 3199.2 |

## Results

| Run | Verdict | Provenance % | Coverage % | Strong | Possible | Best asset | Best score | Wall (s) |
|-----|---------|--------------|------------|--------|----------|------------|------------|----------|
| ex1 MP3 run0 | PASS | 98.73 | 98.73 | 14 | 1 | Untitled 1#17.wav | 0.9895 | 2.26 |
| ex1 MP3 run1 | PASS | 98.73 | 98.73 | 14 | 1 | Untitled 1#17.wav | 0.9895 | 1.85 |
| ex1 WAV run0 | PASS | 99.09 | 99.09 | 14 | 1 | Untitled 1#17.wav | 0.9895 | 1.93 |
| ex2 run0 | PASS | 99.98 | 99.98 | 4 | 3 | Brit and Clean#19.wav | 0.9068 | 2.07 |
| ex2 run1 | PASS | 99.98 | 99.98 | 4 | 3 | Brit and Clean#19.wav | 0.9068 | 2.08 |
| ex3 run0 | PASS | 99.66 | 99.66 | 42 | 5 | Black DI#02.wav | 0.9713 | 55.01 |
| ex3 run1 | PASS | 99.66 | 99.66 | 42 | 5 | Black DI#02.wav | 0.9713 | 55.18 |

## Cross-validation

- **All 7 runs: PASS**, exit code 0.
- **Repeat runs:** identical verdict, provenance, coverage, match counts, best asset/score (deterministic pipeline).
- **example_1 MP3 vs WAV:** both PASS; WAV export +0.36 pp coverage (lossy MP3 vs PCM); same best stem and score.
- **Timing:** ~2 s for small projects (≤15 assets, ≤50 s song); ~55 s for example_3 (50 assets, 206 s song, ~3200 s total stem audio) — ffmpeg normalize dominates.

## Artifacts

- `data/example_1/cli_bench/{mp3_run0,mp3_run1,wav_run0}/report.json`
- `data/example_2/cli_bench/{run0,run1}/report.json`
- `data/example_3/cli_bench/{run0,run1}/report.json`
