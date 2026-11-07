//! Saved portfolios (M15 Task 1): a name and target weights per workspace.
//!
//! The database refuses a portfolio whose weights do not sum to 1 within 1e-6 (a deferred
//! trigger, checked at commit). The application checks the same rule first with
//! `spectra_backtest::portfolio::validate_weights`, so the caller gets a clear error before
//! anything is written.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::Error;

const INSERT_SQL: &str = include_str!("../queries/portfolio_insert.sql");
const INSERT_HOLDING_SQL: &str = include_str!("../queries/portfolio_holding_insert.sql");
const GET_SQL: &str = include_str!("../queries/portfolio_get.sql");
const LIST_SQL: &str = include_str!("../queries/portfolios_list.sql");
const HOLDINGS_SQL: &str = include_str!("../queries/portfolio_holdings_list.sql");

/// A saved portfolio, without its holdings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PortfolioRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    /// `fixed`, `equal` or `inverse_volatility`.
    pub method: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// One holding: a symbol and its target weight (a fraction, not a percentage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct HoldingRow {
    pub symbol: String,
    pub weight: f64,
}

/// A saved portfolio with its holdings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioWithHoldings {
    pub portfolio: PortfolioRow,
    pub holdings: Vec<HoldingRow>,
}

/// Save a named portfolio with its target weights, in one transaction.
///
/// # Errors
/// - A database error if the name is already used in the workspace (unique violation).
/// - A database error (check violation) if the weights do not sum to 1 within 1e-6, have no
///   holdings, or a weight is outside `[0, 1]`. Nothing is stored in that case.
pub async fn create_portfolio(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    holdings: &[(String, f64)],
    created_by: Option<Uuid>,
) -> Result<PortfolioWithHoldings, Error> {
    create_portfolio_with_method(pool, workspace_id, name, FIXED_METHOD, holdings, created_by).await
}

/// The method a portfolio was built with, as stored: `fixed`, `equal` or `inverse_volatility`.
pub const FIXED_METHOD: &str = "fixed";

/// As [`create_portfolio`], recording how the weights were built (`method`).
///
/// # Errors
/// As [`create_portfolio`]; also a check violation if `method` is not one of the three.
pub async fn create_portfolio_with_method(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    method: &str,
    holdings: &[(String, f64)],
    created_by: Option<Uuid>,
) -> Result<PortfolioWithHoldings, Error> {
    let mut tx = pool.begin().await?;
    let portfolio: PortfolioRow = sqlx::query_as(INSERT_SQL)
        .bind(workspace_id)
        .bind(name)
        .bind(created_by)
        .bind(method)
        .fetch_one(&mut *tx)
        .await?;
    for (symbol, weight) in holdings {
        sqlx::query(INSERT_HOLDING_SQL)
            .bind(portfolio.id)
            .bind(workspace_id)
            .bind(symbol)
            .bind(weight)
            .execute(&mut *tx)
            .await?;
    }
    // The weight-sum trigger runs here, at commit.
    tx.commit().await?;
    let holdings = list_holdings(pool, workspace_id, portfolio.id).await?;
    Ok(PortfolioWithHoldings { portfolio, holdings })
}

/// One portfolio in a workspace, with its holdings, or `None` if it does not exist there.
///
/// # Errors
/// Any database error.
pub async fn get_portfolio(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<PortfolioWithHoldings>, Error> {
    let Some(portfolio) =
        sqlx::query_as::<_, PortfolioRow>(GET_SQL).bind(workspace_id).bind(id).fetch_optional(pool).await?
    else {
        return Ok(None);
    };
    let holdings = list_holdings(pool, workspace_id, id).await?;
    Ok(Some(PortfolioWithHoldings { portfolio, holdings }))
}

/// The portfolios of a workspace, by name, without holdings.
///
/// # Errors
/// Any database error.
pub async fn list_portfolios(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<PortfolioRow>, Error> {
    sqlx::query_as(LIST_SQL).bind(workspace_id).fetch_all(pool).await
}

async fn list_holdings(
    pool: &PgPool,
    workspace_id: Uuid,
    portfolio_id: Uuid,
) -> Result<Vec<HoldingRow>, Error> {
    sqlx::query_as(HOLDINGS_SQL).bind(workspace_id).bind(portfolio_id).fetch_all(pool).await
}
