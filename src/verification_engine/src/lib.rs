pub mod audio;
pub mod catalog;
pub mod chroma;
pub mod config;
pub mod coverage;
pub mod discrimination;
pub mod matcher;
pub mod normalize;
pub mod progress;
pub mod project_features;
pub mod report;
pub mod verdict;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use coverage::compute_timeline_coverage;
use matcher::match_all_assets;
use garageband::metadata::{collect_metadata, project_manifest_hash, sha256_file};
use normalize::{media_duration_seconds, normalize_to_wav_with_progress, TARGET_SR};
use progress::{
    ProgressEmitter, ProgressEvent, WorkPlan, STAGE_DONE, STAGE_FEATURES, STAGE_MATCH,
    STAGE_METADATA, STAGE_NORMALIZE, STAGE_REPORT, STAGE_SCAN,
};
use report::{build_report_payload, write_html_report, write_json_report};
use garageband::{scan_project, scan_summary};
use catalog::discover_band_projects;
use config::VerifyConfig;
use discrimination::{compute_project_competition, passes_discrimination};
use project_features::load_project_chromas;
use verdict::{compute_verdict_discriminated, compute_verdict_monolithic};

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub config: VerifyConfig,
    /// Directory to scan for rival `.band` projects (e.g. `data/`)
    pub catalog_dir: Option<PathBuf>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            config: VerifyConfig::default(),
            catalog_dir: None,
        }
    }
}

pub fn run_verify(
    project: &Path,
    audio: &Path,
    out: &Path,
    on_progress: Option<&mut dyn FnMut(ProgressEvent)>,
) -> Result<serde_json::Value> {
    run_verify_with_options(
        project,
        audio,
        out,
        VerifyOptions::default(),
        on_progress,
    )
}

pub fn run_verify_with_options(
    project: &Path,
    audio: &Path,
    out: &Path,
    options: VerifyOptions,
    on_progress: Option<&mut dyn FnMut(ProgressEvent)>,
) -> Result<serde_json::Value> {
    let mut progress = ProgressEmitter::new(on_progress);
    run_verify_inner(project, audio, out, &options, &mut progress)
}

fn run_verify_inner(
    project: &Path,
    audio: &Path,
    out: &Path,
    options: &VerifyOptions,
    progress: &mut ProgressEmitter<'_>,
) -> Result<serde_json::Value> {
    let config = &options.config;
    progress.emit(
        STAGE_SCAN,
        "start",
        "Opening GarageBand project bundle",
        0.0,
        Some(project.display().to_string()),
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

    progress.emit(
        STAGE_SCAN,
        "assets",
        "Found embedded recordings",
        1.0,
        Some(format!(
            "{} files · {} registered in MetaData.plist",
            scan.audio_assets.len(),
            scan.registered_assets.len()
        )),
        None,
        Some(scan.audio_assets.len()),
        None,
    );

    progress.emit(
        STAGE_SCAN,
        "probe",
        "Measuring audio durations for time estimate",
        0.0,
        None,
        None,
        None,
        None,
    );
    let probed_target = media_duration_seconds(audio).unwrap_or(180.0);
    let probed_assets: Vec<f64> = scan
        .audio_assets
        .iter()
        .map(|p| media_duration_seconds(p).unwrap_or(30.0))
        .collect();
    let plan = WorkPlan::estimate(probed_target, &probed_assets);
    let plan_detail = plan.summary_detail();
    progress.set_plan(plan);
    progress.mark_scan_done();

    progress.emit(
        STAGE_SCAN,
        "done",
        "Project scan complete",
        1.0,
        Some(plan_detail),
        None,
        None,
        scan.garageband_version.clone(),
    );

    let plan = progress.plan().expect("work plan set after scan").clone();

    std::fs::create_dir_all(out)?;
    let temp_dir = out.join("normalized_temp");
    std::fs::create_dir_all(&temp_dir)?;

    let asset_total = scan.audio_assets.len();
    let audio_name = audio.file_name().map(|n| n.to_string_lossy().to_string());

    progress.begin_step(plan.norm_target_units);
    progress.emit(
        STAGE_NORMALIZE,
        "target_start",
        "Converting released audio to mono 44.1 kHz",
        0.0,
        audio_name.clone(),
        None,
        None,
        audio_name.clone(),
    );
    let target_norm_path = temp_dir.join("target.wav");
    {
        let mut ffmpeg_progress = |frac: f64| {
            progress.emit_step(
                STAGE_NORMALIZE,
                "target_ffmpeg",
                "Transcoding released audio",
                frac,
                None,
                None,
                None,
                audio_name.clone(),
            );
        };
        normalize_to_wav_with_progress(audio, &target_norm_path, Some(&mut ffmpeg_progress))?;
    }
    progress.finish_step();
    progress.emit(
        STAGE_NORMALIZE,
        "target_done",
        "Released audio normalized",
        1.0,
        Some(target_norm_path.display().to_string()),
        None,
        None,
        audio_name.clone(),
    );

    let mut asset_norm: HashMap<PathBuf, PathBuf> = HashMap::new();
    for (idx, source) in scan.audio_assets.iter().enumerate() {
        let name = source.file_name().unwrap().to_string_lossy().to_string();

        progress.begin_step(plan.norm_asset_units[idx]);
        progress.emit(
            STAGE_NORMALIZE,
            "asset_start",
            "Normalizing project recording",
            0.0,
            Some(source.display().to_string()),
            Some(idx),
            Some(asset_total),
            Some(name.clone()),
        );

        let safe: String = source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let dest = temp_dir.join(format!("asset_{idx:03}_{safe}.wav"));
        {
            let mut ffmpeg_progress = |frac: f64| {
                progress.emit_step(
                    STAGE_NORMALIZE,
                    "asset_ffmpeg",
                    "Transcoding project recording",
                    frac,
                    None,
                    Some(idx),
                    Some(asset_total),
                    Some(name.clone()),
                );
            };
            normalize_to_wav_with_progress(source, &dest, Some(&mut ffmpeg_progress))?;
        }
        asset_norm.insert(source.clone(), dest);
        progress.finish_step();

        progress.emit(
            STAGE_NORMALIZE,
            "asset_done",
            "Recording normalized",
            1.0,
            None,
            Some(idx),
            Some(asset_total),
            Some(name),
        );
    }

    progress.begin_step(plan.load_target_units);
    progress.emit(
        STAGE_FEATURES,
        "target_load",
        "Loading normalized released audio",
        0.0,
        None,
        None,
        None,
        None,
    );
    let (target_audio, sr) = audio::load_mono_float(&target_norm_path)?;
    progress.finish_step();
    if sr != TARGET_SR {
        anyhow::bail!("unexpected sample rate after normalization: {sr}");
    }
    let target_duration = target_audio.len() as f64 / sr as f64;

    progress.begin_step(plan.chroma_target_units);
    progress.emit(
        STAGE_FEATURES,
        "target_chroma",
        "Computing pitch fingerprint for released audio",
        0.0,
        Some(format!("{target_duration:.1}s · {sr} Hz")),
        None,
        None,
        audio_name.clone(),
    );
    let target_chroma = {
        let mut chroma_progress = |frac: f64| {
            progress.emit_step(
                STAGE_FEATURES,
                "target_chroma",
                "Computing pitch fingerprint for released audio",
                frac,
                Some(format!("{:.0}% of STFT frames", frac * 100.0)),
                None,
                None,
                audio_name.clone(),
            );
        };
        chroma::chroma_matrix_with_progress(&target_audio, sr, Some(&mut chroma_progress))
    };
    progress.finish_step();
    progress.emit(
        STAGE_FEATURES,
        "target_chroma_done",
        "Released audio fingerprint ready",
        1.0,
        Some(format!("{} chroma frames", target_chroma.frames)),
        None,
        None,
        None,
    );

    let mut asset_chromas: HashMap<String, chroma::ChromaMatrix> = HashMap::new();
    let mut asset_durations: HashMap<String, f64> = HashMap::new();
    for (idx, (source, normalized)) in asset_norm.iter().enumerate() {
        let name = source.file_name().unwrap().to_string_lossy().to_string();

        progress.begin_step(plan.load_asset_units[idx]);
        progress.emit(
            STAGE_FEATURES,
            "asset_load",
            "Loading project recording",
            0.0,
            None,
            Some(idx),
            Some(asset_total),
            Some(name.clone()),
        );
        let (samples, _) = audio::load_mono_float(normalized)?;
        progress.finish_step();
        let duration = samples.len() as f64 / sr as f64;

        progress.begin_step(plan.chroma_asset_units[idx]);
        progress.emit(
            STAGE_FEATURES,
            "asset_chroma",
            "Computing pitch fingerprint",
            0.0,
            Some(format!("{duration:.1}s")),
            Some(idx),
            Some(asset_total),
            Some(name.clone()),
        );
        let matrix = {
            let mut chroma_progress = |frac: f64| {
                progress.emit_step(
                    STAGE_FEATURES,
                    "asset_chroma",
                    "Computing pitch fingerprint",
                    frac,
                    Some(format!("{:.0}% of STFT frames", frac * 100.0)),
                    Some(idx),
                    Some(asset_total),
                    Some(name.clone()),
                );
            };
            chroma::chroma_matrix_with_progress(&samples, sr, Some(&mut chroma_progress))
        };
        progress.finish_step();
        asset_chromas.insert(name.clone(), matrix);
        asset_durations.insert(name.clone(), duration);

        progress.emit(
            STAGE_FEATURES,
            "asset_chroma_done",
            "Fingerprint ready",
            1.0,
            None,
            Some(idx),
            Some(asset_total),
            Some(name),
        );
    }

    progress.begin_step(plan.match_units);
    progress.emit(
        STAGE_MATCH,
        "start",
        "Matching project recordings against released audio",
        0.0,
        Some(format!("{asset_total} assets")),
        None,
        Some(asset_total),
        None,
    );

    let mut matches = match_all_assets(
        &asset_norm,
        &target_chroma,
        sr,
        target_duration,
        &asset_chromas,
        &asset_durations,
        &config.match_config,
    );
    matches.sort_by(|a, b| {
        b.match_score
            .partial_cmp(&a.match_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (idx, m) in matches.iter().enumerate() {
        let step_frac = (idx + 1) as f64 / matches.len().max(1) as f64;
        progress.emit_step(
            STAGE_MATCH,
            "asset_result",
            "Asset match scored",
            step_frac,
            Some(format!(
                "score {:.3} · {} · offset {:.2}s",
                m.match_score, m.status, m.offset_seconds
            )),
            Some(idx),
            Some(matches.len()),
            Some(m.asset.clone()),
        );
    }
    progress.finish_step();

    let asset_chroma_list: Vec<chroma::ChromaMatrix> = scan
        .audio_assets
        .iter()
        .map(|p| {
            asset_chromas
                .get(&p.file_name().unwrap().to_string_lossy().to_string())
                .cloned()
                .unwrap()
        })
        .collect();

    let coverage_opts = coverage::CoverageOptions::from(&config.coverage);

    let timeline = compute_timeline_coverage(
        target_duration,
        &target_chroma,
        &asset_chroma_list,
        sr,
        coverage_opts.clone(),
        progress,
    );

    let catalog_root = options.catalog_dir.clone().or_else(|| {
        project
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
    });

    let verdict = if config.discrimination.enabled {
        let catalog_root = catalog_root.context(
            "discrimination requires --catalog-dir (folder containing rival .band projects)",
        )?;
        let rivals = discover_band_projects(&catalog_root, project);
        if rivals.is_empty() {
            anyhow::bail!(
                "discrimination enabled but no rival .band projects found under {}",
                catalog_root.display()
            );
        }

        let catalog_temp = temp_dir.join("catalog_chromas");
        std::fs::create_dir_all(&catalog_temp)?;

        let mut catalog_projects = Vec::new();
        for (idx, rival_path) in rivals.iter().enumerate() {
            let label = format!("rival_{idx:02}");
            catalog_projects.push(load_project_chromas(rival_path, &catalog_temp, &label)?);
        }

        let claimed_name = scan.project_name.clone();
        let mut all_projects: Vec<(String, Vec<chroma::ChromaMatrix>)> = Vec::new();
        all_projects.push((claimed_name.clone(), asset_chroma_list.clone()));
        for cp in &catalog_projects {
            all_projects.push((cp.name.clone(), cp.asset_chromas.clone()));
        }

        let claimed_index = 0usize;
        let project_refs: Vec<(&str, &[chroma::ChromaMatrix])> = all_projects
            .iter()
            .map(|(name, chromas)| (name.as_str(), chromas.as_slice()))
            .collect();

        let (competitive, rival_fits) = compute_project_competition(
            &target_chroma,
            &project_refs,
            claimed_index,
            sr,
            config.coverage.window_seconds,
            config.coverage.hop_seconds,
            config.coverage.window_score_min,
            config.discrimination.competitive_margin,
        );

        let discrimination_pass = passes_discrimination(
            &competitive,
            &rival_fits,
            &config.discrimination,
        );

        compute_verdict_discriminated(
            &matches,
            &timeline,
            &competitive,
            rival_fits,
            discrimination_pass,
            config.verdict.pass_coverage_min,
            config.verdict.require_strong_match,
        )
    } else {
        compute_verdict_monolithic(
            &matches,
            &timeline,
            config.verdict.pass_coverage_min,
            config.verdict.require_strong_match,
        )
    };

    progress.begin_step(plan.metadata_units);
    progress.emit(
        STAGE_METADATA,
        "hash",
        "Computing file hashes",
        0.0,
        None,
        None,
        None,
        None,
    );
    let asset_hashes: HashMap<String, String> = scan
        .audio_assets
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            progress.emit_step(
                STAGE_METADATA,
                "hash_asset",
                "Hashing project recording",
                (idx + 1) as f64 / asset_total.max(1) as f64,
                None,
                Some(idx),
                Some(asset_total),
                Some(name.clone()),
            );
            Ok((name, sha256_file(p)?))
        })
        .collect::<Result<_>>()?;

    progress.emit(
        STAGE_METADATA,
        "evidence",
        "Collecting metadata evidence",
        0.85,
        None,
        None,
        None,
        None,
    );
    let metadata_evidence = collect_metadata(&scan, audio, &asset_hashes)?;
    progress.finish_step();

    progress.begin_step(plan.report_units);
    progress.emit(
        STAGE_REPORT,
        "build",
        "Building verification report",
        0.0,
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
        "competitive_win_rate": verdict.competitive_win_rate,
        "exclusive_advantage": verdict.exclusive_advantage,
        "discrimination_pass": verdict.discrimination_pass,
        "rival_scores": verdict.rival_scores.iter().map(|r| serde_json::json!({
            "project": r.project_name,
            "win_rate": r.win_rate,
            "exclusive_advantage": r.exclusive_advantage,
        })).collect::<Vec<_>>(),
    });

    let payload = build_report_payload(
        &scan,
        audio,
        &matches,
        &verdict,
        timeline_payload,
        metadata_evidence,
        sha256_file(audio)?,
        project_manifest_hash(&scan)?,
        asset_hashes,
        &config,
    )?;

    progress.emit_step(
        STAGE_REPORT,
        "json",
        "Writing report.json",
        0.55,
        None,
        None,
        None,
        None,
    );
    write_json_report(&payload, out)?;

    progress.emit_step(
        STAGE_REPORT,
        "html",
        "Writing report.html",
        1.0,
        None,
        None,
        None,
        None,
    );
    write_html_report(&payload, out)?;
    progress.finish_step();

    progress.complete_all();
    progress.emit(
        STAGE_DONE,
        "complete",
        "Verification complete",
        1.0,
        Some(verdict.verdict.clone()),
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
