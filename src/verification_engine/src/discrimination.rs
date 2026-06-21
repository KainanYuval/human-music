use crate::chroma::{best_chroma_offset_frames, ChromaMatrix, CHROMA_HOP};
use crate::config::DiscriminationConfig;
use crate::coverage::window_matrix;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectFitScore {
    pub project_name: String,
    pub win_rate: f64,
    pub exclusive_advantage: f64,
}

#[derive(Debug, Clone)]
pub struct CompetitiveCoverageResult {
    pub win_rate: f64,
    pub exclusive_advantage: f64,
    pub windows_won: usize,
    pub windows_total: usize,
    pub raw_coverage_ratio: f64,
}

/// Per-window: compare best stem score from each project pool (not pooled rivals).
pub fn compute_project_competition(
    target_chroma: &ChromaMatrix,
    project_pools: &[(&str, &[ChromaMatrix])],
    claimed_index: usize,
    sr: u32,
    window_seconds: f64,
    hop_seconds: f64,
    window_score_min: f64,
    margin: f64,
) -> (CompetitiveCoverageResult, Vec<ProjectFitScore>) {
    let hop = CHROMA_HOP;
    let mut win_frames = ((window_seconds * sr as f64) / hop as f64).round() as usize;
    win_frames = win_frames.max(8);
    let mut hop_frames = ((hop_seconds * sr as f64) / hop as f64).round() as usize;
    hop_frames = hop_frames.max(1);

    let total_frames = target_chroma.frames;
    let win_frames = win_frames.min(total_frames.max(1));

    let n_projects = project_pools.len();
    let mut per_project_wins = vec![0usize; n_projects];
    let mut per_project_advantage = vec![0.0f64; n_projects];
    let mut total_windows = 0usize;
    let mut claimed_raw_explained = 0usize;

    if total_frames >= win_frames {
        let mut start = 0usize;
        while start + win_frames <= total_frames {
            total_windows += 1;
            let query = window_matrix(target_chroma, start, start + win_frames);

            let scores: Vec<f64> = project_pools
                .iter()
                .map(|(_, assets)| best_pool_score(&query, assets))
                .collect();

            if scores[claimed_index] >= window_score_min {
                claimed_raw_explained += 1;
            }

            for (idx, &score) in scores.iter().enumerate() {
                let best_other = scores
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != idx)
                    .map(|(_, s)| *s)
                    .fold(0.0f64, f64::max);
                per_project_advantage[idx] += score - best_other;
                if score >= window_score_min && score >= best_other + margin {
                    per_project_wins[idx] += 1;
                }
            }

            start += hop_frames;
        }
    }

    let win_rate = if total_windows == 0 {
        0.0
    } else {
        per_project_wins[claimed_index] as f64 / total_windows as f64
    };

    let exclusive_advantage = if total_windows == 0 {
        0.0
    } else {
        per_project_advantage[claimed_index] / total_windows as f64
    };

    let raw_ratio = if total_windows == 0 {
        0.0
    } else {
        claimed_raw_explained as f64 / total_windows as f64
    };

    let rival_scores: Vec<ProjectFitScore> = project_pools
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx != claimed_index)
        .map(|(idx, (name, _))| ProjectFitScore {
            project_name: (*name).to_string(),
            win_rate: if total_windows == 0 {
                0.0
            } else {
                per_project_wins[idx] as f64 / total_windows as f64
            },
            exclusive_advantage: if total_windows == 0 {
                0.0
            } else {
                per_project_advantage[idx] / total_windows as f64
            },
        })
        .collect();

    let competitive = CompetitiveCoverageResult {
        win_rate,
        exclusive_advantage,
        windows_won: per_project_wins[claimed_index],
        windows_total: total_windows,
        raw_coverage_ratio: raw_ratio,
    };

    (competitive, rival_scores)
}

fn best_pool_score(query: &ChromaMatrix, pool: &[ChromaMatrix]) -> f64 {
    pool.iter()
        .map(|asset| best_chroma_offset_frames(query, asset).0)
        .fold(0.0f64, f64::max)
}

pub fn passes_discrimination(
    claimed: &CompetitiveCoverageResult,
    rival_fits: &[ProjectFitScore],
    config: &DiscriminationConfig,
) -> bool {
    if claimed.win_rate < config.pass_win_rate {
        return false;
    }
    if claimed.exclusive_advantage < config.pass_exclusive_advantage {
        return false;
    }
    if config.require_beat_all_competitors {
        for rival in rival_fits {
            if claimed.exclusive_advantage
                < rival.exclusive_advantage + config.rival_advantage_margin
            {
                return false;
            }
            if claimed.win_rate < rival.win_rate + 0.05 {
                return false;
            }
        }
    }
    true
}
