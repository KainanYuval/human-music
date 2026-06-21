use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use verification_engine::config::VerifyConfig;
use verification_engine::progress::ProgressEvent;
use verification_engine::{run_verify_with_options, VerifyOptions};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

#[derive(Serialize)]
struct VerifyResult {
    verdict: String,
    provenance_score: f64,
    coverage_ratio: f64,
    claim: String,
    best_match: Option<serde_json::Value>,
    strong_match_count: u32,
    possible_match_count: u32,
    report_dir: String,
    report_json: String,
    report_html: String,
}

fn catalog_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../data")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data"))
}

fn verify_options() -> VerifyOptions {
    let config: VerifyConfig =
        toml::from_str(verification_engine::config::default_config_toml()).unwrap_or_default();
    VerifyOptions {
        config,
        catalog_dir: Some(catalog_dir()),
    }
}

fn report_output_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gb-verify-{stamp}"))
}

fn file_path_to_string(path: FilePath) -> String {
    match path {
        FilePath::Path(p) => p.to_string_lossy().into_owned(),
        FilePath::Url(url) => url.to_string(),
    }
}

async fn pick_folder(app: &AppHandle, title: &str) -> Result<Option<String>, String> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });
    rx.await
        .map_err(|_| "Folder picker closed".to_string())
        .map(|path| path.map(file_path_to_string))
}

async fn pick_file(app: &AppHandle, title: &str) -> Result<Option<String>, String> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter("Audio", &["wav", "mp3", "m4a", "aiff", "flac"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    rx.await
        .map_err(|_| "File picker closed".to_string())
        .map(|path| path.map(file_path_to_string))
}

#[tauri::command]
async fn pick_band_project(app: AppHandle) -> Result<Option<String>, String> {
    pick_folder(&app, "Select GarageBand project (.band folder)").await
}

#[tauri::command]
async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    pick_file(&app, "Select released audio (WAV or MP3)").await
}

fn payload_to_result(payload: serde_json::Value, out_dir: &PathBuf) -> VerifyResult {
    let summary = payload
        .get("summary")
        .cloned()
        .unwrap_or(serde_json::json!({}));

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
        coverage_ratio: payload
            .get("coverage")
            .and_then(|c| c.get("ratio"))
            .and_then(|v| v.as_f64())
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
    let payload = tokio::task::spawn_blocking(move || {
        let mut progress_cb = move |evt: ProgressEvent| {
            let _ = app_for_progress.emit("verify-progress", &evt);
        };
        run_verify_with_options(
            &project_path,
            &audio_path,
            &out_for_task,
            options,
            Some(&mut progress_cb),
        )
    })
    .await
    .map_err(|e| format!("Verification task failed: {e}"))?
    .map_err(|e| e.to_string())?;

    Ok(payload_to_result(payload, &out_dir))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            pick_band_project,
            pick_audio_file,
            run_verification
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
