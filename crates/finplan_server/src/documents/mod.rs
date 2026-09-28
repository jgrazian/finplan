//! Documents for AI-guided drafts: statements, pay stubs, tax returns and
//! exports a person attaches so the plan starts from real numbers.
//!
//! Privacy is the design constraint (spec 16, G5 and G12):
//!
//! * **Originals are never persisted.** An upload is parsed in memory; only the
//!   *redacted* extracted text, its SHA-256 and the figures read from it are
//!   stored. Backups never hold a statement or a tax return.
//! * **Redaction happens before storage** ([`redact`]): SSNs and ITINs, EINs,
//!   full account and routing numbers (last four kept), dates of birth (one is
//!   kept apart, as a hint the plan needs), street addresses, e-mail and phone.
//! * **Files with no text layer** (screenshots, scanned PDFs) cannot be
//!   redacted, and no pure-Rust OCR is available here. They are held only in a
//!   per-draft temp folder ([`images`]) until the model reads them, sent
//!   unredacted (the consent text must say so), and deleted with the draft.
//!   Their rows say `image_unredacted` or `needs_ocr`.
//!
//! Layout: [`ingest`] turns bytes into pages, kind, status and structured data
//! (using [`tabular`], [`ofx`], [`pdf`], [`hints`], [`classify`]); [`store`]
//! reads and writes rows; [`tools`] are the deterministic functions the
//! drafting agent calls over stored documents; `api::documents` is the HTTP
//! surface.
//!
//! For the drafting agent: [`store::manifest`] (context), [`store::document_pages`]
//! (`read_document`), [`pending_file`] and [`store_extraction`] (images),
//! and [`tools::summarize_transactions`], [`tools::match_account`],
//! [`tools::reconcile`].

pub mod classify;
pub mod data;
pub mod hints;
pub mod images;
pub mod ofx;
pub mod pdf;
pub mod redact;
pub mod store;
pub mod tabular;
pub mod tools;

#[cfg(test)]
mod tests;

use sha2::{Digest, Sha256};

use crate::config::ServerConfig;
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
pub use classify::DocumentKind;
pub use data::DocumentData;
pub use store::{DocumentManifest, DocumentStatus};

/// A file's bytes, read.
#[derive(Debug)]
pub struct Ingested {
    pub mime: &'static str,
    pub kind: DocumentKind,
    pub status: DocumentStatus,
    pub note: Option<String>,
    /// Redacted text of each page.
    pub pages: Vec<String>,
    /// What the file counts for against a draft's page limit.
    pub page_units: u32,
    pub data: Option<DocumentData>,
    pub birth_date_hint: Option<String>,
    /// Whether the original must be held for the model (no text layer).
    pub hold_original: bool,
}

/// A file the server does not read.
#[derive(Debug)]
pub struct Unsupported(pub String);

/// Text lines per page of plain text.
const LINES_PER_PAGE: usize = 200;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A safe display name: no path, no control characters, bounded length.
pub fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() {
        "document".into()
    } else {
        cleaned
    }
}

fn extension(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The image type of `bytes` by magic number.
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.len() > 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"heim" | b"heis"
        )
    {
        Some("image/heic")
    } else {
        None
    }
}

/// Read an upload. CPU-bound: call from a blocking task.
pub fn ingest(filename: &str, bytes: &[u8]) -> Result<Ingested, Unsupported> {
    let ext = extension(filename);

    if let Some(mime) = image_mime(bytes) {
        return Ok(held_original(
            mime,
            DocumentKind::Image,
            DocumentStatus::ImageUnredacted,
            1,
            "An image has no text layer to redact and no OCR is available, so it is held \
             only until it is read and is sent to the model unredacted.",
        ));
    }
    if pdf::sniff(&bytes[..bytes.len().min(1024)]) || ext == "pdf" {
        return Ok(ingest_pdf(filename, bytes));
    }
    let head = &bytes[..bytes.len().min(4096)];
    if matches!(ext.as_str(), "ofx" | "qfx") || (ext != "csv" && ofx::sniff(head)) {
        return Ok(ingest_ofx(bytes));
    }
    match ext.as_str() {
        "csv" | "tsv" => Ok(ingest_tabular(filename, bytes)),
        "txt" | "text" | "md" | "log" => Ok(ingest_text(filename, bytes)),
        _ => Err(Unsupported(format!(
            "{filename}: supported files are PDF, CSV, OFX/QFX, plain text and images \
             (PNG, JPEG, WebP, HEIC)"
        ))),
    }
}

fn held_original(
    mime: &'static str,
    kind: DocumentKind,
    status: DocumentStatus,
    pages: u32,
    note: &str,
) -> Ingested {
    Ingested {
        mime,
        kind,
        status,
        note: Some(note.to_owned()),
        pages: vec![String::new(); pages as usize],
        page_units: pages,
        data: None,
        birth_date_hint: None,
        hold_original: true,
    }
}

fn failed(mime: &'static str, note: &str) -> Ingested {
    Ingested {
        mime,
        kind: DocumentKind::Other,
        status: DocumentStatus::Failed,
        note: Some(note.to_owned()),
        pages: vec![String::new()],
        page_units: 1,
        data: None,
        birth_date_hint: None,
        hold_original: false,
    }
}

/// Redact each page in turn, keeping the first birth date found.
fn redact_pages(pages: &[String]) -> (Vec<String>, Option<String>) {
    let mut hint = None;
    let out = pages
        .iter()
        .map(|p| {
            let r = redact::redact(p);
            hint = hint.take().or(r.birth_date_hint);
            r.text
        })
        .collect();
    (out, hint)
}

fn ingest_pdf(filename: &str, bytes: &[u8]) -> Ingested {
    const MIME: &str = "application/pdf";
    let text = match pdf::extract(bytes) {
        Ok(t) => t,
        Err(pdf::PdfError::Encrypted) => {
            return failed(
                MIME,
                "This PDF is password-protected. Remove the password and upload it again.",
            );
        }
        Err(pdf::PdfError::Unreadable) => return failed(MIME, "This PDF could not be read."),
    };
    let pages_total = text.pages.len() as u32;
    if text.pages.iter().all(|p| p.trim().is_empty()) {
        return held_original(
            MIME,
            classify::classify(filename, ""),
            DocumentStatus::NeedsOcr,
            pages_total,
            "This PDF has no text layer (a scan) and no OCR is available, so it is held only \
             until it is read and is sent to the model unredacted.",
        );
    }
    let (pages, birth_date_hint) = redact_pages(&text.pages);
    let joined = pages.join("\n");
    let empty = text.pages.iter().filter(|p| p.trim().is_empty()).count();
    let note = (empty > 0).then(|| {
        format!(
            "{empty} of {pages_total} pages have no text layer and were skipped; upload them \
             as images if they matter."
        )
    });
    Ingested {
        mime: MIME,
        kind: classify::classify(filename, &joined),
        status: DocumentStatus::Parsed,
        note,
        page_units: pages_total,
        data: hints::from_text(&joined),
        pages,
        birth_date_hint,
        hold_original: false,
    }
}

fn ingest_ofx(bytes: &[u8]) -> Ingested {
    const MIME: &str = "application/x-ofx";
    let text = String::from_utf8_lossy(bytes);
    let Some(data) = ofx::parse(&text) else {
        return failed(MIME, "No account statement was found in this OFX file.");
    };
    let pages = render_data(&data);
    let has_positions = data.accounts.iter().any(|a| !a.positions.is_empty());
    let has_balances = data.accounts.iter().any(|a| !a.balances.is_empty());
    let kind = if has_positions {
        DocumentKind::BrokerageStatement
    } else if has_balances {
        DocumentKind::BankStatement
    } else {
        DocumentKind::Transactions
    };
    Ingested {
        mime: MIME,
        kind,
        status: DocumentStatus::Parsed,
        note: None,
        pages,
        page_units: 1,
        data: Some(data),
        birth_date_hint: None,
        hold_original: false,
    }
}

fn ingest_tabular(filename: &str, bytes: &[u8]) -> Ingested {
    const MIME: &str = "text/csv";
    let text = String::from_utf8_lossy(bytes);
    let rows = tabular::parse_rows(&text);
    if rows.is_empty() {
        return failed(MIME, "This file has no rows.");
    }
    let (pages, birth_date_hint) = redact_pages(&tabular::render_pages(&rows));
    let mut data = tabular::mine(&rows);
    if let Some(data) = data.as_mut() {
        for account in &mut data.accounts {
            for t in &mut account.transactions {
                t.description = redact::redact(&t.description).text;
            }
        }
    }
    let kind = match &data {
        Some(d) if d.accounts.iter().any(|a| !a.transactions.is_empty()) => {
            DocumentKind::Transactions
        }
        Some(d) if d.accounts.iter().any(|a| !a.positions.is_empty()) => {
            DocumentKind::BrokerageStatement
        }
        _ => classify::classify(filename, &pages.join("\n")),
    };
    Ingested {
        mime: MIME,
        kind,
        status: DocumentStatus::Parsed,
        note: None,
        pages,
        page_units: 1,
        data,
        birth_date_hint,
        hold_original: false,
    }
}

fn ingest_text(filename: &str, bytes: &[u8]) -> Ingested {
    const MIME: &str = "text/plain";
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return failed(MIME, "This file is empty.");
    }
    let raw: Vec<String> = lines
        .chunks(LINES_PER_PAGE)
        .map(|chunk| chunk.join("\n"))
        .collect();
    let (pages, birth_date_hint) = redact_pages(&raw);
    let joined = pages.join("\n");
    Ingested {
        mime: MIME,
        kind: classify::classify(filename, &joined),
        status: DocumentStatus::Parsed,
        note: None,
        page_units: 1,
        data: hints::from_text(&joined),
        pages,
        birth_date_hint,
        hold_original: false,
    }
}

/// Structured statement data as readable text pages: a summary page, then
/// transactions [`tabular::ROWS_PER_PAGE`] to a page.
fn render_data(data: &DocumentData) -> Vec<String> {
    let mut head = String::new();
    if let Some(name) = &data.institution {
        head.push_str(&format!("Institution: {name}\n"));
    }
    let mut txn_lines: Vec<String> = Vec::new();
    for (i, account) in data.accounts.iter().enumerate() {
        head.push_str(&format!("\nAccount {}", i + 1));
        if let Some(last4) = &account.last4 {
            head.push_str(&format!(" ••••{last4}"));
        }
        if let Some(kind) = &account.kind {
            head.push_str(&format!(" ({kind})"));
        }
        if let Some(currency) = &account.currency {
            head.push_str(&format!(" {currency}"));
        }
        head.push('\n');
        for b in &account.balances {
            head.push_str(&format!("{} balance: {:.2}", b.label, b.amount));
            if let Some(d) = &b.as_of {
                head.push_str(&format!(" as of {d}"));
            }
            head.push('\n');
        }
        if !account.positions.is_empty() {
            head.push_str("Positions (symbol | name | units | price | value):\n");
            for p in &account.positions {
                head.push_str(&format!(
                    "{} | {} | {} | {} | {}\n",
                    p.symbol.as_deref().unwrap_or("?"),
                    p.name.as_deref().unwrap_or(""),
                    p.units,
                    p.unit_price.map_or(String::new(), |v| format!("{v:.4}")),
                    p.market_value.map_or(String::new(), |v| format!("{v:.2}")),
                ));
            }
        }
        for t in &account.transactions {
            txn_lines.push(format!(
                "{} | {:.2} | {}{}",
                t.date,
                t.amount,
                t.description,
                if data.accounts.len() > 1 {
                    format!(" | account {}", i + 1)
                } else {
                    String::new()
                }
            ));
        }
    }
    let (mut pages, _) = redact_pages(&[head]);
    for chunk in txn_lines.chunks(tabular::ROWS_PER_PAGE) {
        pages.push(format!(
            "Transactions (date | amount | description):\n{}",
            chunk.join("\n")
        ));
    }
    pages
}

/// An original held for the model: a screenshot or a scanned PDF.
#[derive(Debug)]
pub struct PendingFile {
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The held original of a document (status `image_unredacted` or `needs_ocr`),
/// for the drafting agent to attach to a request. `None` once it has been
/// read into text or the draft is gone.
pub async fn pending_file(
    db: &Db,
    config: &ServerConfig,
    scenario_id: i64,
    doc_id: i64,
) -> ApiResult<Option<PendingFile>> {
    let doc = store::load(db, scenario_id, doc_id).await?;
    if !doc.status.holds_original() {
        return Ok(None);
    }
    let root = images::root(config);
    Ok(images::read(&root, scenario_id, &doc.sha256)
        .await?
        .map(|bytes| PendingFile {
            filename: doc.filename,
            mime: doc.mime,
            bytes,
        }))
}

/// Keep the model's reading of a held original: the text is redacted like any
/// other, becomes the document's single page (status `parsed`), and the held
/// original is deleted. This is the "kept text" for an image.
pub async fn store_extraction(
    db: &Db,
    config: &ServerConfig,
    scenario_id: i64,
    doc_id: i64,
    text: &str,
) -> ApiResult<()> {
    let doc = store::load(db, scenario_id, doc_id).await?;
    if !doc.status.holds_original() {
        return Err(ApiError::Conflict(
            "this document has no held original to replace".into(),
        ));
    }
    let redacted = redact::redact(text);
    let kind = if doc.kind == DocumentKind::Image {
        classify::classify(&doc.filename, &redacted.text)
    } else {
        doc.kind
    };
    let data = hints::from_text(&redacted.text);
    let data_json = data.and_then(|d| serde_json::to_string(&d).ok());
    sqlx::query(
        "UPDATE documents
            SET text = ?3, pages = 1, status = 'parsed', kind = ?4, note = ?5,
                birth_date_hint = COALESCE(?6, birth_date_hint), data_json = ?7
          WHERE id = ?2 AND scenario_id = ?1",
    )
    .bind(scenario_id)
    .bind(doc_id)
    .bind(redacted.text.replace(store::PAGE_BREAK, " "))
    .bind(kind.as_str())
    .bind("Read from the original by the model; the original was deleted.")
    .bind(redacted.birth_date_hint)
    .bind(data_json)
    .execute(db)
    .await?;
    images::discard(&images::root(config), scenario_id, &doc.sha256);
    Ok(())
}

/// Delete a draft's held originals and, unless kept, its documents: what
/// Create & run does once the plan is queued. The held originals always go.
pub async fn finish_draft(db: &Db, config: &ServerConfig, scenario_id: i64) -> ApiResult<()> {
    sqlx::query("DELETE FROM documents WHERE scenario_id = ?1 AND retain = 0")
        .bind(scenario_id)
        .execute(db)
        .await?;
    // A retained document that was never read into text has nothing to keep.
    sqlx::query(
        "DELETE FROM documents
          WHERE scenario_id = ?1 AND status IN ('needs_ocr', 'image_unredacted')",
    )
    .bind(scenario_id)
    .execute(db)
    .await?;
    images::discard_draft(&images::root(config), scenario_id);
    Ok(())
}
