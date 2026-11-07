//! Portfolio backtest runs (M15 Task 6): a saved portfolio run through the rebalancing simulator.
//!
//! A run is stored when it starts (`running`), with every input in `request`, and ended once with
//! its result (`succeeded`) or the reason it failed (`failed`). Runs are listed per portfolio,
//! newest first, with a cursor, the way backtest jobs are.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::jobs::Page;
use crate::Error;

const INSERT_SQL: &str = include_str!("../queries/portfolio_run_insert.sql");
const FINISH_SQL: &str = include_str!("../queries/portfolio_run_finish.sql");
const GET_SQL: &str = include_str!("../queries/portfolio_run_get.sql");
const LIST_SQL: &str = include_str!("../queries/portfolio_runs_list.sql");

/// A run is computed.
pub const RUNNING: &str = "running";
/// A run finished with a result.
pub const SUCCEEDED: &str = "succeeded";
/// A run ended with an error.
pub const FAILED: &str = "failed";

/// One stored portfolio run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct PortfolioRunRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// `None` once the portfolio has been deleted.
    pub portfolio_id: Option<Uuid>,
    /// `running`, `succeeded` or `failed`.
    pub status: String,
    /// Every input of the run, including a snapshot of the portfolio.
    pub request: serde_json::Value,
    /// The result, once succeeded.
    pub result: Option<serde_json::Value>,
    /// Why it failed.
    pub error: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// Store a new run as `running`.
///
/// # Errors
/// Any database error, including a foreign-key violation if the portfolio is not in the workspace.
pub async fn start_run(
    pool: &PgPool,
    workspace_id: Uuid,
    portfolio_id: Uuid,
    request: &serde_json::Value,
    created_by: Option<Uuid>,
) -> Result<PortfolioRunRow, Error> {
    sqlx::query_as(INSERT_SQL)
        .bind(workspace_id)
        .bind(portfolio_id)
        .bind(request)
        .bind(created_by)
        .fetch_one(pool)
        .await
}

/// End a running run with its result. `None` if the run is not running (or not in the workspace).
///
/// # Errors
/// Any database error.
pub async fn succeed_run(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    result: &serde_json::Value,
) -> Result<Option<PortfolioRunRow>, Error> {
    finish(pool, workspace_id, id, SUCCEEDED, Some(result), None).await
}

/// End a running run with the reason it failed. `None` if the run is not running.
///
/// # Errors
/// Any database error.
pub async fn fail_run(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    error: &str,
) -> Result<Option<PortfolioRunRow>, Error> {
    finish(pool, workspace_id, id, FAILED, None, Some(error)).await
}

async fn finish(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    status: &str,
    result: Option<&serde_json::Value>,
    error: Option<&str>,
) -> Result<Option<PortfolioRunRow>, Error> {
    sqlx::query_as(FINISH_SQL)
        .bind(workspace_id)
        .bind(id)
        .bind(status)
        .bind(result)
        .bind(error)
        .fetch_optional(pool)
        .await
}

/// One run in a workspace, or `None`.
///
/// # Errors
/// Any database error.
pub async fn get_run(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<PortfolioRunRow>, Error> {
    sqlx::query_as(GET_SQL).bind(workspace_id).bind(id).fetch_optional(pool).await
}

/// A page of a portfolio's runs, newest first. `limit` is clamped to 1..=200.
///
/// # Errors
/// Any database error.
pub async fn list_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    portfolio_id: Uuid,
    cursor: Option<Uuid>,
    limit: u32,
) -> Result<Page<PortfolioRunRow>, Error> {
    let limit = limit.clamp(1, 200);
    let mut items: Vec<PortfolioRunRow> = sqlx::query_as(LIST_SQL)
        .bind(workspace_id)
        .bind(portfolio_id)
        .bind(cursor)
        .bind(i64::from(limit) + 1)
        .fetch_all(pool)
        .await?;
    let next_cursor = if items.len() > limit as usize {
        items.truncate(limit as usize);
        items.last().map(|r| r.id)
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}
