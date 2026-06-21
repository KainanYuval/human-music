use anyhow::{Context, Result};
use clap::Parser;
use verification_engine::config::VerifyConfig;
use verification_engine::progress::{write_jsonl_progress, ProgressEvent};
use verification_engine::{run_verify_with_options, scan_only, VerifyOptions, VERSION};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gb-verify", version = VERSION, about = "GarageBand project ↔ released audio provenance verifier")]
struct Cli {
    #[arg(long)]
    project: PathBuf,

    #[arg(long)]
    audio: PathBuf,

    #[arg(long)]
    out: PathBuf,

    #[arg(long)]
    scan_only: bool,

    /// Emit structured progress events as JSON lines on stderr
    #[arg(long)]
    progress_jsonl: bool,

    /// Directory tree to scan for rival `.band` projects (required for discrimination)
    #[arg(long)]
    catalog_dir: Option<PathBuf>,

    /// TOML config for matching / coverage / discrimination thresholds
    #[arg(long)]
    config: Option<PathBuf>,
}

fn load_config(path: Option<&PathBuf>) -> Result<VerifyConfig> {
    match path {
        Some(p) => VerifyConfig::from_toml_path(p),
        None => Ok(toml::from_str(verification_engine::config::default_config_toml())?),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.audio.is_file() {
        anyhow::bail!("Audio file not found: {}", cli.audio.display());
    }

    if cli.scan_only {
        let summary = scan_only(&cli.project)?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    let config = load_config(cli.config.as_ref()).context("load config")?;
    let options = VerifyOptions {
        config,
        catalog_dir: cli.catalog_dir,
    };

    let payload = if cli.progress_jsonl {
        let mut progress_cb = |evt: ProgressEvent| {
            let _ = write_jsonl_progress(&evt);
        };
        run_verify_with_options(
            &cli.project,
            &cli.audio,
            &cli.out,
            options,
            Some(&mut progress_cb),
        )?
    } else {
        run_verify_with_options(&cli.project, &cli.audio, &cli.out, options, None)?
    };

    let verdict = payload["verdict"].as_str().unwrap_or("FAIL");
    let score = payload["provenance_score"].as_f64().unwrap_or(0.0) * 100.0;
    println!("Verdict: {verdict}");
    println!("Production Provenance: {score:.1}%");
    if let Some(win) = payload["timeline_coverage"]["competitive_win_rate"].as_f64() {
        println!("Competitive win rate: {:.1}%", win * 100.0);
    }
    if let Some(adv) = payload["timeline_coverage"]["exclusive_advantage"].as_f64() {
        println!("Exclusive advantage: {:.4}", adv);
    }
    if let Some(best) = payload["summary"]["best_match"].as_object() {
        println!(
            "Best match: {} @ {:.2}s (score {:.4})",
            best["asset"].as_str().unwrap_or(""),
            best["offset_seconds"].as_f64().unwrap_or(0.0),
            best["match_score"].as_f64().unwrap_or(0.0),
        );
    }
    println!("Reports written to {}", cli.out.display());

    if verdict != "PASS" {
        std::process::exit(2);
    }
    Ok(())
}
