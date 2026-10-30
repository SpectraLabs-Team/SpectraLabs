//! `/api/v1/backtests` and `/api/v1/strategies/builtin` (M3 task 6).
//!
//! A backtest is a job of kind `backtest` (see `docs/backtests.md`): these
//! routes are the backtest-shaped view of the job queue. Creating one goes
//! through the same validation and dataset check as `POST /api/v1/jobs`; a
//! job of another kind is a 404 here. Every route acts within the caller's
//! workspace. `/api/v1/datasets` lists the dataset versions a backtest can
//! run on.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use spectra_db::artifacts::find_for_run;
use spectra_db::jobs::{self, Job, JobStatus};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error, submit, to_json, ListJobs};
use crate::{engine, error_response, json_response, AppState, Identity};

/// `POST /api/v1/backtests` body: the `backtest` job payload, plus a priority.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateBacktest {
    /// A dataset version of the caller's workspace.
    pub dataset_version_id: Uuid,
    /// A saved strategy version (see `/api/v1/strategies`). Give it alone,
    /// without `strategy`, `params` or `strategies`: the run uses the version's
    /// built-in and parameters and records the version.
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    /// A built-in strategy name (see `GET /api/v1/strategies/builtin`).
    /// Give either `strategy` (with `params`), `strategies`, or `strategy_version_id`.
    #[serde(default)]
    pub strategy: Option<String>,
    /// Strategy parameters; missing ones take their defaults.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
    /// Several strategies on one account: `[{"strategy", "params", "weight"}]`,
    /// each `weight` a share of the starting cash (adding up to at most 1).
    #[serde(default)]
    #[schema(value_type = Option<Vec<Object>>)]
    pub strategies: Option<Vec<serde_json::Value>>,
    /// Splits and dividends to apply during the run, on unadjusted prices:
    /// `[{"symbol", "ex_date", "type": "split", "from", "to"}]` or
    /// `[{"symbol", "ex_date", "type": "dividend", "amount"}]`.
    #[serde(default)]
    #[schema(value_type = Option<Vec<Object>>)]
    pub corporate_actions: Option<Vec<serde_json::Value>>,
    /// Configuration in the user form (`preset`, `initial_cash`, `slippage_bps`, ...).
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub config: Option<serde_json::Value>,
    /// Optional point-in-time timestamp (RFC 3339 string or integer nanoseconds).
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub as_of: Option<serde_json::Value>,
    /// The currency the run books in. Default: the workspace's base currency.
    #[serde(default)]
    pub base_currency: Option<String>,
    /// Instrument records (`[{"symbol", "type", "currency", "multiplier", "tick_size", ...}]`).
    /// Default: the workspace's records that change a run (see `/api/v1/instruments`).
    #[serde(default)]
    #[schema(value_type = Option<Vec<Object>>)]
    pub instruments: Option<Vec<serde_json::Value>>,
    /// FX rates by currency: `{"EUR": [{"as_of", "rate"}]}`. Default: the
    /// workspace's rates for the currencies the instruments are quoted in.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub fx_rates: Option<serde_json::Value>,
    /// Futures roll rule: `{"business_days_before_expiry": 5, "adjust": "ratio"}`
    /// (both optional; those are the defaults).
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub futures_roll: Option<serde_json::Value>,
    /// Whether bond bars are `clean` (default: accrued interest is added at
    /// every fill and mark) or `dirty` prices.
    #[serde(default)]
    pub bond_quote: Option<String>,
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

/// One backtest in a list: what was asked for, its state and headline numbers.
#[derive(Debug, Serialize, ToSchema)]
pub struct BacktestListItem {
    /// The job id (UUIDv7).
    pub id: Uuid,
    /// `queued`, `running`, `succeeded`, `failed` or `cancelled`.
    pub status: String,
    /// 0..1.
    pub progress: f32,
    /// A cancel was asked for while it ran.
    pub cancel_requested: bool,
    /// Why it failed.
    pub error: Option<String>,
    /// What was asked for: dataset version, strategy or strategies, params, config.
    #[schema(value_type = Object)]
    pub request: serde_json::Value,
    /// The recorded run, once finished.
    pub run_id: Option<Uuid>,
    /// `{total_return, sharpe, max_drawdown, final_equity, fill_count}`, once finished.
    #[schema(value_type = Option<Object>)]
    pub headline: Option<serde_json::Value>,
    /// Queued at (UTC, RFC 3339).
    pub created_at: DateTime<Utc>,
    /// Finished at.
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<Job> for BacktestListItem {
    fn from(j: Job) -> Self {
        let v = BacktestView::from(j);
        let headline = v.summary.as_ref().map(|s| {
            let at = |p: &str| s.pointer(p).cloned().unwrap_or(serde_json::Value::Null);
            serde_json::json!({
                "total_return": at("/metrics/total_return"),
                "sharpe": at("/metrics/sharpe"),
                "max_drawdown": at("/metrics/max_drawdown"),
                "final_equity": at("/activity/final_equity"),
                "fill_count": at("/activity/fill_count"),
            })
        });
        Self {
            id: v.id,
            status: v.status,
            progress: v.progress,
            cancel_requested: v.cancel_requested,
            error: v.error,
            request: v.request,
            run_id: v.run_id,
            headline,
            created_at: v.created_at,
            finished_at: v.finished_at,
        }
    }
}

/// A page of backtests, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct BacktestPage {
    /// This page.
    pub items: Vec<BacktestListItem>,
    /// Pass as `cursor` for the next page; null on the last.
    pub next_cursor: Option<Uuid>,
}

/// A dataset version a backtest can run on.
#[derive(Debug, Serialize, ToSchema)]
pub struct DatasetVersionView {
    /// Version id: a backtest's `dataset_version_id`.
    pub id: Uuid,
    /// The dataset's name.
    pub dataset_name: String,
    /// Version number within the dataset.
    pub version: i32,
    /// Dataset-store checksum.
    pub checksum: String,
    /// Number of bars.
    pub row_count: i64,
    /// Stored size in bytes.
    pub byte_size: i64,
    /// Registered at.
    pub created_at: DateTime<Utc>,
}

/// The workspace's dataset versions, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct DatasetList {
    /// At most 500.
    pub items: Vec<DatasetVersionView>,
    /// Only with `?view=grouped`: the same versions' datasets, most recently
    /// updated first, each with its latest version (the Studio's Data tab).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datasets: Option<Vec<crate::datasets_api::DatasetSummaryView>>,
}

/// `GET /api/v1/datasets` query.
#[derive(Debug, Deserialize, IntoParams)]
pub struct DatasetListQuery {
    /// `grouped` adds `datasets` (one entry per dataset). Without it the
    /// response is exactly the version list the backtest form reads.
    pub view: Option<String>,
}

/// Stored files of a finished backtest that `/artifacts/{kind}` serves.
pub const DOWNLOADABLE_ARTIFACTS: [&str; 9] = [
    "summary",
    "request",
    "equity_csv",
    "orders_csv",
    "fills_csv",
    "returns_csv",
    "strategies_csv",
    "rolls_csv",
    "continuous_csv",
];

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
    // `submit` checks the workspace, and the database refuses (and logs) a sealed holdout there.
    let mut payload = serde_json::json!({ "dataset_version_id": req.dataset_version_id.to_string() });
    if let Some(v) = req.strategy_version_id {
        payload["strategy_version_id"] = serde_json::Value::String(v.to_string());
    }
    if let Some(s) = req.strategy {
        payload["strategy"] = serde_json::Value::String(s);
    }
    if let Some(list) = req.strategies {
        payload["strategies"] = serde_json::Value::Array(list);
    }
    if let Some(list) = req.corporate_actions {
        payload["corporate_actions"] = serde_json::Value::Array(list);
    }
    if let Some(p) = req.params {
        payload["params"] = p;
    }
    if let Some(c) = req.config {
        payload["config"] = c;
    }
    if let Some(a) = req.as_of {
        payload["as_of"] = a;
    }
    if let Some(b) = req.base_currency {
        payload["base_currency"] = serde_json::Value::String(b);
    }
    if let Some(list) = req.instruments {
        payload["instruments"] = serde_json::Value::Array(list);
    }
    if let Some(fx) = req.fx_rates {
        payload["fx_rates"] = fx;
    }
    if let Some(r) = req.futures_roll {
        payload["futures_roll"] = r;
    }
    if let Some(q) = req.bond_quote {
        payload["bond_quote"] = serde_json::Value::String(q);
    }
    // Snapshot the workspace's instrument data into the run, so it reproduces.
    if let Ok((pool, ws)) = crate::jobs_api::context(&st, &id) {
        if let Err(e) = crate::instruments_api::embed_instruments(&pool, ws, &mut payload).await {
            return db_error(&e);
        }
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

/// Detailed P&L attribution breakdown for a finished backtest (Module 11).
#[utoipa::path(
    get,
    path = "/api/v1/backtests/{id}/attribution",
    tag = "backtests",
    params(("id" = Uuid, Path, description = "Backtest job id")),
    responses(
        (status = 200, description = "The attribution breakdown"),
        (status = 404, description = "No such backtest", body = crate::openapi::ErrorDoc),
        (status = 409, description = "The backtest has not succeeded yet", body = crate::openapi::ErrorDoc),
    )
)]
pub(crate) async fn backtest_attribution(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(job_id)) = job_id else { return error_response(404, NOT_FOUND) };
    let job = match backtest_job(&st, &id, job_id).await {
        Ok(j) => j,
        Err(r) => return r,
    };
    if job.status != "succeeded" {
        return error_response(
            409,
            format!("The backtest is {}; attribution exists once it has succeeded.", job.status),
        );
    }
    let view = BacktestView::from(job);
    let Some(summary) = view.summary else {
        return error_response(404, "This backtest has no summary recorded.");
    };
    let serde_json::Value::Object(map) = summary else {
        return error_response(500, "Corrupted summary document.");
    };
    match map.get("attribution") {
        Some(attr) => json_response(200, attr.to_string()),
        None => error_response(404, "This backtest has no attribution breakdown recorded."),
    }
}

/// A monthly return point for the heatmap and dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MonthlyReturn {
    /// Calendar year (e.g. 2024).
    pub year: i32,
    /// Calendar month (1 = January, 12 = December).
    pub month: u32,
    /// Monthly rate of return (e.g. 0.05 for +5.0%).
    #[serde(rename = "return")]
    pub return_pct: f64,
    /// True if this month is partial (the backtest began after the 1st or ended before month-end).
    pub partial: bool,
    /// Equity at the start of the month (or start of the run).
    pub start_equity: f64,
    /// Equity at the end of the month.
    pub end_equity: f64,
}

/// Monthly returns response for `/api/v1/backtests/{id}/monthly-returns`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MonthlyReturnsResponse {
    /// The backtest (job) ID.
    pub backtest_id: Uuid,
    /// The run ID if recorded.
    pub run_id: Option<Uuid>,
    /// Chronological list of monthly returns.
    pub months: Vec<MonthlyReturn>,
    /// Alias to `months` for API compatibility.
    pub returns: Vec<MonthlyReturn>,
}

fn ts_ns_to_datetime(ts_ns: i64) -> Option<DateTime<Utc>> {
    let secs = ts_ns.div_euclid(1_000_000_000);
    let nsecs = ts_ns.rem_euclid(1_000_000_000) as u32;
    DateTime::from_timestamp(secs, nsecs)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 { 1 } else { month + 1 };
    let next_year = if month == 12 { year + 1 } else { year };
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|next| {
            chrono::NaiveDate::from_ymd_opt(year, month, 1)
                .map(|cur| next.signed_duration_since(cur).num_days() as u32)
        })
        .unwrap_or(30)
}

/// Computes calendar-month returns in UTC from an equity series `[(ts_ns, equity)]`.
pub fn compute_monthly_returns(points: &[(i64, f64)]) -> Vec<MonthlyReturn> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut sorted_points = points.to_vec();
    sorted_points.sort_by_key(|p| p.0);

    let Some(first_dt) = ts_ns_to_datetime(sorted_points[0].0) else {
        return Vec::new();
    };
    let Some(last_dt) = ts_ns_to_datetime(sorted_points.last().unwrap().0) else {
        return Vec::new();
    };

    let start_year = first_dt.year();
    let start_month = first_dt.month();
    let start_day = first_dt.day();

    let end_year = last_dt.year();
    let end_month = last_dt.month();
    let end_day = last_dt.day();

    let mut points_by_month: std::collections::BTreeMap<(i32, u32), Vec<(i64, f64)>> =
        std::collections::BTreeMap::new();
    for &(ts, eq) in &sorted_points {
        if let Some(dt) = ts_ns_to_datetime(ts) {
            points_by_month.entry((dt.year(), dt.month())).or_default().push((ts, eq));
        }
    }

    let mut out = Vec::new();
    let mut cur_year = start_year;
    let mut cur_month = start_month;
    let mut prev_equity = sorted_points[0].1;

    while (cur_year < end_year) || (cur_year == end_year && cur_month <= end_month) {
        let is_first = cur_year == start_year && cur_month == start_month;
        let is_last = cur_year == end_year && cur_month == end_month;
        let d_max = days_in_month(cur_year, cur_month);

        let partial = if is_first && is_last {
            start_day > 1 || end_day < d_max
        } else if is_first {
            start_day > 1
        } else if is_last {
            end_day < d_max
        } else {
            false
        };

        let (start_equity, end_equity) = match points_by_month.get(&(cur_year, cur_month)) {
            Some(pts) if !pts.is_empty() => {
                let start_eq = if is_first { pts[0].1 } else { prev_equity };
                let end_eq = pts.last().unwrap().1;
                (start_eq, end_eq)
            }
            _ => (prev_equity, prev_equity),
        };

        let return_pct = if start_equity > 0.0 { (end_equity - start_equity) / start_equity } else { 0.0 };

        out.push(MonthlyReturn {
            year: cur_year,
            month: cur_month,
            return_pct,
            partial,
            start_equity,
            end_equity,
        });

        prev_equity = end_equity;

        if cur_month == 12 {
            cur_year += 1;
            cur_month = 1;
        } else {
            cur_month += 1;
        }
    }

    out
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawPoint {
    Tuple(i64, f64),
    FloatTuple(f64, f64),
}

impl RawPoint {
    fn to_point(&self) -> (i64, f64) {
        match *self {
            Self::Tuple(ts, val) => (ts, val),
            Self::FloatTuple(ts, val) => (ts as i64, val),
        }
    }
}

fn parse_points_from_series_json(bytes: &[u8]) -> Result<Vec<(i64, f64)>, String> {
    #[derive(Deserialize)]
    struct RawSeries {
        points: Vec<RawPoint>,
    }
    serde_json::from_slice::<RawSeries>(bytes)
        .map(|s| s.points.into_iter().map(|p| p.to_point()).collect())
        .map_err(|e| e.to_string())
}

fn parse_points_from_equity_csv(bytes: &[u8]) -> Result<Vec<(i64, f64)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut points = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(',');
        if let (Some(ts_str), Some(eq_str)) = (parts.next(), parts.next()) {
            if let (Ok(ts), Ok(eq)) = (ts_str.trim().parse::<i64>(), eq_str.trim().parse::<f64>()) {
                points.push((ts, eq));
            }
        }
    }
    Ok(points)
}

/// A finished backtest's calendar-month returns, computed in the API from its equity series.
#[utoipa::path(get, path = "/api/v1/backtests/{id}/monthly-returns", tag = "backtests",
    params(("id" = Uuid, Path, description = "Backtest (job) id")),
    responses(
        (status = 200, description = "Monthly returns", body = MonthlyReturnsResponse),
        (status = 404, description = "No such backtest, or no stored equity series", body = crate::openapi::ErrorDoc),
        (status = 409, description = "The backtest has not succeeded (yet)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn backtest_monthly_returns(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    job_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(job_id)) = job_id else { return error_response(404, NOT_FOUND) };
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

    let found = match find_for_run(&pool, ws, run_id, "equity_series").await {
        Ok(Some(a)) => Some((a, true)),
        Ok(None) => match find_for_run(&pool, ws, run_id, "equity_csv").await {
            Ok(Some(a)) => Some((a, false)),
            Ok(None) => None,
            Err(e) => return db_error(&e),
        },
        Err(e) => return db_error(&e),
    };

    let Some((found, is_series_json)) = found else {
        return error_response(404, "This backtest has no stored equity series.");
    };

    let bytes = match store.get(&found.sha256).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(%run_id, error = %e, "cannot read a backtest equity artifact");
            return error_response(500, "The stored series could not be read.");
        }
    };

    let points = if is_series_json {
        match parse_points_from_series_json(&bytes) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(%run_id, error = %e, "failed to parse equity_series.json");
                return error_response(500, "The stored equity series is corrupt.");
            }
        }
    } else {
        match parse_points_from_equity_csv(&bytes) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(%run_id, error = %e, "failed to parse equity.csv");
                return error_response(500, "The stored equity csv is corrupt.");
            }
        }
    };

    let months = compute_monthly_returns(&points);
    to_json(
        200,
        &MonthlyReturnsResponse {
            backtest_id: job_id,
            run_id: Some(run_id),
            months: months.clone(),
            returns: months,
        },
    )
}

/// The workspace's backtests, newest first, with headline numbers once finished.
#[utoipa::path(get, path = "/api/v1/backtests", tag = "backtests",
    params(
        ("status" = Option<String>, Query, description = "queued, running, succeeded, failed or cancelled"),
        ("cursor" = Option<Uuid>, Query, description = "next_cursor from the previous page"),
        ("limit" = Option<u32>, Query, description = "1..200, default 50"),
    ),
    responses((status = 200, body = BacktestPage), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn list_backtests(
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
    match jobs::list_kind(&pool, ws, spectra_backtest::JOB_KIND, status, q.cursor, q.limit.unwrap_or(50))
        .await
    {
        Ok(page) => to_json(
            200,
            &BacktestPage {
                items: page.items.into_iter().map(BacktestListItem::from).collect(),
                next_cursor: page.next_cursor,
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// The dataset versions of the caller's workspace, newest first (for the
/// backtest form); with `?view=grouped`, also one entry per dataset.
#[utoipa::path(get, path = "/api/v1/datasets", tag = "backtests", params(DatasetListQuery),
    responses(
        (status = 200, body = DatasetList),
        (status = 422, description = "Unknown view", body = crate::openapi::ErrorDoc),
        (status = 503, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn list_datasets(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<DatasetListQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let grouped = match query.ok().and_then(|q| q.0.view) {
        None => false,
        Some(v) if v == "grouped" => true,
        Some(_) => return error_response(422, "view: use grouped, or leave it out."),
    };
    let datasets = if grouped {
        match spectra_db::datasets::list_datasets(&pool, ws, 500).await {
            Ok(rows) => Some(rows.into_iter().map(crate::datasets_api::DatasetSummaryView::from).collect()),
            Err(e) => return db_error(&e),
        }
    } else {
        None
    };
    match spectra_db::datasets::list_versions(&pool, ws, 500).await {
        Ok(rows) => to_json(
            200,
            &DatasetList {
                items: rows
                    .into_iter()
                    .map(|v| DatasetVersionView {
                        id: v.id,
                        dataset_name: v.dataset_name,
                        version: v.version,
                        checksum: v.checksum,
                        row_count: v.row_count,
                        byte_size: v.byte_size,
                        created_at: v.created_at,
                    })
                    .collect(),
                datasets,
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// A stored file of a finished backtest, as stored (CSV or JSON), with a
/// download file name. `kind` is one of `summary`, `request`, `equity_csv`,
/// `orders_csv`, `fills_csv`, `returns_csv`, `strategies_csv`, and for a run with
/// futures `rolls_csv` and `continuous_csv`.
#[utoipa::path(get, path = "/api/v1/backtests/{id}/artifacts/{kind}", tag = "backtests",
    params(
        ("id" = Uuid, Path, description = "Backtest (job) id"),
        ("kind" = String, Path, description = "summary, request, equity_csv, orders_csv, fills_csv, returns_csv, strategies_csv, rolls_csv or continuous_csv"),
    ),
    responses(
        (status = 200, description = "The file"),
        (status = 404, description = "No such backtest, kind or file", body = crate::openapi::ErrorDoc),
        (status = 409, description = "The backtest has not succeeded (yet)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn backtest_artifact(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<(Uuid, String)>, PathRejection>,
) -> Response {
    let Ok(Path((job_id, kind))) = path else { return error_response(404, NOT_FOUND) };
    if !DOWNLOADABLE_ARTIFACTS.contains(&kind.as_str()) {
        return error_response(
            404,
            format!("No artifact kind {kind:?}; use one of {}.", DOWNLOADABLE_ARTIFACTS.join(", ")),
        );
    }
    let job = match backtest_job(&st, &id, job_id).await {
        Ok(j) => j,
        Err(r) => return r,
    };
    if job.status != "succeeded" {
        return error_response(
            409,
            format!("The backtest is {}; its files exist once it has succeeded.", job.status),
        );
    }
    let Some(store) = st.cfg.artifacts.clone() else {
        return error_response(503, "This server was started without artifact storage.");
    };
    let Some(run_id) = BacktestView::from(job).run_id else {
        return error_response(404, "This backtest has no recorded run.");
    };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let found = match find_for_run(&pool, ws, run_id, &kind).await {
        Ok(Some(a)) => a,
        Ok(None) => return error_response(404, "This backtest has no stored file of that kind."),
        Err(e) => return db_error(&e),
    };
    let bytes = match store.get(&found.sha256).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(%run_id, error = %e, "cannot read a backtest artifact");
            return error_response(500, "The stored file could not be read.");
        }
    };
    let ext = if found.content_type == "text/csv" { "csv" } else { "json" };
    let name =
        format!("backtest-{}-{}.{ext}", &job_id.simple().to_string()[..8], kind.trim_end_matches("_csv"));
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, found.content_type),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        bytes,
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn utc_ns(year: i32, month: u32, day: u32, hour: u32, min: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, min, 0)
            .unwrap()
            .and_utc()
            .timestamp_nanos_opt()
            .unwrap()
    }

    #[test]
    fn test_compute_monthly_returns_known_12_months() {
        // Starting cash on Jan 1, followed by month-end closes through Dec 31
        let mut points = vec![(utc_ns(2024, 1, 1, 0, 0), 100_000.0)];
        let month_closes = [
            (1, 31, 105_000.0), // +5.0%
            (2, 29, 107_100.0), // +2.0%
            (3, 31, 101_745.0), // -5.0%
            (4, 30, 104_800.0),
            (5, 31, 110_040.0),
            (6, 30, 112_240.8),
            (7, 31, 114_485.6),
            (8, 31, 116_775.3),
            (9, 30, 115_607.5),
            (10, 31, 117_919.7),
            (11, 30, 120_278.1),
            (12, 31, 125_000.0),
        ];
        for (m, d, eq) in month_closes {
            points.push((utc_ns(2024, m, d, 23, 59), eq));
        }

        let months = compute_monthly_returns(&points);
        assert_eq!(months.len(), 12, "expected exactly 12 months for 2024");

        for (i, mr) in months.iter().enumerate() {
            assert_eq!(mr.year, 2024);
            assert_eq!(mr.month, (i + 1) as u32);
            assert!(!mr.partial, "full month from day 1 to day 31 should not be partial");
        }

        // Parity checks against hand-calculated values:
        // Month 1 (Jan): (105,000 - 100,000) / 100,000 = 0.05 (+5%)
        assert!((months[0].return_pct - 0.05).abs() < 1e-6);
        assert_eq!(months[0].start_equity, 100_000.0);
        assert_eq!(months[0].end_equity, 105_000.0);

        // Month 2 (Feb): (107,100 - 105,000) / 105,000 = 0.02 (+2%)
        assert!((months[1].return_pct - 0.02).abs() < 1e-6);
        assert_eq!(months[1].start_equity, 105_000.0);
        assert_eq!(months[1].end_equity, 107_100.0);

        // Month 3 (Mar): (101,745 - 107,100) / 107,100 = -0.05 (-5%)
        assert!((months[2].return_pct - (-0.05)).abs() < 1e-6);
        assert_eq!(months[2].start_equity, 107_100.0);
        assert_eq!(months[2].end_equity, 101_745.0);

        // Compounded return matches overall total return: 125_000 / 100_000 - 1 = 25%
        let compounded = months.iter().fold(1.0, |acc, m| acc * (1.0 + m.return_pct)) - 1.0;
        assert!((compounded - 0.25).abs() < 1e-4);
    }

    #[test]
    fn test_compute_monthly_returns_partial_first_and_last() {
        // Starts mid-January (Jan 15) and ends mid-December (Dec 10)
        let mut points = vec![(utc_ns(2024, 1, 15, 9, 30), 100_000.0)];
        for m in 1..=11 {
            let d = days_in_month(2024, m);
            points.push((utc_ns(2024, m, d, 23, 59), 100_000.0 + (f64::from(m) * 1000.0)));
        }
        points.push((utc_ns(2024, 12, 10, 16, 0), 115_000.0));

        let months = compute_monthly_returns(&points);
        assert_eq!(months.len(), 12);

        // First month is partial (started Jan 15 > 1)
        assert!(months[0].partial, "Jan must be flagged partial");
        // Last month is partial (ended Dec 10 < 31)
        assert!(months[11].partial, "Dec must be flagged partial");

        // Intermediate months (Feb through Nov) are not partial
        for m in &months[1..11] {
            assert!(!m.partial, "Month {} should not be partial", m.month);
        }
    }

    #[test]
    fn test_compute_monthly_returns_3_year_run_36_months() {
        // 3 years: 2022-01-01 to 2024-12-31
        let mut points = vec![(utc_ns(2022, 1, 1, 0, 0), 100_000.0)];
        for y in 2022..=2024 {
            for m in 1..=12 {
                let d = days_in_month(y, m);
                points.push((utc_ns(y, m, d, 23, 59), 100_000.0 + 500.0 * f64::from(m)));
            }
        }

        let months = compute_monthly_returns(&points);
        assert_eq!(months.len(), 36, "3-year run should render exactly 36 months");
        assert_eq!(months[0].year, 2022);
        assert_eq!(months[0].month, 1);
        assert_eq!(months[35].year, 2024);
        assert_eq!(months[35].month, 12);
    }

    #[test]
    fn test_compute_monthly_returns_empty_and_single() {
        assert!(compute_monthly_returns(&[]).is_empty());

        let single = vec![(utc_ns(2024, 5, 10, 10, 0), 50_000.0), (utc_ns(2024, 5, 20, 16, 0), 52_000.0)];
        let res = compute_monthly_returns(&single);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].year, 2024);
        assert_eq!(res[0].month, 5);
        assert!(res[0].partial);
        assert!((res[0].return_pct - 0.04).abs() < 1e-6);
    }

    #[test]
    fn test_parse_points_from_series_json_and_csv() {
        let json_data = br#"{"kind":"equity","total_points":2,"downsampled":false,"points":[[1704067200000000000,100000.0],[1704153600000000000,105000.0]]}"#;
        let points = parse_points_from_series_json(json_data).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0], (1_704_067_200_000_000_000, 100_000.0));
        assert_eq!(points[1], (1_704_153_600_000_000_000, 105_000.0));

        let csv_data = b"ts_ns,equity,cash,drawdown\n1704067200000000000,100000.00,100000.00,0.0\n1704153600000000000,105000.00,100000.00,0.0\n";
        let csv_points = parse_points_from_equity_csv(csv_data).unwrap();
        assert_eq!(csv_points.len(), 2);
        assert_eq!(csv_points[0], (1_704_067_200_000_000_000, 100_000.0));
        assert_eq!(csv_points[1], (1_704_153_600_000_000_000, 105_000.0));
    }
}
