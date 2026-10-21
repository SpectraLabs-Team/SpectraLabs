//! Files uploaded through the Studio (M1 tasks 4-5), and the audit log.
//!
//! An upload's bytes go into the artifact store first (content-addressed, so
//! the same file is stored once); [`record`] then adds the `uploads` row,
//! counting the bytes against the workspace's storage quota. An `ingest` job
//! turns it into a dataset version and [`mark_ingested`] keeps it as that
//! version's source. Uploads never ingested are removed after their expiry
//! by [`expire`], which also says which stored files nothing uses any more.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// How long an upload waits to be ingested before it is removed.
pub const UPLOAD_TTL_SECS: i32 = 24 * 60 * 60;

/// An uploaded file to record.
#[derive(Debug, Clone)]
pub struct NewUpload<'a> {
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// Lower-case hex SHA-256 of the bytes (already in the artifact store).
    pub sha256: &'a str,
    /// Byte count.
    pub size_bytes: i64,
    /// The browser's file name, for display.
    pub filename: &'a str,
    /// Who uploaded it.
    pub created_by: Option<Uuid>,
    /// Quota for workspaces without their own.
    pub default_quota: i64,
}

/// A recorded upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    /// Upload id.
    pub id: Uuid,
    /// SHA-256 of the bytes.
    pub sha256: String,
    /// Byte count.
    pub size_bytes: i64,
    /// File name as uploaded.
    pub filename: String,
    /// Who uploaded it.
    pub created_by: Option<Uuid>,
    /// When.
    pub created_at: DateTime<Utc>,
    /// When it is removed unless ingested.
    pub expires_at: DateTime<Utc>,
    /// When it became a dataset version, if it has.
    pub ingested_at: Option<DateTime<Utc>>,
}

/// Records an upload whose bytes are in the artifact store. `Ok(None)` when
/// the workspace's quota would be exceeded (checked under a lock on the
/// workspace, as for run artifacts).
///
/// # Errors
/// Any database error.
pub async fn record(pool: &PgPool, u: &NewUpload<'_>) -> Result<Option<(Uuid, DateTime<Utc>)>, Error> {
    sqlx::query_as(include_str!("../queries/upload_insert.sql"))
        .bind(u.workspace_id)
        .bind(u.sha256)
        .bind(u.size_bytes)
        .bind(u.filename)
        .bind(u.created_by)
        .bind(u.default_quota)
        .bind(UPLOAD_TTL_SECS)
        .fetch_optional(pool)
        .await
}

/// The upload `id` of `workspace_id`, unless it has expired.
///
/// # Errors
/// Any database error.
pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<Upload>, Error> {
    type Row = (Uuid, String, i64, String, Option<Uuid>, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>);
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/upload_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, sha256, size_bytes, filename, created_by, created_at, expires_at, ingested_at)| {
        Upload { id, sha256, size_bytes, filename, created_by, created_at, expires_at, ingested_at }
    }))
}

/// Keeps an upload (it became a dataset version's source).
///
/// # Errors
/// Any database error.
pub async fn mark_ingested(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<(), Error> {
    sqlx::query(include_str!("../queries/upload_mark_ingested.sql"))
        .bind(workspace_id)
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Removes up to `limit` expired, never-ingested uploads. Returns the hashes
/// nothing references any more, whose bytes the caller may delete from the
/// artifact store.
///
/// # Errors
/// Any database error.
pub async fn expire(pool: &PgPool, limit: i32) -> Result<Vec<String>, Error> {
    sqlx::query_scalar(include_str!("../queries/upload_expire.sql")).bind(limit).fetch_all(pool).await
}

/// One audit-log entry.
#[derive(Debug, Clone)]
pub struct AuditEvent<'a> {
    /// Workspace it happened in.
    pub workspace_id: Uuid,
    /// Who did it, if known.
    pub actor_id: Option<Uuid>,
    /// What happened: `upload.created`, `dataset.version_created`, ...
    pub action: &'a str,
    /// Kind of the thing acted on: `upload`, `dataset_version`, ...
    pub target_type: &'a str,
    /// Its id.
    pub target_id: String,
    /// The request that did it, if any.
    pub request_id: &'a str,
    /// Anything else worth keeping (file name, size, hashes).
    pub details: serde_json::Value,
}

/// Appends to the audit log.
///
/// # Errors
/// Any database error.
pub async fn audit(pool: &PgPool, e: &AuditEvent<'_>) -> Result<i64, Error> {
    sqlx::query_scalar(include_str!("../queries/audit_insert.sql"))
        .bind(e.workspace_id)
        .bind(e.actor_id)
        .bind(e.action)
        .bind(e.target_type)
        .bind(&e.target_id)
        .bind(e.request_id)
        .bind(&e.details)
        .fetch_one(pool)
        .await
}

// ---------------------------------------------------------------------------
// Resumable uploads (M1b task 9)
// ---------------------------------------------------------------------------

/// How long a resumable upload may sit idle before it is removed.
pub const SESSION_TTL_SECS: i32 = 24 * 60 * 60;

/// A resumable upload in progress (or finished: `upload_id` set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadSession {
    /// Session id (also names its staging file).
    pub id: Uuid,
    /// File name as given.
    pub filename: String,
    /// Bytes the file will have.
    pub size_bytes: i64,
    /// Bytes received so far: where the next chunk starts.
    pub received_bytes: i64,
    /// The upload it became, once completed.
    pub upload_id: Option<Uuid>,
    /// Removed after this unless more arrives.
    pub expires_at: DateTime<Utc>,
}

type SessionRow = (Uuid, String, i64, i64, Option<Uuid>, DateTime<Utc>);

fn session((id, filename, size_bytes, received_bytes, upload_id, expires_at): SessionRow) -> UploadSession {
    UploadSession { id, filename, size_bytes, received_bytes, upload_id, expires_at }
}

/// Opens a session. `Ok(None)` when the workspace already has `max_open`
/// unfinished sessions.
///
/// # Errors
/// Any database error.
pub async fn create_session(
    pool: &PgPool,
    workspace_id: Uuid,
    created_by: Option<Uuid>,
    filename: &str,
    size_bytes: i64,
    max_open: i32,
) -> Result<Option<UploadSession>, Error> {
    let row: Option<SessionRow> = sqlx::query_as(include_str!("../queries/upload_session_create.sql"))
        .bind(workspace_id)
        .bind(created_by)
        .bind(filename)
        .bind(size_bytes)
        .bind(SESSION_TTL_SECS)
        .bind(max_open)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(session))
}

/// The session `id` of `workspace_id`, unless it has expired.
///
/// # Errors
/// Any database error.
pub async fn get_session(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<UploadSession>, Error> {
    let row: Option<SessionRow> = sqlx::query_as(include_str!("../queries/upload_session_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(session))
}

/// Locks the session row for the rest of the transaction (chunks of one
/// session are written one at a time).
///
/// # Errors
/// Any database error.
pub async fn lock_session(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<UploadSession>, Error> {
    let row: Option<SessionRow> = sqlx::query_as(include_str!("../queries/upload_session_lock.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(conn)
        .await?;
    Ok(row.map(session))
}

/// Records `len` bytes written at `offset`; the new received total, or
/// `None` if the session is not at `offset` or the chunk would not fit.
///
/// # Errors
/// Any database error.
pub async fn advance_session(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    id: Uuid,
    offset: i64,
    len: i64,
) -> Result<Option<i64>, Error> {
    sqlx::query_scalar(include_str!("../queries/upload_session_advance.sql"))
        .bind(workspace_id)
        .bind(id)
        .bind(offset)
        .bind(len)
        .bind(SESSION_TTL_SECS)
        .fetch_optional(conn)
        .await
}

/// Points a fully received session at its upload.
///
/// # Errors
/// Any database error.
pub async fn finish_session(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    id: Uuid,
    upload_id: Uuid,
) -> Result<(), Error> {
    sqlx::query(include_str!("../queries/upload_session_finish.sql"))
        .bind(workspace_id)
        .bind(id)
        .bind(upload_id)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Abandons a session; `true` if it existed.
///
/// # Errors
/// Any database error.
pub async fn delete_session(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let gone: Option<Uuid> = sqlx::query_scalar(include_str!("../queries/upload_session_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(gone.is_some())
}

/// Removes up to `limit` expired sessions; their ids, so the caller can
/// remove their staging files.
///
/// # Errors
/// Any database error.
pub async fn expire_sessions(pool: &PgPool, limit: i32) -> Result<Vec<Uuid>, Error> {
    sqlx::query_scalar(include_str!("../queries/upload_session_expire.sql")).bind(limit).fetch_all(pool).await
}
