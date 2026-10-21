//! `POST /api/v1/uploads` (M1 task 4): a whole data file, streamed into the
//! artifact store.
//!
//! The signed proxy route (`web/frontend/app/api/engine-upload/route.ts`)
//! receives the browser's file, hashes it while saving it aside, then sends it
//! here with the usual signature headers plus `X-Spectra-Content-SHA256`.
//! The signature covers that hash (it is the canonical string's body hash),
//! so this route checks the signature first, then streams the body to a
//! staging file while hashing it, and refuses it unless it matches. Nothing
//! larger than [`MAX_UPLOAD_BYTES`] is read.
//!
//! The file name travels in the signed query string (`?filename=`). The bytes
//! count against the workspace's storage quota; an upload that is not
//! ingested within 24 hours is removed by the worker.

use axum::extract::{Query, Request, State};
use axum::http::header;
use axum::response::Response;
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use spectra_core::Sha256;
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{auth, error_response, error_with_code, mirror_identity, AppState, RequestId};

/// Largest upload, in bytes (100 MiB).
pub const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// `POST /api/v1/uploads` query.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UploadQuery {
    /// The file's name as the person chose it (for display; at most 255 characters kept).
    pub filename: Option<String>,
}

/// A stored upload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadView {
    /// Pass this to `POST /api/v1/datasets/ingest`.
    pub id: Uuid,
    /// Hex SHA-256 of the file.
    pub sha256: String,
    /// Bytes.
    pub size: u64,
    /// As uploaded.
    pub filename: String,
    /// Removed at this time unless ingested (UTC, RFC 3339).
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Keeps a file name for display: no path, no control characters.
pub(crate) fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    base.chars().filter(|c| !c.is_control()).take(255).collect::<String>().trim().to_string()
}

/// Upload a data file (raw bytes; at most 100 MiB).
#[utoipa::path(post, path = "/api/v1/uploads", tag = "datasets", params(UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream",
        description = "The file's bytes. Send X-Spectra-Content-SHA256 (signed) and Content-Length."),
    responses(
        (status = 201, description = "Stored", body = UploadView),
        (status = 400, description = "The body does not match its declared SHA-256 or length", body = crate::openapi::ErrorDoc),
        (status = 401, description = "Bad signature", body = crate::openapi::ErrorDoc),
        (status = 411, description = "No Content-Length", body = crate::openapi::ErrorDoc),
        (status = 413, description = "Over 100 MiB, or the workspace's storage quota (code `quota_exceeded`)", body = crate::openapi::ErrorDoc),
    ))]
#[allow(clippy::too_many_lines)] // one linear flow: check, stream, store, record
pub(crate) async fn upload(State(st): State<AppState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());

    // 1. The signature, over the declared hash; nothing is read before it checks out.
    let Some(declared) = header(auth::headers::CONTENT_SHA256).map(str::to_ascii_lowercase) else {
        return error_response(400, "Missing X-Spectra-Content-SHA256.");
    };
    if !is_sha256_hex(&declared) {
        return error_response(400, "X-Spectra-Content-SHA256 must be 64 hex characters.");
    }
    let signed = auth::SignedRequest {
        method: parts.method.as_str(),
        path_and_query: parts.uri.path_and_query().map_or_else(|| parts.uri.path(), |p| p.as_str()),
        user: header(auth::headers::USER),
        workspace: header(auth::headers::WORKSPACE),
        request_id: header(auth::headers::REQUEST_ID),
        timestamp: header(auth::headers::TIMESTAMP),
        signature: header(auth::headers::SIGNATURE),
        body: &[],
    };
    let id = match auth::verify_with_body_hash(
        st.cfg.engine_secret.expose(),
        &signed,
        &declared,
        crate::now_secs(),
    ) {
        Ok(id) => id,
        Err(reason) => return error_response(401, reason),
    };
    mirror_identity(&st, &id).await;
    let request_id = parts.extensions.get::<RequestId>().map_or_else(String::new, |r| r.0.clone());

    let Some(length) = header(header::CONTENT_LENGTH.as_str()).and_then(|v| v.trim().parse::<u64>().ok())
    else {
        return error_response(411, "Uploads need a Content-Length.");
    };
    if length > MAX_UPLOAD_BYTES {
        return error_response(413, "Uploads are limited to 100 MiB for now.");
    }
    if length == 0 {
        return error_response(400, "The file is empty.");
    }
    let filename = Query::<UploadQuery>::try_from_uri(&parts.uri)
        .ok()
        .and_then(|q| q.0.filename)
        .map(|n| clean_filename(&n))
        .unwrap_or_default();
    let (pool, ws) = match crate::jobs_api::context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Some(store) = st.cfg.artifacts.clone() else {
        return error_response(
            503,
            "Uploads need the artifact store, which this server was started without.",
        );
    };
    let user = Uuid::parse_str(&id.user).ok();
    let size = i64::try_from(length).unwrap_or(i64::MAX);

    // 2. Room for it? (Checked again, under a lock, when the row is recorded.)
    match spectra_db::artifacts::quota(&pool, ws, &declared, st.cfg.artifact_quota_bytes).await {
        Ok(Some(q)) if !q.present && q.used + size > q.quota => {
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
        Err(e) => return crate::jobs_api::db_error(&e),
    }

    // 3. Stream to a staging file, hashing; stop at the declared length.
    let staging = store.staging_dir();
    if let Err(e) = tokio::fs::create_dir_all(&staging).await {
        tracing::error!(error = %e, "cannot create the upload staging directory");
        return error_response(503, "Cannot store uploads right now.");
    }
    let path = staging.join(format!("upload-{}.part", Uuid::now_v7().simple()));
    let streamed = stream_to_file(body, &path, length).await;
    let (sha256, written) = match streamed {
        Ok(r) => r,
        Err(resp) => {
            let _ = tokio::fs::remove_file(&path).await;
            return resp;
        }
    };
    if written != length || sha256 != declared {
        let _ = tokio::fs::remove_file(&path).await;
        return error_response(400, "The file did not arrive intact (size or SHA-256 differs); try again.");
    }

    // 4. Into the store (a rename for the local store), then the row.
    match store_and_record(&st, &pool, ws, user, &request_id, &path, &sha256, length, &filename).await {
        Ok(view) => crate::jobs_api::to_json(201, &view),
        Err(r) => r,
    }
}

/// Moves a staged file whose SHA-256 is known into the artifact store,
/// records the upload (re-checking the quota under a lock) and audits it.
/// Shared by single uploads and completed resumable ones.
#[allow(clippy::too_many_arguments, clippy::result_large_err)] // the error is the finished response
pub(crate) async fn store_and_record(
    st: &AppState,
    pool: &spectra_db::PgPool,
    ws: Uuid,
    user: Option<Uuid>,
    request_id: &str,
    path: &std::path::Path,
    sha256: &str,
    length: u64,
    filename: &str,
) -> Result<UploadView, Response> {
    let Some(store) = st.cfg.artifacts.clone() else {
        return Err(error_response(
            503,
            "Uploads need the artifact store, which this server was started without.",
        ));
    };
    if let Err(e) = store.put_file(path.to_path_buf(), sha256).await {
        let _ = tokio::fs::remove_file(path).await;
        tracing::error!(error = %e, "upload not stored");
        return Err(error_response(503, "Could not store the file; try again."));
    }
    let size = i64::try_from(length).unwrap_or(i64::MAX);
    let recorded = spectra_db::uploads::record(
        pool,
        &spectra_db::uploads::NewUpload {
            workspace_id: ws,
            sha256,
            size_bytes: size,
            filename,
            created_by: user,
            default_quota: st.cfg.artifact_quota_bytes,
        },
    )
    .await;
    let (upload_id, expires_at) = match recorded {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Err(error_with_code(413, "quota_exceeded", "This workspace's storage quota is full."))
        }
        Err(e) => return Err(crate::jobs_api::db_error(&e)),
    };
    let audit = spectra_db::uploads::audit(
        pool,
        &spectra_db::uploads::AuditEvent {
            workspace_id: ws,
            actor_id: user,
            action: "upload.created",
            target_type: "upload",
            target_id: upload_id.to_string(),
            request_id,
            details: serde_json::json!({ "filename": filename, "size": length, "sha256": sha256 }),
        },
    )
    .await;
    if let Err(e) = audit {
        tracing::error!(error = %e, %upload_id, "upload not written to the audit log");
    }
    Ok(UploadView {
        id: upload_id,
        sha256: sha256.to_string(),
        size: length,
        filename: filename.to_string(),
        expires_at,
    })
}

/// Writes the body to `path`, hashing it, refusing more than `limit` bytes.
#[allow(clippy::result_large_err)] // the error is the finished response
async fn stream_to_file(
    mut body: axum::body::Body,
    path: &std::path::Path,
    limit: u64,
) -> Result<(String, u64), Response> {
    let mut file = tokio::fs::File::create(path).await.map_err(|e| {
        tracing::error!(error = %e, "cannot create an upload staging file");
        error_response(503, "Cannot store uploads right now.")
    })?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| error_response(400, "The upload was interrupted."))?;
        let Ok(chunk) = frame.into_data() else { continue };
        written += chunk.len() as u64;
        if written > limit {
            return Err(error_response(400, "The body is longer than its Content-Length."));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| {
            tracing::error!(error = %e, "upload staging write failed");
            error_response(503, "Could not store the file; try again.")
        })?;
    }
    file.sync_all().await.map_err(|_| error_response(503, "Could not store the file; try again."))?;
    Ok((hasher.hex_digest(), written))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_keep_only_the_base_name() {
        assert_eq!(clean_filename("C:\\Users\\me\\prices.csv"), "prices.csv");
        assert_eq!(clean_filename("../../etc/passwd"), "passwd");
        assert_eq!(clean_filename("  spy\u{0}daily.csv "), "spydaily.csv");
        assert_eq!(clean_filename(&"x".repeat(400)).len(), 255);
    }
}
