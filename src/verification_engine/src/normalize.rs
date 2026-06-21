use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

pub const TARGET_SR: u32 = 44100;

pub fn media_duration_seconds(source: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(source)
        .output()
        .context("spawn ffprobe")?;
    if !output.status.success() {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse::<f64>()
        .context("parse ffprobe duration")
}

pub fn normalize_to_wav(source: &Path, dest: &Path) -> Result<()> {
    normalize_to_wav_with_progress(source, dest, None::<&mut dyn FnMut(f64)>)
}

pub fn normalize_to_wav_with_progress(
    source: &Path,
    dest: &Path,
    mut on_progress: Option<&mut dyn FnMut(f64)>,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let duration = media_duration_seconds(source).unwrap_or(0.0);

    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-nostats")
        .arg("-progress")
        .arg("pipe:2")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(source)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg(TARGET_SR.to_string())
        .arg("-c:a")
        .arg("pcm_f32le")
        .arg(dest)
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn ffmpeg")?;

    if let (Some(progress), Some(stderr)) = (on_progress.as_mut(), child.stderr.take()) {
        let reader = BufReader::new(stderr);
        let mut last_emit = 0.0f64;
        for line in reader.lines() {
            let line = line.context("read ffmpeg progress")?;
            if let Some(raw) = line.strip_prefix("out_time_us=") {
                if duration <= 0.0 {
                    continue;
                }
                if let Ok(us) = raw.trim().parse::<f64>() {
                    let frac = (us / 1_000_000.0 / duration).clamp(0.0, 1.0);
                    if frac - last_emit >= 0.02 || frac >= 1.0 {
                        progress(frac);
                        last_emit = frac;
                    }
                }
            }
        }
    }

    let status = child.wait().context("wait for ffmpeg")?;
    if !status.success() {
        anyhow::bail!("ffmpeg failed for {}", source.display());
    }

    if let Some(progress) = on_progress.as_mut() {
        progress(1.0);
    }

    Ok(())
}
