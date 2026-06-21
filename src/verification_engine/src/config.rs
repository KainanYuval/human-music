use serde::{Deserialize, Serialize};

use crate::coverage::CoverageOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchConfig {
    /// Chroma score above this → strong_match
    pub strong_min: f64,
    /// Chroma score above this → possible_match
    pub possible_min: f64,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            strong_min: 0.85,
            possible_min: 0.70,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageConfig {
    pub window_seconds: f64,
    pub hop_seconds: f64,
    /// Minimum best chroma score for a window to count as explainable (monolithic mode)
    pub window_score_min: f64,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        Self {
            window_seconds: 2.0,
            hop_seconds: 0.5,
            window_score_min: 0.82,
        }
    }
}

impl From<&CoverageConfig> for CoverageOptions {
    fn from(c: &CoverageConfig) -> Self {
        Self {
            window_seconds: c.window_seconds,
            hop_seconds: c.hop_seconds,
            threshold: c.window_score_min,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscriminationConfig {
    /// When true, require beating pooled competitor stems from catalog
    pub enabled: bool,
    /// Claimed stem must exceed best competitor stem by this much per window
    pub competitive_margin: f64,
    /// Fraction of windows claimed must win competitively
    pub pass_win_rate: f64,
    /// Mean per-window score advantage (claimed − best competitor)
    pub pass_exclusive_advantage: f64,
    /// Claimed project advantage must exceed every rival project's advantage
    pub require_beat_all_competitors: bool,
    /// Minimum rival-gap when require_beat_all_competitors is true
    pub rival_advantage_margin: f64,
}

impl Default for DiscriminationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            competitive_margin: 0.015,
            pass_win_rate: 0.55,
            pass_exclusive_advantage: 0.04,
            require_beat_all_competitors: true,
            rival_advantage_margin: 0.02,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictConfig {
    /// Legacy monolithic mode: timeline coverage ratio threshold
    pub pass_coverage_min: f64,
    /// Require at least one strong match (monolithic / hybrid)
    pub require_strong_match: bool,
}

impl Default for VerdictConfig {
    fn default() -> Self {
        Self {
            pass_coverage_min: 0.70,
            require_strong_match: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyConfig {
    #[serde(default)]
    pub match_config: MatchConfig,
    #[serde(default)]
    pub coverage: CoverageConfig,
    #[serde(default)]
    pub discrimination: DiscriminationConfig,
    #[serde(default)]
    pub verdict: VerdictConfig,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            match_config: MatchConfig::default(),
            coverage: CoverageConfig::default(),
            discrimination: DiscriminationConfig::default(),
            verdict: VerdictConfig::default(),
        }
    }
}

impl VerifyConfig {
    pub fn from_toml_path(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

pub fn default_config_toml() -> &'static str {
    include_str!("../config/default.toml")
}
