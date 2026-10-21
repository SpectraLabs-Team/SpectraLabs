//! Corporate actions and action sets (M1b task 11).
//!
//! Workspace-scoped tables for split and dividend events per symbol.
//! All queries live in `queries/corporate_action_*.sql` and are tested in
//! `tests/corporate_action_checks.sql`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// A corporate action set summary row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActionSetSummary {
    /// Action set id.
    pub id: Uuid,
    /// Human name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// Number of corporate actions in this set.
    pub action_count: i64,
}

/// A corporate action set detail row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActionSetDetail {
    /// Action set id.
    pub id: Uuid,
    /// Human name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// A single corporate action event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateActionRow {
    /// Action id.
    pub id: Uuid,
    /// Instrument symbol.
    pub symbol: String,
    /// Ex-action timestamp.
    pub ex_date: DateTime<Utc>,
    /// "split" or "dividend".
    pub action_kind: String,
    /// Split from ratio (e.g. 1 in 2:1).
    pub split_from: i32,
    /// Split to ratio (e.g. 2 in 2:1).
    pub split_to: i32,
    /// Cash dividend amount per share (text representation).
    pub dividend_amount: String,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// Creates a new corporate action set in `workspace_id`.
///
/// # Errors
/// Any database error; unique violation if name is taken.
pub async fn create_action_set(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: &str,
    created_by: Option<Uuid>,
) -> Result<CorporateActionSetDetail, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>);
    let row: Row = sqlx::query_as(include_str!("../queries/corporate_action_set_create.sql"))
        .bind(workspace_id)
        .bind(name)
        .bind(description)
        .bind(created_by)
        .fetch_one(pool)
        .await?;
    Ok(CorporateActionSetDetail { id: row.0, name: row.1, description: row.2, created_at: row.3 })
}

/// Lists all corporate action sets in `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn list_action_sets(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<CorporateActionSetSummary>, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>, i64);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/corporate_action_set_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, description, created_at, action_count)| CorporateActionSetSummary {
            id,
            name,
            description,
            created_at,
            action_count,
        })
        .collect())
}

/// Fetches one corporate action set by `id`.
///
/// # Errors
/// Any database error.
pub async fn get_action_set(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<CorporateActionSetDetail>, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>);
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/corporate_action_set_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, name, description, created_at)| CorporateActionSetDetail {
        id,
        name,
        description,
        created_at,
    }))
}

/// Deletes a corporate action set by `id`.
///
/// # Errors
/// Any database error.
pub async fn delete_action_set(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let row: Option<Uuid> = sqlx::query_scalar(include_str!("../queries/corporate_action_set_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Inserts an individual corporate action into `set_id`.
///
/// # Errors
/// Any database error.
pub async fn insert_action(
    pool: &PgPool,
    workspace_id: Uuid,
    set_id: Uuid,
    symbol: &str,
    ex_date: DateTime<Utc>,
    action_kind: &str,
    split_from: i32,
    split_to: i32,
    dividend_amount: &str,
) -> Result<Uuid, Error> {
    let id: Uuid = sqlx::query_scalar(include_str!("../queries/corporate_action_insert.sql"))
        .bind(workspace_id)
        .bind(set_id)
        .bind(symbol)
        .bind(ex_date)
        .bind(action_kind)
        .bind(split_from)
        .bind(split_to)
        .bind(dividend_amount)
        .fetch_one(pool)
        .await?;
    Ok(id)
}

/// Lists all actions in `set_id`.
///
/// # Errors
/// Any database error.
pub async fn list_actions_by_set(
    pool: &PgPool,
    workspace_id: Uuid,
    set_id: Uuid,
) -> Result<Vec<CorporateActionRow>, Error> {
    type Row = (Uuid, String, DateTime<Utc>, String, i32, i32, String, DateTime<Utc>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/corporate_actions_list_by_set.sql"))
        .bind(workspace_id)
        .bind(set_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, symbol, ex_date, action_kind, split_from, split_to, dividend_amount, created_at)| {
            CorporateActionRow {
                id,
                symbol,
                ex_date,
                action_kind,
                split_from,
                split_to,
                dividend_amount,
                created_at,
            }
        })
        .collect())
}
