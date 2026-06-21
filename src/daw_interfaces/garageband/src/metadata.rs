use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::UNIX_EPOCH;

use crate::scanner::ScanResult;

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn project_manifest_hash(scan: &ScanResult) -> Result<String> {
    let mut assets = scan.audio_assets.clone();
    assets.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut lines = Vec::new();
    for asset in assets {
        let rel = asset.strip_prefix(&scan.project_path).unwrap_or(&asset);
        let meta = std::fs::metadata(&asset)?;
        lines.push(format!(
            "{}\t{}\t{}",
            rel.display(),
            meta.len(),
            sha256_file(&asset)?
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(lines.join("\n").as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn iso_mtime(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    let modified = meta.modified()?;
    let duration = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + duration);
    Ok(dt.to_rfc3339())
}

fn finder_comment(path: &Path) -> Option<String> {
    let hex_out = Command::new("xattr")
        .args([
            "-px",
            "com.apple.metadata:kMDItemFinderComment",
            &path.to_string_lossy(),
        ])
        .output()
        .ok()?;
    if !hex_out.status.success() {
        return None;
    }
    let hex = String::from_utf8_lossy(&hex_out.stdout)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    let raw = Command::new("xxd")
        .args(["-r", "-p"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    use std::io::Write;
    let mut child = raw;
    {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(hex.as_bytes()).ok()?;
    }
    let raw_out = child.wait_with_output().ok()?;
    if !raw_out.status.success() {
        return None;
    }
    let plutil = Command::new("plutil")
        .args(["-p", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut child = plutil;
    {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(&raw_out.stdout).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        Some(text[1..text.len() - 1].to_string())
    } else {
        Some(text)
    }
}

pub fn collect_metadata(
    scan: &ScanResult,
    target_path: &Path,
    asset_hashes: &HashMap<String, String>,
) -> Result<Vec<serde_json::Value>> {
    let mut evidence = Vec::new();
    evidence.push(serde_json::json!({
        "kind": "project_modified",
        "value": iso_mtime(&scan.project_path)?,
        "note": "GarageBand bundle last modified",
    }));
    evidence.push(serde_json::json!({
        "kind": "target_modified",
        "value": iso_mtime(target_path)?,
        "note": "Released audio last modified",
    }));
    if let Some(v) = &scan.garageband_version {
        evidence.push(serde_json::json!({
            "kind": "garageband_version",
            "value": v,
            "note": "From ProjectInformation.plist",
        }));
    }
    if let Some(v) = &scan.variant_name {
        evidence.push(serde_json::json!({
            "kind": "variant_name",
            "value": v,
            "note": "GarageBand project variant",
        }));
    }
    evidence.push(serde_json::json!({
        "kind": "project_asset_count",
        "value": scan.audio_assets.len(),
        "note": "Embedded recordings found",
    }));
    evidence.push(serde_json::json!({
        "kind": "registered_asset_count",
        "value": scan.registered_assets.len(),
        "note": "Assets listed in MetaData.plist",
    }));

    for sample in scan.audio_assets.iter().take(3) {
        if let Some(comment) = finder_comment(sample) {
            evidence.push(serde_json::json!({
                "kind": "garageband_xattr",
                "value": comment,
                "file": sample.file_name().unwrap().to_string_lossy(),
                "note": "macOS extended attribute on project recording",
            }));
            break;
        }
    }

    let mut names: Vec<_> = asset_hashes.keys().cloned().collect();
    names.sort();
    for name in names {
        let digest = asset_hashes.get(&name).cloned().unwrap_or_default();
        evidence.push(serde_json::json!({
            "kind": "asset_sha256",
            "file": name,
            "value": digest,
            "note": "Embedded recording hash",
        }));
    }

    Ok(evidence)
}
