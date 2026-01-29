//! I/O utility functions

use std::fs;
use std::io;
use std::path::Path;

/// Write content to a file atomically using write-then-rename pattern.
///
/// This prevents data corruption if the process is interrupted during write.
/// The content is first written to a temporary file, then atomically renamed
/// to the target path.
///
/// # Arguments
/// * `path` - Target file path
/// * `content` - Content to write
///
/// # Example
/// ```ignore
/// atomic_write(Path::new("config.yaml"), &yaml_content)?;
/// ```
pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    // Create temp file path with .tmp extension
    let temp_path = path.with_extension("yaml.tmp");

    // Write to temp file first
    fs::write(&temp_path, content)?;

    // Atomically rename temp file to target (atomic on POSIX systems)
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Write bytes to a file atomically using write-then-rename pattern.
pub fn atomic_write_bytes(path: &Path, content: &[u8]) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.yaml");

        atomic_write(&path, "key: value\n").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "key: value\n");

        // Temp file should not exist
        let temp_path = path.with_extension("yaml.tmp");
        assert!(!temp_path.exists());
    }

    #[test]
    fn test_atomic_write_overwrites() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.yaml");

        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "second").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "second");
    }
}
