//! Fundamentals endpoints (M34).
//!
//!  * Point-in-time reads (M34b): `GET /api/v1/fundamentals` (each value's latest revision known
//!    at `as_of`) and `GET /api/v1/fundamentals/statement` (an M37 statement built from them). Only
//!    rows with `known_at <= as_of` are read, and quarantined rows are never read.
//!  * Loading (M34a): queue a refresh from SEC EDGAR, list loads and quarantined rows, and manage
//!    the workspace's refresh schedule.
//!
//! Writes need an editor (viewers get 403 `viewer_read_only`); reads need any member.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, NaiveDate, Utc};
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

/// Query of `GET /api/v1/fundamentals`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FactsQuery {
    /// Ticker (required).
    pub symbol: Option<String>,
    /// What was known at this time: `YYYY-MM-DD` (the end of that day, UTC) or an RFC 3339 time.
    /// Default: now.
    pub as_of: Option<String>,
    /// Only this metric, e.g. `revenue`.
    pub metric: Option<String>,
}

/// Query of `GET /api/v1/fundamentals/statement`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StatementQuery {
    /// Ticker (required).
    pub symbol: Option<String>,
    /// As in `GET /api/v1/fundamentals`.
    pub as_of: Option<String>,
    /// Period, e.g. `2024-Q1` or `2024-FY`. Default: the latest period with a value known at `as_of`.
    pub period: Option<String>,
}

/// One value: its latest revision known at the requested time.
#[derive(Debug, Serialize, ToSchema)]
pub struct FactView {
    pub period: String,
    pub fiscal_year: i16,
    /// 1 to 4, or 0 for a full year.
    pub fiscal_quarter: i16,
    pub metric: String,
    pub value: f64,
    /// `USD`, or empty for share counts.
    pub currency: String,
    /// When this revision became public.
    pub known_at: DateTime<Utc>,
    /// 0 for the first filing, then one per restatement.
    pub revision: i32,
    pub filing_ref: String,
    pub source: String,
}

/// Values of a symbol known at `as_of`.
#[derive(Debug, Serialize, ToSchema)]
pub struct FactsView {
    pub symbol: String,
    pub as_of: DateTime<Utc>,
    pub items: Vec<FactView>,
}

/// Statement lines (M37 `FinancialStatementItems`); null when no value was known.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatementLines {
    pub revenue: Option<f64>,
    pub cost_of_goods_sold: Option<f64>,
    pub gross_profit: Option<f64>,
    pub operating_expenses: Option<f64>,
    pub operating_income: Option<f64>,
    pub interest_expense: Option<f64>,
    pub net_income: Option<f64>,
    pub diluted_shares_outstanding: Option<f64>,
    pub cash_and_equivalents: Option<f64>,
    pub current_assets: Option<f64>,
    pub inventory: Option<f64>,
    pub total_assets: Option<f64>,
    pub current_liabilities: Option<f64>,
    /// Long-term debt.
    pub total_debt: Option<f64>,
    pub total_liabilities: Option<f64>,
    pub stockholders_equity: Option<f64>,
    pub operating_cash_flow: Option<f64>,
    pub capital_expenditures: Option<f64>,
    pub free_cash_flow: Option<f64>,
}

/// A company statement as it was known at `as_of`.
#[derive(Debug, Serialize, ToSchema)]
pub struct StatementView {
    pub symbol: String,
    pub period: String,
    pub fiscal_year: u16,
    pub as_of: DateTime<Utc>,
    /// The latest known-at time among the values used.
    pub known_at: DateTime<Utc>,
    pub lines: StatementLines,
    /// Lines with no value known at `as_of`.
    pub missing: Vec<String>,
}

/// `as_of` as a time: the end of a `YYYY-MM-DD` day (UTC, to the microsecond Postgres keeps), an
/// RFC 3339 time, or now when absent.
fn parse_as_of(text: Option<&str>) -> Result<DateTime<Utc>, String> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(Utc::now()) };
    let parsed = if text.len() == 10 {
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_micro_opt(23, 59, 59, 999_999))
            .map(|t| t.and_utc())
    } else {
        DateTime::parse_from_rfc3339(text).ok().map(|t| t.with_timezone(&Utc))
    };
    parsed.filter(|t| t.timestamp_nanos_opt().is_some()).ok_or_else(|| {
        format!("as_of must be YYYY-MM-DD or an RFC 3339 time between 1677 and 2262, got {text:?}")
    })
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn one_symbol(symbol: Option<String>) -> Result<String, Response> {
    let Some(symbol) = symbol else { return Err(error_response(422, "symbol is required")) };
    match validate_symbols(&[symbol]) {
        Ok(mut s) => Ok(s.remove(0)),
        Err(e) => Err(error_response(422, e)),
    }
}

/// The latest revision of each value of a symbol known at `as_of`. Values published later, and
/// quarantined rows, are never returned.
#[utoipa::path(get, path = "/api/v1/fundamentals", tag = "fundamentals", params(FactsQuery),
    responses(
        (status = 200, body = FactsView),
        (status = 422, description = "No symbol, or a bad as_of", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn facts(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<FactsQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else { return error_response(422, "Invalid query string.") };
    let symbol = match one_symbol(q.symbol) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let as_of = match parse_as_of(q.as_of.as_deref()) {
        Ok(t) => t,
        Err(e) => return error_response(422, e),
    };
    let metric = q.metric.map(|m| m.trim().to_ascii_lowercase()).filter(|m| !m.is_empty());
    match spectra_db::fundamentals::facts_as_of(&pool, ws, &symbol, as_of, metric.as_deref()).await {
        Ok(rows) => to_json(
            200,
            &FactsView {
                symbol,
                as_of,
                items: rows
                    .into_iter()
                    .map(|r| FactView {
                        period: r.period,
                        fiscal_year: r.fiscal_year,
                        fiscal_quarter: r.fiscal_quarter,
                        metric: r.metric,
                        value: r.value,
                        currency: r.currency,
                        known_at: r.known_at,
                        revision: r.revision,
                        filing_ref: r.filing_ref,
                        source: r.source,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// A company statement (M37 line items) built from the values known at `as_of`. Lines with no
/// value known then are null and listed in `missing`; they are never reported as zero.
#[utoipa::path(get, path = "/api/v1/fundamentals/statement", tag = "fundamentals", params(StatementQuery),
    responses(
        (status = 200, body = StatementView),
        (status = 404, description = "Nothing for that symbol and period was known at as_of", body = crate::openapi::ErrorDoc),
        (status = 422, description = "No symbol, or a bad as_of or period", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn statement(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<StatementQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else { return error_response(422, "Invalid query string.") };
    let symbol = match one_symbol(q.symbol) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let as_of = match parse_as_of(q.as_of.as_deref()) {
        Ok(t) => t,
        Err(e) => return error_response(422, e),
    };
    let Some(as_of_ns) = as_of.timestamp_nanos_opt() else {
        return error_response(422, "as_of is out of range");
    };
    let period = q.period.map(|p| p.trim().to_ascii_uppercase()).filter(|p| !p.is_empty());
    if period.as_ref().is_some_and(|p| p.len() > 16) {
        return error_response(422, "period is too long; use e.g. 2024-Q1 or 2024-FY");
    }
    // Only rows known at as_of are read, so the store cannot see later revisions at all.
    let rows = match spectra_db::fundamentals::facts_known_by(&pool, ws, std::slice::from_ref(&symbol), as_of)
        .await
    {
        Ok(r) => r,
        Err(e) => return db_error(&e),
    };
    let mut store = spectra_data::fundamentals::PointInTimeFundamentalsStore::new();
    store.ingest(rows.iter().filter_map(spectra_db::fundamentals::FactRow::to_record).collect());
    let not_known = || error_response(404, format!("No fundamentals for {symbol} were known at {as_of}."));
    let Some(period) = period.or_else(|| spectra_research::latest_period(&store, &symbol, as_of_ns)) else {
        return not_known();
    };
    let Some(s) = spectra_research::statement_as_of(&store, &symbol, &period, as_of_ns) else {
        return not_known();
    };
    let line = |field: &str| s.get(field);
    let view = StatementView {
        symbol,
        period,
        fiscal_year: s.items.fiscal_year,
        as_of,
        known_at: DateTime::from_timestamp_nanos(s.known_at_ns),
        lines: StatementLines {
            revenue: line("revenue"),
            cost_of_goods_sold: line("cost_of_goods_sold"),
            gross_profit: line("gross_profit"),
            operating_expenses: line("operating_expenses"),
            operating_income: line("operating_income"),
            interest_expense: line("interest_expense"),
            net_income: line("net_income"),
            diluted_shares_outstanding: line("diluted_shares_outstanding"),
            cash_and_equivalents: line("cash_and_equivalents"),
            current_assets: line("current_assets"),
            inventory: line("inventory"),
            total_assets: line("total_assets"),
            current_liabilities: line("current_liabilities"),
            total_debt: line("total_debt"),
            total_liabilities: line("total_liabilities"),
            stockholders_equity: line("stockholders_equity"),
            operating_cash_flow: line("operating_cash_flow"),
            capital_expenditures: line("capital_expenditures"),
            free_cash_flow: line("free_cash_flow"),
        },
        missing: spectra_research::STATEMENT_FIELDS
            .iter()
            .filter(|f| !s.found.contains(f))
            .map(|f| (*f).to_string())
            .collect(),
    };
    to_json(200, &view)
}

#[cfg(test)]
mod tests {
    use super::parse_as_of;

    #[test]
    fn as_of_is_the_end_of_a_day_or_an_exact_time() {
        let day = parse_as_of(Some("2024-02-03")).unwrap();
        assert_eq!(day.to_rfc3339(), "2024-02-03T23:59:59.999999+00:00");
        let exact = parse_as_of(Some("2024-02-03T11:59:59Z")).unwrap();
        assert_eq!(exact.to_rfc3339(), "2024-02-03T11:59:59+00:00");
        assert!(parse_as_of(Some("2024-02-30")).is_err());
        assert!(parse_as_of(Some("yesterday")).is_err());
        assert!(parse_as_of(None).is_ok());
    }
}
