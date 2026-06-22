use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use verification_engine::config::VerifyConfig;
use verification_engine::progress::ProgressEvent;
use verification_engine::{run_verify_with_options, VerifyOptions};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::task;

#[derive(Serialize)]
struct VerifyResult {
    verdict: String,
    provenance_score: f64,
    coverage_ratio: f64,
    timeline_explained_seconds: f64,
    timeline_target_seconds: f64,
    claim: String,
    best_match: Option<serde_json::Value>,
    strong_match_count: u32,
    possible_match_count: u32,
    report_dir: String,
    report_json: String,
    report_html: String,
}

#[derive(Serialize)]
struct PublishResult {
    id: String,
    url: String,
    qr_url: String,
}

fn registry_url() -> String {
    std::env::var("REGISTRY_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
}

fn publish_api_key() -> String {
    std::env::var("PUBLISH_API_KEY").unwrap_or_else(|_| "dev-change-me".to_string())
}

fn verify_options() -> VerifyOptions {
    let config: VerifyConfig =
        toml::from_str(verification_engine::config::default_config_toml()).unwrap_or_default();
    VerifyOptions { config }
}

fn report_output_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("human-music-{stamp}"))
}

fn payload_to_result(payload: serde_json::Value, out_dir: &PathBuf) -> VerifyResult {
    let summary = payload
        .get("summary")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let timeline = payload
        .get("timeline_coverage")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let coverage = payload.get("coverage").cloned().unwrap_or(serde_json::json!({}));

    let timeline_ratio = timeline
        .get("coverage_ratio")
        .and_then(|v| v.as_f64())
        .or_else(|| coverage.get("ratio").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);

    VerifyResult {
        verdict: payload
            .get("verdict")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN")
            .to_string(),
        provenance_score: payload
            .get("provenance_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        coverage_ratio: timeline_ratio,
        timeline_explained_seconds: timeline
            .get("explained_seconds")
            .and_then(|v| v.as_f64())
            .or_else(|| coverage.get("matched_seconds").and_then(|v| v.as_f64()))
            .unwrap_or(0.0),
        timeline_target_seconds: timeline
            .get("target_seconds")
            .and_then(|v| v.as_f64())
            .or_else(|| coverage.get("target_seconds").and_then(|v| v.as_f64()))
            .unwrap_or(0.0),
        claim: payload
            .get("claim")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        best_match: summary.get("best_match").cloned(),
        strong_match_count: summary
            .get("strong_match_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        possible_match_count: summary
            .get("possible_match_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        report_dir: out_dir.to_string_lossy().into_owned(),
        report_json: out_dir
            .join("report.json")
            .to_string_lossy()
            .into_owned(),
        report_html: out_dir
            .join("report.html")
            .to_string_lossy()
            .into_owned(),
    }
}

#[tauri::command]
async fn run_verification(
    app: AppHandle,
    project: String,
    audio: String,
) -> Result<VerifyResult, String> {
    let project_path = PathBuf::from(&project);
    if !project_path.exists() {
        return Err(format!("GarageBand project not found: {project}"));
    }
    if project_path.extension().and_then(|s| s.to_str()) != Some("band") {
        return Err(format!("Expected a .band GarageBand project: {project}"));
    }

    let audio_path = PathBuf::from(&audio);
    if !audio_path.is_file() {
        return Err(format!("Audio file not found: {audio}"));
    }

    let out_dir = report_output_dir();
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let app_for_progress = app.clone();
    let out_for_task = out_dir.clone();

    let options = verify_options();
    let payload = task::spawn_blocking(move || {
        run_verify_with_options(
            &project_path,
            &audio_path,
            &out_for_task,
            options,
            Some(move |evt: ProgressEvent| {
                let _ = app_for_progress.emit("verify-progress", &evt);
            }),
        )
    })
    .await
    .map_err(|e| format!("Verification task failed: {e}"))?
    .map_err(|e| e.to_string())?;

    Ok(payload_to_result(payload, &out_dir))
}

#[tauri::command]
async fn publish_verification(
    report_json: String,
    artist_name: String,
    song_title: String,
) -> Result<PublishResult, String> {
    let artist = artist_name.trim();
    let title = song_title.trim();
    if artist.is_empty() {
        return Err("Artist name is required".to_string());
    }
    if title.is_empty() {
        return Err("Song title is required".to_string());
    }

    let report_path = PathBuf::from(&report_json);
    if !report_path.is_file() {
        return Err(format!("Report not found: {report_json}"));
    }

    let raw = std::fs::read_to_string(&report_path).map_err(|e| e.to_string())?;
    let report: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid report JSON: {e}"))?;

    let verdict = report
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if verdict != "PASS" {
        return Err("Only PASS verifications can be published".to_string());
    }

    let base = registry_url().trim_end_matches('/').to_string();
    let endpoint = format!("{base}/api/publish");
    let client = reqwest::Client::new();
    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("X-Publish-Key", publish_api_key())
        .json(&serde_json::json!({
            "artist_name": artist,
            "song_title": title,
            "report": report,
        }))
        .send()
        .await
        .map_err(|e| format!("Publish request failed: {e}"))?;

    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Invalid publish response: {e}"))?;

    if !status.is_success() {
        let message = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Publish failed");
        return Err(format!("{message} (HTTP {status})"));
    }

    Ok(PublishResult {
        id: body
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        url: body
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        qr_url: body
            .get("qr_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![run_verification, publish_verification])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
