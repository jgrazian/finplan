use clap::Parser;
use finplan::App;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "finplan")]
#[command(about = "A terminal-based financial planning simulator")]
struct Args {
    /// Path to the data directory (default: ~/.finplan/)
    #[arg(short, long)]
    data_dir: Option<PathBuf>,
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

    let mut app = App::with_data_dir(data_dir);

    ratatui::run(|terminal| app.run(terminal))?;

    if let Err(err) = ratatui::try_restore() {
        eprintln!(
            "failed to restore terminal. Run `reset` or restart your terminal to recover: {err}"
        );
    }

    Ok(())
}
