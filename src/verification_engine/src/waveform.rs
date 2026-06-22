use crate::chroma::{MatchMode, CHROMA_HOP};

/// Pearson correlation on equal-length slices (zero-mean normalized).
pub fn pearson(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mean_a = a.iter().map(|&v| v as f64).sum::<f64>() / a.len() as f64;
    let mean_b = b.iter().map(|&v| v as f64).sum::<f64>() / b.len() as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..a.len() {
        let va = a[i] as f64 - mean_a;
        let vb = b[i] as f64 - mean_b;
        num += va * vb;
        da += va * va;
        db += vb * vb;
    }
    let denom = (da * db).sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        (num / denom).clamp(-1.0, 1.0)
    }
}

/// Waveform Pearson after chroma alignment, with small local search around offset.
pub fn refined_pearson(
    ref_audio: &[f32],
    target_audio: &[f32],
    sr: u32,
    offset_seconds: f64,
    mode: MatchMode,
) -> f64 {
    let center = (offset_seconds * sr as f64).round() as isize;
    let search = CHROMA_HOP as isize;
    let mut best = 0.0f64;
    for delta in -search..=search {
        let p = pearson_aligned(ref_audio, target_audio, center + delta, mode, sr);
        best = best.max(p);
    }
    best.max(0.0)
}

fn pearson_aligned(
    ref_audio: &[f32],
    target_audio: &[f32],
    offset_samples: isize,
    mode: MatchMode,
    sr: u32,
) -> f64 {
    match mode {
        MatchMode::RefInTarget => {
            if offset_samples < 0 {
                return 0.0;
            }
            let start = offset_samples as usize;
            if start >= target_audio.len() {
                return 0.0;
            }
            let len = ref_audio.len().min(target_audio.len() - start);
            if len < sr as usize / 20 {
                return 0.0;
            }
            pearson(&ref_audio[..len], &target_audio[start..start + len])
        }
        MatchMode::TargetInRef => {
            if offset_samples < 0 {
                return 0.0;
            }
            let start = offset_samples as usize;
            if start >= ref_audio.len() {
                return 0.0;
            }
            let len = target_audio.len().min(ref_audio.len() - start);
            if len < sr as usize / 20 {
                return 0.0;
            }
            pearson(&target_audio[..len], &ref_audio[start..start + len])
        }
    }
}
