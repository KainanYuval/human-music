use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

pub const CHROMA_HOP: usize = 512;
pub const CHROMA_N_FFT: usize = 4096;
pub const CHROMA_BANDS: usize = 12;

#[derive(Debug, Clone)]
pub struct ChromaMatrix {
    pub data: Vec<f32>,
    pub frames: usize,
}

impl ChromaMatrix {
    pub fn band(&self, band: usize) -> &[f32] {
        let start = band * self.frames;
        &self.data[start..start + self.frames]
    }

    pub fn band_slice(&self, band: usize, start: usize, end: usize) -> &[f32] {
        let base = band * self.frames + start;
        &self.data[base..base + (end - start)]
    }
}

pub fn chroma_matrix(signal: &[f32], sr: u32) -> ChromaMatrix {
    chroma_matrix_with_progress(signal, sr, None::<&mut dyn FnMut(f64)>)
}

pub fn chroma_matrix_with_progress(
    signal: &[f32],
    sr: u32,
    mut on_progress: Option<&mut dyn FnMut(f64)>,
) -> ChromaMatrix {
    let n_fft = CHROMA_N_FFT;
    let hop = CHROMA_HOP;
    if signal.len() < n_fft {
        if let Some(progress) = on_progress.as_mut() {
            progress(1.0);
        }
        return ChromaMatrix {
            data: vec![0.0; CHROMA_BANDS],
            frames: 1,
        };
    }

    let frames = 1 + (signal.len() - n_fft) / hop;
    let emit_every = (frames / 100).max(1);
    let mut out = vec![0.0f32; CHROMA_BANDS * frames];
    let window = hann_window(n_fft);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let mut scratch = vec![Complex::<f32>::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut buffer = vec![Complex::<f32>::new(0.0, 0.0); n_fft];

    let bin_freqs: Vec<f32> = (0..=n_fft / 2)
        .map(|i| i as f32 * sr as f32 / n_fft as f32)
        .collect();

    for frame_idx in 0..frames {
        let start = frame_idx * hop;
        for i in 0..n_fft {
            let sample = if start + i < signal.len() {
                signal[start + i]
            } else {
                0.0
            };
            buffer[i] = Complex::new(sample * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);

        let mut chroma = [0.0f32; CHROMA_BANDS];
        for (bin, &freq) in bin_freqs.iter().enumerate().take(n_fft / 2 + 1) {
            if freq < 80.0 {
                continue;
            }
            let pitch = ((12.0 * (freq / 440.0).log2()).round() as i32).rem_euclid(12) as usize;
            chroma[pitch] += buffer[bin].norm();
        }

        for band in 0..CHROMA_BANDS {
            chroma[band] = (1.0 + chroma[band]).ln();
        }
        let norm = (chroma.iter().map(|v| v * v).sum::<f32>()).sqrt().max(1e-9);
        for band in 0..CHROMA_BANDS {
            out[band * frames + frame_idx] = chroma[band] / norm;
        }

        if let Some(progress) = on_progress.as_mut() {
            if frame_idx % emit_every == 0 || frame_idx + 1 == frames {
                progress((frame_idx + 1) as f64 / frames as f64);
            }
        }
    }

    ChromaMatrix { data: out, frames }
}

fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos()))
        .collect()
}

fn correlate_valid_fft(signal: &[f64], kernel: &[f64]) -> Vec<f64> {
    if kernel.is_empty() || signal.len() < kernel.len() {
        return Vec::new();
    }
    let out_len = signal.len() - kernel.len() + 1;
    let n = signal.len() + kernel.len() - 1;
    let fft_len = n.next_power_of_two();

    let mut planner = FftPlanner::<f64>::new();
    let mut fft = planner.plan_fft_forward(fft_len);
    let mut ifft = planner.plan_fft_inverse(fft_len);
    let mut scratch_fwd = vec![Complex::<f64>::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut scratch_inv = vec![Complex::<f64>::new(0.0, 0.0); ifft.get_inplace_scratch_len()];

    let mut fx = vec![Complex::<f64>::new(0.0, 0.0); fft_len];
    let mut fk = vec![Complex::<f64>::new(0.0, 0.0); fft_len];
    let mut prod = vec![Complex::<f64>::new(0.0, 0.0); fft_len];

    for (i, &v) in signal.iter().enumerate() {
        fx[i] = Complex::new(v, 0.0);
    }
    for (i, &v) in kernel.iter().enumerate() {
        fk[i] = Complex::new(v, 0.0);
    }

    fft.process_with_scratch(&mut fx, &mut scratch_fwd);
    fft.process_with_scratch(&mut fk, &mut scratch_fwd);

    for i in 0..fft_len {
        prod[i] = fx[i] * fk[i].conj();
    }

    ifft.process_with_scratch(&mut prod, &mut scratch_inv);

    let scale = fft_len as f64;
    (0..out_len).map(|i| prod[i].re / scale).collect()
}

fn offset_scores(query: &ChromaMatrix, search: &ChromaMatrix) -> Vec<f64> {
    let win = query.frames;
    if search.frames < win {
        return Vec::new();
    }
    let out_len = search.frames - win + 1;
    let mut score = vec![0.0f64; out_len];
    for band in 0..CHROMA_BANDS {
        let q: Vec<f64> = query.band(band).iter().map(|&v| v as f64).collect();
        let s: Vec<f64> = search.band(band).iter().map(|&v| v as f64).collect();
        let corr = correlate_valid_fft(&s, &q);
        for (i, v) in corr.into_iter().enumerate() {
            score[i] += v;
        }
    }
    let win_f = win as f64;
    score.iter_mut().for_each(|v| *v /= win_f);
    score
}

fn frame_to_seconds(frame: usize, sr: u32) -> f64 {
    frame as f64 * CHROMA_HOP as f64 / sr as f64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    RefInTarget,
    TargetInRef,
}

impl MatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RefInTarget => "ref_in_target",
            Self::TargetInRef => "target_in_ref",
        }
    }
}

pub fn best_chroma_offset_from_matrices(
    ref_c: &ChromaMatrix,
    tgt_c: &ChromaMatrix,
    sr: u32,
) -> (f64, f64, MatchMode) {
    if ref_c.frames <= tgt_c.frames {
        let scores = offset_scores(ref_c, tgt_c);
        if scores.is_empty() {
            return (0.0, 0.0, MatchMode::RefInTarget);
        }
        let (idx, &best) = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        return (best, frame_to_seconds(idx, sr), MatchMode::RefInTarget);
    }

    let scores = offset_scores(tgt_c, ref_c);
    if scores.is_empty() {
        return (0.0, 0.0, MatchMode::TargetInRef);
    }
    let (idx, &best) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    (best, frame_to_seconds(idx, sr), MatchMode::TargetInRef)
}

pub fn best_chroma_offset_frames(query: &ChromaMatrix, search: &ChromaMatrix) -> (f64, usize) {
    let scores = offset_scores(query, search);
    if scores.is_empty() {
        return (0.0, 0);
    }
    let (idx, &best) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    (best, idx)
}
