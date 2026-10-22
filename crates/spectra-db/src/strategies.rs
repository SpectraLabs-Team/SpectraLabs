//! Saved strategies and their versions (M2, Strategy Studio).
//!
//! A saved strategy is a named record in a workspace. Each of its versions is
//! an immutable row: a built-in strategy (by name) or DSL script with a full parameter set,
//! keyed by `source_hash` (the source SHA-256) and `config_hash`
//! (SHA-256 of the canonical parameters). [`save_version`] returns the existing
//! version when the same code and parameters were saved before, so a version id
//! always means one exact computation. Every statement lives in `queries/`,
//! exercised by `tests/strategy_checks.sql`.

use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// A saved strategy, with its version count and latest version.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Strategy {
    /// Strategy id (UUIDv7).
    pub id: Uuid,
    /// Name, unique in the workspace.
    pub name: String,
    /// Free text.
    pub description: String,
    /// Who created it.
    pub created_by: Option<Uuid>,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Number of versions.
    pub version_count: i64,
    /// The newest version's id.
    pub latest_version_id: Option<Uuid>,
    /// The newest version's number.
    pub latest_version: Option<i32>,
    /// The newest version's built-in strategy.
    pub latest_builtin: Option<String>,
}

/// One immutable version of a saved strategy.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct StrategyVersion {
    /// Version id (UUIDv7): what a backtest's `strategy_version_id` names.
    pub id: Uuid,
    /// The strategy it belongs to.
    pub strategy_id: Uuid,
    /// That strategy's name.
    pub strategy_name: String,
    /// 1, 2, ... within the strategy.
    pub version: i32,
    /// Built-in strategy name, e.g. `sma_cross` or `dsl`.
    pub builtin: String,
    /// SHA-256 of the source when saved.
    pub source_hash: String,
    /// SHA-256 of the canonical parameter text.
    pub config_hash: String,
    /// The parameter schema when saved.
    pub parameter_schema: JsonValue,
    /// Every parameter, defaults filled in.
    pub parameters: JsonValue,
    /// User-authored DSL code (None for pure Rust built-ins).
    pub code: Option<String>,
    /// Who saved it.
    pub created_by: Option<Uuid>,
    /// Saved at.
    pub created_at: DateTime<Utc>,
}

/// What [`save_version`] stores.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVersion {
    /// Built-in strategy name, or `dsl`.
    pub builtin: String,
    /// SHA-256 of its source.
    pub source_hash: String,
    /// SHA-256 of the canonical parameters.
    pub config_hash: String,
    /// The parameter schema.
    pub parameter_schema: JsonValue,
    /// Every parameter, defaults filled in.
    pub parameters: JsonValue,
    /// User-authored DSL code (None for pure built-ins).
    pub code: Option<String>,
    /// Who saves it.
    pub created_by: Option<Uuid>,
}

/// A mutable draft of a strategy (autosaved by the editor).
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct StrategyDraft {
    pub strategy_id: Uuid,
    pub workspace_id: Uuid,
    pub code: String,
    pub parameters: JsonValue,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

/// A named parameter set belonging to a strategy version.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ParameterSet {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_version_id: Uuid,
    pub name: String,
    pub description: String,
    pub parameters: JsonValue,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// Creates a saved strategy. `None` when the workspace already has one called `name`.
///
/// # Errors
/// Any database error.
pub async fn create(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: &str,
    created_by: Option<Uuid>,
) -> Result<Option<Strategy>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_create.sql"))
        .bind(workspace_id)
        .bind(name)
        .bind(description)
        .bind(created_by)
        .fetch_optional(pool)
        .await
}

/// The workspace's saved strategies, newest first (at most `limit`).
///
/// # Errors
/// Any database error.
pub async fn list(pool: &PgPool, workspace_id: Uuid, limit: i64) -> Result<Vec<Strategy>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_list.sql"))
        .bind(workspace_id)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// The saved strategy `id`, if it belongs to `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<Strategy>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Every version of the strategy `strategy_id`, newest first.
///
/// # Errors
/// Any database error.
pub async fn versions(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
) -> Result<Vec<StrategyVersion>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_versions_of.sql"))
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_all(pool)
        .await
}

/// The version `id`, if it belongs to `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn get_version(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<StrategyVersion>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_version_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Saves a version of `strategy_id`: the existing version when this code and
/// these parameters were saved before (`created` false), else the next version
/// number (`created` true). `None` when the strategy is not in the workspace.
///
/// The strategy row is locked for the transaction, so concurrent saves are
/// numbered one after another and never collide.
///
/// # Errors
/// Any database error.
pub async fn save_version(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
    v: &NewVersion,
) -> Result<Option<(StrategyVersion, bool)>, Error> {
    let mut tx = pool.begin().await?;
    let locked: Option<(Uuid,)> = sqlx::query_as(include_str!("../queries/strategy_lock.sql"))
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_optional(&mut *tx)
        .await?;
    if locked.is_none() {
        return Ok(None);
    }
    let existing: Option<StrategyVersion> =
        sqlx::query_as(include_str!("../queries/strategy_version_find.sql"))
            .bind(workspace_id)
            .bind(strategy_id)
            .bind(&v.source_hash)
            .bind(&v.config_hash)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(found) = existing {
        tx.commit().await?;
        return Ok(Some((found, false)));
    }
    let created: StrategyVersion = sqlx::query_as(include_str!("../queries/strategy_version_insert.sql"))
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(&v.builtin)
        .bind(&v.source_hash)
        .bind(&v.config_hash)
        .bind(&v.parameter_schema)
        .bind(&v.parameters)
        .bind(&v.code)
        .bind(v.created_by)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some((created, true)))
}

/// Fetch the active draft for a strategy.
///
/// # Errors
/// Any database error.
pub async fn get_draft(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
) -> Result<Option<StrategyDraft>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_draft_get.sql"))
        .bind(workspace_id)
        .bind(strategy_id)
        .fetch_optional(pool)
        .await
}

/// Autosave or update a mutable draft for a strategy.
///
/// # Errors
/// Any database error.
pub async fn save_draft(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Uuid,
    code: &str,
    parameters: &JsonValue,
    updated_by: Option<Uuid>,
) -> Result<StrategyDraft, Error> {
    sqlx::query_as(include_str!("../queries/strategy_draft_save.sql"))
        .bind(workspace_id)
        .bind(strategy_id)
        .bind(code)
        .bind(parameters)
        .bind(updated_by)
        .fetch_one(pool)
        .await
}

/// Create a named parameter set for a strategy version.
///
/// # Errors
/// Any database error.
pub async fn create_parameter_set(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_version_id: Uuid,
    name: &str,
    description: &str,
    parameters: &JsonValue,
    created_by: Option<Uuid>,
) -> Result<ParameterSet, Error> {
    let id = crate::new_id();
    sqlx::query_as(include_str!("../queries/strategy_param_set_create.sql"))
        .bind(id)
        .bind(workspace_id)
        .bind(strategy_version_id)
        .bind(name)
        .bind(description)
        .bind(parameters)
        .bind(created_by)
        .fetch_one(pool)
        .await
}

/// List parameter sets for a strategy version.
///
/// # Errors
/// Any database error.
pub async fn list_parameter_sets(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_version_id: Uuid,
) -> Result<Vec<ParameterSet>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_param_set_list.sql"))
        .bind(workspace_id)
        .bind(strategy_version_id)
        .fetch_all(pool)
        .await
}

/// Get a parameter set by id.
///
/// # Errors
/// Any database error.
pub async fn get_parameter_set(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<ParameterSet>, Error> {
    sqlx::query_as(include_str!("../queries/strategy_param_set_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await
}
