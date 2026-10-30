//! `/api/v1/jobs`: queue engine work for `spectra-worker`, follow it, cancel it.
//!
//! Every route acts within the caller's workspace (from the signed
//! `X-Spectra-Workspace` header); a job in another workspace is a 404.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_core::Json;
use spectra_db::jobs::{self, Job, JobStatus, NewJob};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{engine, error_response, error_with_code, json_response, AppState, Identity};

/// Attempts a job gets before it fails for good (retryable failures only).
pub const MAX_ATTEMPTS: i32 = 3;

/// `POST /api/v1/jobs` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateJob {
    /// One of `benchmark`, `risk`, `stress`.
    pub kind: String,
    /// The same body the matching synchronous route takes.
    #[schema(value_type = Object)]
    pub payload: serde_json::Value,
    /// -10..10; higher runs first. Default 0.
    #[serde(default)]
    pub priority: i32,
}

/// A job as the API shows it (the payload is not echoed: it can be megabytes).
#[derive(Debug, Serialize, ToSchema)]
pub struct JobView {
    /// UUIDv7.
    pub id: Uuid,
    /// What runs.
    pub kind: String,
    /// `queued`, `running`, `succeeded`, `failed` or `cancelled`.
    pub status: String,
    /// Higher runs first.
    pub priority: i32,
    /// Attempts started.
    pub attempts: i32,
    /// Attempts allowed.
    pub max_attempts: i32,
    /// 0..1.
    pub progress: f32,
    /// A cancel was asked for while it ran.
    pub cancel_requested: bool,
    /// The latest error.
    pub error: Option<String>,
    /// On success: `{run_id, report, artifact}`.
    #[schema(value_type = Option<Object>)]
    pub result: Option<serde_json::Value>,
    /// Queued at (UTC, RFC 3339).
    pub created_at: DateTime<Utc>,
    /// Latest attempt started at.
    pub started_at: Option<DateTime<Utc>>,
    /// Finished at.
    pub finished_at: Option<DateTime<Utc>>,
    /// Not run before (retry backoff).
    pub run_after: DateTime<Utc>,
}

impl From<Job> for JobView {
    fn from(j: Job) -> Self {
        Self {
            id: j.id,
            kind: j.kind,
            status: j.status,
            priority: j.priority,
            attempts: j.attempts,
            max_attempts: j.max_attempts,
            progress: j.progress,
            cancel_requested: j.cancel_requested,
            error: j.error,
            result: j.result,
            created_at: j.created_at,
            started_at: j.started_at,
            finished_at: j.finished_at,
            run_after: j.run_after,
        }
    }
}

/// A page of jobs.
#[derive(Debug, Serialize, ToSchema)]
pub struct JobPage {
    /// Newest first.
    pub items: Vec<JobView>,
    /// Pass as `cursor` for the next page; null on the last.
    pub next_cursor: Option<Uuid>,
}

/// `GET /api/v1/jobs` query.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ListJobs {
    /// Only jobs in this status.
    pub status: Option<String>,
    /// `next_cursor` from the previous page.
    pub cursor: Option<Uuid>,
    /// 1..200, default 50.
    pub limit: Option<u32>,
}

pub(crate) fn to_json<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_string(value) {
        Ok(text) => json_response(status, text),
        Err(_) => error_response(500, "Could not encode the response."),
    }
}

pub(crate) fn db_error(e: &spectra_db::Error) -> Response {
    // 23503: foreign key violation, i.e. the signed workspace or user id does not exist.
    if e.as_database_error().is_some_and(|d| d.code().as_deref() == Some("23503")) {
        return error_response(422, "The workspace or user for this job does not exist.");
    }
    tracing::error!(error = %e, "jobs database error");
    error_response(503, "The database is unavailable; try again.")
}

/// The pool and the caller's workspace, or the response explaining why not.
#[allow(clippy::result_large_err)] // the error is the finished response
pub(crate) fn context(st: &AppState, id: &Identity) -> Result<(spectra_db::PgPool, Uuid), Response> {
    let Some(pool) = st.db.clone() else {
        return Err(error_response(503, "Jobs need the database, which this server was started without."));
    };
    let Some(ws) = id.workspace else {
        return Err(error_with_code(400, "no_workspace", "This account has no workspace yet."));
    };
    Ok((pool, ws))
}

/// Queue a job.
#[utoipa::path(post, path = "/api/v1/jobs", tag = "jobs", request_body = CreateJob,
    responses(
        (status = 202, description = "Queued", body = JobView),
        (status = 400, description = "The account has no workspace (code `no_workspace`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Unknown kind or an invalid payload", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_job(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateJob = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid job request: {e}")),
    };
    match submit(&st, &id, req.kind, req.payload, req.priority).await {
        Ok(job) => to_json(202, &JobView::from(job)),
        Err(r) => r,
    }
}

/// Validates and queues a job in the caller's workspace: the shared path of
/// `POST /api/v1/jobs` and `POST /api/v1/backtests`.
///
/// # Errors
/// The finished error response (400, 422, 503).
#[allow(clippy::result_large_err)] // the error is the finished response
pub(crate) async fn submit(
    st: &AppState,
    id: &Identity,
    kind: String,
    mut payload: serde_json::Value,
    priority: i32,
) -> Result<Job, Response> {
    let (pool, ws) = context(st, id)?;
    if !(-10..=10).contains(&priority) {
        return Err(error_response(422, "priority must be between -10 and 10."));
    }
    // A saved strategy version stands for a built-in and its parameters (M2a task 4).
    if kind == spectra_backtest::JOB_KIND {
        crate::strategies_api::apply_version(&pool, ws, &mut payload).await?;
    }
    // Validate exactly as the synchronous route would, off the async threads
    // (parsing a large portfolio CSV is real work).
    let (k, payload_text) = (kind.clone(), payload.to_string());
    let checked = tokio::task::spawn_blocking(move || {
        let payload = Json::parse(&payload_text).map_err(|e| (422, format!("Invalid payload: {e}")))?;
        engine::validate_job(&k, &payload)
    })
    .await;
    match checked {
        Ok(Ok(())) => {}
        Ok(Err((code, msg))) => return Err(error_response(code, msg)),
        Err(_) => return Err(error_response(500, "Could not validate the job.")),
    }
    // A backtest must name a dataset version of this workspace; say so now rather than on the worker.
    if kind == spectra_backtest::JOB_KIND {
        let version = Json::parse(&payload.to_string())
            .ok()
            .and_then(|p| engine::backtest_job(&p).ok())
            .and_then(|j| Uuid::parse_str(&j.dataset_version_id).ok());
        let Some(version) = version else { return Err(error_response(422, "Invalid backtest payload.")) };
        let access = spectra_db::datasets::Access { purpose: "job", user: Uuid::parse_str(&id.user).ok() };
        match spectra_db::datasets::resolve_version(&pool, ws, version, access).await {
            Ok(spectra_db::datasets::Resolved::Found(_)) => {}
            Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
                return Err(crate::optimization_api::sealed_response(&holdout, "backtests"));
            }
            Ok(spectra_db::datasets::Resolved::Missing) => {
                return Err(error_response(
                    422,
                    "dataset_version_id: no such dataset version in this workspace.",
                ));
            }
            Err(e) => return Err(db_error(&e)),
        }
    }
    let new = NewJob {
        workspace_id: ws,
        kind,
        payload,
        priority,
        max_attempts: MAX_ATTEMPTS,
        created_by: Uuid::parse_str(&id.user).ok(),
    };
    jobs::enqueue(&pool, &new).await.map_err(|e| db_error(&e))
}

/// One job.
#[utoipa::path(get, path = "/api/v1/jobs/{id}", tag = "jobs",
    params(("id" = Uuid, Path, description = "Job id")),
    responses((status = 200, body = JobView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_job(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(job_id)) = job_id else { return error_response(404, "No such job.") };
    match jobs::get(&pool, ws, job_id).await {
        Ok(Some(job)) => to_json(200, &JobView::from(job)),
        Ok(None) => error_response(404, "No such job."),
        Err(e) => db_error(&e),
    }
}

/// The workspace's jobs, newest first.
#[utoipa::path(get, path = "/api/v1/jobs", tag = "jobs",
    params(
        ("status" = Option<String>, Query, description = "queued, running, succeeded, failed or cancelled"),
        ("cursor" = Option<Uuid>, Query, description = "next_cursor from the previous page"),
        ("limit" = Option<u32>, Query, description = "1..200, default 50"),
    ),
    responses((status = 200, body = JobPage), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn list_jobs(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<ListJobs>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query: use status, cursor and limit.");
    };
    let status = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => match JobStatus::parse(s) {
            Some(v) => Some(v),
            None => {
                return error_response(422, "status must be queued, running, succeeded, failed or cancelled.")
            }
        },
    };
    match jobs::list(&pool, ws, status, q.cursor, q.limit.unwrap_or(50)).await {
        Ok(page) => to_json(
            200,
            &JobPage {
                items: page.items.into_iter().map(JobView::from).collect(),
                next_cursor: page.next_cursor,
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Cancel a job: queued jobs stop at once, running jobs at the worker's next
/// check (within a few seconds). Finished jobs are returned unchanged.
#[utoipa::path(post, path = "/api/v1/jobs/{id}/cancel", tag = "jobs",
    params(("id" = Uuid, Path, description = "Job id")),
    responses((status = 200, body = JobView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn cancel_job(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(job_id)) = job_id else { return error_response(404, "No such job.") };
    match jobs::request_cancel(&pool, ws, job_id).await {
        Ok(Some(job)) => to_json(200, &JobView::from(job)),
        Ok(None) => error_response(404, "No such job."),
        Err(e) => db_error(&e),
    }
}
