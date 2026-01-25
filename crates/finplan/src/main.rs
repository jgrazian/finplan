#[cfg(feature = "native")]
use clap::Parser;
#[cfg(feature = "native")]
use finplan::{App, init_logging};
#[cfg(feature = "native")]
use std::path::PathBuf;

#[cfg(feature = "native")]
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

#[cfg(feature = "native")]
fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".finplan")
}

#[cfg(feature = "native")]
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

#[cfg(not(feature = "native"))]
fn main() {
    // Web entry point is handled via wasm_bindgen in lib.rs
    // This main() exists only to satisfy the binary target requirement
    panic!(
        "This binary requires the 'native' feature. For web, use trunk to build the WASM target."
    );
}
