//! `finplan-server` entry point.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use finplan_server::config::{LogFormat, ServerConfig};
use finplan_server::observability::{Component, ErrorClass, ObservabilityRuntime};
use tracing_subscriber::EnvFilter;

const HTTP_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

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
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    init_logging(&cli.config);
    // The default panic hook prints arbitrary engine/user payloads to stderr,
    // even when spawn_blocking catches the panic. Preserve safe source location.
    std::panic::set_hook(Box::new(|info| {
        let location = info.location();
        tracing::error!(
            event = "server.panic",
            class = "task_panic",
            file = location.map(|l| l.file()),
            line = location.map(|l| l.line())
        );
    }));
    match run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            // Third-party errors may include database URLs, SQL values, or secrets.
            tracing::error!(
                event = "server.failed",
                class = "startup_or_service_failure"
            );
            std::process::ExitCode::FAILURE
        }
    }
}

fn init_logging(config: &ServerConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,finplan_server=info"));
    let json = matches!(config.log_format, LogFormat::Json)
        || (matches!(config.log_format, LogFormat::Auto) && config.hosted);
    if json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.command {
        Some(Command::Migrate) => {
            let db = finplan_server::db::connect(&cli.config.database_url, 1)
                .await
                .inspect_err(|_| {
                    tracing::error!(
                        event = "server.initialization_failed",
                        phase = "migration",
                        class = "database"
                    );
                })?;
            db.close().await;
            tracing::info!(event = "server.migrated");
            return Ok(());
        }
        Some(Command::RebuildDatabase {
            source,
            destination,
        }) => {
            let report = finplan_server::db::rebuild(source, destination)
                .await
                .inspect_err(|_| {
                    tracing::error!(
                        event = "server.initialization_failed",
                        phase = "database_rebuild",
                        class = "database"
                    );
                })?;
            tracing::info!(
                event = "server.database_rebuilt",
                tables = report.tables,
                rows = report.rows,
                "database rebuilt and verified"
            );
            return Ok(());
        }
        _ => {}
    }

    tracing::info!(
        event = "server.starting",
        workers = cli.config.sim_workers,
        hosted = cli.config.hosted,
        version = env!("CARGO_PKG_VERSION")
    );
    let bind = cli.config.bind.clone();
    let (router, state) = finplan_server::build(cli.config).await?;
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .inspect_err(|error| {
            tracing::error!(event = "server.listener_failed", listener = "api", address = %bind,
            class = ?error.kind(), os_error = error.raw_os_error());
        })?;
    let mut runtime = ObservabilityRuntime::start(&state).await?;
    tracing::info!(event = "server.ready", address = %listener.local_addr()?,
        metrics_address = ?runtime.metrics_address());
    let (stop, mut receive) = tokio::sync::watch::channel(false);
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = receive.changed().await;
    });
    let server = std::future::IntoFuture::into_future(server);
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result,
        failure = runtime.wait_for_failure() => {
            state.telemetry.error(Component::Server, ErrorClass::Unavailable);
            let _ = stop.send(true);
            let _ = drain_server(&mut server, HTTP_DRAIN_TIMEOUT).await;
            // The supervision failure remains the exit reason even if draining
            // succeeds or independently times out.
            Err(failure)
        },
        () = shutdown_signal() => {
            let _ = stop.send(true);
            drain_server(&mut server, HTTP_DRAIN_TIMEOUT).await
        }
    };
    runtime.shutdown().await;
    tracing::info!(event = "server.shutdown");
    result?;
    Ok(())
}

/// An incomplete request body must not prevent observability shutdown forever.
/// Return an error on timeout, then let run() finish all managed cleanup before
/// the process runtime drops the remaining connection tasks.
async fn drain_server(
    server: impl Future<Output = std::io::Result<()>>,
    timeout: Duration,
) -> std::io::Result<()> {
    match tokio::time::timeout(timeout, server).await {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                event = "server.shutdown_timeout",
                listener = "api",
                timeout_seconds = timeout.as_secs_f64()
            );
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "public HTTP listener did not drain before its shutdown deadline",
            ))
        }
    }
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

    tracing::info!(event = "server.shutdown_requested");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn stalled_drain_times_out_and_releases_the_draining_future() {
        struct Release(Arc<AtomicBool>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let released = Arc::new(AtomicBool::new(false));
        let guard = Release(released.clone());
        let stalled = async move {
            let _guard = guard;
            std::future::pending::<std::io::Result<()>>().await
        };
        let result = drain_server(stalled, Duration::ZERO).await;
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(released.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn drain_preserves_success_and_service_failure() {
        assert!(
            drain_server(async { Ok(()) }, HTTP_DRAIN_TIMEOUT)
                .await
                .is_ok()
        );
        let result = drain_server(
            async { Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)) },
            HTTP_DRAIN_TIMEOUT,
        )
        .await;
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::ConnectionAborted
        );
    }
}
