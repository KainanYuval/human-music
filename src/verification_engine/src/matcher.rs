use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use shazam_engine::{build_index, match_landmarks, Fingerprint, FingerprintIndex};

use crate::config::MatchConfig;
use crate::verdict::match_status;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MatchResult {
    pub asset: String,
    pub pearson: f64,
    pub spectral_pearson: f64,
    pub chroma_pearson: f64,
    pub match_score: f64,
    pub mse: f64,
    pub gain: f64,
    pub offset_seconds: f64,
    pub coverage_seconds: f64,
    pub asset_duration_seconds: f64,
    pub status: String,
    pub match_mode: String,
}

pub struct MatchItemDone {
    pub name: String,
    pub item_index: usize,
    pub item_total: usize,
    pub match_score: f64,
}

fn status(score: f64, config: &MatchConfig) -> String {
    match_status(score, config)
}

pub fn match_asset_fingerprints(
    asset_name: &str,
    stem_fps: &[Fingerprint],
    target_index: &FingerprintIndex,
    ref_duration_seconds: f64,
    target_duration_seconds: f64,
    match_config: &MatchConfig,
) -> MatchResult {
    let m = match_landmarks(stem_fps, target_index);
    let score = m.score as f64;
    let coverage = ref_duration_seconds.min(target_duration_seconds.max(0.0));
    MatchResult {
        asset: asset_name.to_string(),
        pearson: 0.0,
        spectral_pearson: 0.0,
        chroma_pearson: score,
        match_score: score,
        mse: 0.0,
        gain: 1.0,
        offset_seconds: m.offset_s as f64,
        coverage_seconds: coverage,
        asset_duration_seconds: ref_duration_seconds,
        status: status(score, match_config),
        match_mode: "ref_in_target".to_string(),
    }
}

pub fn match_all_assets(
    asset_sources: &[PathBuf],
    target_index: &FingerprintIndex,
    target_duration: f64,
    asset_fps: &HashMap<String, Vec<Fingerprint>>,
    asset_durations: &HashMap<String, f64>,
    match_config: &MatchConfig,
) -> Vec<MatchResult> {
    match_all_assets_with_progress(
        asset_sources,
        target_index,
        target_duration,
        asset_fps,
        asset_durations,
        match_config,
        None,
    )
}

pub fn match_all_assets_with_progress(
    asset_sources: &[PathBuf],
    target_index: &FingerprintIndex,
    target_duration: f64,
    asset_fps: &HashMap<String, Vec<Fingerprint>>,
    asset_durations: &HashMap<String, f64>,
    match_config: &MatchConfig,
    on_item_done: Option<Arc<dyn Fn(MatchItemDone) + Send + Sync>>,
) -> Vec<MatchResult> {
    let total = asset_sources.len();
    let done = Arc::new(AtomicUsize::new(0));

    asset_sources
        .par_iter()
        .map(|source| {
            let name = source.file_name().unwrap().to_string_lossy().to_string();
            let stem = &asset_fps[&name];
            let ref_duration = asset_durations[&name];
            let result = match_asset_fingerprints(
                &name,
                stem,
                target_index,
                ref_duration,
                target_duration,
                match_config,
            );
            if let Some(cb) = on_item_done.as_ref() {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                cb(MatchItemDone {
                    name: name.clone(),
                    item_index: n - 1,
                    item_total: total,
                    match_score: result.match_score,
                });
            }
            result
        })
        .collect()
}

pub fn build_asset_indexes(
    asset_sources: &[PathBuf],
    asset_fps: &HashMap<String, Vec<Fingerprint>>,
) -> Vec<FingerprintIndex> {
    asset_sources
        .par_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            index_from_fingerprints(&asset_fps[&name])
        })
        .collect()
}

pub fn match_stem_to_target(stem: &[Fingerprint], target_index: &FingerprintIndex) -> f64 {
    match_landmarks(stem, target_index).score as f64
}

pub fn index_from_fingerprints(fps: &[Fingerprint]) -> FingerprintIndex {
    build_index(fps)
}
