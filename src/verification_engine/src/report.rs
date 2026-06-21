use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

use crate::config::VerifyConfig;
use crate::matcher::MatchResult;
use garageband::ScanResult;
use crate::verdict::VerdictResult;
use crate::VERSION;

fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn match_json(m: &MatchResult) -> serde_json::Value {
    serde_json::json!({
        "asset": m.asset,
        "pearson": round6(m.pearson),
        "spectral_pearson": round6(m.spectral_pearson),
        "chroma_pearson": round6(m.chroma_pearson),
        "match_score": round6(m.match_score),
        "mse": round6(m.mse),
        "gain": round6(m.gain),
        "offset_seconds": round4(m.offset_seconds),
        "coverage_seconds": round4(m.coverage_seconds),
        "asset_duration_seconds": round4(m.asset_duration_seconds),
        "status": m.status,
        "match_mode": m.match_mode,
    })
}

pub fn build_report_payload(
    scan: &ScanResult,
    target_path: &Path,
    matches: &[MatchResult],
    verdict: &VerdictResult,
    timeline_coverage: serde_json::Value,
    metadata_evidence: Vec<serde_json::Value>,
    target_sha256: String,
    project_sha256: String,
    asset_hashes: HashMap<String, String>,
    config: &VerifyConfig,
) -> Result<serde_json::Value> {
    let claim = if config.discrimination.enabled {
        "This released audio is uniquely explained by this GarageBand project versus rival projects in the catalog."
    } else {
        "This released audio is strongly explained by recordings found inside this GarageBand project."
    };

    let unused: Vec<&str> = matches
        .iter()
        .filter(|m| m.status == "no_match")
        .map(|m| m.asset.as_str())
        .collect();

    let mut payload = serde_json::json!({
        "verifier_version": VERSION,
        "generated_at": Utc::now().to_rfc3339(),
        "claim": claim,
        "verify_config": config,
        "verdict": verdict.verdict,
        "provenance_score": verdict.provenance_score,
        "coverage": {
            "matched_seconds": verdict.matched_coverage_seconds,
            "target_seconds": verdict.target_duration_seconds,
            "ratio": verdict.coverage_ratio,
        },
        "timeline_coverage": timeline_coverage,
        "project_name": scan.project_name,
        "project_path": scan.project_path.display().to_string(),
        "target_file": target_path.display().to_string(),
        "target_sha256": target_sha256,
        "project_sha256": project_sha256,
        "asset_hashes": asset_hashes,
        "matches": matches.iter().map(match_json).collect::<Vec<_>>(),
        "unused_assets": unused,
        "metadata_evidence": metadata_evidence,
        "summary": {
            "strong_match_count": verdict.strong_match_count,
            "possible_match_count": verdict.possible_match_count,
            "best_match": verdict.best_match.as_ref().map(match_json),
        },
    });

    let hash_input = serde_json::to_string(&payload)?;
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let report_hash = format!("{:x}", hasher.finalize());
    payload
        .as_object_mut()
        .unwrap()
        .insert("report_sha256".to_string(), serde_json::Value::String(report_hash));

    Ok(payload)
}

pub fn write_json_report(payload: &serde_json::Value, out_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let text = serde_json::to_string_pretty(payload)?;
    std::fs::write(out_dir.join("report.json"), text)?;
    Ok(())
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn write_html_report(payload: &serde_json::Value, out_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let verdict = payload["verdict"].as_str().unwrap_or("FAIL");
    let score_pct = (payload["provenance_score"].as_f64().unwrap_or(0.0) * 100.0).round() as i64;
    let best = &payload["summary"]["best_match"];

    let mut strong_lines = Vec::new();
    if payload["matches"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        strong_lines.push("✓ Recordings found in project");
    }
    if payload["matches"]
        .as_array()
        .map(|arr| arr.iter().any(|m| m["status"] == "strong_match"))
        .unwrap_or(false)
    {
        strong_lines.push("✓ Recording matches final song");
        strong_lines.push("✓ Gain-normalized waveform match");
    }
    if payload["metadata_evidence"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        strong_lines.push("✓ Export timestamps / metadata collected");
    }

    let mut match_rows = String::new();
    if let Some(arr) = payload["matches"].as_array() {
        for m in arr {
            if m["status"] == "no_match" {
                continue;
            }
            match_rows.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{:.4}</td><td>{:.4}</td><td>{:.4}</td><td>{:.4}</td><td>{:.2}s</td><td>{:.2}s</td></tr>",
                html_escape(m["asset"].as_str().unwrap_or("")),
                m["status"].as_str().unwrap_or(""),
                m["match_score"].as_f64().unwrap_or(0.0),
                m["pearson"].as_f64().unwrap_or(0.0),
                m["spectral_pearson"].as_f64().unwrap_or(0.0),
                m["chroma_pearson"].as_f64().unwrap_or(0.0),
                m["offset_seconds"].as_f64().unwrap_or(0.0),
                m["coverage_seconds"].as_f64().unwrap_or(0.0),
            ));
        }
    }
    if match_rows.is_empty() {
        match_rows = "<tr><td colspan=\"8\">No matches above threshold</td></tr>".to_string();
    }

    let unused_html = payload["unused_assets"]
        .as_array()
        .map(|arr| {
            if arr.is_empty() {
                "<li>None</li>".to_string()
            } else {
                arr.iter()
                    .map(|u| format!("<li>{}</li>", html_escape(u.as_str().unwrap_or(""))))
                    .collect::<String>()
            }
        })
        .unwrap_or_else(|| "<li>None</li>".to_string());

    let best_html = if !best.is_null() {
        format!(
            "<p><strong>Best match:</strong> {} @ {:.2}s (score {:.4})</p>",
            html_escape(best["asset"].as_str().unwrap_or("")),
            best["offset_seconds"].as_f64().unwrap_or(0.0),
            best["match_score"].as_f64().unwrap_or(0.0),
        )
    } else {
        String::new()
    };

    let pass_class = if verdict == "PASS" { "pass" } else { "fail" };
    let strong_ul = strong_lines
        .iter()
        .map(|l| format!("<li>{}</li>", html_escape(l)))
        .collect::<String>();

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>GarageBand Verification Report</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, sans-serif; margin: 2rem; max-width: 960px; }}
    h1 {{ margin-bottom: 0.2rem; }}
    .verdict {{ font-size: 1.4rem; font-weight: 700; }}
    .pass {{ color: #0a7a2f; }}
    .fail {{ color: #b00020; }}
    table {{ border-collapse: collapse; width: 100%; margin-top: 1rem; }}
    th, td {{ border: 1px solid #ddd; padding: 0.5rem; text-align: left; }}
    th {{ background: #f5f5f5; }}
    ul {{ line-height: 1.6; }}
    .meta {{ color: #555; font-size: 0.9rem; }}
  </style>
</head>
<body>
  <h1>GarageBand Verification Report</h1>
  <p class="verdict {pass_class}">Verdict: {verdict}</p>
  <p>Production Provenance: {score_pct}%</p>
  {best_html}
  <h2>Strong Evidence</h2>
  <ul>{strong_ul}</ul>
  <h2>Matched Assets</h2>
  <table>
    <thead><tr><th>Asset</th><th>Status</th><th>Score</th><th>Wave</th><th>Spectral</th><th>Chroma</th><th>Offset</th><th>Coverage</th></tr></thead>
    <tbody>{match_rows}</tbody>
  </table>
  <h2>Unused Project Assets</h2>
  <ul>{unused_html}</ul>
  <h2>Hashes</h2>
  <p class="meta">Target SHA256: {target_sha256}</p>
  <p class="meta">Project manifest SHA256: {project_sha256}</p>
  <p class="meta">Report SHA256: {report_sha256}</p>
  <p class="meta">Verifier v{version} (Rust)</p>
</body>
</html>
"#,
        pass_class = pass_class,
        verdict = verdict,
        score_pct = score_pct,
        best_html = best_html,
        strong_ul = strong_ul,
        match_rows = match_rows,
        unused_html = unused_html,
        target_sha256 = payload["target_sha256"].as_str().unwrap_or(""),
        project_sha256 = payload["project_sha256"].as_str().unwrap_or(""),
        report_sha256 = payload["report_sha256"].as_str().unwrap_or(""),
        version = VERSION,
    );

    std::fs::write(out_dir.join("report.html"), html)?;
    Ok(())
}
