//! Paper trading accounts, orders, and fills (Module 9).
//!
//! A paper account exists per strategy (in the caller's workspace), stored in the
//! existing `portfolio_balances` and `portfolio_positions` tables.
//! Orders and fills are recorded in `paper_orders` and `paper_fills`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::risk_center::{PortfolioBalanceRow, PortfolioPositionRow};
use crate::Error;

/// Default starting capital for a new simulated paper account ($100,000).
pub const DEFAULT_PAPER_STARTING_CASH: f64 = 100_000.0;

/// A paper trading account holding balance and open positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperAccount {
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub balance: PortfolioBalanceRow,
    pub positions: Vec<PortfolioPositionRow>,
}

/// A stored paper order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PaperOrderRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub qty: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub tif: String,
    pub status: String,
    pub filled_qty: f64,
    pub filled_avg_price: Option<f64>,
    pub reject_reason: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A stored fill for a paper order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PaperFillRow {
    pub id: Uuid,
    pub order_id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub fee: f64,
    pub created_at: DateTime<Utc>,
}

const INSERT_ORDER_SQL: &str = include_str!("../queries/paper_order_insert.sql");
const GET_ORDER_SQL: &str = include_str!("../queries/paper_order_get.sql");
const LIST_ORDERS_SQL: &str = include_str!("../queries/paper_orders_list.sql");
const UPDATE_ORDER_STATUS_SQL: &str = include_str!("../queries/paper_order_update_status.sql");
const INSERT_FILL_SQL: &str = include_str!("../queries/paper_fill_insert.sql");
const LIST_FILLS_FOR_ORDER_SQL: &str = include_str!("../queries/paper_fills_list_for_order.sql");
const CLEAR_POSITIONS_SQL: &str = include_str!("../queries/paper_positions_clear.sql");

/// Get the paper account for a strategy, initializing starting balance if it does not exist yet.
///
/// # Errors
/// Any database error.
pub async fn get_or_init_paper_account(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
    initial_cash: Option<f64>,
) -> Result<PaperAccount, Error> {
    let starting = initial_cash.unwrap_or(DEFAULT_PAPER_STARTING_CASH);

    let balance = if let Some(b) = crate::risk_center::get_balance(pool, workspace_id, Some(strategy_id)).await? {
        b
    } else {
        // Task 1: One paper account per strategy, with starting balance and no positions.
        crate::risk_center::upsert_balance(
            pool,
            workspace_id,
            Some(strategy_id),
            starting,
            starting,
            starting,
            starting,
        )
        .await?;
        crate::risk_center::get_balance(pool, workspace_id, Some(strategy_id))
            .await?
            .unwrap_or(PortfolioBalanceRow {
                cash: starting,
                equity: starting,
                peak_equity: starting,
                daily_start_equity: starting,
                updated_at: Utc::now(),
            })
    };

    let positions = crate::risk_center::get_positions(pool, workspace_id, Some(strategy_id)).await?;

    Ok(PaperAccount {
        workspace_id,
        strategy_id,
        balance,
        positions,
    })
}

/// Reset a strategy's paper account: clear all open positions and reset balance to `initial_cash`.
///
/// # Errors
/// Any database error.
pub async fn reset_paper_account(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
    initial_cash: Option<f64>,
) -> Result<PaperAccount, Error> {
    let starting = initial_cash.unwrap_or(DEFAULT_PAPER_STARTING_CASH);

    // Clear positions
    sqlx::query(CLEAR_POSITIONS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .execute(pool)
        .await?;

    // Reset balance
    crate::risk_center::upsert_balance(
        pool,
        workspace_id,
        Some(strategy_id),
        starting,
        starting,
        starting,
        starting,
    )
    .await?;

    let balance = crate::risk_center::get_balance(pool, workspace_id, Some(strategy_id))
        .await?
        .unwrap_or(PortfolioBalanceRow {
            cash: starting,
            equity: starting,
            peak_equity: starting,
            daily_start_equity: starting,
            updated_at: Utc::now(),
        });

    Ok(PaperAccount {
        workspace_id,
        strategy_id,
        balance,
        positions: Vec::new(),
    })
}

/// Insert a paper order.
///
/// # Errors
/// Any database error.
#[allow(clippy::too_many_arguments)]
pub async fn insert_paper_order(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
    symbol: &str,
    side: &str,
    order_type: &str,
    qty: f64,
    limit_price: Option<f64>,
    stop_price: Option<f64>,
    tif: &str,
    status: &str,
    reject_reason: Option<&str>,
    created_by: Option<Uuid>,
) -> Result<PaperOrderRow, Error> {
    sqlx::query_as(INSERT_ORDER_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(symbol)
        .bind(side)
        .bind(order_type)
        .bind(qty)
        .bind(limit_price)
        .bind(stop_price)
        .bind(tif)
        .bind(status)
        .bind(reject_reason)
        .bind(created_by)
        .fetch_one(pool)
        .await
}

/// Get a paper order by id.
///
/// # Errors
/// Any database error.
pub async fn get_paper_order(
    pool: &PgPool,
    workspace_id: Uuid,
    order_id: Uuid,
) -> Result<Option<PaperOrderRow>, Error> {
    sqlx::query_as(GET_ORDER_SQL)
        .bind(workspace_id)
        .bind(order_id)
        .fetch_optional(pool)
        .await
}

/// List paper orders for a workspace, optionally filtered by strategy and/or status.
///
/// # Errors
/// Any database error.
pub async fn list_paper_orders(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<PaperOrderRow>, Error> {
    sqlx::query_as(LIST_ORDERS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Update a paper order's status and execution details.
///
/// # Errors
/// Any database error.
pub async fn update_paper_order_status(
    pool: &PgPool,
    workspace_id: Uuid,
    order_id: Uuid,
    status: &str,
    filled_qty: f64,
    filled_avg_price: Option<f64>,
    reject_reason: Option<&str>,
) -> Result<Option<PaperOrderRow>, Error> {
    sqlx::query_as(UPDATE_ORDER_STATUS_SQL)
        .bind(workspace_id)
        .bind(order_id)
        .bind(status)
        .bind(filled_qty)
        .bind(filled_avg_price)
        .bind(reject_reason)
        .fetch_optional(pool)
        .await
}

/// Record a paper fill.
///
/// # Errors
/// Any database error.
#[allow(clippy::too_many_arguments)]
pub async fn insert_paper_fill(
    pool: &PgPool,
    order_id: Uuid,
    workspace_id: Uuid,
    strategy_id: Uuid,
    symbol: &str,
    side: &str,
    qty: f64,
    price: f64,
    fee: f64,
) -> Result<PaperFillRow, Error> {
    sqlx::query_as(INSERT_FILL_SQL)
        .bind(order_id)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(symbol)
        .bind(side)
        .bind(qty)
        .bind(price)
        .bind(fee)
        .fetch_one(pool)
        .await
}

/// List fills for an order.
///
/// # Errors
/// Any database error.
pub async fn list_paper_fills_for_order(
    pool: &PgPool,
    workspace_id: Uuid,
    order_id: Uuid,
) -> Result<Vec<PaperFillRow>, Error> {
    sqlx::query_as(LIST_FILLS_FOR_ORDER_SQL)
        .bind(workspace_id)
        .bind(order_id)
        .fetch_all(pool)
        .await
}
