use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "ariafin",
    version,
    about = "A polished terminal music player for Jellyfin & Navidrome"
)]
struct Args {
    /// Override the default config file location.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Log verbosity level (error, warn, info, debug, trace).
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    setup_logging(&args.log_level)?;

    tracing::info!("ariafin starting");

    // Load (or create) the config file.
    let config = af_core::config::loader::load_or_create()?;

    // Hand control to the TUI — this blocks until the user quits.
    af_tui::run(config).await?;

    tracing::info!("ariafin exiting cleanly");
    Ok(())
}

fn setup_logging(level: &str) -> Result<()> {
    use tracing_subscriber::{fmt, EnvFilter};

    // Write logs to a file so they never corrupt the ratatui terminal.
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ariafin");
    std::fs::create_dir_all(&log_dir)?;

    let log_path = log_dir.join("ariafin.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    fmt()
        .with_env_filter(EnvFilter::new(level))
        .with_writer(file)
        .with_ansi(false)
        .init();

    Ok(())
}
