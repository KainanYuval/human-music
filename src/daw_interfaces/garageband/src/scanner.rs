use anyhow::{Context, Result};
use plist::Value;
use serde::Serialize;
use std::path::{Path, PathBuf};

const AUDIO_EXTENSIONS: &[&str] = &[".wav", ".aif", ".aiff", ".caf", ".m4a"];

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub project_path: PathBuf,
    pub project_name: String,
    pub is_package: bool,
    pub media_dir: Option<PathBuf>,
    pub audio_assets: Vec<PathBuf>,
    pub registered_assets: Vec<String>,
    pub garageband_version: Option<String>,
    pub variant_name: Option<String>,
    pub beats_per_minute: Option<f64>,
    pub sample_rate: Option<u64>,
}

fn read_plist(path: &Path) -> Option<plist::Dictionary> {
    let file = std::fs::File::open(path).ok()?;
    Value::from_reader(file)
        .ok()
        .and_then(|v| v.into_dictionary())
}

fn project_info(project_path: &Path) -> (Option<String>, Option<String>) {
    let info = read_plist(&project_path.join("Resources/ProjectInformation.plist"));
    let Some(dict) = info else {
        return (None, None);
    };
    let version = dict.get("LastSavedFrom").and_then(|v| v.as_string()).map(str::to_string);
    let variant = dict
        .get("VariantNamesV2")
        .or_else(|| dict.get("VariantNames"))
        .and_then(|v| v.as_dictionary())
        .and_then(|d| d.get("0"))
        .and_then(|v| v.as_string())
        .map(str::to_string);
    (version, variant)
}

fn metadata_assets(project_path: &Path) -> (Vec<String>, plist::Dictionary) {
    let meta_path = project_path.join("Alternatives/000/MetaData.plist");
    let dict = read_plist(&meta_path).unwrap_or_default();
    let registered = dict
        .get("AudioFiles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_string().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    (registered, dict)
}

pub fn scan_project(project_path: &Path) -> Result<ScanResult> {
    let project_path = project_path
        .canonicalize()
        .with_context(|| format!("resolve project path {}", project_path.display()))?;

    if !project_path.is_dir() || project_path.extension().and_then(|s| s.to_str()) != Some("band") {
        anyhow::bail!("Not a GarageBand package folder: {}", project_path.display());
    }

    let media_dir = project_path.join("Media/Audio Files");
    let mut audio_assets = Vec::new();
    if media_dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&media_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| {
                            let lower = format!(".{}", ext.to_ascii_lowercase());
                            AUDIO_EXTENSIONS.contains(&lower.as_str())
                        })
                        .unwrap_or(false)
            })
            .collect();
        entries.sort();
        audio_assets = entries;
    }

    let (registered, meta) = metadata_assets(&project_path);
    let (gb_version, variant) = project_info(&project_path);

    Ok(ScanResult {
        project_name: project_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        is_package: true,
        media_dir: media_dir.is_dir().then_some(media_dir),
        project_path,
        audio_assets,
        registered_assets: registered,
        garageband_version: gb_version,
        variant_name: variant,
        beats_per_minute: meta.get("BeatsPerMinute").and_then(|v| v.as_real()),
        sample_rate: meta.get("SampleRate").and_then(|v| v.as_unsigned_integer()),
    })
}

#[derive(Serialize)]
pub struct ScanSummary<'a> {
    pub project_name: &'a str,
    pub project_path: String,
    pub is_package: bool,
    pub media_dir: Option<String>,
    pub audio_assets: Vec<String>,
    pub registered_assets: &'a [String],
    pub garageband_version: Option<&'a str>,
    pub variant_name: Option<&'a str>,
}

pub fn scan_summary(scan: &ScanResult) -> serde_json::Value {
    serde_json::to_value(ScanSummary {
        project_name: &scan.project_name,
        project_path: scan.project_path.display().to_string(),
        is_package: scan.is_package,
        media_dir: scan.media_dir.as_ref().map(|p| p.display().to_string()),
        audio_assets: scan
            .audio_assets
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect(),
        registered_assets: &scan.registered_assets,
        garageband_version: scan.garageband_version.as_deref(),
        variant_name: scan.variant_name.as_deref(),
    })
    .unwrap_or(serde_json::json!({}))
}
