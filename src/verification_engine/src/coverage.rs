use rayon::prelude::*;
use shazam_engine::{match_landmarks, Fingerprint, FingerprintIndex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::config::MatchConfig;
use crate::matcher::MatchResult;
use crate::progress::{ProgressHandle, U_COV, STAGE_COVERAGE};

#[derive(Debug, Clone)]
pub struct CoverageResult {
    pub explained_seconds: f64,
    pub target_seconds: f64,
    pub coverage_ratio: f64,
    pub window_seconds: f64,
    pub hop_seconds: f64,
    pub threshold: f64,
    pub explained_windows: usize,
    pub total_windows: usize,
}

#[derive(Debug, Clone)]
pub struct CoverageOptions {
    pub window_seconds: f64,
    pub hop_seconds: f64,
    pub threshold: f64,
}

impl Default for CoverageOptions {
    fn default() -> Self {
        Self {
            window_seconds: 2.0,
            hop_seconds: 0.5,
            threshold: 0.30,
        }
    }
}

impl CoverageOptions {
    pub fn from_config(
        window_seconds: f64,
        hop_seconds: f64,
        window_score_min: f64,
        match_config: &MatchConfig,
    ) -> Self {
        Self {
            window_seconds,
            hop_seconds,
            threshold: window_score_min.max(match_config.possible_min),
        }
    }
}

#[derive(Clone, Copy)]
struct WindowOutcome {
    start: f64,
    end: f64,
    explained: bool,
}

fn window_query(target_fps: &[Fingerprint], start_s: f64, end_s: f64) -> Vec<Fingerprint> {
    let start = start_s as f32;
    let end = end_s as f32;
    target_fps
        .iter()
        .filter(|fp| fp.time_s >= start && fp.time_s < end)
        .cloned()
        .collect()
}

fn score_window(
    target_fps: &[Fingerprint],
    asset_indexes: &[FingerprintIndex],
    start: f64,
    end: f64,
    threshold: f64,
) -> WindowOutcome {
    let query = window_query(target_fps, start, end);
    let best = if query.is_empty() {
        0.0
    } else {
        asset_indexes
            .par_iter()
            .map(|index| match_landmarks(&query, index).score as f64)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    };
    WindowOutcome {
        start,
        end,
        explained: best >= threshold,
    }
}

pub fn compute_timeline_coverage(
    target_seconds: f64,
    target_fps: &[Fingerprint],
    asset_indexes: &[FingerprintIndex],
    opts: CoverageOptions,
    progress: Option<&ProgressHandle>,
) -> CoverageResult {
    let hop = opts.hop_seconds.max(0.1);
    let win = opts.window_seconds.max(0.5);
    let bin_count = ((target_seconds / hop).ceil() as usize).max(1);
    let window_count = if target_seconds >= win {
        ((target_seconds - win) / hop).floor() as usize + 1
    } else {
        0
    };

    if let Some(p) = progress {
        p.emit(
            STAGE_COVERAGE,
            "start",
            "Scanning timeline windows against project recordings",
            Some(format!(
                "{window_count} windows × {} assets (parallel)",
                asset_indexes.len()
            )),
            None,
            Some(window_count),
            None,
            None,
        );
    }

    let mut explained_bins = vec![false; bin_count];
    let window_starts: Vec<f64> = (0..window_count).map(|i| i as f64 * hop).collect();
    let done = Arc::new(AtomicUsize::new(0));
    let progress_for_windows = progress.cloned();

    let outcomes: Vec<WindowOutcome> = window_starts
        .par_iter()
        .map(|&start| {
            let outcome = score_window(
                target_fps,
                asset_indexes,
                start,
                start + win,
                opts.threshold,
            );
            if let Some(p) = progress_for_windows.as_ref() {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if window_count > 0 {
                    p.tick(
                        STAGE_COVERAGE,
                        "windows",
                        "Timeline window",
                        U_COV,
                        Some(format!("{n}/{window_count}")),
                        Some(n - 1),
                        Some(window_count),
                        None,
                        None,
                    );
                }
            }
            outcome
        })
        .collect();

    let total_windows = outcomes.len();
    let mut explained_windows = 0usize;
    for outcome in &outcomes {
        if outcome.explained {
            explained_windows += 1;
            let start_bin = (outcome.start / hop).floor() as usize;
            let end_bin = (outcome.end / hop).ceil() as usize;
            for bin in explained_bins
                .iter_mut()
                .take(end_bin.min(bin_count))
                .skip(start_bin.min(bin_count))
            {
                *bin = true;
            }
        }
    }

    let explained_bins_count = explained_bins.iter().filter(|&&v| v).count();
    let ratio = if bin_count == 0 {
        0.0
    } else {
        (explained_bins_count as f64 / bin_count as f64).min(1.0)
    };

    if let Some(p) = progress {
        p.emit(
            STAGE_COVERAGE,
            "done",
            "Timeline coverage complete",
            Some(format!(
                "{explained_windows}/{total_windows} windows · {:.1}% of song",
                ratio * 100.0
            )),
            None,
            None,
            None,
            None,
        );
    }

    CoverageResult {
        explained_seconds: round3(explained_bins_count as f64 * hop),
        target_seconds: round3(target_seconds),
        coverage_ratio: (ratio * 10000.0).round() / 10000.0,
        window_seconds: win,
        hop_seconds: hop,
        threshold: opts.threshold,
        explained_windows,
        total_windows,
    }
}

/// Union-of-intervals coverage from pass-1 stem offsets (stem → target alignment).
pub fn compute_interval_coverage_from_matches(
    target_seconds: f64,
    matches: &[MatchResult],
    score_threshold: f64,
) -> CoverageResult {
    let mut intervals: Vec<(f64, f64)> = matches
        .iter()
        .filter(|m| m.match_score >= score_threshold)
        .filter_map(|m| {
            let start = m.offset_seconds.max(0.0);
            let end = (m.offset_seconds + m.asset_duration_seconds).min(target_seconds);
            if end > start + 1e-6 {
                Some((start, end))
            } else {
                None
            }
        })
        .collect();

    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in intervals {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 + 1e-6 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    let explained_seconds: f64 = merged.iter().map(|(s, e)| e - s).sum();
    let ratio = if target_seconds <= 0.0 {
        0.0
    } else {
        (explained_seconds / target_seconds).min(1.0)
    };

    CoverageResult {
        explained_seconds: round3(explained_seconds),
        target_seconds: round3(target_seconds),
        coverage_ratio: (ratio * 10000.0).round() / 10000.0,
        window_seconds: 0.0,
        hop_seconds: 0.0,
        threshold: score_threshold,
        explained_windows: merged.len(),
        total_windows: matches.len(),
    }
}

/// If no stem reaches possible/strong match, timeline coverage must be zero.
pub fn gate_timeline_by_matches(
    timeline: CoverageResult,
    matches: &[MatchResult],
) -> CoverageResult {
    let has_qualifying = matches
        .iter()
        .any(|m| m.status == "strong_match" || m.status == "possible_match");
    if has_qualifying {
        return timeline;
    }
    CoverageResult {
        explained_seconds: 0.0,
        coverage_ratio: 0.0,
        explained_windows: 0,
        ..timeline
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_zeros_when_no_matches() {
        let timeline = CoverageResult {
            explained_seconds: 10.0,
            target_seconds: 20.0,
            coverage_ratio: 0.5,
            window_seconds: 2.0,
            hop_seconds: 0.5,
            threshold: 0.3,
            explained_windows: 5,
            total_windows: 10,
        };
        let gated = gate_timeline_by_matches(timeline, &[]);
        assert_eq!(gated.coverage_ratio, 0.0);
    }
}
