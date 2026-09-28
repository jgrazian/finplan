//! Originals waiting for the model: the only place an uploaded file is ever
//! written down.
//!
//! A document with no text layer (a screenshot, a scanned PDF) cannot be
//! redacted and there is no OCR here, so its bytes are held in a per-draft
//! temp folder for the drafting agent to hand to the model once, unredacted
//! (the upload consent text says so). Everything else uploaded is parsed in
//! memory and dropped.
//!
//! * The folder is `<root>/<draft id>/`, one file per document named by its
//!   SHA-256; `root` is `FINPLAN_DRAFT_TEMP_DIR`, else a folder under the OS
//!   temp directory that is specific to this database.
//! * Never in the database, never in a backup: nothing here is reachable from
//!   the schema except the row's `status` saying a file is waiting.
//! * Removed with the draft: cancel, create (always, even when documents are
//!   retained), a replaced draft, and the TTL sweep, which also deletes any
//!   folder whose draft no longer exists ([`purge_orphans`]).
//! * Removed once read into text: [`super::store_extraction`].

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::ServerConfig;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// The folder draft originals live under for this server.
pub fn root(config: &ServerConfig) -> PathBuf {
    if let Some(dir) = &config.draft.temp_dir {
        return dir.clone();
    }
    // Servers (and test databases) sharing a machine must not sweep each
    // other's folders, so the default is keyed by the database.
    let digest = Sha256::digest(config.database_url.as_bytes());
    let key: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    std::env::temp_dir().join("finplan-draft-files").join(key)
}

fn draft_dir(root: &Path, scenario_id: i64) -> PathBuf {
    root.join(scenario_id.to_string())
}

fn file_path(root: &Path, scenario_id: i64, sha256: &str) -> PathBuf {
    draft_dir(root, scenario_id).join(sha256)
}

/// Write `bytes` for a document of the draft, owner-only.
pub async fn hold(root: &Path, scenario_id: i64, sha256: &str, bytes: Vec<u8>) -> ApiResult<()> {
    let dir = draft_dir(root, scenario_id);
    let path = file_path(root, scenario_id, sha256);
    tokio::task::spawn_blocking(move || write_private(&dir, &path, &bytes))
        .await
        .map_err(|e| ApiError::internal(format!("holding an upload: {e}")))?
        .map_err(|e| ApiError::internal(format!("holding an upload: {e}")))
}

fn write_private(dir: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)?;
        std::fs::File::create(path)?.write_all(bytes)
    }
}

/// The held bytes, when they are still there.
pub async fn read(root: &Path, scenario_id: i64, sha256: &str) -> ApiResult<Option<Vec<u8>>> {
    let path = file_path(root, scenario_id, sha256);
    tokio::task::spawn_blocking(move || match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    })
    .await
    .map_err(|e| ApiError::internal(format!("reading a held upload: {e}")))?
    .map_err(|e| ApiError::internal(format!("reading a held upload: {e}")))
}

/// Delete one held original (a failed delete is logged, not returned: the
/// draft's folder goes with the draft regardless).
pub fn discard(root: &Path, scenario_id: i64, sha256: &str) {
    let path = file_path(root, scenario_id, sha256);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(event = "draft.file_discard_failed", scenario_id, error = %e.kind());
    }
}

/// Delete everything held for a draft.
pub fn discard_draft(root: &Path, scenario_id: i64) {
    let dir = draft_dir(root, scenario_id);
    if let Err(e) = std::fs::remove_dir_all(&dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(event = "draft.files_discard_failed", scenario_id, error = %e.kind());
    }
}

/// Delete the folders of drafts that no longer exist (swept, replaced,
/// promoted, deleted with their user, or left by a crash). Returns how many.
pub async fn purge_orphans(db: &Db, root: &Path) -> ApiResult<usize> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(0);
    };
    let live: std::collections::HashSet<i64> =
        sqlx::query_scalar("SELECT id FROM scenarios WHERE status = 'draft'")
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();
    let mut purged = 0;
    for entry in entries.flatten() {
        let keep = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i64>().ok())
            .is_some_and(|id| live.contains(&id));
        if !keep && std::fs::remove_dir_all(entry.path()).is_ok() {
            purged += 1;
        }
    }
    Ok(purged)
}
