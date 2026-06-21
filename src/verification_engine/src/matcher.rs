use std::collections::HashMap;
use std::path::PathBuf;

use crate::chroma::{best_chroma_offset_from_matrices, ChromaMatrix, MatchMode};
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

fn status(score: f64, config: &MatchConfig) -> String {
    match_status(score, config)
}

pub fn match_asset_from_chroma(
    asset_name: &str,
    ref_c: &ChromaMatrix,
    tgt_c: &ChromaMatrix,
    sr: u32,
    ref_duration_seconds: f64,
    target_duration_seconds: f64,
    match_config: &MatchConfig,
) -> MatchResult {
    let (chroma_score, chroma_offset, mode) = best_chroma_offset_from_matrices(ref_c, tgt_c, sr);
    let coverage = match mode {
        MatchMode::RefInTarget => ref_duration_seconds,
        MatchMode::TargetInRef => target_duration_seconds,
    };
    MatchResult {
        asset: asset_name.to_string(),
        pearson: 0.0,
        spectral_pearson: 0.0,
        chroma_pearson: chroma_score,
        match_score: chroma_score,
        mse: 0.0,
        gain: 1.0,
        offset_seconds: chroma_offset,
        coverage_seconds: coverage,
        asset_duration_seconds: ref_duration_seconds,
        status: status(chroma_score, match_config),
        match_mode: mode.as_str().to_string(),
    }
}

pub fn match_all_assets(
    asset_norm: &HashMap<PathBuf, PathBuf>,
    target_chroma: &ChromaMatrix,
    sr: u32,
    target_duration: f64,
    asset_chromas: &HashMap<String, ChromaMatrix>,
    asset_durations: &HashMap<String, f64>,
    match_config: &MatchConfig,
) -> Vec<MatchResult> {
    let mut results = Vec::new();
    for source in asset_norm.keys() {
        let name = source.file_name().unwrap().to_string_lossy().to_string();
        let ref_c = &asset_chromas[&name];
        let ref_duration = asset_durations[&name];
        results.push(match_asset_from_chroma(
            &name,
            ref_c,
            target_chroma,
            sr,
            ref_duration,
            target_duration,
            match_config,
        ));
    }
    results
}
