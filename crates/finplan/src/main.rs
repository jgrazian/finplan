use clap::Parser;
use finplan::{App, init_logging};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "finplan")]
#[command(about = "A terminal-based financial planning simulator")]
struct Args {
    /// Path to the data directory (default: ~/.finplan/)
    #[arg(short, long)]
    data_dir: Option<PathBuf>,

    /// Log level (debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".finplan")
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = Args::parse();
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);

    init_logging(&data_dir, &args.log_level)?;

    let mut app = App::with_data_dir(data_dir);

    ratatui::run(|terminal| app.run(terminal))?;

    tracing::info!("Application shutting down");

    if let Err(err) = ratatui::try_restore() {
        tracing::error!("Failed to restore terminal: {err}");
    }

    Ok(())
}
