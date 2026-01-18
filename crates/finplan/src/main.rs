use clap::Parser;
use finplan::App;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "finplan")]
#[command(about = "A terminal-based financial planning simulator")]
struct Args {
    /// Path to the configuration file
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".finplan.yaml")
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = Args::parse();
    let config_path = args.config.unwrap_or_else(default_config_path);

    let mut app = App::with_config_path(config_path);

    ratatui::run(|terminal| app.run(terminal))?;

    if let Err(err) = ratatui::try_restore() {
        eprintln!(
            "failed to restore terminal. Run `reset` or restart your terminal to recover: {err}"
        );
    }

    Ok(())
}
