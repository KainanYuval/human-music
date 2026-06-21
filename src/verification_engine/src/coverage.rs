use crate::chroma::{best_chroma_offset_frames, ChromaMatrix, CHROMA_HOP};
use crate::progress::{ProgressEmitter, STAGE_COVERAGE};

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
            threshold: 0.82,
        }
    }
}

pub fn compute_timeline_coverage(
    target_seconds: f64,
    target_chroma: &ChromaMatrix,
    asset_chromas: &[ChromaMatrix],
    sr: u32,
    opts: CoverageOptions,
    progress: &mut ProgressEmitter<'_>,
) -> CoverageResult {
    let coverage_units = progress
        .plan()
        .map(|p| p.coverage_units)
        .unwrap_or(1.0);
    progress.begin_step(coverage_units);

    let hop = CHROMA_HOP;
    let mut win_frames = ((opts.window_seconds * sr as f64) / hop as f64).round() as usize;
    win_frames = win_frames.max(8);
    let mut hop_frames = ((opts.hop_seconds * sr as f64) / hop as f64).round() as usize;
    hop_frames = hop_frames.max(1);

    let total_frames = target_chroma.frames;
    let win_frames = win_frames.min(total_frames.max(1));

    let mut explained_mask = vec![false; total_frames];
    let mut total_windows = 0usize;
    let mut explained_windows = 0usize;

    let window_count = if total_frames >= win_frames {
        (total_frames - win_frames) / hop_frames + 1
    } else {
        0
    };

    progress.emit(
        STAGE_COVERAGE,
        "start",
        "Scanning timeline windows against project recordings",
        0.0,
        Some(format!("{window_count} windows × {} assets", asset_chromas.len())),
        None,
        Some(window_count),
        None,
    );

    if total_frames >= win_frames {
        let mut start = 0usize;
        let mut window_idx = 0usize;
        while start + win_frames <= total_frames {
            total_windows += 1;
            let mut best = 0.0f64;
            for asset in asset_chromas {
                let query = window_matrix(target_chroma, start, start + win_frames);
                let (score, _) = best_chroma_offset_frames(&query, asset);
                best = best.max(score);
            }
            if best >= opts.threshold {
                explained_windows += 1;
                for slot in &mut explained_mask[start..start + win_frames] {
                    *slot = true;
                }
            }

            let step_frac = if window_count > 0 {
                (window_idx + 1) as f64 / window_count as f64
            } else {
                1.0
            };
            let offset_sec = start as f64 * hop as f64 / sr as f64;
            progress.emit_step(
                STAGE_COVERAGE,
                "window",
                "Checking timeline segment",
                step_frac,
                Some(format!(
                    "{offset_sec:.1}s–{:.1}s · best {best:.3} · {}",
                    offset_sec + opts.window_seconds,
                    if best >= opts.threshold {
                        "explained"
                    } else {
                        "no match"
                    }
                )),
                Some(window_idx),
                Some(window_count),
                None,
            );

            window_idx += 1;
            start += hop_frames;
        }
    }

    let explained_frames = explained_mask.iter().filter(|&&v| v).count();
    let ratio = if total_frames == 0 {
        0.0
    } else {
        (explained_frames as f64 / total_frames as f64).min(1.0)
    };

    progress.finish_step();
    progress.emit(
        STAGE_COVERAGE,
        "done",
        "Timeline coverage complete",
        1.0,
        Some(format!(
            "{explained_windows}/{total_windows} windows · {:.1}% of song",
            ratio * 100.0
        )),
        None,
        None,
        None,
    );

    CoverageResult {
        explained_seconds: round3(ratio * target_seconds),
        target_seconds: round3(target_seconds),
        coverage_ratio: (ratio * 10000.0).round() / 10000.0,
        window_seconds: opts.window_seconds,
        hop_seconds: opts.hop_seconds,
        threshold: opts.threshold,
        explained_windows,
        total_windows,
    }
}

pub(crate) fn window_matrix(source: &ChromaMatrix, start: usize, end: usize) -> ChromaMatrix {
    let frames = end - start;
    let mut data = vec![0.0f32; crate::chroma::CHROMA_BANDS * frames];
    for band in 0..crate::chroma::CHROMA_BANDS {
        let slice = source.band_slice(band, start, end);
        data[band * frames..(band + 1) * frames].copy_from_slice(slice);
    }
    ChromaMatrix { data, frames }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
