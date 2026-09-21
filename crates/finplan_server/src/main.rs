//! `finplan-server` entry point.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use finplan_server::config::ServerConfig;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "finplan-server",
    version,
    about = "FinPlan simulation API server"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    config: ServerConfig,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (default).
    Serve,
    /// Apply pending migrations and exit.
    Migrate,
    /// Copy a legacy database into the current consolidated schema.
    RebuildDatabase {
        /// Existing database to read without modifying.
        #[arg(long)]
        source: PathBuf,
        /// New database to create. Must not already exist.
        #[arg(long)]
        destination: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("finplan_server=info,tower_http=info,warn")),
        )
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Migrate) => {
            let db = finplan_server::db::connect(&cli.config.database_url, 1).await?;
            db.close().await;
            tracing::info!("migrations applied");
            return Ok(());
        }
        Some(Command::RebuildDatabase {
            source,
            destination,
        }) => {
            let report = finplan_server::db::rebuild(source, destination).await?;
            tracing::info!(
                source = %source.display(),
                destination = %destination.display(),
                tables = report.tables,
                rows = report.rows,
                "database rebuilt and verified"
            );
            return Ok(());
        }
        _ => {}
    }

    let bind = cli.config.bind.clone();
    let (router, _state) = finplan_server::build(cli.config).await?;

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(address = %listener.local_addr()?, "finplan-server listening");

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

/// Stop accepting connections on Ctrl-C or SIGTERM. In-flight simulation runs
/// stay `running` in the database and are re-queued by the next process.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
