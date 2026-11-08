//! Saved portfolios (M15 Tasks 1 and 2): create, list and read named portfolios with target weights.
//!
//! Four methods build the weights: `fixed` (the caller's weights), `equal` (1/N over the symbols),
//! `inverse_volatility` (proportional to 1 / volatility, where the caller supplies each symbol's
//! annualized volatility) and `risk_parity` (equal risk contributions, from the annualized
//! covariance of the holdings' daily closes in a dataset version the caller names, up to `as_of`,
//! which defaults to and may not be after the day of the save). A save is checked here with `spectra_backtest::portfolio`, so the caller
//! gets a clear message, then by the database, which refuses rows whose weights do not sum to 1
//! within 1e-6. Everything is scoped to the caller's workspace; another workspace's portfolio is
//! reported as not found.
//!
//! M15 Task 6 adds editing (`PUT`, the same body and checks as create) and deleting. Writes need
//! the editor role, like backtests: a viewer gets 403 `viewer_read_only`.

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use spectra_backtest::portfolio::{
    equal_weights, inverse_volatility_weights, risk_parity_weights_from_closes, validate_weights, Holding,
    PortfolioError, DEFAULT_WEIGHT_TOLERANCE,
};
use spectra_backtest::rebalance::Date;
use spectra_db::portfolios::{self, HoldingRow, PortfolioRow, PortfolioWithHoldings};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, AppState, Identity};

pub(crate) const NOT_FOUND: &str = "Portfolio not found.";
const FORBIDDEN_VIEWER: &str = "Viewers have read-only access to portfolios.";
const MAX_NAME_CHARS: usize = 120;
const METHOD_FIXED: &str = "fixed";
const METHOD_EQUAL: &str = "equal";
const METHOD_INVERSE_VOLATILITY: &str = "inverse_volatility";
const METHOD_RISK_PARITY: &str = "risk_parity";

/// One holding in a request. Which fields are needed depends on the method.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HoldingInput {
    pub symbol: String,
    /// Target weight, a fraction (not a percentage). Required for `fixed`; ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// Annualized volatility, e.g. 0.2 for 20%. Required for `inverse_volatility`; ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility: Option<f64>,
}

/// A holding with its weight, as stored and returned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HoldingBody {
    pub symbol: String,
    pub weight: f64,
}

/// Create a saved portfolio.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreatePortfolioBody {
    /// Unique within the workspace, at most 120 characters.
    pub name: String,
    /// `fixed` (default), `equal`, `inverse_volatility` or `risk_parity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    pub holdings: Vec<HoldingInput>,
    /// The dataset version whose daily closes give the covariance. Required for `risk_parity`;
    /// ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_version_id: Option<Uuid>,
    /// `YYYY-MM-DD`: only closes on or before this UTC day are used. For `risk_parity`; defaults to
    /// the day of the save and may not be after it (no look-ahead). Ignored otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
}

/// A saved portfolio with its holdings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    /// `fixed`, `equal`, `inverse_volatility` or `risk_parity`.
    pub method: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub holdings: Vec<HoldingBody>,
}

/// A saved portfolio without its holdings (the list view).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSummaryView {
    pub id: Uuid,
    pub name: String,
    pub method: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A deleted portfolio.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeletedPortfolioView {
    pub id: Uuid,
    pub deleted: bool,
}

impl From<PortfolioWithHoldings> for PortfolioView {
    fn from(p: PortfolioWithHoldings) -> Self {
        Self {
            id: p.portfolio.id,
            workspace_id: p.portfolio.workspace_id,
            name: p.portfolio.name,
            method: p.portfolio.method,
            created_by: p.portfolio.created_by,
            created_at: p.portfolio.created_at,
            holdings: p.holdings.into_iter().map(HoldingBody::from).collect(),
        }
    }
}

impl From<PortfolioRow> for PortfolioSummaryView {
    fn from(p: PortfolioRow) -> Self {
        Self { id: p.id, name: p.name, method: p.method, created_by: p.created_by, created_at: p.created_at }
    }
}

impl From<HoldingRow> for HoldingBody {
    fn from(h: HoldingRow) -> Self {
        Self { symbol: h.symbol, weight: h.weight }
    }
}

/// What a checked body's method needs to give its weights.
enum Weights {
    /// Built from the request alone.
    Ready(Vec<Holding>),
    /// Risk parity: built from the closes of `symbols` in a dataset version, up to `as_of`.
    FromPrices { symbols: Vec<String>, dataset_version_id: Uuid, as_of: Date },
}

/// Builds the target weights for a method, or the reason the request cannot be built. Risk parity
/// is only checked here; its weights need the prices (see [`risk_parity_from_dataset`]).
fn build_weights(req: &CreatePortfolioBody, method: &str, today: Date) -> Result<Weights, String> {
    let holdings = &req.holdings;
    let symbols: Vec<String> = holdings.iter().map(|h| h.symbol.trim().to_string()).collect();
    let refused = |e: PortfolioError| e.to_string();
    match method {
        METHOD_FIXED => {
            let mut out = Vec::with_capacity(holdings.len());
            for (h, symbol) in holdings.iter().zip(&symbols) {
                let Some(weight) = h.weight else {
                    return Err(format!("holding {symbol} needs a weight for the fixed method"));
                };
                out.push(Holding { symbol: symbol.clone(), weight });
            }
            validate_weights(&out, DEFAULT_WEIGHT_TOLERANCE).map_err(refused)?;
            Ok(Weights::Ready(out))
        }
        METHOD_EQUAL => equal_weights(&symbols).map(Weights::Ready).map_err(refused),
        METHOD_INVERSE_VOLATILITY => {
            let mut vols = Vec::with_capacity(holdings.len());
            for (h, symbol) in holdings.iter().zip(&symbols) {
                let Some(vol) = h.volatility else {
                    return Err(format!(
                        "holding {symbol} needs a volatility for the inverse_volatility method"
                    ));
                };
                vols.push(vol);
            }
            inverse_volatility_weights(&symbols, &vols).map(Weights::Ready).map_err(refused)
        }
        METHOD_RISK_PARITY => {
            // The symbol checks run now, so a bad list is refused before any prices are read.
            equal_weights(&symbols).map_err(refused)?;
            let Some(dataset_version_id) = req.dataset_version_id else {
                return Err("the risk_parity method needs a dataset_version_id for the prices".to_string());
            };
            let as_of = match req.as_of.as_deref() {
                None => today,
                Some(text) => crate::portfolio_runs_api::parse_date(text)
                    .ok_or_else(|| format!("as_of must be a date (YYYY-MM-DD), got {text:?}"))?,
            };
            if as_of > today {
                return Err(format!(
                    "as_of {as_of} is after the day of the save ({today}); weights may only use prices known when the portfolio is saved"
                ));
            }
            Ok(Weights::FromPrices { symbols, dataset_version_id, as_of })
        }
        _ => Err("method must be fixed, equal, inverse_volatility or risk_parity.".to_string()),
    }
}

/// Risk parity weights from a dataset version: each holding's last close of each UTC day, from
/// the version's first day to `as_of`, on the days every holding has a bar (the portfolio run's
/// price table), annualized with 252 trading days.
#[allow(clippy::result_large_err)] // the error is the finished response
async fn risk_parity_from_dataset(
    st: &AppState,
    pool: &spectra_db::PgPool,
    ws: Uuid,
    id: &Identity,
    symbols: Vec<String>,
    dataset_version_id: Uuid,
    as_of: Date,
) -> Result<Vec<Holding>, Response> {
    // The bars come from the checksum the database hands out, which it refuses (and logs) for a
    // sealed holdout.
    let access =
        spectra_db::datasets::Access { purpose: "portfolio_weights", user: Uuid::parse_str(&id.user).ok() };
    let version = match spectra_db::datasets::resolve_version(pool, ws, dataset_version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(v)) => v,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            return Err(crate::optimization_api::sealed_response(
                &holdout,
                "building portfolio weights from it",
            ))
        }
        Ok(spectra_db::datasets::Resolved::Missing) => {
            return Err(error_response(404, "No such dataset version."))
        }
        Err(e) => return Err(db_error(&e)),
    };
    let dataset =
        crate::datasets_api::load_for_chart(st, &version.checksum, version.blob_sha256.as_deref()).await?;
    let outcome = tokio::task::spawn_blocking(move || {
        let first_day =
            dataset.bars.iter().map(|b| b.ts_ns.div_euclid(crate::portfolio_runs_api::DAY_NS)).min();
        let start = first_day.and_then(Date::from_days).unwrap_or(as_of);
        let (_, rows, _) = crate::portfolio_runs_api::price_table(&dataset, &symbols, start, as_of)?;
        let series: Vec<Vec<f64>> = (0..symbols.len()).map(|k| rows.iter().map(|r| r[k]).collect()).collect();
        risk_parity_weights_from_closes(&symbols, &series, spectra_risk::returns::TRADING_DAYS_PER_YEAR)
            .map_err(|e| e.to_string())
    })
    .await;
    match outcome {
        Ok(Ok(weights)) => Ok(weights),
        Ok(Err(message)) => Err(error_response(422, message)),
        Err(_) => Err(error_response(500, "Could not compute the risk parity weights.")),
    }
}

/// Refuses a viewer of the workspace with 403 `viewer_read_only`; editors and owners pass. Callers
/// without a user id (service calls) pass, as in the other workspace-scoped write routes.
#[allow(clippy::result_large_err)] // the error is the finished response
pub(crate) async fn ensure_can_edit(
    pool: &spectra_db::PgPool,
    ws: Uuid,
    id: &Identity,
) -> Result<(), Response> {
    let Ok(uid) = Uuid::parse_str(&id.user) else { return Ok(()) };
    let role: Option<(String,)> =
        sqlx::query_as("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(ws)
            .bind(uid)
            .fetch_optional(pool)
            .await
            .map_err(|e| db_error(&e))?;
    if role.is_some_and(|(r,)| r == "viewer") {
        return Err(crate::error_with_code(403, "viewer_read_only", FORBIDDEN_VIEWER));
    }
    Ok(())
}

/// The trimmed name, the method, and the weights a checked body builds (or how to build them).
type CheckedPortfolio = (String, String, Weights);

/// A checked create or edit body: the trimmed name, the method, and its weights.
#[allow(clippy::result_large_err)] // the error is the finished response
fn checked_body(bytes: &Bytes) -> Result<CheckedPortfolio, Response> {
    let req: CreatePortfolioBody =
        serde_json::from_slice(bytes).map_err(|e| error_response(422, format!("Invalid portfolio: {e}")))?;
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(error_response(422, "Name must be 1 to 120 characters."));
    }
    let method = req.method.as_deref().unwrap_or(METHOD_FIXED);
    let now = Utc::now().date_naive();
    let today = Date::new(now.year(), now.month(), now.day());
    let weights = build_weights(&req, method, today).map_err(|message| error_response(422, message))?;
    Ok((name.to_string(), method.to_string(), weights))
}

/// The (symbol, weight) pairs to store, computing risk parity weights from the dataset.
#[allow(clippy::result_large_err)] // the error is the finished response
async fn weight_pairs(
    st: &AppState,
    pool: &spectra_db::PgPool,
    ws: Uuid,
    id: &Identity,
    weights: Weights,
) -> Result<Vec<(String, f64)>, Response> {
    let holdings = match weights {
        Weights::Ready(h) => h,
        Weights::FromPrices { symbols, dataset_version_id, as_of } => {
            risk_parity_from_dataset(st, pool, ws, id, symbols, dataset_version_id, as_of).await?
        }
    };
    Ok(holdings.into_iter().map(|h| (h.symbol, h.weight)).collect())
}

/// The response for a failed save: 409 for a taken name, 422 for weights the database refused.
fn save_error(e: &spectra_db::Error) -> Response {
    if let Some(db) = e.as_database_error() {
        match db.code().as_deref() {
            // 23505: unique violation, the name is taken.
            Some("23505") => return error_response(409, "A portfolio with this name already exists."),
            // 23514: check violation, raised by the weight-sum trigger or the method check.
            Some("23514") => return error_response(422, db.message().to_string()),
            _ => {}
        }
    }
    db_error(e)
}

/// Save a named portfolio with target weights.
#[utoipa::path(post, path = "/api/v1/portfolios", tag = "portfolios", request_body = CreatePortfolioBody,
    responses(
        (status = 201, description = "Saved", body = PortfolioView),
        (status = 403, description = "Viewers cannot save portfolios (code `viewer_read_only`), or the risk_parity dataset version is a sealed holdout (code `holdout_sealed`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "risk_parity: no such dataset version in this workspace", body = crate::openapi::ErrorDoc),
        (status = 409, description = "A portfolio with this name already exists in the workspace", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid name, method, holdings or weights; for risk_parity, a holding with no bars, too little history (60 closes) or a singular covariance", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let (name, method, weights) = match checked_body(&bytes) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let pairs = match weight_pairs(&st, &pool, ws, &id, weights).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let created_by = Uuid::parse_str(&id.user).ok();
    match portfolios::create_portfolio_with_method(&pool, ws, &name, &method, &pairs, created_by).await {
        Ok(saved) => to_json(201, &PortfolioView::from(saved)),
        Err(e) => save_error(&e),
    }
}

/// Edit a saved portfolio: its name, method and holdings, with the same checks as a save. The
/// holdings are replaced as a whole; if the new weights are refused, the old ones are kept.
#[utoipa::path(put, path = "/api/v1/portfolios/{id}", tag = "portfolios", request_body = CreatePortfolioBody,
    params(("id" = Uuid, Path, description = "The portfolio id")),
    responses(
        (status = 200, description = "Saved", body = PortfolioView),
        (status = 403, description = "Viewers cannot edit portfolios (code `viewer_read_only`), or the risk_parity dataset version is a sealed holdout (code `holdout_sealed`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "Not found in this workspace (the portfolio, or for risk_parity the dataset version)", body = crate::openapi::ErrorDoc),
        (status = 409, description = "Another portfolio in the workspace has this name", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid name, method, holdings or weights; for risk_parity, a holding with no bars, too little history (60 closes) or a singular covariance", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn update_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let Ok(Path(portfolio_id)) = path else { return error_response(404, NOT_FOUND) };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let (name, method, weights) = match checked_body(&bytes) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let pairs = match weight_pairs(&st, &pool, ws, &id, weights).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match portfolios::update_portfolio(&pool, ws, portfolio_id, &name, &method, &pairs).await {
        Ok(Some(saved)) => to_json(200, &PortfolioView::from(saved)),
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => save_error(&e),
    }
}

/// Delete a saved portfolio and its holdings. Its runs are kept (with their recorded inputs) and
/// no longer listed under it.
#[utoipa::path(delete, path = "/api/v1/portfolios/{id}", tag = "portfolios",
    params(("id" = Uuid, Path, description = "The portfolio id")),
    responses(
        (status = 200, description = "Deleted", body = DeletedPortfolioView),
        (status = 403, description = "Viewers cannot delete portfolios (code `viewer_read_only`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "Not found in this workspace", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(portfolio_id)) = path else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    match portfolios::delete_portfolio(&pool, ws, portfolio_id).await {
        Ok(true) => to_json(200, &DeletedPortfolioView { id: portfolio_id, deleted: true }),
        Ok(false) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

/// The saved portfolios of the caller's workspace, by name.
#[utoipa::path(get, path = "/api/v1/portfolios", tag = "portfolios",
    responses(
        (status = 200, description = "Portfolios in the workspace", body = Vec<PortfolioSummaryView>),
    ))]
pub(crate) async fn list_portfolios(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match portfolios::list_portfolios(&pool, ws).await {
        Ok(list) => {
            let views: Vec<PortfolioSummaryView> = list.into_iter().map(PortfolioSummaryView::from).collect();
            to_json(200, &views)
        }
        Err(e) => db_error(&e),
    }
}

/// One saved portfolio with its holdings.
#[utoipa::path(get, path = "/api/v1/portfolios/{id}", tag = "portfolios",
    params(("id" = Uuid, Path, description = "The portfolio id")),
    responses(
        (status = 200, description = "The portfolio", body = PortfolioView),
        (status = 404, description = "Not found in this workspace", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn get_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(portfolio_id)) = path else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match portfolios::get_portfolio(&pool, ws, portfolio_id).await {
        Ok(Some(p)) => to_json(200, &PortfolioView::from(p)),
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}
