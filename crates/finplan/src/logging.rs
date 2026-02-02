use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Maximum log file size before rotation (5 MB)
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;
/// Size to keep after rotation (1 MB of most recent logs)
const KEEP_SIZE: u64 = 1024 * 1024;

/// Rotate log file if it exceeds the maximum size.
/// Keeps only the most recent KEEP_SIZE bytes.
fn rotate_log_if_needed(log_path: &Path) -> std::io::Result<()> {
    if !log_path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(log_path)?;
    if metadata.len() <= MAX_LOG_SIZE {
        return Ok(());
    }

    // Read the last KEEP_SIZE bytes
    let mut file = File::open(log_path)?;
    let file_size = metadata.len();
    let start_pos = file_size.saturating_sub(KEEP_SIZE);

    file.seek(SeekFrom::Start(start_pos))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    drop(file);

    // Skip to the first newline to avoid partial lines
    let skip = buffer
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let trimmed = &buffer[skip..];

    // Write back the trimmed content
    let mut file = File::create(log_path)?;
    file.write_all(b"--- Log rotated (older entries removed) ---\n")?;
    file.write_all(trimmed)?;

    Ok(())
}

/// A writer factory that produces writers for the shared log file
#[derive(Clone)]
struct LogWriterFactory {
    file: Arc<Mutex<File>>,
}

impl LogWriterFactory {
    fn new(file: File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

/// A writer that holds a reference to the shared file
struct LogWriter {
    file: Arc<Mutex<File>>,
}

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut file = self.file.lock().unwrap();
        file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut file = self.file.lock().unwrap();
        file.flush()
    }
}

impl<'a> MakeWriter<'a> for LogWriterFactory {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter {
            file: self.file.clone(),
        }
    }
}

/// Initialize logging to write to a file in the data directory.
///
/// Logs are written to `{data_dir}/finplan.log` with size-based rotation.
/// When the log exceeds 5MB, older entries are removed keeping only the last 1MB.
/// The log level can be controlled via the `level` parameter or the `RUST_LOG` environment variable.
pub fn init_logging(data_dir: &Path, level: &str) -> color_eyre::Result<()> {
    // Ensure data directory exists
    std::fs::create_dir_all(data_dir)?;

    let log_path = data_dir.join("finplan.log");

    // Rotate log if needed before opening
    if let Err(e) = rotate_log_if_needed(&log_path) {
        eprintln!("Warning: Failed to rotate log file: {}", e);
    }

    // Open log file for appending
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let writer_factory = LogWriterFactory::new(file);

    // Build filter from RUST_LOG env var or use provided level
    let default_filter = format!("finplan={level},finplan_core=warn");
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&default_filter));

    // Build and initialize the subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            fmt::layer()
                .with_writer(writer_factory)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(false),
        )
        .init();

    tracing::info!(
        "FinPlan logging initialized (log_path={})",
        log_path.display()
    );
    Ok(())
}
