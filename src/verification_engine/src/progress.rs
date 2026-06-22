use serde::Serialize;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

pub const STAGE_SCAN: &str = "scan";
pub const STAGE_NORMALIZE: &str = "normalize";
pub const STAGE_FEATURES: &str = "features";
pub const STAGE_MATCH: &str = "match";
pub const STAGE_COVERAGE: &str = "coverage";
pub const STAGE_METADATA: &str = "metadata";
pub const STAGE_REPORT: &str = "report";
pub const STAGE_DONE: &str = "done";

// Progress bar units — one bump per discrete work item (stem, window, file, …).
pub const U_SCAN: f64 = 2.0;
pub const U_PROBE: f64 = 1.0;
pub const U_NORM: f64 = 8.0;
pub const U_FP: f64 = 5.0;
pub const U_MATCH: f64 = 2.0;
pub const U_COV: f64 = 0.18;
pub const U_META: f64 = 0.5;
pub const U_REPORT: f64 = 1.0;

// Wall-time estimate (ETA label only).
const SECS_SCAN_BASE: f64 = 0.06;
const SECS_PROBE_PER_FILE: f64 = 0.003;
const SECS_NORM_PER_TARGET_SEC: f64 = 0.014;
const SECS_FP_PER_AUDIO_SEC: f64 = 0.00045;
const SECS_FP_PER_FILE: f64 = 0.006;
const SECS_MATCH_PER_STEM: f64 = 0.005;
const SECS_MATCH_TARGET_PER_STEM: f64 = 0.000025;
const SECS_COV_PER_WINDOW_PER_STEM: f64 = 0.000007;
const SECS_METADATA_BASE: f64 = 0.10;
const SECS_METADATA_PER_FILE: f64 = 0.003;
const SECS_REPORT: f64 = 0.05;

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

#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub song_seconds: f64,
    pub stem_count: usize,
    pub stem_seconds_total: f64,
    pub fingerprint_audio_seconds: f64,
    pub estimated_seconds: f64,
    pub total_work_items: f64,
}

#[derive(Debug, Clone)]
pub struct WorkPlan {
    pub target_seconds: f64,
    pub asset_seconds: Vec<f64>,
    pub stem_count: usize,
    pub stem_seconds_total: f64,
    pub needs_target_normalize: bool,
    pub fingerprint_audio_seconds: f64,
    pub window_count: usize,
    pub fp_item_count: usize,
    pub total_units: f64,
    pub total_seconds: f64,
}

impl WorkPlan {
    pub fn total_units_for(stem_count: usize, window_count: usize, needs_norm: bool) -> f64 {
        let n = stem_count;
        let fp_items = n + 1;
        U_SCAN
            + (n + 1) as f64 * U_PROBE
            + if needs_norm { U_NORM } else { 0.0 }
            + fp_items as f64 * U_FP
            + n as f64 * U_MATCH
            + window_count as f64 * U_COV
            + (n + 3) as f64 * U_META
            + 3.0 * U_REPORT
    }

    pub fn estimate(
        target_seconds: f64,
        asset_seconds: &[f64],
        needs_target_normalize: bool,
    ) -> Self {
        let target_seconds = target_seconds.max(0.1);
        let n = asset_seconds.len();
        let stem_seconds_total: f64 = asset_seconds.iter().map(|&s| s.max(0.1)).sum();
        let fingerprint_audio_seconds = target_seconds + stem_seconds_total;
        let window_count = ((target_seconds / 0.5).ceil() as usize).max(1);
        let fp_item_count = n + 1;
        let total_units = Self::total_units_for(n, window_count, needs_target_normalize);

        let scan_seconds = SECS_SCAN_BASE + (n + 1) as f64 * SECS_PROBE_PER_FILE;
        let normalize_seconds = if needs_target_normalize {
            target_seconds * SECS_NORM_PER_TARGET_SEC
        } else {
            0.0
        };
        let cores = rayon::current_num_threads().max(1) as f64;
        let max_stem = asset_seconds
            .iter()
            .copied()
            .fold(0.0f64, |m, s| m.max(s.max(0.1)));
        let parallel_tracks = target_seconds.max(max_stem) + stem_seconds_total / cores;
        let fingerprint_seconds =
            parallel_tracks * SECS_FP_PER_AUDIO_SEC + n as f64 * SECS_FP_PER_FILE;
        let match_seconds =
            n as f64 * (SECS_MATCH_PER_STEM + target_seconds * SECS_MATCH_TARGET_PER_STEM);
        let coverage_seconds =
            window_count as f64 * n as f64 * SECS_COV_PER_WINDOW_PER_STEM;
        let metadata_seconds = SECS_METADATA_BASE + (n + 1) as f64 * SECS_METADATA_PER_FILE;
        let report_seconds = SECS_REPORT;
        let total_seconds = (scan_seconds
            + normalize_seconds
            + fingerprint_seconds
            + match_seconds
            + coverage_seconds
            + metadata_seconds
            + report_seconds)
            .max(0.25);

        Self {
            target_seconds,
            asset_seconds: asset_seconds.to_vec(),
            stem_count: n,
            stem_seconds_total,
            needs_target_normalize,
            fingerprint_audio_seconds,
            window_count,
            fp_item_count,
            total_units: total_units.max(1.0),
            total_seconds,
        }
    }

    pub fn estimated_seconds(&self) -> f64 {
        self.total_seconds
    }

    pub fn summary_detail(&self) -> String {
        format!(
            "~{:.1}s est. · {:.1}s song · {} stems · {} fingerprint items · {} work units",
            self.total_seconds,
            self.target_seconds,
            self.stem_count,
            self.fp_item_count,
            self.total_units.round() as u64
        )
    }

    pub fn plan_summary(&self) -> PlanSummary {
        PlanSummary {
            song_seconds: self.target_seconds,
            stem_count: self.stem_count,
            stem_seconds_total: self.stem_seconds_total,
            fingerprint_audio_seconds: self.fingerprint_audio_seconds,
            estimated_seconds: self.total_seconds,
            total_work_items: self.total_units,
        }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_summary: Option<PlanSummary>,
}

struct ProgressCore {
    sink: Option<Box<dyn FnMut(ProgressEvent) + Send>>,
    plan: Option<WorkPlan>,
    completed_units: f64,
}

impl ProgressCore {
    fn global_fraction(&self) -> f64 {
        let total = self.plan.as_ref().map(|p| p.total_units).unwrap_or(1.0);
        if total <= 0.0 {
            return 0.0;
        }
        if self.completed_units >= total {
            return 1.0;
        }
        (self.completed_units / total).clamp(0.0, 0.999)
    }

    fn emit(
        &mut self,
        stage: &str,
        step: &str,
        message: impl Into<String>,
        detail: Option<String>,
        item_index: Option<usize>,
        item_total: Option<usize>,
        item_name: Option<String>,
        plan_summary: Option<PlanSummary>,
    ) {
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
            step_percent: (global * 10000.0).round() / 10000.0,
            message: message.into(),
            stage_label: stage_label(stage),
            detail,
            item_index,
            item_total,
            item_name,
            estimated_seconds,
            plan_summary,
        };
        sink(event);
    }
}

/// Thread-safe progress handle; clone for parallel workers.
#[derive(Clone)]
pub struct ProgressHandle {
    inner: Arc<Mutex<ProgressCore>>,
}

impl ProgressHandle {
    pub fn new(sink: Option<Box<dyn FnMut(ProgressEvent) + Send>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ProgressCore {
                sink,
                plan: None,
                completed_units: 0.0,
            })),
        }
    }

    pub fn set_plan(&self, plan: WorkPlan) {
        let mut g = self.inner.lock().unwrap();
        g.plan = Some(plan);
    }

    pub fn plan(&self) -> Option<WorkPlan> {
        self.inner.lock().unwrap().plan.clone()
    }

    pub fn advance_units(&self, delta: f64) {
        if delta <= 0.0 {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        let cap = g.plan.as_ref().map(|p| p.total_units).unwrap_or(f64::MAX);
        g.completed_units = (g.completed_units + delta).min(cap * 0.999);
    }

    pub fn complete_all(&self) {
        let mut g = self.inner.lock().unwrap();
        if let Some(plan) = &g.plan {
            g.completed_units = plan.total_units;
        }
    }

    /// Advance the bar by `units`, then emit one progress event.
    pub fn tick(
        &self,
        stage: &str,
        step: &str,
        message: impl Into<String>,
        units: f64,
        detail: Option<String>,
        item_index: Option<usize>,
        item_total: Option<usize>,
        item_name: Option<String>,
        plan_summary: Option<PlanSummary>,
    ) {
        self.advance_units(units);
        let mut g = self.inner.lock().unwrap();
        g.emit(
            stage,
            step,
            message,
            detail,
            item_index,
            item_total,
            item_name,
            plan_summary,
        );
    }

    pub fn emit(
        &self,
        stage: &str,
        step: &str,
        message: impl Into<String>,
        detail: Option<String>,
        item_index: Option<usize>,
        item_total: Option<usize>,
        item_name: Option<String>,
        plan_summary: Option<PlanSummary>,
    ) {
        let mut g = self.inner.lock().unwrap();
        g.emit(
            stage,
            step,
            message,
            detail,
            item_index,
            item_total,
            item_name,
            plan_summary,
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
    fn each_stem_gets_equal_fp_weight() {
        let plan = WorkPlan::estimate(60.0, &vec![10.0; 50], false);
        let fp_share = 51.0 * U_FP / plan.total_units;
        assert!(fp_share > 0.35);
        assert!((U_FP / plan.total_units) > 0.008);
    }

    #[test]
    fn longer_song_increases_estimate() {
        let short = WorkPlan::estimate(60.0, &vec![10.0; 5], false);
        let long = WorkPlan::estimate(600.0, &vec![10.0; 5], false);
        assert!(long.total_seconds > short.total_seconds * 2.0);
    }
}
