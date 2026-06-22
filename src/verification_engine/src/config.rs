use serde::{Deserialize, Serialize};

use crate::coverage::CoverageOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchConfig {
    /// Landmark vote fraction above this → strong_match
    pub strong_min: f64,
    /// Landmark vote fraction above this → possible_match
    pub possible_min: f64,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            strong_min: 0.40,
            possible_min: 0.30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageConfig {
    pub window_seconds: f64,
    pub hop_seconds: f64,
    /// Minimum best landmark score for a window to count as explainable
    pub window_score_min: f64,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        Self {
            window_seconds: 2.0,
            hop_seconds: 0.5,
            window_score_min: 0.30,
        }
    }
}

impl CoverageConfig {
    pub fn window_threshold(&self, match_config: &MatchConfig) -> f64 {
        self.window_score_min.max(match_config.possible_min)
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
pub struct VerdictConfig {
    /// Minimum timeline coverage ratio for PASS
    pub pass_coverage_min: f64,
    /// Require at least one strong stem match
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
    pub verdict: VerdictConfig,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            match_config: MatchConfig::default(),
            coverage: CoverageConfig::default(),
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
