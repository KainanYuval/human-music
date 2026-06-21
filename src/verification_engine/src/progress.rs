use serde::Serialize;
use std::io::{self, Write};

pub const STAGE_SCAN: &str = "scan";
pub const STAGE_NORMALIZE: &str = "normalize";
pub const STAGE_FEATURES: &str = "features";
pub const STAGE_MATCH: &str = "match";
pub const STAGE_COVERAGE: &str = "coverage";
pub const STAGE_METADATA: &str = "metadata";
pub const STAGE_REPORT: &str = "report";
pub const STAGE_DONE: &str = "done";

// Calibrated so overall percent tracks wall time on typical projects.
// ffmpeg normalize dominates; chroma/match/coverage scale with duration × asset count.
const W_NORM_PER_SEC: f64 = 0.92;
const W_LOAD_PER_SEC: f64 = 0.012;
const W_CHROMA_PER_SEC: f64 = 0.095;
const W_MATCH_PER_ASSET: f64 = 0.10;
const W_MATCH_TARGET_SEC: f64 = 0.00035;
const W_COV_WINDOW_ASSET: f64 = 0.00032;
const W_SCAN: f64 = 0.35;
const W_METADATA_BASE: f64 = 0.20;
const W_METADATA_PER_FILE: f64 = 0.006;
const W_REPORT: f64 = 0.12;

/// Rough wall-time estimate: total work units / this ≈ seconds.
pub const UNITS_PER_SECOND: f64 = 28.0;

fn stage_label(stage: &str) -> &'static str {
    match stage {
        STAGE_SCAN => "Scanning project",
        STAGE_NORMALIZE => "Normalizing audio",
        STAGE_FEATURES => "Analyzing fingerprints",
        STAGE_MATCH => "Matching recordings",
        STAGE_COVERAGE => "Timeline coverage",
        STAGE_METADATA => "Collecting evidence",
        STAGE_REPORT => "Writing report",
        STAGE_DONE => "Complete",
        _ => "Working",
    }
}

#[derive(Debug, Clone)]
pub struct WorkPlan {
    pub total_units: f64,
    pub scan_units: f64,
    pub norm_target_units: f64,
    pub norm_asset_units: Vec<f64>,
    pub load_target_units: f64,
    pub chroma_target_units: f64,
    pub load_asset_units: Vec<f64>,
    pub chroma_asset_units: Vec<f64>,
    pub match_units: f64,
    pub coverage_units: f64,
    pub metadata_units: f64,
    pub report_units: f64,
    pub target_seconds: f64,
    pub asset_seconds: Vec<f64>,
}

impl WorkPlan {
    pub fn estimate(target_seconds: f64, asset_seconds: &[f64]) -> Self {
        let target_seconds = target_seconds.max(0.1);
        let n = asset_seconds.len().max(1);

        let norm_target_units = target_seconds * W_NORM_PER_SEC;
        let norm_asset_units: Vec<f64> = asset_seconds
            .iter()
            .map(|&s| s.max(0.1) * W_NORM_PER_SEC)
            .collect();

        let load_target_units = target_seconds * W_LOAD_PER_SEC;
        let chroma_target_units = target_seconds * W_CHROMA_PER_SEC;
        let load_asset_units: Vec<f64> = asset_seconds
            .iter()
            .map(|&s| s.max(0.1) * W_LOAD_PER_SEC)
            .collect();
        let chroma_asset_units: Vec<f64> = asset_seconds
            .iter()
            .map(|&s| s.max(0.1) * W_CHROMA_PER_SEC)
            .collect();

        let windows = (target_seconds / 0.5).ceil().max(1.0);
        let coverage_units = windows * n as f64 * W_COV_WINDOW_ASSET;

        let match_units =
            n as f64 * (W_MATCH_PER_ASSET + target_seconds * W_MATCH_TARGET_SEC);

        let metadata_units =
            W_METADATA_BASE + (n + 1) as f64 * W_METADATA_PER_FILE;

        let scan_units = W_SCAN;
        let report_units = W_REPORT;

        let total_units = scan_units
            + norm_target_units
            + norm_asset_units.iter().sum::<f64>()
            + load_target_units
            + chroma_target_units
            + load_asset_units.iter().sum::<f64>()
            + chroma_asset_units.iter().sum::<f64>()
            + match_units
            + coverage_units
            + metadata_units
            + report_units;

        Self {
            total_units,
            scan_units,
            norm_target_units,
            norm_asset_units,
            load_target_units,
            chroma_target_units,
            load_asset_units,
            chroma_asset_units,
            match_units,
            coverage_units,
            metadata_units,
            report_units,
            target_seconds,
            asset_seconds: asset_seconds.to_vec(),
        }
    }

    pub fn estimated_seconds(&self) -> f64 {
        self.total_units / UNITS_PER_SECOND
    }

    pub fn summary_detail(&self) -> String {
        let asset_secs: f64 = self.asset_seconds.iter().sum();
        format!(
            "~{:.0}s est. · {:.0}s song · {} assets · {:.0}s stems",
            self.estimated_seconds(),
            self.target_seconds,
            self.asset_seconds.len(),
            asset_secs
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub stage: String,
    pub step: String,
    pub percent: f64,
    pub stage_percent: f64,
    pub step_percent: f64,
    pub message: String,
    pub stage_label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_seconds: Option<f64>,
}

pub struct ProgressEmitter<'a> {
    sink: Option<&'a mut dyn FnMut(ProgressEvent)>,
    plan: Option<WorkPlan>,
    completed: f64,
    step_base: f64,
    step_units: f64,
}

impl<'a> ProgressEmitter<'a> {
    pub fn new(sink: Option<&'a mut dyn FnMut(ProgressEvent)>) -> Self {
        Self {
            sink,
            plan: None,
            completed: 0.0,
            step_base: 0.0,
            step_units: 0.0,
        }
    }

    pub fn set_plan(&mut self, plan: WorkPlan) {
        self.plan = Some(plan);
    }

    pub fn complete_all(&mut self) {
        if let Some(plan) = &self.plan {
            self.completed = plan.total_units;
        }
    }

    pub fn plan(&self) -> Option<&WorkPlan> {
        self.plan.as_ref()
    }

    pub fn begin_step(&mut self, units: f64) {
        self.step_base = self.completed;
        self.step_units = units.max(0.0);
    }

    pub fn set_step_fraction(&mut self, frac: f64) {
        let frac = frac.clamp(0.0, 1.0);
        self.completed = self.step_base + self.step_units * frac;
    }

    pub fn finish_step(&mut self) {
        self.completed = self.step_base + self.step_units;
        self.step_base = self.completed;
        self.step_units = 0.0;
    }

    pub fn mark_scan_done(&mut self) {
        if let Some(plan) = &self.plan {
            self.completed = plan.scan_units;
            self.step_base = self.completed;
        }
    }

    fn global_fraction(&self) -> f64 {
        let total = self.plan.as_ref().map(|p| p.total_units).unwrap_or(1.0);
        if total <= 0.0 {
            return 0.0;
        }
        if self.completed >= total {
            return 1.0;
        }
        (self.completed / total).clamp(0.0, 0.999)
    }

    pub fn emit(
        &mut self,
        stage: &str,
        step: &str,
        message: impl Into<String>,
        step_percent: f64,
        detail: Option<String>,
        item_index: Option<usize>,
        item_total: Option<usize>,
        item_name: Option<String>,
    ) {
        let step_percent = step_percent.clamp(0.0, 1.0);
        let global = self.global_fraction();
        let estimated_seconds = self.plan.as_ref().map(|p| p.estimated_seconds());
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        let event = ProgressEvent {
            event_type: "progress",
            stage: stage.to_string(),
            step: step.to_string(),
            percent: (global * 10000.0).round() / 10000.0,
            stage_percent: (global * 10000.0).round() / 10000.0,
            step_percent: (step_percent * 10000.0).round() / 10000.0,
            message: message.into(),
            stage_label: stage_label(stage),
            detail,
            item_index,
            item_total,
            item_name,
            estimated_seconds,
        };
        sink(event);
    }

    pub fn emit_step(
        &mut self,
        stage: &str,
        step: &str,
        message: impl Into<String>,
        step_frac: f64,
        detail: Option<String>,
        item_index: Option<usize>,
        item_total: Option<usize>,
        item_name: Option<String>,
    ) {
        self.set_step_fraction(step_frac);
        self.emit(
            stage,
            step,
            message,
            step_frac,
            detail,
            item_index,
            item_total,
            item_name,
        );
    }
}

pub fn write_jsonl_progress(event: &ProgressEvent) -> io::Result<()> {
    let line = serde_json::to_string(event)?;
    let mut err = io::stderr();
    writeln!(err, "{line}")?;
    err.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_project_weights_normalize_heaviest() {
        let plan = WorkPlan::estimate(240.0, &vec![30.0; 50]);
        let norm: f64 = plan.norm_target_units + plan.norm_asset_units.iter().sum::<f64>();
        assert!(norm / plan.total_units > 0.65);
    }

    #[test]
    fn longer_song_increases_total() {
        let short = WorkPlan::estimate(60.0, &vec![10.0; 5]);
        let long = WorkPlan::estimate(600.0, &vec![10.0; 5]);
        assert!(long.total_units > short.total_units * 3.0);
    }
}
