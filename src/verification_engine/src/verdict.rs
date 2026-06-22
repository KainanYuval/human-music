use crate::config::MatchConfig;
use crate::matcher::MatchResult;

use crate::coverage::CoverageResult;

#[derive(Debug, Clone)]
pub struct VerdictResult {
    pub verdict: String,
    pub provenance_score: f64,
    pub matched_coverage_seconds: f64,
    pub target_duration_seconds: f64,
    pub coverage_ratio: f64,
    pub has_strong_match: bool,
    pub strong_match_count: usize,
    pub possible_match_count: usize,
    pub best_match: Option<MatchResult>,
}

pub fn match_status(score: f64, config: &MatchConfig) -> String {
    if score >= config.strong_min {
        "strong_match".to_string()
    } else if score >= config.possible_min {
        "possible_match".to_string()
    } else {
        "no_match".to_string()
    }
}

pub fn compute_verdict_monolithic(
    matches: &[MatchResult],
    timeline: &CoverageResult,
    pass_coverage_min: f64,
    require_strong_match: bool,
) -> VerdictResult {
    let strong_count = matches.iter().filter(|m| m.status == "strong_match").count();
    let possible_count = matches
        .iter()
        .filter(|m| m.status == "possible_match")
        .count();
    let best = matches.first().cloned();
    let best_score = best.as_ref().map(|m| m.match_score).unwrap_or(0.0);
    let coverage_ratio = timeline.coverage_ratio;
    let has_strong = strong_count > 0;
    let has_possible = possible_count > 0;

    let verdict = if coverage_ratio >= pass_coverage_min
        && (!require_strong_match || has_strong)
        && (has_strong || (has_possible && best_score >= 0.75))
    {
        "PASS".to_string()
    } else {
        "FAIL".to_string()
    };

    VerdictResult {
        verdict,
        provenance_score: coverage_ratio,
        matched_coverage_seconds: timeline.explained_seconds,
        target_duration_seconds: timeline.target_seconds,
        coverage_ratio,
        has_strong_match: has_strong,
        strong_match_count: strong_count,
        possible_match_count: possible_count,
        best_match: best,
    }
}

pub use crate::config::MatchConfig as MatchThresholds;
