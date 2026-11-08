//! Fundamentals refresh endpoints (M34a): queue a refresh from SEC EDGAR, list loads and
//! quarantined rows, and manage the workspace's refresh schedule.
//!
//! Writes need an editor (viewers get 403 `viewer_read_only`); reads need any member.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::fundamentals::{validate_symbols, RefreshPayload, FUNDAMENTALS_REFRESH_JOB_KIND};
use crate::jobs_api::{context, db_error, to_json};
use crate::portfolios_api::ensure_can_edit;
use crate::{error_response, AppState, Identity};

/// Rows a list returns unless `limit` says otherwise.
const DEFAULT_LIMIT: i64 = 50;
/// Most rows a list returns.
const MAX_LIMIT: i64 = 500;
/// Shortest and longest schedule interval, in hours (the table allows 1 hour to 30 days).
const MIN_INTERVAL_HOURS: i32 = 1;
const MAX_INTERVAL_HOURS: i32 = 720;

/// Body of `POST /api/v1/fundamentals/refresh`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshRequest {
    /// Tickers to refresh (1 to 100); case and surrounding spaces are ignored.
    pub symbols: Vec<String>,
}

/// A queued refresh.
#[derive(Debug, Serialize, ToSchema)]
pub struct RefreshQueuedView {
    pub job_id: Uuid,
    pub status: String,
    /// The symbols as they will be refreshed: trimmed, upper-case, without duplicates.
    pub symbols: Vec<String>,
}

/// `symbol` and `limit` for the list endpoints.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// Only this symbol.
    pub symbol: Option<String>,
    /// Rows to return, 1 to 500 (default 50).
    pub limit: Option<i64>,
}

/// One refresh of one symbol.
#[derive(Debug, Serialize, ToSchema)]
pub struct LoadView {
    pub id: Uuid,
    pub symbol: String,
    pub source: String,
    /// Content-addressed version (`fund_v_…`) of the accepted records.
    pub version_id: String,
    pub accepted: i32,
    /// Accepted records not stored before: new values and new revisions.
    pub new_rows: i32,
    pub quarantined: i32,
    pub job_id: Option<Uuid>,
    pub loaded_at: DateTime<Utc>,
}

/// A row that failed the quality checks; it is never served as a fundamental.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuarantineView {
    pub symbol: String,
    pub period: String,
    pub metric: String,
    /// The value, or null when it is not finite (see `value_text`).
    pub value: Option<f64>,
    /// The value as text, e.g. `NaN` or `-5`.
    pub value_text: String,
    pub known_at: DateTime<Utc>,
    pub filing_ref: String,
    pub category: String,
    pub reason: String,
    pub recorded_at: DateTime<Utc>,
}

/// `{ "items": [...] }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct LoadList {
    pub items: Vec<LoadView>,
}

/// `{ "items": [...] }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuarantineList {
    pub items: Vec<QuarantineView>,
}

/// Body of `PUT /api/v1/fundamentals/schedule`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScheduleRequest {
    /// Tickers to refresh (1 to 100).
    pub symbols: Vec<String>,
    /// Hours between refreshes, 1 to 720. The first refresh is queued within a minute.
    pub interval_hours: i32,
}

/// The workspace's refresh schedule.
#[derive(Debug, Serialize, ToSchema)]
pub struct ScheduleView {
    pub symbols: Vec<String>,
    pub interval_hours: i32,
    pub next_run_at: DateTime<Utc>,
    pub last_enqueued_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// `{ "deleted": true }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ScheduleDeleted {
    pub deleted: bool,
}

fn schedule_view(s: spectra_db::fundamentals::ScheduleRow) -> ScheduleView {
    ScheduleView {
        symbols: s.symbols,
        interval_hours: s.interval_seconds / 3600,
        next_run_at: s.next_run_at,
        last_enqueued_at: s.last_enqueued_at,
        updated_at: s.updated_at,
    }
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn body<T: for<'de> Deserialize<'de>>(bytes: Result<Bytes, BytesRejection>) -> Result<T, Response> {
    let bytes = bytes.map_err(|_| error_response(400, "Could not read the request body."))?;
    serde_json::from_slice(&bytes).map_err(|e| error_response(422, format!("Invalid request: {e}")))
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn list_query(query: Result<Query<ListQuery>, QueryRejection>) -> Result<(Option<String>, i64), Response> {
    let Ok(Query(q)) = query else { return Err(error_response(422, "Invalid query string.")) };
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(error_response(422, format!("limit must be 1 to {MAX_LIMIT}")));
    }
    Ok((q.symbol.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()), limit))
}

/// Queue a fundamentals refresh from SEC EDGAR (editor).
#[utoipa::path(post, path = "/api/v1/fundamentals/refresh", tag = "fundamentals",
    request_body = RefreshRequest,
    responses(
        (status = 202, description = "Refresh queued as a fundamentals_refresh job", body = RefreshQueuedView),
        (status = 403, description = "Viewers cannot refresh (code `viewer_read_only`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "No symbols, too many, or one that is not a ticker", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn refresh(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    bytes: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let req: RefreshRequest = match body(bytes) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let symbols = match validate_symbols(&req.symbols) {
        Ok(s) => s,
        Err(e) => return error_response(422, e),
    };
    let job = spectra_db::jobs::NewJob {
        workspace_id: ws,
        kind: FUNDAMENTALS_REFRESH_JOB_KIND.to_string(),
        payload: serde_json::json!(RefreshPayload { symbols: symbols.clone(), schedule_id: None }),
        priority: 10,
        max_attempts: 3,
        created_by: Uuid::parse_str(&id.user).ok(),
    };
    match spectra_db::jobs::enqueue(&pool, &job).await {
        Ok(j) => to_json(202, &RefreshQueuedView { job_id: j.id, status: j.status, symbols }),
        Err(e) => db_error(&e),
    }
}

/// Recent fundamentals loads, newest first.
#[utoipa::path(get, path = "/api/v1/fundamentals/loads", tag = "fundamentals", params(ListQuery),
    responses((status = 200, body = LoadList), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn list_loads(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let (symbol, limit) = match list_query(query) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match spectra_db::fundamentals::list_loads(&pool, ws, symbol.as_deref(), limit).await {
        Ok(rows) => to_json(
            200,
            &LoadList {
                items: rows
                    .into_iter()
                    .map(|r| LoadView {
                        id: r.id,
                        symbol: r.symbol,
                        source: r.source,
                        version_id: r.version_id,
                        accepted: r.accepted,
                        new_rows: r.new_rows,
                        quarantined: r.quarantined,
                        job_id: r.job_id,
                        loaded_at: r.loaded_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Rows that failed the quality checks, newest first. They are never served as fundamentals.
#[utoipa::path(get, path = "/api/v1/fundamentals/quarantine", tag = "fundamentals", params(ListQuery),
    responses((status = 200, body = QuarantineList), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn list_quarantine(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let (symbol, limit) = match list_query(query) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match spectra_db::fundamentals::list_quarantine(&pool, ws, symbol.as_deref(), limit).await {
        Ok(rows) => to_json(
            200,
            &QuarantineList {
                items: rows
                    .into_iter()
                    .map(|q| QuarantineView {
                        symbol: q.symbol,
                        period: q.period,
                        metric: q.metric,
                        value: q.value.is_finite().then_some(q.value),
                        value_text: q.value.to_string(),
                        known_at: q.known_at,
                        filing_ref: q.filing_ref,
                        category: q.category,
                        reason: q.reason,
                        recorded_at: q.recorded_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// The workspace's fundamentals refresh schedule.
#[utoipa::path(get, path = "/api/v1/fundamentals/schedule", tag = "fundamentals",
    responses((status = 200, body = ScheduleView), (status = 404, description = "No schedule", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_schedule(State(st): State<AppState>, Extension(id): Extension<Identity>) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::fundamentals::get_schedule(&pool, ws).await {
        Ok(Some(s)) => to_json(200, &schedule_view(s)),
        Ok(None) => error_response(404, "This workspace has no fundamentals refresh schedule."),
        Err(e) => db_error(&e),
    }
}

/// Create or replace the workspace's fundamentals refresh schedule (editor).
#[utoipa::path(put, path = "/api/v1/fundamentals/schedule", tag = "fundamentals",
    request_body = ScheduleRequest,
    responses(
        (status = 200, body = ScheduleView),
        (status = 403, description = "Viewers cannot schedule (code `viewer_read_only`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Bad symbols or interval", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn put_schedule(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    bytes: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let req: ScheduleRequest = match body(bytes) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let symbols = match validate_symbols(&req.symbols) {
        Ok(s) => s,
        Err(e) => return error_response(422, e),
    };
    if !(MIN_INTERVAL_HOURS..=MAX_INTERVAL_HOURS).contains(&req.interval_hours) {
        return error_response(
            422,
            format!("interval_hours must be {MIN_INTERVAL_HOURS} to {MAX_INTERVAL_HOURS}"),
        );
    }
    let saved = spectra_db::fundamentals::upsert_schedule(
        &pool,
        ws,
        &symbols,
        req.interval_hours * 3600,
        Utc::now(),
        Uuid::parse_str(&id.user).ok(),
    )
    .await;
    match saved {
        Ok(s) => to_json(200, &schedule_view(s)),
        Err(e) => db_error(&e),
    }
}

/// Remove the workspace's fundamentals refresh schedule (editor).
#[utoipa::path(delete, path = "/api/v1/fundamentals/schedule", tag = "fundamentals",
    responses(
        (status = 200, body = ScheduleDeleted),
        (status = 403, body = crate::openapi::ErrorDoc),
        (status = 404, description = "No schedule", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_schedule(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    match spectra_db::fundamentals::delete_schedule(&pool, ws).await {
        Ok(true) => to_json(200, &ScheduleDeleted { deleted: true }),
        Ok(false) => error_response(404, "This workspace has no fundamentals refresh schedule."),
        Err(e) => db_error(&e),
    }
}
