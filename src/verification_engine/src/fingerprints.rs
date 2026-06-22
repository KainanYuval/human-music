use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use shazam_engine::{fingerprint_file_cached, Fingerprint};

use crate::normalize::media_duration_seconds;

pub fn fingerprint_cached(path: &Path, cache_dir: &Path) -> Result<Vec<Fingerprint>> {
    let (fps, _) = fingerprint_file_cached(path, cache_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(fps)
}

pub struct StemFingerprint {
    pub name: String,
    pub fingerprints: Vec<Fingerprint>,
    pub duration_seconds: f64,
}

pub struct FingerprintItemDone {
    pub name: String,
    pub duration_seconds: f64,
    pub item_index: usize,
    pub item_total: usize,
    pub completed_audio_seconds: f64,
    pub total_audio_seconds: f64,
    pub is_target: bool,
}

fn notify_item(
    cb: &Option<Arc<dyn Fn(FingerprintItemDone) + Send + Sync>>,
    completed_ms: &AtomicU64,
    total_audio: f64,
    name: String,
    duration: f64,
    item_index: usize,
    item_total: usize,
    is_target: bool,
) {
    let Some(cb) = cb else {
        return;
    };
    let prev = completed_ms.fetch_add((duration * 1000.0) as u64, Ordering::Relaxed);
    cb(FingerprintItemDone {
        name,
        duration_seconds: duration,
        item_index,
        item_total,
        completed_audio_seconds: (prev as f64) / 1000.0 + duration,
        total_audio_seconds: total_audio,
        is_target,
    });
}

/// Fingerprint many stems in parallel (each file → separate cache entry).
pub fn fingerprint_stems_parallel(
    sources: &[PathBuf],
    cache_dir: &Path,
) -> Result<Vec<StemFingerprint>> {
    fingerprint_stems_parallel_with_progress(sources, cache_dir, None, None)
}

pub fn fingerprint_stems_parallel_with_progress(
    sources: &[PathBuf],
    cache_dir: &Path,
    on_item_done: Option<Arc<dyn Fn(FingerprintItemDone) + Send + Sync>>,
    completed_ms: Option<Arc<AtomicU64>>,
) -> Result<Vec<StemFingerprint>> {
    let total = sources.len();
    let completed_ms = completed_ms.unwrap_or_else(|| Arc::new(AtomicU64::new(0)));
    let total_audio: f64 = sources
        .iter()
        .map(|p| media_duration_seconds(p).unwrap_or(30.0))
        .sum();

    sources
        .par_iter()
        .enumerate()
        .map(|(idx, source)| {
            let name = source.file_name().unwrap().to_string_lossy().to_string();
            let duration = media_duration_seconds(source).unwrap_or(30.0);
            let fingerprints = fingerprint_cached(source, cache_dir)?;
            notify_item(
                &on_item_done,
                &completed_ms,
                total_audio,
                name.clone(),
                duration,
                idx,
                total,
                false,
            );
            Ok(StemFingerprint {
                name,
                fingerprints,
                duration_seconds: duration,
            })
        })
        .collect()
}

/// Fingerprint target + all stems concurrently.
pub fn fingerprint_target_and_stems_parallel(
    target: &Path,
    sources: &[PathBuf],
    cache_dir: &Path,
) -> Result<(Vec<Fingerprint>, Vec<StemFingerprint>)> {
    fingerprint_target_and_stems_parallel_with_progress(target, sources, cache_dir, None)
}

pub fn fingerprint_target_and_stems_parallel_with_progress(
    target: &Path,
    sources: &[PathBuf],
    cache_dir: &Path,
    on_item_done: Option<Arc<dyn Fn(FingerprintItemDone) + Send + Sync>>,
) -> Result<(Vec<Fingerprint>, Vec<StemFingerprint>)> {
    let cache_dir = cache_dir.to_path_buf();
    let target = target.to_path_buf();
    let sources: Vec<PathBuf> = sources.to_vec();
    let target_duration = media_duration_seconds(&target).unwrap_or(30.0);
    let stem_audio: f64 = sources
        .iter()
        .map(|p| media_duration_seconds(p).unwrap_or(30.0))
        .sum();
    let total_audio = target_duration + stem_audio;
    let item_total = sources.len() + 1;
    let completed_ms = Arc::new(AtomicU64::new(0));

    let on_target = on_item_done.clone();
    let on_stems = on_item_done;
    let completed_target = Arc::clone(&completed_ms);
    let completed_stems = Arc::clone(&completed_ms);
    let cache_dir_stems = cache_dir.clone();
    let target_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "release".to_string());

    let (target_res, stems_res): (Result<Vec<Fingerprint>>, Result<Vec<StemFingerprint>>) =
        rayon::join(
        move || -> Result<Vec<Fingerprint>> {
            let fps = fingerprint_cached(&target, &cache_dir)?;
            notify_item(
                &on_target,
                &completed_target,
                total_audio,
                target_name,
                target_duration,
                0,
                item_total,
                true,
            );
            Ok(fps)
        },
        move || {
            fingerprint_stems_parallel_with_progress(
                &sources,
                &cache_dir_stems,
                on_stems.map(|cb| {
                    let cb = Arc::clone(&cb);
                    Arc::new(move |mut item: FingerprintItemDone| {
                        item.item_index += 1;
                        item.item_total = item_total;
                        item.total_audio_seconds = total_audio;
                        cb(item);
                    }) as Arc<dyn Fn(FingerprintItemDone) + Send + Sync>
                }),
                Some(completed_stems),
            )
        },
        );

    Ok((target_res?, stems_res?))
}
