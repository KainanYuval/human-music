#!/usr/bin/env python3
"""Wall-clock benchmark for gb-verify pipeline phases."""

from __future__ import annotations

import argparse
import json
import platform
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from gb_verify.chroma import chroma_matrix
from gb_verify.cli import run
from gb_verify.coverage import compute_timeline_coverage
from gb_verify.matcher import load_mono_float, match_asset
from gb_verify.metadata import collect_metadata, project_manifest_hash, sha256_file
from gb_verify.normalize import normalize_to_wav
from gb_verify.report import build_report_payload, write_html_report, write_json_report
from gb_verify.scanner import scan_project
from gb_verify.verdict import compute_verdict


@dataclass
class PhaseTimings:
    scan_s: float = 0.0
    normalize_target_s: float = 0.0
    normalize_assets_s: float = 0.0
    features_s: float = 0.0
    match_s: float = 0.0
    match_per_asset_s: list[float] = field(default_factory=list)
    coverage_s: float = 0.0
    metadata_report_s: float = 0.0
    total_s: float = 0.0

    def to_dict(self) -> dict:
        d = asdict(self)
        if self.match_per_asset_s:
            d["match_per_asset_mean_s"] = round(
                sum(self.match_per_asset_s) / len(self.match_per_asset_s), 3
            )
            d["match_per_asset_max_s"] = round(max(self.match_per_asset_s), 3)
        return d


def _now() -> float:
    return time.perf_counter()


def benchmark_run(
    project: Path,
    audio: Path,
    out: Path,
    *,
    reuse_normalized: bool = False,
) -> tuple[dict, PhaseTimings]:
    timings = PhaseTimings()
    t0 = _now()

    t = _now()
    scan = scan_project(project)
    timings.scan_s = _now() - t
    if not scan.audio_assets:
        raise SystemExit(f"No audio assets under {project}")

    out.mkdir(parents=True, exist_ok=True)
    temp_dir = out / "normalized_temp"
    temp_dir.mkdir(parents=True, exist_ok=True)

    target_norm = temp_dir / "target.wav"
    if reuse_normalized and target_norm.is_file():
        timings.normalize_target_s = 0.0
    else:
        t = _now()
        normalize_to_wav(audio, target_norm)
        timings.normalize_target_s = _now() - t

    asset_norm: dict[Path, Path] = {}
    t_assets = _now()
    for idx, source in enumerate(scan.audio_assets):
        safe = "".join(c if c.isalnum() or c in "._-" else "_" for c in source.stem)
        dest = temp_dir / f"asset_{idx:03d}_{safe}.wav"
        if reuse_normalized and dest.is_file():
            asset_norm[source] = dest
            continue
        normalize_to_wav(source, dest)
        asset_norm[source] = dest
    timings.normalize_assets_s = _now() - t_assets

    t = _now()
    target_audio, sr = load_mono_float(target_norm)
    target_chroma = chroma_matrix(target_audio, sr)
    asset_chroma_list = []
    for normalized in asset_norm.values():
        asset_audio, _ = load_mono_float(normalized)
        asset_chroma_list.append(chroma_matrix(asset_audio, sr))
    timings.features_s = _now() - t

    matches = []
    t_match = _now()
    for source, normalized in asset_norm.items():
        t_one = _now()
        matches.append(match_asset(normalized, target_norm, asset_name=source.name))
        timings.match_per_asset_s.append(_now() - t_one)
    matches.sort(key=lambda r: r.match_score, reverse=True)
    timings.match_s = _now() - t_match

    t = _now()
    timeline = compute_timeline_coverage(
        target_norm,
        list(asset_norm.values()),
        target_chroma=target_chroma,
        asset_chromas=asset_chroma_list,
        sr=sr,
    )
    verdict = compute_verdict(matches, timeline)
    timings.coverage_s = _now() - t

    t = _now()
    timeline_payload = {
        "explained_seconds": timeline.explained_seconds,
        "target_seconds": timeline.target_seconds,
        "coverage_ratio": timeline.coverage_ratio,
        "window_seconds": timeline.window_seconds,
        "hop_seconds": timeline.hop_seconds,
        "threshold": timeline.threshold,
        "explained_windows": timeline.explained_windows,
        "total_windows": timeline.total_windows,
    }
    asset_hashes = {p.name: sha256_file(p) for p in scan.audio_assets}
    payload = build_report_payload(
        scan=scan,
        target_path=audio,
        matches=matches,
        verdict=verdict,
        timeline_coverage=timeline_payload,
        metadata_evidence=collect_metadata(scan, audio, asset_hashes),
        target_sha256=sha256_file(audio),
        project_sha256=project_manifest_hash(scan),
        asset_hashes=asset_hashes,
    )
    write_json_report(payload, out)
    write_html_report(payload, out)
    timings.metadata_report_s = _now() - t

    timings.total_s = _now() - t0
    return payload, timings


def cli_wall_clock(project: Path, audio: Path, out: Path) -> float | None:
    if out.exists():
        shutil.rmtree(out)
    cmd = [
        sys.executable,
        "-m",
        "gb_verify.cli",
        "--project",
        str(project),
        "--audio",
        str(audio),
        "--out",
        str(out),
    ]
    t0 = _now()
    proc = subprocess.run(cmd, capture_output=True, text=True)
    elapsed = _now() - t0
    if proc.returncode not in (0, 2):
        print(proc.stderr, file=sys.stderr)
        return None
    return elapsed


def environment_info() -> dict:
    py = f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    ffmpeg = shutil.which("ffmpeg") or "missing"
    fpcalc = shutil.which("fpcalc") or "missing"
    return {
        "platform": platform.platform(),
        "processor": platform.processor() or platform.machine(),
        "python": py,
        "ffmpeg": ffmpeg,
        "fpcalc": fpcalc,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark gb-verify pipeline timing")
    parser.add_argument("--project", required=True, type=Path)
    parser.add_argument("--audio", action="append", required=True, type=Path, dest="audios")
    parser.add_argument("--out-root", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=1, help="Timed runs per audio (after optional warm-up)")
    parser.add_argument("--warm-up", action="store_true", help="Discard one cold run before timed runs")
    parser.add_argument("--json", type=Path, help="Write machine-readable results here")
    args = parser.parse_args()

    results: dict = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "environment": environment_info(),
        "cases": [],
    }

    for audio in args.audios:
        label = audio.name
        case: dict = {"label": label, "audio": str(audio), "runs": []}

        for run_idx in range(args.runs + (1 if args.warm_up else 0)):
            is_warmup = args.warm_up and run_idx == 0
            reuse = not is_warmup and run_idx > (1 if args.warm_up else 0)
            out = args.out_root / f"{label.replace(' ', '_')}_run{run_idx}"
            if out.exists():
                shutil.rmtree(out)

            payload, timings = benchmark_run(
                args.project,
                audio,
                out,
                reuse_normalized=reuse and run_idx > 0,
            )
            entry = {
                "run": run_idx,
                "warm_up": is_warmup,
                "reuse_normalized": reuse and run_idx > 0,
                "verdict": payload["verdict"],
                "provenance_score": payload["provenance_score"],
                "asset_count": len(payload["matches"]),
                "timings_s": {k: round(v, 3) for k, v in timings.to_dict().items() if k != "match_per_asset_s"},
                "match_per_asset_s": [round(x, 3) for x in timings.match_per_asset_s],
            }
            if not is_warmup:
                case["runs"].append(entry)
            print(f"\n=== {label} run {run_idx}{' (warm-up)' if is_warmup else ''} ===")
            print(f"Verdict: {payload['verdict']}  coverage: {payload['provenance_score']*100:.1f}%")
            for phase in (
                "scan_s",
                "normalize_target_s",
                "normalize_assets_s",
                "features_s",
                "match_s",
                "coverage_s",
                "metadata_report_s",
                "total_s",
            ):
                print(f"  {phase.replace('_s', ''):18s} {getattr(timings, phase):7.2f}s")

        cli_out = args.out_root / f"{label.replace(' ', '_')}_cli"
        cli_s = cli_wall_clock(args.project, audio, cli_out)
        case["cli_wall_clock_s"] = round(cli_s, 3) if cli_s is not None else None
        if cli_s is not None:
            print(f"\nCLI subprocess wall clock ({label}): {cli_s:.2f}s")

        results["cases"].append(case)

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")
        print(f"\nWrote {args.json}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
