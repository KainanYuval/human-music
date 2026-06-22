use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader};
use std::path::Path;

pub fn load_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    let sr = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        SampleFormat::Int => match spec.bits_per_sample {
            16 => reader
                .samples::<i16>()
                .map(|s| s.map(|v| v as f32 / 32768.0))
                .collect::<Result<Vec<_>, _>>()?,
            32 => reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / 2147483648.0))
                .collect::<Result<Vec<_>, _>>()?,
            24 => reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / 8388608.0))
                .collect::<Result<Vec<_>, _>>()?,
            other => anyhow::bail!("unsupported bit depth {other}"),
        },
    };

    let mono = if spec.channels == 1 {
        samples
    } else {
        samples
            .chunks(spec.channels as usize)
            .map(|f| f.iter().sum::<f32>() / f.len() as f32)
            .collect()
    };
    Ok((mono, sr))
}

pub fn resample(input: &[f32], from_sr: u32, to_sr: u32) -> Vec<f32> {
    if from_sr == to_sr {
        return input.to_vec();
    }
    let ratio = from_sr as f64 / to_sr as f64;
    let out_len = (input.len() as f64 / ratio).ceil() as usize;
    (0..out_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let idx = src as usize;
            let frac = (src - idx as f64) as f32;
            if idx + 1 < input.len() {
                input[idx] * (1.0 - frac) + input[idx + 1] * frac
            } else {
                input[input.len().saturating_sub(1)]
            }
        })
        .collect()
}
