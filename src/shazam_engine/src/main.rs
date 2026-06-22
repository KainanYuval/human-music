use std::path::{Path, PathBuf};
use std::time::Instant;

use shazam_engine::{build_index, fingerprint_file, match_landmarks};

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
        .find(|p| {
            p.is_file()
                && p.extension().and_then(|s| s.to_str()) == Some("wav")
        })
        .unwrap()
}

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/example_1");
    let release = find_release_wav(&root);
    let band = find_band_dir(&root);
    let stems: Vec<PathBuf> = std::fs::read_dir(band.join("Media/Audio Files"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("wav"))
        .collect();

    println!("shazam-bench: example_1 release vs {} stems\n", stems.len());

    let t0 = Instant::now();
    let release_fps = fingerprint_file(&release)?;
    let fp_release_s = t0.elapsed().as_secs_f64();

    let t0 = Instant::now();
    let index = build_index(&release_fps);
    let index_s = t0.elapsed().as_secs_f64();

    println!(
        "release: {}  fingerprints={}  fp={:.3}s  index={:.3}s",
        release.file_name().unwrap().to_string_lossy(),
        release_fps.len(),
        fp_release_s,
        index_s
    );

    let mut fp_stems_s = 0.0;
    let mut match_s = 0.0;
    let mut results = Vec::new();

    for stem in &stems {
        let t0 = Instant::now();
        let stem_fps = fingerprint_file(stem)?;
        fp_stems_s += t0.elapsed().as_secs_f64();

        let t0 = Instant::now();
        let m = match_landmarks(&stem_fps, &index);
        match_s += t0.elapsed().as_secs_f64();

        results.push((stem.file_name().unwrap().to_string_lossy().to_string(), m));
    }

    results.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap());

    println!("\nstems: fp_total={:.3}s  match_total={:.3}s", fp_stems_s, match_s);
    println!(
        "TOTAL: {:.3}s\n",
        fp_release_s + index_s + fp_stems_s + match_s
    );

    println!("Top 5:");
    for (name, m) in results.iter().take(5) {
        println!(
            "  {:.4}  {}  aligned={}/{} offset={:.2}s",
            m.score, name, m.aligned, m.query_hashes, m.offset_s
        );
    }

    Ok(())
}
