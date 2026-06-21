use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::audio;
use crate::chroma;
use crate::normalize::{normalize_to_wav, TARGET_SR};
use garageband::{scan_project, ScanResult};

#[derive(Debug, Clone)]
pub struct ProjectChromas {
    pub name: String,
    pub path: PathBuf,
    pub asset_chromas: Vec<chroma::ChromaMatrix>,
}

pub fn load_project_chromas(
    project: &Path,
    temp_dir: &Path,
    label: &str,
) -> Result<ProjectChromas> {
    let scan = scan_project(project)?;
    if scan.audio_assets.is_empty() {
        anyhow::bail!(
            "No audio assets in {}",
            scan.project_path.join("Media/Audio Files").display()
        );
    }

    let project_temp = temp_dir.join(format!("catalog_{label}"));
    std::fs::create_dir_all(&project_temp)?;

    let mut asset_chromas = Vec::with_capacity(scan.audio_assets.len());
    for (idx, source) in scan.audio_assets.iter().enumerate() {
        let safe: String = source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let dest = project_temp.join(format!("asset_{idx:03}_{safe}.wav"));
        normalize_to_wav(source, &dest)?;
        let (samples, sr) = audio::load_mono_float(&dest)?;
        if sr != TARGET_SR {
            anyhow::bail!("unexpected sample rate after normalization: {sr}");
        }
        asset_chromas.push(chroma::chroma_matrix(&samples, sr));
    }

    Ok(ProjectChromas {
        name: scan.project_name.clone(),
        path: scan.project_path.clone(),
        asset_chromas,
    })
}

pub fn scan_claimed(project: &Path) -> Result<ScanResult> {
    scan_project(project).context("scan claimed project")
}
