//! Documents on an AI-guided draft: upload, list and remove.
//!
//! Uploads are read in memory and only their redacted text, hash and parsed
//! figures are stored (see [`crate::documents`]); a file with no text layer is
//! held in the draft's temp folder for the model and never in the database.
//! The tier's [`DraftLimits`](crate::suggest::ai::DraftLimits) (files, bytes,
//! pages) count across the whole draft. Only drafts take documents.

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};

use crate::auth::session::CurrentUser;
use crate::documents::store::{self, DocumentManifest, NewDocument};
use crate::documents::{self, images};
use crate::error::{ApiError, ApiResult};
use crate::observability::{EventFields, Operation, Resource};
use crate::state::AppState;

/// Room in a request beyond its files: multipart boundaries and field headers.
const MULTIPART_OVERHEAD: usize = 1024 * 1024;

pub fn router(config: &crate::config::ServerConfig) -> Router<AppState> {
    Router::new()
        .route("/drafts/{id}/documents", get(list).post(upload))
        .route("/drafts/{id}/documents/{doc_id}", delete(remove))
        // The body cap is the most any tier may attach (plus multipart
        // framing); each request is then held to its own tier's remaining
        // bytes as it streams in.
        .layer(DefaultBodyLimit::max(body_limit(config)))
}

/// The route's body limit for `config`: the larger tier's byte limit.
pub fn body_limit(config: &crate::config::ServerConfig) -> usize {
    let bytes = config.draft.free_max_bytes.max(config.draft.pro_max_bytes);
    usize::try_from(bytes)
        .unwrap_or(usize::MAX - MULTIPART_OVERHEAD)
        .saturating_add(MULTIPART_OVERHEAD)
}

/// Confirm the draft is the caller's and still a draft; returns whether its
/// documents are to be retained.
async fn owned_draft(state: &AppState, id: i64, user: &str) -> ApiResult<bool> {
    let retain: Option<i64> = sqlx::query_scalar(
        "SELECT retain_documents FROM scenarios
          WHERE id = ?1 AND user_id = ?2 AND status = 'draft'",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(&state.db)
    .await?;
    retain.map(|r| r != 0).ok_or(ApiError::NotFound("draft"))
}

/// `GET /drafts/{id}/documents` — the manifest: kind, filename, pages, status
/// and the start of the first page for each document.
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<DocumentManifest>>> {
    owned_draft(&state, id, &user.id).await?;
    Ok(Json(store::manifest(&state.db, id).await?))
}

/// A failure reading the multipart body: too big is 413, anything else a bad request.
fn read_error(e: axum::extract::multipart::MultipartError) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge("the upload is larger than a draft may take".into())
    } else {
        ApiError::bad_request(format!("reading the upload: {}", e.body_text()))
    }
}

/// A file read from the request, before it is stored.
struct Parsed {
    filename: String,
    sha256: String,
    bytes: u64,
    original: Option<Vec<u8>>,
    doc: documents::Ingested,
}

/// `POST /drafts/{id}/documents` — multipart, one or more file parts. All the
/// files of a request are read first and stored together, so a request that
/// breaks a limit stores none of them. 413 for too many files, bytes or pages;
/// 415 for a kind of file that is not read; 409 for a file already attached.
async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Vec<DocumentManifest>>)> {
    let retain = owned_draft(&state, id, &user.id).await?;
    if state.limited_guest(&user) {
        return Err(ApiError::Forbidden(
            "Create a free account to upload documents.".into(),
        ));
    }
    let pro = crate::billing::entitlements(&state.db, &user.id, &state.config)
        .await?
        .pro;
    let limits = state.config.draft.limits(pro);

    let mut conn = state.db.acquire().await?;
    let used = store::usage(&mut conn, id).await?;
    drop(conn);
    let mut remaining_bytes = limits.max_bytes.saturating_sub(used.bytes);
    let mut files_left = limits.max_files.saturating_sub(used.files);

    let mut parsed: Vec<Parsed> = Vec::new();
    while let Some(mut field) = multipart.next_field().await.map_err(read_error)? {
        let Some(filename) = field.file_name().map(documents::clean_filename) else {
            // A plain form field, not a file.
            continue;
        };
        if files_left == 0 {
            return Err(ApiError::PayloadTooLarge(format!(
                "a draft takes at most {} files",
                limits.max_files
            )));
        }
        files_left -= 1;

        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(read_error)? {
            if chunk.len() as u64 > remaining_bytes {
                return Err(ApiError::PayloadTooLarge(format!(
                    "a draft takes at most {} MB of documents",
                    limits.max_bytes / (1024 * 1024)
                )));
            }
            remaining_bytes -= chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err(ApiError::bad_request(format!("{filename} is empty")));
        }
        let len = bytes.len() as u64;
        let name = filename.clone();
        // Parsing is CPU-bound and PDF parsing untrusted: off the runtime.
        let (sha256, original, doc) = tokio::task::spawn_blocking(move || {
            let sha256 = documents::sha256_hex(&bytes);
            documents::ingest(&name, &bytes).map(|doc| {
                let original = doc.hold_original.then_some(bytes);
                (sha256, original, doc)
            })
        })
        .await
        .map_err(|_| ApiError::internal("reading an upload failed"))?
        .map_err(|documents::Unsupported(why)| ApiError::UnsupportedMedia(why))?;
        if parsed.iter().any(|p| p.sha256 == sha256) {
            return Err(ApiError::Conflict(format!(
                "{filename} is already attached to this draft"
            )));
        }
        parsed.push(Parsed {
            filename,
            sha256,
            bytes: len,
            original,
            doc,
        });
    }
    if parsed.is_empty() {
        return Err(ApiError::bad_request("no file was uploaded"));
    }

    // Store under one write lock so the limits hold against a concurrent upload.
    let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await?;
    let used = store::usage(&mut tx, id).await?;
    let new_pages: u64 = parsed.iter().map(|p| u64::from(p.doc.page_units)).sum();
    let new_bytes: u64 = parsed.iter().map(|p| p.bytes).sum();
    if used.files as usize + parsed.len() > limits.max_files as usize {
        return Err(ApiError::PayloadTooLarge(format!(
            "a draft takes at most {} files",
            limits.max_files
        )));
    }
    if used.bytes + new_bytes > limits.max_bytes {
        return Err(ApiError::PayloadTooLarge(format!(
            "a draft takes at most {} MB of documents",
            limits.max_bytes / (1024 * 1024)
        )));
    }
    if u64::from(used.pages) + new_pages > u64::from(limits.max_pages) {
        return Err(ApiError::PayloadTooLarge(format!(
            "a draft takes at most {} pages of documents (PDF pages and images)",
            limits.max_pages
        )));
    }
    // Still a draft (a cancel may have raced this upload).
    let still_draft: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scenarios WHERE id = ?1 AND user_id = ?2 AND status = 'draft')",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !still_draft {
        return Err(ApiError::NotFound("draft"));
    }
    let mut ids = Vec::new();
    for p in &parsed {
        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE scenario_id = ?1 AND sha256 = ?2)",
        )
        .bind(id)
        .bind(&p.sha256)
        .fetch_one(&mut *tx)
        .await?;
        if duplicate {
            return Err(ApiError::Conflict(format!(
                "{} is already attached to this draft",
                p.filename
            )));
        }
        let doc_id = store::insert(
            &mut tx,
            &user.id,
            id,
            &NewDocument {
                filename: p.filename.clone(),
                mime: p.doc.mime.to_owned(),
                sha256: p.sha256.clone(),
                bytes: p.bytes as i64,
                pages: p.doc.pages.clone(),
                page_units: i64::from(p.doc.page_units),
                kind: p.doc.kind,
                status: p.doc.status,
                note: p.doc.note.clone(),
                retain,
                birth_date_hint: p.doc.birth_date_hint.clone(),
                data: p.doc.data.clone(),
            },
        )
        .await?;
        ids.push(doc_id);
    }
    // Attaching a document is activity: it keeps the draft from being swept.
    sqlx::query("UPDATE scenarios SET updated_at = datetime('now') WHERE id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    // Held originals are written before the rows are committed, so a row that
    // says one is waiting never lacks it; a failure rolls the rows back.
    let root = images::root(&state.config);
    let mut held: Vec<&str> = Vec::new();
    for p in &parsed {
        if let Some(bytes) = &p.original {
            if let Err(err) = images::hold(&root, id, &p.sha256, bytes.clone()).await {
                for sha in held {
                    images::discard(&root, id, sha);
                }
                // A write that failed part-way may have left a piece.
                images::discard(&root, id, &p.sha256);
                return Err(err);
            }
            held.push(&p.sha256);
        }
    }
    if let Err(err) = tx.commit().await {
        for sha in held {
            images::discard(&root, id, sha);
        }
        return Err(err.into());
    }

    state.telemetry.mutation(
        Resource::Scenario,
        Operation::Updated,
        &EventFields {
            user_id: Some(&user.id),
            scenario_id: Some(id),
            resource_id: Some(id),
            ..Default::default()
        },
    );
    let mut out = Vec::new();
    for doc_id in ids {
        out.push(store::manifest_entry(&state.db, id, doc_id).await?);
    }
    Ok((StatusCode::CREATED, Json(out)))
}

/// `DELETE /drafts/{id}/documents/{doc_id}` — remove one document (and its
/// held original, if any) from the draft.
async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((id, doc_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    owned_draft(&state, id, &user.id).await?;
    let sha256: Option<String> = sqlx::query_scalar(
        "DELETE FROM documents WHERE id = ?1 AND scenario_id = ?2 RETURNING sha256",
    )
    .bind(doc_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let sha256 = sha256.ok_or(ApiError::NotFound("document"))?;
    images::discard(&images::root(&state.config), id, &sha256);
    Ok(StatusCode::NO_CONTENT)
}
