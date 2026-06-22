pub mod audio;
pub mod chroma;
pub mod config;
pub mod coverage;
pub mod fingerprints;
pub mod matcher;
pub mod normalize;
pub mod progress;
pub mod report;
pub mod verdict;
pub mod waveform;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use coverage::{compute_timeline_coverage, gate_timeline_by_matches};
use fingerprints::{
    fingerprint_target_and_stems_parallel_with_progress, FingerprintItemDone,
};
use matcher::{build_asset_indexes, match_all_assets_with_progress, MatchItemDone};
use garageband::metadata::{collect_metadata, project_manifest_hash, sha256_file};
use normalize::{media_duration_seconds, normalize_to_wav_with_progress};
use shazam_engine::{build_index, default_cache_dir, Fingerprint};
use progress::{
    ProgressHandle, ProgressEvent, WorkPlan, U_FP, U_MATCH, U_META, U_NORM, U_PROBE, U_REPORT,
    U_SCAN, STAGE_DONE, STAGE_FEATURES, STAGE_MATCH, STAGE_METADATA, STAGE_NORMALIZE,
    STAGE_REPORT, STAGE_SCAN,
};
use report::{build_report_payload, write_html_report, write_json_report};
use garageband::{scan_project, scan_summary};
use config::VerifyConfig;
use verdict::compute_verdict_monolithic;

fn is_wav(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("wav"))
}

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub config: VerifyConfig,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            config: VerifyConfig::default(),
        }
    }
}

pub fn run_verify<F>(
    project: &Path,
    audio: &Path,
    out: &Path,
    on_progress: Option<F>,
) -> Result<serde_json::Value>
where
    F: FnMut(ProgressEvent) + Send + 'static,
{
    run_verify_with_options(project, audio, out, VerifyOptions::default(), on_progress)
}

pub fn run_verify_with_options<F>(
    project: &Path,
    audio: &Path,
    out: &Path,
    options: VerifyOptions,
    on_progress: Option<F>,
) -> Result<serde_json::Value>
where
    F: FnMut(ProgressEvent) + Send + 'static,
{
    let handle = ProgressHandle::new(
        on_progress.map(|f| Box::new(f) as Box<dyn FnMut(ProgressEvent) + Send>),
    );
    run_verify_inner(project, audio, out, &options, &handle)
}

fn run_verify_inner(
    project: &Path,
    audio: &Path,
    out: &Path,
    options: &VerifyOptions,
    progress: &ProgressHandle,
) -> Result<serde_json::Value> {
    let config = &options.config;
    progress.emit(
        STAGE_SCAN,
        "start",
        "Opening GarageBand project bundle",
        Some(project.display().to_string()),
        None,
        None,
        None,
        None,
    );

    let scan = scan_project(project).context("scan project")?;
    if scan.audio_assets.is_empty() {
        anyhow::bail!(
            "No audio assets found under {}",
            scan.project_path.join("Media/Audio Files").display()
        );
    }

    let asset_total = scan.audio_assets.len();
    let probe_total = asset_total + 1;
    let audio_name = audio.file_name().map(|n| n.to_string_lossy().to_string());
    let needs_normalize = !is_wav(audio);

    let probed_target = media_duration_seconds(audio).unwrap_or(180.0);
    let placeholder_assets = vec![30.0; asset_total];
    progress.set_plan(WorkPlan::estimate(
        probed_target,
        &placeholder_assets,
        needs_normalize,
    ));

    progress.tick(
        STAGE_SCAN,
        "assets",
        "Found embedded recordings",
        U_SCAN,
        Some(format!(
            "{} stems · {} registered in MetaData.plist",
            scan.audio_assets.len(),
            scan.registered_assets.len()
        )),
        None,
        Some(asset_total),
        None,
        None,
    );

    progress.tick(
        STAGE_SCAN,
        "probe_target",
        "Song duration measured",
        U_PROBE,
        Some(format!("{probed_target:.1}s release")),
        Some(0),
        Some(probe_total),
        audio_name.clone(),
        None,
    );

    let probed_assets: Vec<f64> = scan
        .audio_assets
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let dur = media_duration_seconds(p).unwrap_or(30.0);
            let name = p.file_name().map(|n| n.to_string_lossy().to_string());
            progress.tick(
                STAGE_SCAN,
                "probe_stem",
                "Stem duration measured",
                U_PROBE,
                Some(format!("{dur:.1}s")),
                Some(idx + 1),
                Some(probe_total),
                name,
                None,
            );
            dur
        })
        .collect();

    let plan = WorkPlan::estimate(probed_target, &probed_assets, needs_normalize);
    let plan_summary = plan.plan_summary();
    let plan_detail = plan.summary_detail();
    progress.set_plan(plan);

    progress.emit(
        STAGE_SCAN,
        "done",
        "Scan complete — work plan ready",
        Some(plan_detail),
        None,
        None,
        scan.garageband_version.clone(),
        Some(plan_summary),
    );

    let plan = progress.plan().expect("work plan set after scan");

    std::fs::create_dir_all(out)?;
    let temp_dir = out.join("normalized_temp");
    std::fs::create_dir_all(&temp_dir)?;
    let fp_cache_dir = default_cache_dir();
    std::fs::create_dir_all(&fp_cache_dir)?;

    let target_fp_path = if is_wav(audio) {
        audio.to_path_buf()
    } else {
        progress.emit(
            STAGE_NORMALIZE,
            "target_start",
            "Converting released audio to mono WAV",
            audio_name.clone(),
            None,
            None,
            audio_name.clone(),
            None,
        );
        let target_norm_path = temp_dir.join("target.wav");
        let progress_norm = progress.clone();
        let audio_name_norm = audio_name.clone();
        let norm_done = Arc::new(Mutex::new(0.0f64));
        let mut ffmpeg_progress = move |frac: f64| {
            let target_units = U_NORM * frac;
            let mut last = norm_done.lock().unwrap();
            let delta = target_units - *last;
            *last = target_units;
            drop(last);
            if delta > 0.0 {
                progress_norm.tick(
                    STAGE_NORMALIZE,
                    "target_ffmpeg",
                    "Transcoding released audio",
                    delta,
                    None,
                    None,
                    None,
                    audio_name_norm.clone(),
                    None,
                );
            }
        };
        normalize_to_wav_with_progress(audio, &target_norm_path, Some(&mut ffmpeg_progress))?;
        progress.emit(
            STAGE_NORMALIZE,
            "target_done",
            "Released audio normalized",
            Some(target_norm_path.display().to_string()),
            None,
            None,
            audio_name.clone(),
            None,
        );
        target_norm_path
    };

    let target_duration = probed_target;

    progress.emit(
        STAGE_FEATURES,
        "fingerprint_start",
        "Fingerprinting release and project stems",
        Some(format!(
            "{} items · {:.0}s audio",
            plan.fp_item_count, plan.fingerprint_audio_seconds
        )),
        None,
        Some(plan.fp_item_count),
        audio_name.clone(),
        None,
    );

    let progress_fp = progress.clone();
    let on_fp_done: Arc<dyn Fn(FingerprintItemDone) + Send + Sync> = Arc::new(move |item| {
        let label = if item.is_target {
            "Release fingerprinted".to_string()
        } else {
            format!("Stem fingerprinted ({}/{})", item.item_index, item.item_total - 1)
        };
        progress_fp.tick(
            STAGE_FEATURES,
            if item.is_target {
                "fingerprint_target"
            } else {
                "fingerprint_stem"
            },
            label,
            U_FP,
            Some(format!("{:.1}s audio", item.duration_seconds)),
            Some(item.item_index),
            Some(item.item_total),
            Some(item.name),
            None,
        );
    });

    let (target_fps, stem_results) = fingerprint_target_and_stems_parallel_with_progress(
        &target_fp_path,
        &scan.audio_assets,
        &fp_cache_dir,
        Some(on_fp_done),
    )?;

    let target_index = build_index(&target_fps);
    progress.emit(
        STAGE_FEATURES,
        "fingerprint_done",
        "Fingerprints ready",
        Some(format!(
            "{} release landmarks · {} stems",
            target_fps.len(),
            stem_results.len()
        )),
        None,
        None,
        None,
        None,
    );

    let mut asset_fps: HashMap<String, Vec<Fingerprint>> = HashMap::new();
    let mut asset_durations: HashMap<String, f64> = HashMap::new();
    for stem in stem_results {
        asset_durations.insert(stem.name.clone(), stem.duration_seconds);
        asset_fps.insert(stem.name, stem.fingerprints);
    }

    progress.emit(
        STAGE_MATCH,
        "start",
        "Matching stems to release and scanning timeline",
        Some(format!(
            "{asset_total} stems · {:.0}s song · {} windows",
            target_duration, plan.window_count
        )),
        None,
        Some(asset_total),
        None,
        None,
    );

    let asset_indexes = build_asset_indexes(&scan.audio_assets, &asset_fps);
    let coverage_opts = coverage::CoverageOptions::from_config(
        config.coverage.window_seconds,
        config.coverage.hop_seconds,
        config.coverage.window_score_min,
        &config.match_config,
    );
    let match_config = config.match_config.clone();
    let progress_match = progress.clone();
    let progress_cov = progress.clone();
    let assets_for_match = scan.audio_assets.clone();

    let on_match_done: Arc<dyn Fn(MatchItemDone) + Send + Sync> = Arc::new(move |item| {
        progress_match.tick(
            STAGE_MATCH,
            "match_stem",
            "Stem matched to release",
            U_MATCH,
            Some(format!("score {:.3}", item.match_score)),
            Some(item.item_index),
            Some(item.item_total),
            Some(item.name),
            None,
        );
    });

    let (matches, timeline_raw) = rayon::join(
        move || {
            let mut results = match_all_assets_with_progress(
                &assets_for_match,
                &target_index,
                target_duration,
                &asset_fps,
                &asset_durations,
                &match_config,
                Some(on_match_done),
            );
            results.sort_by(|a, b| {
                b.match_score
                    .partial_cmp(&a.match_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results
        },
        move || {
            compute_timeline_coverage(
                target_duration,
                &target_fps,
                &asset_indexes,
                coverage_opts,
                Some(&progress_cov),
            )
        },
    );

    let timeline = gate_timeline_by_matches(timeline_raw, &matches);

    progress.emit(
        STAGE_MATCH,
        "done",
        "Matching and timeline coverage complete",
        Some(format!(
            "best {:.3} · {:.1}% of song explained",
            matches.first().map(|m| m.match_score).unwrap_or(0.0),
            timeline.coverage_ratio * 100.0
        )),
        None,
        None,
        None,
        None,
    );

    let verdict = compute_verdict_monolithic(
        &matches,
        &timeline,
        config.verdict.pass_coverage_min,
        config.verdict.require_strong_match,
    );

    progress.emit(
        STAGE_METADATA,
        "hash",
        "Computing file hashes",
        None,
        None,
        None,
        None,
        None,
    );

    let audio_for_hash = audio.to_path_buf();
    let assets_for_hash = scan.audio_assets.clone();
    let progress_meta = progress.clone();
    let meta_done = Arc::new(AtomicUsize::new(0));
    let meta_done_b = Arc::clone(&meta_done);
    let progress_meta_b = progress_meta.clone();
    let meta_total = asset_total + 2;
    let scan_for_hash = scan.clone();

    let (asset_hashes, file_hashes) = rayon::join(
        move || -> Result<HashMap<String, String>> {
            assets_for_hash
                .par_iter()
                .map(|p| {
                    let name = p.file_name().unwrap().to_string_lossy().to_string();
                    let hash = sha256_file(p)?;
                    let n = meta_done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress_meta.tick(
                        STAGE_METADATA,
                        "hash_stem",
                        "Stem hash computed",
                        U_META,
                        None,
                        Some(n - 1),
                        Some(meta_total),
                        Some(name.clone()),
                        None,
                    );
                    Ok((name, hash))
                })
                .collect()
        },
        move || -> Result<(String, String)> {
            let target_hash = sha256_file(&audio_for_hash)?;
            let n = meta_done_b.fetch_add(1, Ordering::Relaxed) + 1;
            progress_meta_b.tick(
                STAGE_METADATA,
                "hash_release",
                "Release hash computed",
                U_META,
                None,
                Some(n - 1),
                Some(meta_total),
                None,
                None,
            );
            Ok((target_hash, project_manifest_hash(&scan_for_hash)?))
        },
    );
    let asset_hashes = asset_hashes?;
    let (target_hash, manifest_hash) = file_hashes?;

    progress.tick(
        STAGE_METADATA,
        "evidence",
        "Collecting metadata evidence",
        U_META,
        None,
        None,
        None,
        None,
        None,
    );
    let metadata_evidence = collect_metadata(&scan, audio, &asset_hashes)?;

    progress.tick(
        STAGE_REPORT,
        "build",
        "Building verification report",
        U_REPORT,
        None,
        None,
        None,
        None,
        None,
    );

    let timeline_payload = serde_json::json!({
        "explained_seconds": timeline.explained_seconds,
        "target_seconds": timeline.target_seconds,
        "coverage_ratio": timeline.coverage_ratio,
        "window_seconds": timeline.window_seconds,
        "hop_seconds": timeline.hop_seconds,
        "threshold": timeline.threshold,
        "explained_windows": timeline.explained_windows,
        "total_windows": timeline.total_windows,
    });

    let payload = build_report_payload(
        &scan,
        audio,
        &matches,
        &verdict,
        timeline_payload,
        metadata_evidence,
        target_hash,
        manifest_hash,
        asset_hashes,
        &config,
    )?;

    progress.tick(
        STAGE_REPORT,
        "json",
        "Writing report.json",
        U_REPORT,
        None,
        None,
        None,
        None,
        None,
    );
    write_json_report(&payload, out)?;

    progress.tick(
        STAGE_REPORT,
        "html",
        "Writing report.html",
        U_REPORT,
        None,
        None,
        None,
        None,
        None,
    );
    write_html_report(&payload, out)?;

    progress.complete_all();
    progress.emit(
        STAGE_DONE,
        "complete",
        "Verification complete",
        Some(verdict.verdict.clone()),
        None,
        None,
        None,
        None,
    );

    Ok(payload)
}

pub fn scan_only(project: &Path) -> Result<serde_json::Value> {
    let scan = scan_project(project)?;
    Ok(scan_summary(&scan))
}
