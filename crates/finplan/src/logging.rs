use std::path::Path;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize logging to write to a file in the data directory.
///
/// Logs are written to `{data_dir}/finplan.log` with daily rotation.
/// The log level can be controlled via the `level` parameter or the `RUST_LOG` environment variable.
pub fn init_logging(data_dir: &Path, level: &str) -> color_eyre::Result<()> {
    // Ensure data directory exists
    std::fs::create_dir_all(data_dir)?;

    // Create rolling file appender with daily rotation
    let file_appender = RollingFileAppender::new(Rotation::DAILY, data_dir, "finplan.log");

    // Build filter from RUST_LOG env var or use provided level
    let default_filter = format!("finplan={level},finplan_core=warn");
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&default_filter));

    // Build and initialize the subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(false),
        )
        .init();

    tracing::info!("FinPlan logging initialized");
    Ok(())
}
