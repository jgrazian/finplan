//! Reading and writing the `documents` table. Everything here takes an id the
//! caller has already checked belongs to the user's draft (the routes do; the
//! drafting agent works inside one draft), except where a `user_id` is asked for.

use std::ops::RangeInclusive;

use serde::Serialize;
use sqlx::FromRow;
use ts_rs::TS;

use super::classify::DocumentKind;
use super::data::DocumentData;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};

/// Separates pages inside a document's stored `text`.
pub const PAGE_BREAK: char = '\u{c}';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DocumentStatus {
    /// Text extracted (and redacted).
    Parsed,
    /// No text layer (a scan) and no OCR here: the original is held in the
    /// draft's temp folder for the model, unredacted.
    NeedsOcr,
    /// An image: same as `needs_ocr`; there is no text to redact.
    ImageUnredacted,
    /// Could not be read; `note` says why.
    Failed,
}

impl DocumentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentStatus::Parsed => "parsed",
            DocumentStatus::NeedsOcr => "needs_ocr",
            DocumentStatus::ImageUnredacted => "image_unredacted",
            DocumentStatus::Failed => "failed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "parsed" => DocumentStatus::Parsed,
            "needs_ocr" => DocumentStatus::NeedsOcr,
            "image_unredacted" => DocumentStatus::ImageUnredacted,
            "failed" => DocumentStatus::Failed,
            _ => return None,
        })
    }

    /// Whether the original is waiting in the temp folder.
    pub fn holds_original(self) -> bool {
        matches!(
            self,
            DocumentStatus::NeedsOcr | DocumentStatus::ImageUnredacted
        )
    }
}

/// One line of the draft's document list: what the drafting agent's context
/// carries instead of the documents.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DocumentManifest {
    pub id: i64,
    pub kind: DocumentKind,
    pub filename: String,
    pub mime: String,
    pub pages: i64,
    pub status: DocumentStatus,
    pub note: Option<String>,
    /// Size of the upload (the bytes themselves are not kept).
    pub bytes: i64,
    /// The start of the first page's (redacted) text.
    pub snippet: String,
    pub retain: bool,
    /// The one birth date found in it, `YYYY-MM-DD`.
    pub birth_date_hint: Option<String>,
    pub created_at: String,
}

/// A stored document, whole.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: i64,
    pub scenario_id: i64,
    pub filename: String,
    pub mime: String,
    pub sha256: String,
    pub pages: i64,
    pub kind: DocumentKind,
    pub status: DocumentStatus,
    pub note: Option<String>,
    pub birth_date_hint: Option<String>,
    /// Redacted text, pages separated by [`PAGE_BREAK`].
    pub text: String,
    pub data: DocumentData,
}

#[derive(Debug, FromRow)]
struct Row {
    id: i64,
    scenario_id: i64,
    filename: String,
    mime: String,
    sha256: String,
    bytes: i64,
    pages: i64,
    text: String,
    kind: String,
    status: String,
    note: Option<String>,
    retain: i64,
    birth_date_hint: Option<String>,
    data_json: Option<String>,
    created_at: String,
}

const COLUMNS: &str = "id, scenario_id, filename, mime, sha256, bytes, pages, text, kind, status,
    note, retain, birth_date_hint, data_json, created_at";

impl Row {
    fn kind(&self) -> ApiResult<DocumentKind> {
        DocumentKind::parse(&self.kind)
            .ok_or_else(|| ApiError::internal("a document has an unknown kind"))
    }

    fn status(&self) -> ApiResult<DocumentStatus> {
        DocumentStatus::parse(&self.status)
            .ok_or_else(|| ApiError::internal("a document has an unknown status"))
    }

    fn manifest(&self) -> ApiResult<DocumentManifest> {
        let first = self.text.split(PAGE_BREAK).next().unwrap_or("");
        Ok(DocumentManifest {
            id: self.id,
            kind: self.kind()?,
            filename: self.filename.clone(),
            mime: self.mime.clone(),
            pages: self.pages,
            status: self.status()?,
            note: self.note.clone(),
            bytes: self.bytes,
            snippet: snippet(first),
            retain: self.retain != 0,
            birth_date_hint: self.birth_date_hint.clone(),
            created_at: self.created_at.clone(),
        })
    }

    fn document(self) -> ApiResult<Document> {
        let data = self
            .data_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        Ok(Document {
            kind: self.kind()?,
            status: self.status()?,
            id: self.id,
            scenario_id: self.scenario_id,
            filename: self.filename,
            mime: self.mime,
            sha256: self.sha256,
            pages: self.pages,
            note: self.note,
            birth_date_hint: self.birth_date_hint,
            text: self.text,
            data,
        })
    }
}

/// How much of the first page the manifest shows.
pub const SNIPPET_CHARS: usize = 400;

fn snippet(page: &str) -> String {
    let collapsed: Vec<&str> = page.split_whitespace().collect();
    let joined = collapsed.join(" ");
    if joined.chars().count() <= SNIPPET_CHARS {
        joined
    } else {
        let cut: String = joined.chars().take(SNIPPET_CHARS).collect();
        format!("{cut}…")
    }
}

/// Everything about a new document but its id and time.
pub struct NewDocument {
    pub filename: String,
    pub mime: String,
    pub sha256: String,
    pub bytes: i64,
    pub pages: Vec<String>,
    pub page_units: i64,
    pub kind: DocumentKind,
    pub status: DocumentStatus,
    pub note: Option<String>,
    pub retain: bool,
    pub birth_date_hint: Option<String>,
    pub data: Option<DocumentData>,
}

pub async fn insert(
    tx: &mut sqlx::SqliteConnection,
    user_id: &str,
    scenario_id: i64,
    doc: &NewDocument,
) -> ApiResult<i64> {
    let text = doc
        .pages
        .iter()
        .map(|p| p.replace(PAGE_BREAK, " "))
        .collect::<Vec<_>>()
        .join(&PAGE_BREAK.to_string());
    let data = match &doc.data {
        Some(d) if !d.is_empty() => Some(
            serde_json::to_string(d)
                .map_err(|e| ApiError::internal(format!("document data: {e}")))?,
        ),
        _ => None,
    };
    let id = sqlx::query_scalar(
        "INSERT INTO documents
            (user_id, scenario_id, filename, mime, sha256, bytes, pages, page_units, text,
             kind, status, note, retain, birth_date_hint, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         RETURNING id",
    )
    .bind(user_id)
    .bind(scenario_id)
    .bind(&doc.filename)
    .bind(&doc.mime)
    .bind(&doc.sha256)
    .bind(doc.bytes)
    .bind(doc.pages.len().max(1) as i64)
    .bind(doc.page_units)
    .bind(text)
    .bind(doc.kind.as_str())
    .bind(doc.status.as_str())
    .bind(&doc.note)
    .bind(i64::from(doc.retain))
    .bind(&doc.birth_date_hint)
    .bind(data)
    .fetch_one(&mut *tx)
    .await?;
    Ok(id)
}

/// The draft's documents, oldest first.
pub async fn manifest(db: &Db, scenario_id: i64) -> ApiResult<Vec<DocumentManifest>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM documents WHERE scenario_id = ?1 ORDER BY id"
    ))
    .bind(scenario_id)
    .fetch_all(db)
    .await?;
    rows.iter().map(Row::manifest).collect()
}

pub async fn manifest_entry(db: &Db, scenario_id: i64, id: i64) -> ApiResult<DocumentManifest> {
    let row: Row = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM documents WHERE scenario_id = ?1 AND id = ?2"
    ))
    .bind(scenario_id)
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound("document"))?;
    row.manifest()
}

/// One document, whole (text, structured data), by id within its draft.
pub async fn load(db: &Db, scenario_id: i64, id: i64) -> ApiResult<Document> {
    sqlx::query_as::<_, Row>(&format!(
        "SELECT {COLUMNS} FROM documents WHERE scenario_id = ?1 AND id = ?2"
    ))
    .bind(scenario_id)
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound("document"))?
    .document()
}

/// All of the draft's documents, whole.
pub async fn load_all(db: &Db, scenario_id: i64) -> ApiResult<Vec<Document>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM documents WHERE scenario_id = ?1 ORDER BY id"
    ))
    .bind(scenario_id)
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Row::document).collect()
}

/// One page of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct DocumentPage {
    /// 1-based.
    pub page: u32,
    pub text: String,
}

/// The document's pages, or those within `pages` (1-based, inclusive; a range
/// past the end is clipped). What `read_document(id, pages?)` returns.
pub async fn document_pages(
    db: &Db,
    doc_id: i64,
    pages: Option<RangeInclusive<u32>>,
) -> ApiResult<Vec<DocumentPage>> {
    let text: String = sqlx::query_scalar("SELECT text FROM documents WHERE id = ?1")
        .bind(doc_id)
        .fetch_optional(db)
        .await?
        .ok_or(ApiError::NotFound("document"))?;
    Ok(split_pages(&text, pages))
}

/// [`document_pages`] over text already in hand.
pub fn split_pages(text: &str, pages: Option<RangeInclusive<u32>>) -> Vec<DocumentPage> {
    text.split(PAGE_BREAK)
        .enumerate()
        .map(|(i, t)| DocumentPage {
            page: i as u32 + 1,
            text: t.to_owned(),
        })
        .filter(|p| pages.as_ref().is_none_or(|r| r.contains(&p.page)))
        .collect()
}

/// What a draft's documents already use of its limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub files: u32,
    pub bytes: u64,
    pub pages: u32,
}

pub async fn usage(tx: &mut sqlx::SqliteConnection, scenario_id: i64) -> ApiResult<Usage> {
    let (files, bytes, pages): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(bytes), 0), COALESCE(sum(page_units), 0)
           FROM documents WHERE scenario_id = ?1",
    )
    .bind(scenario_id)
    .fetch_one(&mut *tx)
    .await?;
    Ok(Usage {
        files: files as u32,
        bytes: bytes as u64,
        pages: pages as u32,
    })
}
