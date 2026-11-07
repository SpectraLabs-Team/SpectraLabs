//! Saved portfolios (M15 Tasks 1 and 2): create, list and read named portfolios with target weights.
//!
//! Three methods build the weights: `fixed` (the caller's weights), `equal` (1/N over the symbols)
//! and `inverse_volatility` (proportional to 1 / volatility, where the caller supplies each symbol's
//! annualized volatility). A save is checked here with `spectra_backtest::portfolio`, so the caller
//! gets a clear message, then by the database, which refuses rows whose weights do not sum to 1
//! within 1e-6. Everything is scoped to the caller's workspace; another workspace's portfolio is
//! reported as not found.

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_backtest::portfolio::{
    equal_weights, inverse_volatility_weights, validate_weights, Holding, PortfolioError,
    DEFAULT_WEIGHT_TOLERANCE,
};
use spectra_db::portfolios::{self, HoldingRow, PortfolioRow, PortfolioWithHoldings};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, AppState, Identity};

const NOT_FOUND: &str = "Portfolio not found.";
const MAX_NAME_CHARS: usize = 120;
const METHOD_FIXED: &str = "fixed";
const METHOD_EQUAL: &str = "equal";
const METHOD_INVERSE_VOLATILITY: &str = "inverse_volatility";

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
    /// `fixed` (default), `equal` or `inverse_volatility`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    pub holdings: Vec<HoldingInput>,
}

/// A saved portfolio with its holdings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    /// `fixed`, `equal` or `inverse_volatility`.
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

/// Builds the target weights for a method, or the reason the request cannot be built.
fn build_weights(method: &str, holdings: &[HoldingInput]) -> Result<Vec<Holding>, String> {
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
            Ok(out)
        }
        METHOD_EQUAL => equal_weights(&symbols).map_err(refused),
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
            inverse_volatility_weights(&symbols, &vols).map_err(refused)
        }
        _ => Err("method must be fixed, equal or inverse_volatility.".to_string()),
    }
}

/// Save a named portfolio with target weights.
#[utoipa::path(post, path = "/api/v1/portfolios", tag = "portfolios", request_body = CreatePortfolioBody,
    responses(
        (status = 201, description = "Saved", body = PortfolioView),
        (status = 409, description = "A portfolio with this name already exists in the workspace", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid name, method, holdings or weights", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreatePortfolioBody = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid portfolio: {e}")),
    };
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return error_response(422, "Name must be 1 to 120 characters.");
    }
    let method = req.method.as_deref().unwrap_or(METHOD_FIXED);
    let weights = match build_weights(method, &req.holdings) {
        Ok(w) => w,
        Err(message) => return error_response(422, message),
    };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let pairs: Vec<(String, f64)> = weights.into_iter().map(|h| (h.symbol, h.weight)).collect();
    let created_by = Uuid::parse_str(&id.user).ok();
    match portfolios::create_portfolio_with_method(&pool, ws, name, method, &pairs, created_by).await {
        Ok(saved) => to_json(201, &PortfolioView::from(saved)),
        Err(e) => {
            if let Some(db) = e.as_database_error() {
                match db.code().as_deref() {
                    // 23505: unique violation, the name is taken.
                    Some("23505") => {
                        return error_response(409, "A portfolio with this name already exists.")
                    }
                    // 23514: check violation, raised by the weight-sum trigger or the method check.
                    Some("23514") => return error_response(422, db.message().to_string()),
                    _ => {}
                }
            }
            db_error(&e)
        }
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
