pub mod audio;
pub mod cache;
pub mod fingerprint;
pub mod match_;

pub use cache::default_cache_dir;
pub use fingerprint::{fingerprint, Fingerprint, TARGET_SR};
pub use match_::{build_index, match_landmarks, FingerprintIndex, MatchResult};

use std::path::Path;

pub fn fingerprint_file_inner(path: &Path) -> anyhow::Result<Vec<Fingerprint>> {
    let (samples, sr) = audio::load_mono(path)?;
    let resampled = cache::resample_to_target(&samples, sr);
    Ok(fingerprint::fingerprint(&resampled))
}

pub fn fingerprint_file(path: &Path) -> anyhow::Result<Vec<Fingerprint>> {
    fingerprint_file_inner(path)
}

pub fn fingerprint_file_cached(
    path: &Path,
    cache_dir: &Path,
) -> anyhow::Result<(Vec<Fingerprint>, bool)> {
    cache::get_fingerprints(path, cache_dir)
}
