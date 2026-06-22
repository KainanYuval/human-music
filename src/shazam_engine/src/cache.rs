use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::fingerprint::{fingerprint, Fingerprint, TARGET_SR};
use crate::fingerprint_file_inner;

pub const FP_VERSION: &str = "v2";

pub fn default_cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/_generated/shazam_fp_cache_rs")
}

fn cache_key(path: &Path) -> anyhow::Result<String> {
    let path = path.canonicalize().with_context(|| format!("canonicalize {}", path.display()))?;
    let meta = fs::metadata(&path)?;
    let mut h = DefaultHasher::new();
    FP_VERSION.hash(&mut h);
    path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified()?.hash(&mut h);
    Ok(format!("{:016x}", h.finish()))
}

fn cache_path(path: &Path, cache_dir: &Path) -> anyhow::Result<PathBuf> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' })
        .take(40)
        .collect::<String>();
    Ok(cache_dir.join(format!("{}_{}.fpc", stem, cache_key(path)?)))
}

pub fn load_cache(file: &Path) -> anyhow::Result<Vec<Fingerprint>> {
    let bytes = fs::read(file)?;
    if bytes.len() < 8 {
        anyhow::bail!("cache too small");
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into()?) as usize;
    let ver_len = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
    if bytes.len() < 8 + ver_len {
        anyhow::bail!("cache corrupt");
    }
    let ver = std::str::from_utf8(&bytes[8..8 + ver_len])?;
    if ver != FP_VERSION {
        anyhow::bail!("cache version mismatch");
    }
    let mut off = 8 + ver_len;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if off + 12 > bytes.len() {
            anyhow::bail!("cache truncated");
        }
        let hash = u64::from_le_bytes(bytes[off..off + 8].try_into()?);
        let time_s = f32::from_le_bytes(bytes[off + 8..off + 12].try_into()?);
        out.push(Fingerprint { hash, time_s });
        off += 12;
    }
    Ok(out)
}

pub fn save_cache(file: &Path, fps: &[Fingerprint]) -> anyhow::Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let ver = FP_VERSION.as_bytes();
    let mut bytes = Vec::with_capacity(8 + ver.len() + fps.len() * 12);
    bytes.extend_from_slice(&(fps.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(ver.len() as u32).to_le_bytes());
    bytes.extend_from_slice(ver);
    for fp in fps {
        bytes.extend_from_slice(&fp.hash.to_le_bytes());
        bytes.extend_from_slice(&fp.time_s.to_le_bytes());
    }
    fs::write(file, bytes)?;
    Ok(())
}

pub fn get_fingerprints(path: &Path, cache_dir: &Path) -> anyhow::Result<(Vec<Fingerprint>, bool)> {
    let cache_file = cache_path(path, cache_dir)?;
    if cache_file.is_file() {
        if let Ok(fps) = load_cache(&cache_file) {
            return Ok((fps, true));
        }
        let _ = fs::remove_file(&cache_file);
    }
    let fps = fingerprint_file_inner(path)?;
    save_cache(&cache_file, &fps)?;
    Ok((fps, false))
}

pub fn resample_to_target(samples: &[f32], sr: u32) -> Vec<f32> {
    if sr == TARGET_SR {
        samples.to_vec()
    } else {
        crate::audio::resample(samples, sr, TARGET_SR)
    }
}

pub fn fingerprint_samples(samples: &[f32]) -> Vec<Fingerprint> {
    fingerprint(samples)
}
