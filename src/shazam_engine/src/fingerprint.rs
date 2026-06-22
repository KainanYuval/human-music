use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use std::collections::BTreeMap;

pub const TARGET_SR: u32 = 11025;
pub const N_FFT: usize = 4096;
pub const HOP: usize = 256;
pub const FANOUT: usize = 3;
pub const MIN_DT: usize = 3;
pub const MAX_DT: usize = 45;
pub const PEAK_PERCENTILE: f32 = 99.9;
pub const F1_BITS: u32 = 8;
pub const DF_BITS: u32 = 6;
pub const DT_BITS: u32 = 6;

#[derive(Clone, Debug)]
pub struct Fingerprint {
    pub hash: u64,
    pub time_s: f32,
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos())
        })
        .collect()
}

fn quantize_freq(bin: usize, n_bins: usize) -> u32 {
    let cap = bin.min(n_bins.saturating_sub(1));
    ((cap as u32) * ((1u32 << F1_BITS) - 1)) / n_bins.saturating_sub(1).max(1) as u32
}

fn hash_pair(f1: u32, f2: u32, dt: usize) -> u64 {
    let df = (f2.wrapping_sub(f1)) & ((1u32 << DF_BITS) - 1);
    let dt_q = (dt.min((1usize << DT_BITS as usize) - 1)) as u64;
    ((f1 as u64) << (DF_BITS + DT_BITS)) | ((df as u64) << DT_BITS) | dt_q
}

fn percentile(values: &mut [f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let idx = ((values.len() as f64) * (p as f64 / 100.0)).floor() as usize;
    let idx = idx.min(values.len() - 1);
    values.select_nth_unstable_by(idx, |a, b| a.partial_cmp(b).unwrap());
    values[idx]
}

pub fn fingerprint(signal: &[f32]) -> Vec<Fingerprint> {
    if signal.len() < N_FFT {
        return Vec::new();
    }

    let frames = 1 + (signal.len() - N_FFT) / HOP;
    let n_freq = N_FFT / 2 + 1;
    let window = hann(N_FFT);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let mut scratch = vec![Complex::<f32>::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut buf = vec![Complex::<f32>::new(0.0, 0.0); N_FFT];

    let mut log_spec = vec![0.0f32; n_freq * frames];
    for t in 0..frames {
        let start = t * HOP;
        for i in 0..N_FFT {
            let s = if start + i < signal.len() {
                signal[start + i]
            } else {
                0.0
            };
            buf[i] = Complex::new(s * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        for f in 0..n_freq {
            log_spec[f * frames + t] = (1.0 + buf[f].norm() * 512.0).ln();
        }
    }

    let mut flat = log_spec.clone();
    let threshold = percentile(&mut flat, PEAK_PERCENTILE);

    // local max along frequency (window 20 x 1)
    let half = 10usize;
    let mut peaks: Vec<(usize, usize)> = Vec::new();
    for t in 0..frames {
        for f in 0..n_freq {
            let v = log_spec[f * frames + t];
            if v < threshold {
                continue;
            }
            let f0 = f.saturating_sub(half);
            let f1 = (f + half + 1).min(n_freq);
            let mut is_max = true;
            for ff in f0..f1 {
                if log_spec[ff * frames + t] > v {
                    is_max = false;
                    break;
                }
            }
            if is_max {
                peaks.push((f, t));
            }
        }
    }

    let mut by_time: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (f, t) in peaks {
        by_time.entry(t).or_default().push(f);
    }

    let max_t = frames.saturating_sub(1);
    let mut out = Vec::new();

    for (&t_anchor, freqs) in &by_time {
        for &f_anchor in freqs {
            let f1 = quantize_freq(f_anchor, n_freq);
            let mut targets = 0usize;
            for t_target in (t_anchor + MIN_DT)..=(t_anchor + MAX_DT).min(max_t) {
                if let Some(tf) = by_time.get(&t_target) {
                    for &f_target in tf {
                        if f_anchor.abs_diff(f_target) > 30 {
                            continue;
                        }
                        let h = hash_pair(f1, quantize_freq(f_target, n_freq), t_target - t_anchor);
                        let time_s = t_anchor as f32 * HOP as f32 / TARGET_SR as f32;
                        out.push(Fingerprint { hash: h, time_s });
                        targets += 1;
                        if targets >= FANOUT {
                            break;
                        }
                    }
                }
                if targets >= FANOUT {
                    break;
                }
            }
        }
    }

    out
}
