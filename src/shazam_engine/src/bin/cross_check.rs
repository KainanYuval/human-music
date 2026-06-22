use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use shazam_engine::cache::default_cache_dir;
use shazam_engine::{build_index, fingerprint_file_cached, match_landmarks, FingerprintIndex};

const EXAMPLES: [&str; 3] = ["example_1", "example_2", "example_3"];
const STRONG: f32 = 0.5;
const POSSIBLE: f32 = 0.3;

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn find_band_dir(example: &Path) -> PathBuf {
    std::fs::read_dir(example)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.join("Media/Audio Files").is_dir())
        .unwrap()
}

fn find_release_wav(example: &Path) -> PathBuf {
    std::fs::read_dir(example)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("wav"))
        .unwrap()
}

#[derive(Serialize)]
struct CellTopStem {
    asset: String,
    query_fps: usize,
    score: f32,
    aligned: usize,
    peak_votes: usize,
    offset_s: f32,
}

#[derive(Serialize)]
struct MatrixCell {
    band: String,
    release: String,
    correct_pair: bool,
    stem_count: usize,
    strong_gte_0_5: usize,
    possible_gte_0_3: usize,
    best_score: f32,
    mean_score: f32,
    median_score: f32,
    match_elapsed_s: f64,
    top_stems: Vec<CellTopStem>,
}

#[derive(Serialize)]
struct Report {
    generated_at: String,
    algorithm: &'static str,
    engine: &'static str,
    peak_percentile: f32,
    fingerprint_cache: String,
    precompute_elapsed_s: f64,
    match_elapsed_s: f64,
    total_elapsed_s: f64,
    matrix: Vec<MatrixCell>,
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn main() -> anyhow::Result<()> {
    let cache_dir = default_cache_dir();
    let t_all = Instant::now();

    let mut bands = Vec::new();
    let mut releases = Vec::new();
    let mut all_stems: Vec<PathBuf> = Vec::new();

    for ex in EXAMPLES {
        let dir = data_root().join(ex);
        let band = find_band_dir(&dir);
        let stems: Vec<PathBuf> = std::fs::read_dir(band.join("Media/Audio Files"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wav"))
            .collect();
        all_stems.extend(stems.iter().cloned());
        bands.push((ex.to_string(), band, stems));
        releases.push((ex.to_string(), find_release_wav(&dir)));
    }

    println!("Precompute fingerprints (cache: {})...", cache_dir.display());
    let t_pre = Instant::now();
    let mut cached = 0usize;
    let mut computed = 0usize;

    let mut release_fps = Vec::new();
    for (ex, path) in &releases {
        let (fps, from_cache) = fingerprint_file_cached(path, &cache_dir)?;
        cached += from_cache as usize;
        computed += (!from_cache) as usize;
        println!("  release {ex}: {} fps", fps.len());
        release_fps.push((ex.clone(), fps));
    }

    let mut stem_fps_map = std::collections::HashMap::new();
    for stem in &all_stems {
        let (fps, from_cache) = fingerprint_file_cached(stem, &cache_dir)?;
        cached += from_cache as usize;
        computed += (!from_cache) as usize;
        stem_fps_map.insert(stem.clone(), fps);
    }
    println!(
        "  stems: {} files ({} cached, {} computed)",
        all_stems.len(),
        cached,
        computed
    );

    let mut release_idx: Vec<(String, FingerprintIndex)> = release_fps
        .into_iter()
        .map(|(ex, fps)| (ex, build_index(&fps)))
        .collect();

    let precompute_s = t_pre.elapsed().as_secs_f64();
    let t_match = Instant::now();
    let mut matrix = Vec::new();

    for (band_ex, _band_dir, stems) in &bands {
        println!("\n{band_ex} band ({} stems):", stems.len());
        for (release_ex, _) in &releases {
            let rel_idx = release_idx
                .iter()
                .find(|(ex, _)| ex == release_ex)
                .map(|(_, idx)| idx)
                .unwrap();

            let t0 = Instant::now();
            let mut per_stem = Vec::new();
            for stem in stems {
                let stem_fps = stem_fps_map.get(stem).unwrap();
                let m = match_landmarks(stem_fps, rel_idx);
                per_stem.push(CellTopStem {
                    asset: stem.file_name().unwrap().to_string_lossy().to_string(),
                    query_fps: m.query_hashes,
                    score: (m.score * 10000.0).round() / 10000.0,
                    aligned: m.aligned,
                    peak_votes: m.peak_votes,
                    offset_s: (m.offset_s * 1000.0).round() / 1000.0,
                });
            }

            let mut scores: Vec<f32> = per_stem.iter().map(|s| s.score).collect();
            let strong = scores.iter().filter(|&&s| s >= STRONG).count();
            let poss = scores.iter().filter(|&&s| s >= POSSIBLE).count();
            let best = scores.iter().copied().fold(0.0f32, f32::max);
            let mean_s = mean(&scores);
            let med = median(&mut scores);
            let correct = band_ex == release_ex;

            per_stem.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
            let elapsed = t0.elapsed().as_secs_f64();

            matrix.push(MatrixCell {
                band: band_ex.clone(),
                release: release_ex.clone(),
                correct_pair: correct,
                stem_count: stems.len(),
                strong_gte_0_5: strong,
                possible_gte_0_3: poss,
                best_score: (best * 10000.0).round() / 10000.0,
                mean_score: (mean_s * 10000.0).round() / 10000.0,
                median_score: (med * 10000.0).round() / 10000.0,
                match_elapsed_s: (elapsed * 100.0).round() / 100.0,
                top_stems: per_stem.into_iter().take(5).collect(),
            });

            let tag = if correct { "OK" } else { "X " };
            println!(
                "  {tag} + {release_ex}: strong={strong}/{} best={best:.3} mean={mean_s:.3} ({elapsed:.2}s)",
                stems.len()
            );
        }
    }

    let match_s = t_match.elapsed().as_secs_f64();
    let total_s = t_all.elapsed().as_secs_f64();

    let report = Report {
        generated_at: chrono_lite_now(),
        algorithm: "shazam_landmark_voting",
        engine: "shazam_engine/rust",
        peak_percentile: 99.9,
        fingerprint_cache: cache_dir
            .strip_prefix(data_root().join(".."))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| cache_dir.display().to_string()),
        precompute_elapsed_s: (precompute_s * 100.0).round() / 100.0,
        match_elapsed_s: (match_s * 100.0).round() / 100.0,
        total_elapsed_s: (total_s * 100.0).round() / 100.0,
        matrix,
    };

    let out = data_root().join("_generated/shazam_cross_check_rs.json");
    std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
    println!(
        "\nWrote {}\n  precompute: {:.2}s  match: {:.2}s  total: {:.2}s",
        out.display(),
        precompute_s,
        match_s,
        total_s
    );

    Ok(())
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{secs}")
}
