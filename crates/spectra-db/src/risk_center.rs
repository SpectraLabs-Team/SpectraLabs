//! Database access for Module 8: Risk Center and Enforcement.

#![allow(clippy::missing_errors_doc)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::Error;

/// Configured risk limits for a workspace or strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct RiskLimitsRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub max_gross_exposure: f64,
    pub max_net_exposure: f64,
    pub max_position_size: f64,
    pub max_concentration_pct: f64,
    pub max_leverage: f64,
    pub max_drawdown_pct: f64,
    pub max_daily_loss: f64,
    pub max_var_pct: f64,
    pub size_reduction_enabled: bool,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

/// Kill switch state for a workspace or strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct KillSwitchRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub active: bool,
    pub reason: String,
    pub triggered_by: Option<Uuid>,
    pub triggered_at: DateTime<Utc>,
    pub cleared_at: Option<DateTime<Utc>>,
    pub cleared_by: Option<Uuid>,
}

/// Audit log record of a pre-trade risk check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct RiskDecisionRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub symbol: String,
    pub side: String,
    pub requested_qty: f64,
    pub price: f64,
    pub approved_qty: f64,
    pub verdict: String,
    pub breaches: serde_json::Value,
    pub reasons: Vec<String>,
    pub metrics: serde_json::Value,
    pub actor_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A persisted position in a portfolio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PortfolioPositionRow {
    pub symbol: String,
    pub qty: f64,
    pub price: f64,
    pub updated_at: DateTime<Utc>,
}

/// Persisted account balance and equity high-water mark for drawdown / daily loss.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PortfolioBalanceRow {
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
    pub updated_at: DateTime<Utc>,
}

/// Historical equity step snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct EquitySnapshotRow {
    pub step: i64,
    pub ts_ns: i64,
    pub equity: f64,
    pub cash: f64,
    pub created_at: DateTime<Utc>,
}

const GET_LIMITS_SQL: &str = include_str!("../queries/risk_limits_get.sql");
const UPSERT_LIMITS_SQL: &str = include_str!("../queries/risk_limits_upsert.sql");
const GET_KILL_SWITCH_SQL: &str = include_str!("../queries/risk_kill_switch_get.sql");
const SET_KILL_SWITCH_SQL: &str = include_str!("../queries/risk_kill_switch_set.sql");
const LIST_KILL_SWITCHES_SQL: &str = include_str!("../queries/risk_kill_switches_list.sql");
const INSERT_DECISION_SQL: &str = include_str!("../queries/risk_decision_insert.sql");
const GET_DECISION_SQL: &str = include_str!("../queries/risk_decision_get.sql");
const LIST_DECISIONS_SQL: &str = include_str!("../queries/risk_decisions_list.sql");

const GET_PORTFOLIO_POSITIONS_SQL: &str = include_str!("../queries/portfolio_positions_get.sql");
const UPSERT_PORTFOLIO_POSITION_SQL: &str = include_str!("../queries/portfolio_position_upsert.sql");
const GET_PORTFOLIO_BALANCE_SQL: &str = include_str!("../queries/portfolio_balance_get.sql");
const UPSERT_PORTFOLIO_BALANCE_SQL: &str = include_str!("../queries/portfolio_balance_upsert.sql");
const INSERT_EQUITY_SNAPSHOT_SQL: &str = include_str!("../queries/equity_snapshot_insert.sql");
const LIST_EQUITY_SNAPSHOTS_SQL: &str = include_str!("../queries/equity_snapshots_list.sql");

/// Get risk limits for a workspace and optional strategy.
pub async fn get_limits(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Result<Option<RiskLimitsRow>, Error> {
    sqlx::query_as::<_, RiskLimitsRow>(GET_LIMITS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_optional(pool)
        .await
}

/// Upsert risk limits for a workspace or strategy.
pub async fn upsert_limits(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    max_gross_exposure: f64,
    max_net_exposure: f64,
    max_position_size: f64,
    max_concentration_pct: f64,
    max_leverage: f64,
    max_drawdown_pct: f64,
    max_daily_loss: f64,
    max_var_pct: f64,
    size_reduction_enabled: bool,
    updated_by: Option<Uuid>,
) -> Result<RiskLimitsRow, Error> {
    sqlx::query_as::<_, RiskLimitsRow>(UPSERT_LIMITS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(max_gross_exposure)
        .bind(max_net_exposure)
        .bind(max_position_size)
        .bind(max_concentration_pct)
        .bind(max_leverage)
        .bind(max_drawdown_pct)
        .bind(max_daily_loss)
        .bind(max_var_pct)
        .bind(size_reduction_enabled)
        .bind(updated_by)
        .fetch_one(pool)
        .await
}

/// Get kill switch state for a workspace and optional strategy.
pub async fn get_kill_switch(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Result<Option<KillSwitchRow>, Error> {
    sqlx::query_as::<_, KillSwitchRow>(GET_KILL_SWITCH_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_optional(pool)
        .await
}

/// Set or clear a kill switch.
pub async fn set_kill_switch(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    active: bool,
    reason: &str,
    user_id: Option<Uuid>,
) -> Result<KillSwitchRow, Error> {
    sqlx::query_as::<_, KillSwitchRow>(SET_KILL_SWITCH_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(active)
        .bind(reason)
        .bind(user_id)
        .fetch_one(pool)
        .await
}

/// List all kill switches in a workspace.
pub async fn list_kill_switches(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<KillSwitchRow>, Error> {
    sqlx::query_as::<_, KillSwitchRow>(LIST_KILL_SWITCHES_SQL).bind(workspace_id).fetch_all(pool).await
}

/// Record a pre-trade decision in the audit trail.
pub async fn record_decision(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    symbol: &str,
    side: &str,
    requested_qty: f64,
    price: f64,
    approved_qty: f64,
    verdict: &str,
    breaches: &serde_json::Value,
    reasons: &[String],
    metrics: &serde_json::Value,
    actor_id: Option<Uuid>,
) -> Result<RiskDecisionRow, Error> {
    sqlx::query_as::<_, RiskDecisionRow>(INSERT_DECISION_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(symbol)
        .bind(side)
        .bind(requested_qty)
        .bind(price)
        .bind(approved_qty)
        .bind(verdict)
        .bind(breaches)
        .bind(reasons)
        .bind(metrics)
        .bind(actor_id)
        .fetch_one(pool)
        .await
}

/// Fetch a single pre-trade decision by ID.
pub async fn get_decision(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<RiskDecisionRow>, Error> {
    sqlx::query_as::<_, RiskDecisionRow>(GET_DECISION_SQL)
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// List recent decisions in a workspace.
pub async fn list_decisions(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<RiskDecisionRow>, Error> {
    sqlx::query_as::<_, RiskDecisionRow>(LIST_DECISIONS_SQL)
        .bind(workspace_id)
        .bind(limit as i32)
        .fetch_all(pool)
        .await
}

/// Upsert a position for a workspace and strategy.
pub async fn upsert_position(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    symbol: &str,
    qty: f64,
    price: f64,
) -> Result<(), Error> {
    sqlx::query(UPSERT_PORTFOLIO_POSITION_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(symbol)
        .bind(qty)
        .bind(price)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Fetch all positions for a workspace and strategy.
pub async fn get_positions(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Result<Vec<PortfolioPositionRow>, Error> {
    sqlx::query_as::<_, PortfolioPositionRow>(GET_PORTFOLIO_POSITIONS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_all(pool)
        .await
}

/// Upsert portfolio balances (cash, equity, peak equity, daily start equity).
pub async fn upsert_balance(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    cash: f64,
    equity: f64,
    peak_equity: f64,
    daily_start_equity: f64,
) -> Result<(), Error> {
    sqlx::query(UPSERT_PORTFOLIO_BALANCE_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(cash)
        .bind(equity)
        .bind(peak_equity)
        .bind(daily_start_equity)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Get portfolio balance for a workspace and strategy.
pub async fn get_balance(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Result<Option<PortfolioBalanceRow>, Error> {
    sqlx::query_as::<_, PortfolioBalanceRow>(GET_PORTFOLIO_BALANCE_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_optional(pool)
        .await
}

/// Record a time-series equity snapshot.
pub async fn record_equity_snapshot(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    run_id: Option<Uuid>,
    step: i64,
    ts_ns: i64,
    equity: f64,
    cash: f64,
) -> Result<(), Error> {
    sqlx::query(INSERT_EQUITY_SNAPSHOT_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(run_id)
        .bind(step)
        .bind(ts_ns)
        .bind(equity)
        .bind(cash)
        .execute(pool)
        .await
        .map(|_| ())
}

/// List recent equity snapshots.
pub async fn list_equity_snapshots(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<EquitySnapshotRow>, Error> {
    sqlx::query_as::<_, EquitySnapshotRow>(LIST_EQUITY_SNAPSHOTS_SQL)
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(limit as i32)
        .fetch_all(pool)
        .await
}
