//! Saved portfolios (M15 Task 1): create, list and read named portfolios with target weights.
//!
//! A save is checked twice: first here with `spectra_backtest::portfolio::validate_weights`, so
//! the caller gets a clear message, then by the database, which refuses the rows if the weights
//! do not sum to 1 within 1e-6. Everything is scoped to the caller's workspace; a portfolio in
//! another workspace is reported as not found.

use axum::body::Bytes;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_backtest::portfolio::{validate_weights, Holding, DEFAULT_WEIGHT_TOLERANCE};
use spectra_db::portfolios::{self, HoldingRow, PortfolioRow, PortfolioWithHoldings};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, AppState, Identity};

const NOT_FOUND: &str = "Portfolio not found.";
const MAX_NAME_CHARS: usize = 120;

/// One holding in a request: a symbol and its target weight (a fraction, not a percentage).
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
    /// Target weights. They must be non-negative and sum to 1 within 1e-6.
    pub holdings: Vec<HoldingBody>,
}

/// A saved portfolio with its holdings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub holdings: Vec<HoldingBody>,
}

/// A saved portfolio without its holdings (the list view).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSummaryView {
    pub id: Uuid,
    pub name: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<PortfolioWithHoldings> for PortfolioView {
    fn from(p: PortfolioWithHoldings) -> Self {
        Self {
            id: p.portfolio.id,
            workspace_id: p.portfolio.workspace_id,
            name: p.portfolio.name,
            created_by: p.portfolio.created_by,
            created_at: p.portfolio.created_at,
            holdings: p.holdings.into_iter().map(HoldingBody::from).collect(),
        }
    }
}

impl From<PortfolioRow> for PortfolioSummaryView {
    fn from(p: PortfolioRow) -> Self {
        Self { id: p.id, name: p.name, created_by: p.created_by, created_at: p.created_at }
    }
}

impl From<HoldingRow> for HoldingBody {
    fn from(h: HoldingRow) -> Self {
        Self { symbol: h.symbol, weight: h.weight }
    }
}

/// Save a named portfolio with target weights.
#[utoipa::path(post, path = "/api/v1/portfolios", tag = "portfolios", request_body = CreatePortfolioBody,
    responses(
        (status = 201, description = "Saved", body = PortfolioView),
        (status = 409, description = "A portfolio with this name already exists in the workspace", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid name, holdings or weights", body = crate::openapi::ErrorDoc),
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
    let holdings: Vec<Holding> = req
        .holdings
        .iter()
        .map(|h| Holding { symbol: h.symbol.trim().to_string(), weight: h.weight })
        .collect();
    if let Err(e) = validate_weights(&holdings, DEFAULT_WEIGHT_TOLERANCE) {
        return error_response(422, e.to_string());
    }
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let pairs: Vec<(String, f64)> = holdings.into_iter().map(|h| (h.symbol, h.weight)).collect();
    let created_by = Uuid::parse_str(&id.user).ok();
    match portfolios::create_portfolio(&pool, ws, name, &pairs, created_by).await {
        Ok(saved) => to_json(201, &PortfolioView::from(saved)),
        Err(e) => {
            if let Some(db) = e.as_database_error() {
                match db.code().as_deref() {
                    // 23505: unique violation, the name is taken.
                    Some("23505") => {
                        return error_response(409, "A portfolio with this name already exists.")
                    }
                    // 23514: check violation, raised by the weight-sum trigger.
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
