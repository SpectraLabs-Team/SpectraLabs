//! Instrument lifecycles and survivorship-free universes (M1b task 12).
//!
//! Workspace-scoped tables for symbol changes, delistings, and point-in-time
//! universe resolution. All queries live in `queries/` and are exercised by
//! `tests/instrument_lifecycle_checks.sql`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// A recorded symbol ticker change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolChangeRow {
    /// Change id.
    pub id: Uuid,
    /// Previous ticker.
    pub old_symbol: String,
    /// New ticker.
    pub new_symbol: String,
    /// Effective date of change.
    pub effective_date: DateTime<Utc>,
    /// Optional notes.
    pub notes: String,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// A recorded instrument delisting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelistingRow {
    /// Delisting record id.
    pub id: Uuid,
    /// Ticker symbol.
    pub symbol: String,
    /// Delisted timestamp.
    pub delisted_at: DateTime<Utc>,
    /// Reason or notes.
    pub reason: String,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// A universe summary row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseSummary {
    /// Universe id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// Current member count.
    pub member_count: i64,
}

/// A universe detail row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseDetail {
    /// Universe id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// A member symbol in a universe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniverseMemberRow {
    /// Member record id.
    pub id: Uuid,
    /// Symbol.
    pub symbol: String,
    /// Added timestamp.
    pub added_at: DateTime<Utc>,
    /// Removed timestamp, if any.
    pub removed_at: Option<DateTime<Utc>>,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// A universe constituent resolved as of a point-in-time date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUniverseMember {
    /// Original member symbol.
    pub symbol: String,
    /// Symbol active as of date D (after symbol changes).
    pub resolved_symbol: String,
    /// True if active on date D (not delisted on or prior to date D).
    pub is_active: bool,
    /// When delisted, if delisted at all.
    pub delisted_at: Option<DateTime<Utc>>,
}

// ------------------------------------------------------------- symbol changes

/// Records a symbol change in `workspace_id`.
///
/// # Errors
/// Database error; unique constraint violation if `(workspace_id, old_symbol, effective_date)` exists.
pub async fn create_symbol_change(
    pool: &PgPool,
    workspace_id: Uuid,
    old_symbol: &str,
    new_symbol: &str,
    effective_date: DateTime<Utc>,
    notes: &str,
) -> Result<SymbolChangeRow, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>, String, DateTime<Utc>);
    let row: Row = sqlx::query_as(include_str!("../queries/symbol_change_create.sql"))
        .bind(workspace_id)
        .bind(old_symbol)
        .bind(new_symbol)
        .bind(effective_date)
        .bind(notes)
        .fetch_one(pool)
        .await?;
    Ok(SymbolChangeRow {
        id: row.0,
        old_symbol: row.1,
        new_symbol: row.2,
        effective_date: row.3,
        notes: row.4,
        created_at: row.5,
    })
}

/// Lists all symbol changes for `workspace_id`, newest first.
///
/// # Errors
/// Database error.
pub async fn list_symbol_changes(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<SymbolChangeRow>, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>, String, DateTime<Utc>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/symbol_changes_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, old_symbol, new_symbol, effective_date, notes, created_at)| SymbolChangeRow {
            id,
            old_symbol,
            new_symbol,
            effective_date,
            notes,
            created_at,
        })
        .collect())
}

/// Deletes a symbol change by `id`.
///
/// # Errors
/// Database error.
pub async fn delete_symbol_change(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let row: Option<Uuid> = sqlx::query_scalar(include_str!("../queries/symbol_change_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------- delistings

/// Records an instrument delisting in `workspace_id`.
///
/// # Errors
/// Database error; unique constraint violation if already recorded for the symbol and date.
pub async fn create_delisting(
    pool: &PgPool,
    workspace_id: Uuid,
    symbol: &str,
    delisted_at: DateTime<Utc>,
    reason: &str,
) -> Result<DelistingRow, Error> {
    type Row = (Uuid, String, DateTime<Utc>, String, DateTime<Utc>);
    let row: Row = sqlx::query_as(include_str!("../queries/delisting_create.sql"))
        .bind(workspace_id)
        .bind(symbol)
        .bind(delisted_at)
        .bind(reason)
        .fetch_one(pool)
        .await?;
    Ok(DelistingRow { id: row.0, symbol: row.1, delisted_at: row.2, reason: row.3, created_at: row.4 })
}

/// Lists all delistings for `workspace_id`, newest first.
///
/// # Errors
/// Database error.
pub async fn list_delistings(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<DelistingRow>, Error> {
    type Row = (Uuid, String, DateTime<Utc>, String, DateTime<Utc>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/delistings_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, symbol, delisted_at, reason, created_at)| DelistingRow {
            id,
            symbol,
            delisted_at,
            reason,
            created_at,
        })
        .collect())
}

/// Deletes an instrument delisting by `id`.
///
/// # Errors
/// Database error.
pub async fn delete_delisting(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let row: Option<Uuid> = sqlx::query_scalar(include_str!("../queries/delisting_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------- universes

/// Creates a new universe in `workspace_id`.
///
/// # Errors
/// Database error; unique constraint violation if name is taken.
pub async fn create_universe(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: &str,
) -> Result<UniverseDetail, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>);
    let row: Row = sqlx::query_as(include_str!("../queries/universe_create.sql"))
        .bind(workspace_id)
        .bind(name)
        .bind(description)
        .fetch_one(pool)
        .await?;
    Ok(UniverseDetail { id: row.0, name: row.1, description: row.2, created_at: row.3 })
}

/// Lists all universes in `workspace_id` with member count.
///
/// # Errors
/// Database error.
pub async fn list_universes(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<UniverseSummary>, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>, i64);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/universe_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, description, created_at, member_count)| UniverseSummary {
            id,
            name,
            description,
            created_at,
            member_count,
        })
        .collect())
}

/// Fetches one universe by `id`.
///
/// # Errors
/// Database error.
pub async fn get_universe(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<UniverseDetail>, Error> {
    type Row = (Uuid, String, String, DateTime<Utc>);
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/universe_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, name, description, created_at)| UniverseDetail { id, name, description, created_at }))
}

/// Deletes a universe by `id`.
///
/// # Errors
/// Database error.
pub async fn delete_universe(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let row: Option<Uuid> = sqlx::query_scalar(include_str!("../queries/universe_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Adds a member symbol to a universe.
///
/// # Errors
/// Database error.
pub async fn add_universe_member(
    pool: &PgPool,
    workspace_id: Uuid,
    universe_id: Uuid,
    symbol: &str,
    added_at: DateTime<Utc>,
) -> Result<Uuid, Error> {
    type Row = (Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>);
    let row: Row = sqlx::query_as(include_str!("../queries/universe_member_add.sql"))
        .bind(workspace_id)
        .bind(universe_id)
        .bind(symbol)
        .bind(added_at)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Lists all member records for a universe.
///
/// # Errors
/// Database error.
pub async fn list_universe_members(
    pool: &PgPool,
    workspace_id: Uuid,
    universe_id: Uuid,
) -> Result<Vec<UniverseMemberRow>, Error> {
    type Row = (Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>, DateTime<Utc>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/universe_members_list.sql"))
        .bind(workspace_id)
        .bind(universe_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, symbol, added_at, removed_at, created_at)| UniverseMemberRow {
            id,
            symbol,
            added_at,
            removed_at,
            created_at,
        })
        .collect())
}

/// Resolves active symbols of a universe as of date `as_of`.
///
/// Excludes symbols delisted on or prior to `as_of`, keeps symbols delisted after `as_of`,
/// and maps symbols through any effective symbol changes.
///
/// # Errors
/// Database error.
pub async fn resolve_universe_as_of(
    pool: &PgPool,
    workspace_id: Uuid,
    universe_id: Uuid,
    as_of: DateTime<Utc>,
) -> Result<Vec<ResolvedUniverseMember>, Error> {
    type Row = (String, String, bool, Option<DateTime<Utc>>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/universe_resolve_as_of.sql"))
        .bind(workspace_id)
        .bind(universe_id)
        .bind(as_of)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(symbol, resolved_symbol, is_active, delisted_at)| ResolvedUniverseMember {
            symbol,
            resolved_symbol,
            is_active,
            delisted_at,
        })
        .collect())
}
