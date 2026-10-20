//! `/api/v1/backtests` and `/api/v1/strategies/builtin` (M3 task 6).
//!
//! A backtest is a job of kind `backtest` (see `docs/backtests.md`): these
//! routes are the backtest-shaped view of the job queue. Creating one goes
//! through the same validation and dataset check as `POST /api/v1/jobs`; a
//! job of another kind is a 404 here. Every route acts within the caller's
//! workspace.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_db::artifacts::find_for_run;
use spectra_db::jobs::{self, Job};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, submit, to_json};
use crate::{engine, error_response, json_response, AppState, Identity};

/// `POST /api/v1/backtests` body: the `backtest` job payload, plus a priority.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBacktest {
    /// A dataset version of the caller's workspace.
    pub dataset_version_id: Uuid,
    /// A built-in strategy name (see `GET /api/v1/strategies/builtin`).
    pub strategy: String,
    /// Strategy parameters; missing ones take their defaults.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
    /// Configuration in the user form (`preset`, `initial_cash`, `slippage_bps`, ...).
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub config: Option<serde_json::Value>,
    /// -10..10; higher runs first. Default 0.
    #[serde(default)]
    pub priority: i32,
}

/// A backtest as the API shows it.
#[derive(Debug, Serialize, ToSchema)]
pub struct BacktestView {
    /// The job id (UUIDv7); use it for every `/backtests/{id}` route.
    pub id: Uuid,
    /// `queued`, `running`, `succeeded`, `failed` or `cancelled`.
    pub status: String,
    /// 0..1, updated with every worker heartbeat while it runs.
    pub progress: f32,
    /// A cancel was asked for while it ran.
    pub cancel_requested: bool,
    /// Why it failed (or the latest retryable error).
    pub error: Option<String>,
    /// What was asked for: dataset version, strategy, params, config.
    #[schema(value_type = Object)]
    pub request: serde_json::Value,
    /// The recorded run, once finished.
    pub run_id: Option<Uuid>,
    /// Engine, dataset, config, result and strategy hashes, once finished.
    #[schema(value_type = Option<Object>)]
    pub fingerprints: Option<serde_json::Value>,
    /// Metrics, drawdown, tail, activity and trade statistics, once finished.
    #[schema(value_type = Option<Object>)]
    pub summary: Option<serde_json::Value>,
    /// Stored files `[{kind, file_name, content_type, sha256, size}]`, once finished.
    #[schema(value_type = Option<Object>)]
    pub artifacts: Option<serde_json::Value>,
    /// Queued at (UTC, RFC 3339).
    pub created_at: DateTime<Utc>,
    /// Latest attempt started at.
    pub started_at: Option<DateTime<Utc>>,
    /// Finished at.
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<Job> for BacktestView {
    fn from(j: Job) -> Self {
        let field = |k: &str| j.result.as_ref().and_then(|r| r.get(k)).filter(|v| !v.is_null()).cloned();
        let run_id = field("run_id").and_then(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()));
        Self {
            run_id,
            fingerprints: field("fingerprints"),
            summary: field("summary"),
            artifacts: field("artifacts"),
            id: j.id,
            status: j.status,
            progress: j.progress,
            cancel_requested: j.cancel_requested,
            error: j.error,
            request: j.payload,
            created_at: j.created_at,
            started_at: j.started_at,
            finished_at: j.finished_at,
        }
    }
}

/// `GET /api/v1/backtests/{id}/series` query.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SeriesQuery {
    /// `equity` (default) or `drawdown`.
    pub kind: Option<String>,
}

const NOT_FOUND: &str = "No such backtest.";

/// The backtest job `job_id` of the caller's workspace, or the response saying why not.
#[allow(clippy::result_large_err)] // the error is the finished response
async fn backtest_job(st: &AppState, id: &Identity, job_id: Uuid) -> Result<Job, Response> {
    let (pool, ws) = context(st, id)?;
    match jobs::get(&pool, ws, job_id).await {
        Ok(Some(job)) if job.kind == spectra_backtest::JOB_KIND => Ok(job),
        Ok(_) => Err(error_response(404, NOT_FOUND)),
        Err(e) => Err(db_error(&e)),
    }
}

/// Queue a backtest.
#[utoipa::path(post, path = "/api/v1/backtests", tag = "backtests", request_body = CreateBacktest,
    responses(
        (status = 202, description = "Queued", body = BacktestView),
        (status = 400, description = "The account has no workspace (code `no_workspace`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every problem with the strategy, params, config or dataset version", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_backtest(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateBacktest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid backtest request: {e}")),
    };
    let mut payload = serde_json::json!({
        "dataset_version_id": req.dataset_version_id.to_string(),
        "strategy": req.strategy,
    });
    if let Some(p) = req.params {
        payload["params"] = p;
    }
    if let Some(c) = req.config {
        payload["config"] = c;
    }
    match submit(&st, &id, spectra_backtest::JOB_KIND.to_string(), payload, req.priority).await {
        Ok(job) => to_json(202, &BacktestView::from(job)),
        Err(r) => r,
    }
}

/// One backtest: status and progress, then the summary and fingerprints.
#[utoipa::path(get, path = "/api/v1/backtests/{id}", tag = "backtests",
    params(("id" = Uuid, Path, description = "Backtest (job) id")),
    responses((status = 200, body = BacktestView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_backtest(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(job_id)) = job_id else { return error_response(404, NOT_FOUND) };
    match backtest_job(&st, &id, job_id).await {
        Ok(job) => to_json(200, &BacktestView::from(job)),
        Err(r) => r,
    }
}

/// Cancel a backtest: a queued one stops at once, a running one at its next
/// bar group (the worker checks within a few seconds).
#[utoipa::path(post, path = "/api/v1/backtests/{id}/cancel", tag = "backtests",
    params(("id" = Uuid, Path, description = "Backtest (job) id")),
    responses((status = 200, body = BacktestView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn cancel_backtest(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(job_id)) = job_id else { return error_response(404, NOT_FOUND) };
    if let Err(r) = backtest_job(&st, &id, job_id).await {
        return r;
    }
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match jobs::request_cancel(&pool, ws, job_id).await {
        Ok(Some(job)) => to_json(200, &BacktestView::from(job)),
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

/// A finished backtest's equity or drawdown series, downsampled for charts:
/// `{kind, total_points, downsampled, points: [[ts_ns, value], ...]}`.
#[utoipa::path(get, path = "/api/v1/backtests/{id}/series", tag = "backtests",
    params(
        ("id" = Uuid, Path, description = "Backtest (job) id"),
        ("kind" = Option<String>, Query, description = "equity (default) or drawdown"),
    ),
    responses(
        (status = 200, description = "The series"),
        (status = 404, description = "No such backtest, or no such series", body = crate::openapi::ErrorDoc),
        (status = 409, description = "The backtest has not succeeded (yet)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn backtest_series(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<SeriesQuery>, QueryRejection>,
) -> Response {
    let Ok(Path(job_id)) = job_id else { return error_response(404, NOT_FOUND) };
    let Ok(Query(q)) = query else { return error_response(422, "Invalid query: use kind=equity|drawdown.") };
    let kind = match spectra_backtest::SeriesKind::parse(q.kind.as_deref().unwrap_or("equity")) {
        Ok(k) => k,
        Err(e) => return error_response(422, e.to_string()),
    };
    let job = match backtest_job(&st, &id, job_id).await {
        Ok(j) => j,
        Err(r) => return r,
    };
    if job.status != "succeeded" {
        return error_response(
            409,
            format!("The backtest is {}; series exist once it has succeeded.", job.status),
        );
    }
    let Some(store) = st.cfg.artifacts.clone() else {
        return error_response(503, "This server was started without artifact storage.");
    };
    let run_id = BacktestView::from(job).run_id;
    let Some(run_id) = run_id else { return error_response(404, "This backtest has no recorded run.") };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let artifact_kind = format!("{}_series", kind.as_str());
    let found = match find_for_run(&pool, ws, run_id, &artifact_kind).await {
        Ok(Some(a)) => a,
        Ok(None) => return error_response(404, "This backtest has no stored series of that kind."),
        Err(e) => return db_error(&e),
    };
    // Reading re-verifies the bytes against their hash.
    match store.get(&found.sha256).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => json_response(200, text),
            Err(_) => error_response(500, "The stored series is not valid text."),
        },
        Err(e) => {
            tracing::error!(%run_id, error = %e, "cannot read a backtest series artifact");
            error_response(500, "The stored series could not be read.")
        }
    }
}

/// Built-in strategies with their parameter schemas, the configuration form's
/// JSON Schema, and the configuration presets: everything the Studio needs to
/// build the backtest form.
#[utoipa::path(get, path = "/api/v1/strategies/builtin", tag = "backtests",
    responses((status = 200, description = "{strategies, config_schema, presets}")))]
pub(crate) async fn builtin_strategies() -> Response {
    let doc = spectra_backtest::builtins_json().and_then(|strategies| {
        Ok(spectra_core::Json::object()
            .with("strategies", strategies)
            .with("config_schema", spectra_backtest::config_schema()?)
            .with("presets", spectra_backtest::presets_json()?))
    });
    match doc {
        Ok(j) => json_response(200, engine::to_text(&j)),
        Err(_) => error_response(500, "Could not describe the built-in strategies."),
    }
}
