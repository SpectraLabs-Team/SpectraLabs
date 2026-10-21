//! Resumable uploads (M1b task 9): a file sent in chunks, any of which can be
//! retried, so an interrupted upload continues where it stopped.
//!
//! 1. `POST /api/v1/uploads/sessions` `{filename, size}`: opens a session.
//! 2. `PUT /api/v1/uploads/sessions/{id}/chunk?offset=N`, raw bytes (at most
//!    [`CHUNK_BYTES`]): appends at `N`, which must be exactly the bytes
//!    received so far. Otherwise 409 `offset_mismatch` with
//!    `details.received`, which is where to continue (so a chunk whose answer
//!    was lost can simply be re-sent).
//! 3. `GET /api/v1/uploads/sessions/{id}`: how much has arrived (to resume).
//! 4. `POST /api/v1/uploads/sessions/{id}/complete`: hashes the file, moves it
//!    into the artifact store and records the upload (as `POST /api/v1/uploads`
//!    does). Retrying returns the same upload.
//! 5. `DELETE /api/v1/uploads/sessions/{id}`: abandons it.
//!
//! The bytes go to a staging file next to the artifact store (local backend)
//! or in the system temporary directory (S3), named after the session. With
//! several API instances, the staging directory has to be shared for a
//! session to move between them. Sessions idle for 24 hours are removed by
//! the API (see [`sweep_expired`]). The size limit is
//! `SPECTRA_MAX_UPLOAD_BYTES` (default 1 GiB); each workspace may have
//! [`MAX_OPEN_SESSIONS`] unfinished sessions.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path as FsPath, PathBuf};

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use serde::{Deserialize, Serialize};
use spectra_core::{Json, Sha256};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::uploads_api::{clean_filename, store_and_record, UploadView};
use crate::{
    error_response, error_with_code, error_with_details, invalid_fields, AppState, Identity, RequestId,
};

/// Largest chunk, in bytes (8 MiB: under the dashboard's 10 MB request buffering).
pub const CHUNK_BYTES: u64 = 8 * 1024 * 1024;
/// Unfinished sessions a workspace may have at once.
pub const MAX_OPEN_SESSIONS: i32 = 5;
/// Default `SPECTRA_MAX_UPLOAD_BYTES`: 1 GiB.
pub const DEFAULT_MAX_UPLOAD_BYTES: u64 = 1 << 30;

/// Upload limits, for the Studio.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadLimits {
    /// Largest file a resumable upload accepts.
    pub max_bytes: u64,
    /// Largest chunk.
    pub chunk_bytes: u64,
    /// Unfinished uploads a workspace may have at once.
    pub max_open_sessions: i32,
}

/// `POST /api/v1/uploads/sessions` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSession {
    /// The file's name (display only).
    pub filename: String,
    /// The file's size in bytes.
    pub size: u64,
}

/// A resumable upload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionView {
    /// Session id.
    pub id: Uuid,
    /// File name.
    pub filename: String,
    /// The whole file, in bytes.
    pub size: u64,
    /// Bytes received: where the next chunk starts.
    pub received: u64,
    /// Largest chunk.
    pub chunk_bytes: u64,
    /// The upload it became, once completed.
    pub upload_id: Option<Uuid>,
    /// Removed at this time unless more arrives (UTC, RFC 3339).
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<spectra_db::uploads::UploadSession> for SessionView {
    fn from(s: spectra_db::uploads::UploadSession) -> Self {
        Self {
            id: s.id,
            filename: s.filename,
            size: u64::try_from(s.size_bytes).unwrap_or(0),
            received: u64::try_from(s.received_bytes).unwrap_or(0),
            chunk_bytes: CHUNK_BYTES,
            upload_id: s.upload_id,
            expires_at: s.expires_at,
        }
    }
}

/// `PUT .../chunk` query.
#[derive(Debug, Deserialize, IntoParams)]
pub struct ChunkQuery {
    /// Where this chunk starts: the bytes received so far.
    pub offset: u64,
}

/// After a chunk.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChunkView {
    /// Bytes received so far.
    pub received: u64,
}

const NO_SESSION: &str = "No such upload (it may have expired after 24 hours idle); start again.";

/// The staging directory, or the response saying why there is none.
#[allow(clippy::result_large_err)] // the error is the finished response
fn staging_dir(st: &AppState) -> Result<PathBuf, Response> {
    st.cfg.artifacts.as_ref().map(spectra_db::artifacts::ArtifactBackend::staging_dir).ok_or_else(|| {
        error_response(503, "Uploads need the artifact store, which this server was started without.")
    })
}

/// A session's staging file.
pub(crate) fn session_file(staging: &FsPath, id: Uuid) -> PathBuf {
    staging.join(format!("session-{}.part", id.simple()))
}

/// Upload limits.
#[utoipa::path(get, path = "/api/v1/uploads/limits", tag = "datasets",
    responses((status = 200, body = UploadLimits)))]
#[allow(clippy::unused_async)] // axum handlers are async fns
pub(crate) async fn limits(State(st): State<AppState>) -> Response {
    to_json(
        200,
        &UploadLimits {
            max_bytes: st.cfg.max_upload_bytes,
            chunk_bytes: CHUNK_BYTES,
            max_open_sessions: MAX_OPEN_SESSIONS,
        },
    )
}

/// Start a resumable upload.
#[utoipa::path(post, path = "/api/v1/uploads/sessions", tag = "datasets", request_body = CreateSession,
    responses(
        (status = 201, body = SessionView),
        (status = 409, description = "Too many unfinished uploads (code `too_many_uploads`)", body = crate::openapi::ErrorDoc),
        (status = 413, description = "Over the size limit, or the storage quota (code `quota_exceeded`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every invalid field (`details.errors`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateSession = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid upload request: {e}")),
    };
    if let Err(r) = staging_dir(&st) {
        return r;
    }
    let mut errors = Vec::new();
    if req.size == 0 {
        errors.push(("size".to_string(), "the file is empty".to_string()));
    }
    let filename = clean_filename(&req.filename);
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    if req.size > st.cfg.max_upload_bytes {
        return error_response(
            413,
            format!("Files are limited to {} bytes; this one has {}.", st.cfg.max_upload_bytes, req.size),
        );
    }
    let size = i64::try_from(req.size).unwrap_or(i64::MAX);
    // The bytes count against the quota once recorded; refuse early if they can't fit.
    match spectra_db::artifacts::quota(&pool, ws, &"0".repeat(64), st.cfg.artifact_quota_bytes).await {
        Ok(Some(q)) if q.used + size > q.quota => {
            return error_with_code(
                413,
                "quota_exceeded",
                format!(
                    "This workspace's storage is {} of {} bytes; the file needs {size}.",
                    q.used, q.quota
                ),
            );
        }
        Ok(_) => {}
        Err(e) => return db_error(&e),
    }
    let user = Uuid::parse_str(&id.user).ok();
    match spectra_db::uploads::create_session(&pool, ws, user, &filename, size, MAX_OPEN_SESSIONS).await {
        Ok(Some(s)) => to_json(201, &SessionView::from(s)),
        Ok(None) => error_with_code(
            409,
            "too_many_uploads",
            format!(
                "This workspace already has {MAX_OPEN_SESSIONS} unfinished uploads; finish or cancel one."
            ),
        ),
        Err(e) => db_error(&e),
    }
}

/// How far a resumable upload has got.
#[utoipa::path(get, path = "/api/v1/uploads/sessions/{id}", operation_id = "get_upload_session", tag = "datasets",
    params(("id" = Uuid, Path, description = "Session id")),
    responses((status = 200, body = SessionView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(sid)) = path else { return error_response(404, NO_SESSION) };
    match spectra_db::uploads::get_session(&pool, ws, sid).await {
        Ok(Some(s)) => to_json(200, &SessionView::from(s)),
        Ok(None) => error_response(404, NO_SESSION),
        Err(e) => db_error(&e),
    }
}

/// Writes `data` at `offset` in `path`, cutting off anything after `offset`
/// first (a chunk interrupted earlier).
fn write_at(path: &FsPath, offset: u64, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(path)?;
    f.set_len(offset)?;
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(data)?;
    f.sync_data()
}

/// Send one chunk of a resumable upload.
#[utoipa::path(put, path = "/api/v1/uploads/sessions/{id}/chunk", tag = "datasets",
    params(("id" = Uuid, Path, description = "Session id"), ChunkQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "At most 8 MiB"),
    responses(
        (status = 200, body = ChunkView),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "Not where the upload is (code `offset_mismatch`, `details.received`), or already completed", body = crate::openapi::ErrorDoc),
        (status = 413, description = "Chunk over 8 MiB", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Empty, or past the file's size", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn chunk(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<ChunkQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(sid)) = path else { return error_response(404, NO_SESSION) };
    let Ok(Query(q)) = query else {
        return error_response(422, "offset: a whole number of bytes is required.");
    };
    let Ok(data) = body else { return error_response(400, "The chunk was not received in full.") };
    if data.is_empty() {
        return error_response(422, "The chunk is empty.");
    }
    if data.len() as u64 > CHUNK_BYTES {
        return error_response(413, format!("Chunks are at most {CHUNK_BYTES} bytes."));
    }
    let staging = match staging_dir(&st) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let Ok(mut tx) = pool.begin().await else {
        return error_response(503, "The database is unavailable; try again.");
    };
    let session = match spectra_db::uploads::lock_session(&mut tx, ws, sid).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, NO_SESSION),
        Err(e) => return db_error(&e),
    };
    if session.upload_id.is_some() {
        return error_with_code(409, "already_complete", "This upload is already complete.");
    }
    let received = u64::try_from(session.received_bytes).unwrap_or(0);
    if q.offset != received {
        return error_with_details(
            409,
            "offset_mismatch",
            format!("The upload has {received} bytes; send the chunk that starts there."),
            Json::object().with("received", Json::integer(session.received_bytes)),
        );
    }
    let size = u64::try_from(session.size_bytes).unwrap_or(0);
    if q.offset + data.len() as u64 > size {
        return error_response(422, format!("The chunk goes past the file's {size} bytes."));
    }
    let file = session_file(&staging, sid);
    let (offset, len) = (q.offset, data.len() as u64);
    let written = tokio::task::spawn_blocking(move || write_at(&file, offset, &data)).await;
    if !matches!(written, Ok(Ok(()))) {
        tracing::error!(?written, "upload chunk not written");
        return error_response(503, "Could not store the chunk; send it again.");
    }
    let offset_i = session.received_bytes;
    let len_i = i64::try_from(len).unwrap_or(i64::MAX);
    match spectra_db::uploads::advance_session(&mut tx, ws, sid, offset_i, len_i).await {
        Ok(Some(total)) => {
            if let Err(e) = tx.commit().await {
                return db_error(&e);
            }
            to_json(200, &ChunkView { received: u64::try_from(total).unwrap_or(0) })
        }
        Ok(None) => error_response(409, "The upload changed meanwhile; ask where it is and continue."),
        Err(e) => db_error(&e),
    }
}

/// SHA-256 and length of a file, read in 1 MiB pieces.
fn hash_file(path: &FsPath) -> std::io::Result<(String, u64)> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n_total = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        n_total += n as u64;
    }
    Ok((h.hex_digest(), n_total))
}

/// Finish a resumable upload: the file becomes an upload, ready to ingest.
#[utoipa::path(post, path = "/api/v1/uploads/sessions/{id}/complete", tag = "datasets",
    params(("id" = Uuid, Path, description = "Session id")),
    responses(
        (status = 201, description = "Stored (a retry answers 200 with the same upload)", body = UploadView),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "Not every byte has arrived (code `incomplete`, `details.received`)", body = crate::openapi::ErrorDoc),
        (status = 413, description = "Storage quota (code `quota_exceeded`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn complete(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Extension(request_id): Extension<RequestId>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(sid)) = path else { return error_response(404, NO_SESSION) };
    let staging = match staging_dir(&st) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let Ok(mut tx) = pool.begin().await else {
        return error_response(503, "The database is unavailable; try again.");
    };
    let session = match spectra_db::uploads::lock_session(&mut tx, ws, sid).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, NO_SESSION),
        Err(e) => return db_error(&e),
    };
    if let Some(upload_id) = session.upload_id {
        // A retry: the same upload again.
        return match spectra_db::uploads::get(&pool, ws, upload_id).await {
            Ok(Some(u)) => to_json(
                200,
                &UploadView {
                    id: u.id,
                    sha256: u.sha256,
                    size: u64::try_from(u.size_bytes).unwrap_or(0),
                    filename: u.filename,
                    expires_at: u.expires_at,
                },
            ),
            Ok(None) => error_response(404, "The upload this became has expired; upload the file again."),
            Err(e) => db_error(&e),
        };
    }
    if session.received_bytes != session.size_bytes {
        return error_with_details(
            409,
            "incomplete",
            format!("{} of {} bytes have arrived.", session.received_bytes, session.size_bytes),
            Json::object().with("received", Json::integer(session.received_bytes)),
        );
    }
    let file = session_file(&staging, sid);
    let to_hash = file.clone();
    let Ok(Ok((sha256, length))) = tokio::task::spawn_blocking(move || hash_file(&to_hash)).await else {
        return error_response(503, "The received file could not be read; upload it again.");
    };
    if i64::try_from(length).ok() != Some(session.size_bytes) {
        return error_response(409, "The received file is not the expected size; upload it again.");
    }
    let user = Uuid::parse_str(&id.user).ok();
    let view = match store_and_record(
        &st,
        &pool,
        ws,
        user,
        &request_id.0,
        &file,
        &sha256,
        length,
        &session.filename,
    )
    .await
    {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(e) = spectra_db::uploads::finish_session(&mut tx, ws, sid, view.id).await {
        return db_error(&e);
    }
    if let Err(e) = tx.commit().await {
        return db_error(&e);
    }
    to_json(201, &view)
}

/// Abandon a resumable upload.
#[utoipa::path(delete, path = "/api/v1/uploads/sessions/{id}", tag = "datasets",
    params(("id" = Uuid, Path, description = "Session id")),
    responses((status = 200, body = ChunkView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn abort(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(sid)) = path else { return error_response(404, NO_SESSION) };
    match spectra_db::uploads::delete_session(&pool, ws, sid).await {
        Ok(true) => {
            if let Ok(staging) = staging_dir(&st) {
                let _ = tokio::fs::remove_file(session_file(&staging, sid)).await;
            }
            to_json(200, &ChunkView { received: 0 })
        }
        Ok(false) => error_response(404, NO_SESSION),
        Err(e) => db_error(&e),
    }
}

/// Removes sessions idle past their expiry and their staging files. Returns
/// how many went.
///
/// # Errors
/// Any database error.
pub async fn sweep_expired(pool: &spectra_db::PgPool, staging: &FsPath) -> Result<usize, spectra_db::Error> {
    let ids = spectra_db::uploads::expire_sessions(pool, 500).await?;
    for sid in &ids {
        let _ = tokio::fs::remove_file(session_file(staging, *sid)).await;
    }
    Ok(ids.len())
}
