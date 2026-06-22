//! Compare pass-1 interval coverage vs pass-2 sliding-window coverage.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use garageband::scan_project;
use shazam_engine::{build_index, default_cache_dir, fingerprint_file_cached, FingerprintIndex};

use verification_engine::config::MatchConfig;
use verification_engine::coverage::{
    compute_interval_coverage_from_matches, compute_timeline_coverage,
    gate_timeline_by_matches, CoverageOptions,
};
use verification_engine::matcher::{build_asset_indexes, match_all_assets};
use verification_engine::normalize::media_duration_seconds;

struct Pair {
    label: &'static str,
    band: PathBuf,
    release: PathBuf,
}

fn pairs() -> Vec<Pair> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    vec![
        Pair {
            label: "P1A1",
            band: root.join("example_1/כל מה .band"),
            release: root.join("example_1/כל מה  - 21:06:2026, 19.13.wav"),
        },
        Pair {
            label: "P2A2",
            band: root.join("example_2/השיר הלbm.band"),
            release: root.join("example_2/השיר הלbm - 21:06:2026, 19.22.wav"),
        },
        Pair {
            label: "P3A3",
            band: root.join("example_3/nobodys_listening_anyway.band"),
            release: root.join("example_3/nobodys_listening_anyway - 21:06:2026, 19.33.wav"),
        },
        Pair {
            label: "P1A2",
            band: root.join("example_1/כל מה .band"),
            release: root.join("example_2/השיר הלbm - 21:06:2026, 19.22.wav"),
        },
        Pair {
            label: "P1A3",
            band: root.join("example_1/כל מה .band"),
            release: root.join("example_3/nobodys_listening_anyway - 21:06:2026, 19.33.wav"),
        },
        Pair {
            label: "P2A1",
            band: root.join("example_2/השיר הלbm.band"),
            release: root.join("example_1/כל מה  - 21:06:2026, 19.13.wav"),
        },
        Pair {
            label: "P2A3",
            band: root.join("example_2/השיר הלbm.band"),
            release: root.join("example_3/nobodys_listening_anyway - 21:06:2026, 19.33.wav"),
        },
        Pair {
            label: "P3A1",
            band: root.join("example_3/nobodys_listening_anyway.band"),
            release: root.join("example_1/כל מה  - 21:06:2026, 19.13.wav"),
        },
        Pair {
            label: "P3A2",
            band: root.join("example_3/nobodys_listening_anyway.band"),
            release: root.join("example_2/השיר הלbm - 21:06:2026, 19.22.wav"),
        },
    ]
}

fn run_pair(pair: &Pair, cache: &Path, match_config: &MatchConfig) -> Result<(f64, f64, f64, f64)> {
    let scan = scan_project(&pair.band)?;
    let target_duration = media_duration_seconds(&pair.release).unwrap_or(180.0);
    let (target_fps, _) = fingerprint_file_cached(&pair.release, cache)?;
    let target_index = build_index(&target_fps);

    let mut asset_fps = std::collections::HashMap::new();
    let mut asset_durations = std::collections::HashMap::new();
    for source in &scan.audio_assets {
        let name = source.file_name().unwrap().to_string_lossy().to_string();
        let (fps, _) = fingerprint_file_cached(source, cache)?;
        asset_durations.insert(name.clone(), media_duration_seconds(source).unwrap_or(30.0));
        asset_fps.insert(name, fps);
    }

    let matches = match_all_assets(
        &scan.audio_assets,
        &target_index,
        target_duration,
        &asset_fps,
        &asset_durations,
        match_config,
    );

    let asset_indexes: Vec<FingerprintIndex> = build_asset_indexes(&scan.audio_assets, &asset_fps);
    let coverage_opts = CoverageOptions::from_config(2.0, 0.5, 0.30, match_config);

    let window_raw = compute_timeline_coverage(
        target_duration,
        &target_fps,
        &asset_indexes,
        coverage_opts,
        None,
    );
    let window_raw_ratio = window_raw.coverage_ratio;
    let window_gated = gate_timeline_by_matches(window_raw, &matches);

    let interval_raw = compute_interval_coverage_from_matches(
        target_duration,
        &matches,
        match_config.possible_min,
    );
    let interval_raw_ratio = interval_raw.coverage_ratio;
    let interval_gated = gate_timeline_by_matches(interval_raw, &matches);

    Ok((
        window_raw_ratio,
        window_gated.coverage_ratio,
        interval_raw_ratio,
        interval_gated.coverage_ratio,
    ))
}

fn main() -> Result<()> {
    let cache = default_cache_dir();
    std::fs::create_dir_all(&cache)?;
    let match_config = MatchConfig::default();

    println!(
        "{:<6} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "pair", "win_raw", "win_gate", "int_raw", "int_gate", "Δ_gate"
    );
    println!("{}", "-".repeat(58));

    let mut deltas = Vec::new();
    for pair in pairs() {
        let (w_raw, w_gate, i_raw, i_gate) =
            run_pair(&pair, &cache, &match_config).with_context(|| format!("{}", pair.label))?;
        let delta = (w_gate - i_gate).abs();
        deltas.push((pair.label, delta, w_gate, i_gate));
        println!(
            "{:<6} {:>9.1}% {:>9.1}% {:>9.1}% {:>9.1}% {:>7.1}%",
            pair.label,
            w_raw * 100.0,
            w_gate * 100.0,
            i_raw * 100.0,
            i_gate * 100.0,
            delta * 100.0,
        );
    }

    let mean_delta: f64 = deltas.iter().map(|(_, d, _, _)| d).sum::<f64>() / deltas.len() as f64;
    let max_row = deltas
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .unwrap();

    println!();
    println!("Mean |window_gated − interval_gated|: {:.1}%", mean_delta * 100.0);
    println!(
        "Largest gap: {} (window {:.1}% vs interval {:.1}%)",
        max_row.0,
        max_row.2 * 100.0,
        max_row.3 * 100.0,
    );

    Ok(())
}
