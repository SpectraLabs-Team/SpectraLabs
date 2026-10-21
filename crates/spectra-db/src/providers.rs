//! Market data provider configs and dataset refresh schedules (M1b task 13).
//!
//! Workspace-scoped configuration for external/mock market data providers,
//! vault secret bindings, and automated scheduled refreshes. All queries live in `queries/`
//! and are exercised by `tests/provider_checks.sql`.

use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// A configured market data provider in a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataProviderConfig {
    /// Config id.
    pub id: Uuid,
    /// Workspace id.
    pub workspace_id: Uuid,
    /// Friendly name (e.g. "Alpaca Paper Equities").
    pub name: String,
    /// Provider kind ("fake", "alpaca").
    pub provider: String,
    /// Reference to vault secret, if any.
    pub secret_id: Option<Uuid>,
    /// Provider settings (feed, endpoints, etc.).
    pub settings: JsonValue,
    /// User who created this config, if known.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last updated.
    pub updated_at: DateTime<Utc>,
}

/// A dataset automated refresh schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetRefreshSchedule {
    /// Schedule id.
    pub id: Uuid,
    /// Workspace id.
    pub workspace_id: Uuid,
    /// Target dataset id.
    pub dataset_id: Uuid,
    /// Associated provider config id.
    pub provider_config_id: Uuid,
    /// Symbols to refresh.
    pub symbols: Vec<String>,
    /// Interval in seconds between refreshes (e.g. 86400 for daily).
    pub interval_seconds: i64,
    /// Number of bars to refetch backwards for revision overlap detection.
    pub lookback_bars: i32,
    /// Whether schedule is enabled.
    pub enabled: bool,
    /// When last executed.
    pub last_run_at: Option<DateTime<Utc>>,
    /// When next execution is due.
    pub next_run_at: DateTime<Utc>,
    /// User who created this schedule, if known.
    pub created_by: Option<Uuid>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last updated.
    pub updated_at: DateTime<Utc>,
}

// ------------------------------------------------------------- provider configs

type ProviderConfigRow =
    (Uuid, Uuid, String, String, Option<Uuid>, JsonValue, Option<Uuid>, DateTime<Utc>, DateTime<Utc>);

fn map_provider_config(r: ProviderConfigRow) -> DataProviderConfig {
    DataProviderConfig {
        id: r.0,
        workspace_id: r.1,
        name: r.2,
        provider: r.3,
        secret_id: r.4,
        settings: r.5,
        created_by: r.6,
        created_at: r.7,
        updated_at: r.8,
    }
}

/// Upserts a data provider configuration in `workspace_id`.
///
/// # Errors
/// Database error.
pub async fn upsert_provider_config(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    provider: &str,
    secret_id: Option<Uuid>,
    settings: &JsonValue,
    created_by: Option<Uuid>,
) -> Result<DataProviderConfig, Error> {
    let row: ProviderConfigRow = sqlx::query_as(include_str!("../queries/provider_config_upsert.sql"))
        .bind(workspace_id)
        .bind(name)
        .bind(provider)
        .bind(secret_id)
        .bind(settings)
        .bind(created_by)
        .fetch_one(pool)
        .await?;
    Ok(map_provider_config(row))
}

/// Retrieves a provider config by `(workspace_id, id)`.
///
/// # Errors
/// Database error.
pub async fn get_provider_config(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<DataProviderConfig>, Error> {
    let row: Option<ProviderConfigRow> = sqlx::query_as(include_str!("../queries/provider_config_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_provider_config))
}

/// Lists all provider configs in `workspace_id`, sorted by name.
///
/// # Errors
/// Database error.
pub async fn list_provider_configs(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<DataProviderConfig>, Error> {
    let rows: Vec<ProviderConfigRow> = sqlx::query_as(include_str!("../queries/provider_configs_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_provider_config).collect())
}

/// Deletes a provider config by `(workspace_id, id)`.
///
/// # Errors
/// Database error.
pub async fn delete_provider_config(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let res: Option<(Uuid,)> = sqlx::query_as(include_str!("../queries/provider_config_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(res.is_some())
}

// ------------------------------------------------------------- refresh schedules

type RefreshScheduleRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    Vec<String>,
    i64,
    i32,
    bool,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<Uuid>,
    DateTime<Utc>,
    DateTime<Utc>,
);

fn map_refresh_schedule(r: RefreshScheduleRow) -> DatasetRefreshSchedule {
    DatasetRefreshSchedule {
        id: r.0,
        workspace_id: r.1,
        dataset_id: r.2,
        provider_config_id: r.3,
        symbols: r.4,
        interval_seconds: r.5,
        lookback_bars: r.6,
        enabled: r.7,
        last_run_at: r.8,
        next_run_at: r.9,
        created_by: r.10,
        created_at: r.11,
        updated_at: r.12,
    }
}

/// Upserts a dataset refresh schedule in `workspace_id`.
///
/// # Errors
/// Database error.
pub async fn upsert_refresh_schedule(
    pool: &PgPool,
    workspace_id: Uuid,
    dataset_id: Uuid,
    provider_config_id: Uuid,
    symbols: &[String],
    interval_seconds: i64,
    lookback_bars: i32,
    enabled: bool,
    created_by: Option<Uuid>,
) -> Result<DatasetRefreshSchedule, Error> {
    let row: RefreshScheduleRow = sqlx::query_as(include_str!("../queries/refresh_schedule_upsert.sql"))
        .bind(workspace_id)
        .bind(dataset_id)
        .bind(provider_config_id)
        .bind(symbols)
        .bind(interval_seconds)
        .bind(lookback_bars)
        .bind(enabled)
        .bind(created_by)
        .fetch_one(pool)
        .await?;
    Ok(map_refresh_schedule(row))
}

/// Retrieves a dataset refresh schedule by `(workspace_id, dataset_id)`.
///
/// # Errors
/// Database error.
pub async fn get_refresh_schedule(
    pool: &PgPool,
    workspace_id: Uuid,
    dataset_id: Uuid,
) -> Result<Option<DatasetRefreshSchedule>, Error> {
    let row: Option<RefreshScheduleRow> = sqlx::query_as(include_str!("../queries/refresh_schedule_get.sql"))
        .bind(workspace_id)
        .bind(dataset_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_refresh_schedule))
}

/// Retrieves a refresh schedule by `(workspace_id, id)`.
///
/// # Errors
/// Database error.
pub async fn get_refresh_schedule_by_id(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<DatasetRefreshSchedule>, Error> {
    let row: Option<RefreshScheduleRow> =
        sqlx::query_as(include_str!("../queries/refresh_schedule_get_by_id.sql"))
            .bind(workspace_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(map_refresh_schedule))
}

/// Lists all dataset refresh schedules in `workspace_id`.
///
/// # Errors
/// Database error.
pub async fn list_refresh_schedules(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<DatasetRefreshSchedule>, Error> {
    let rows: Vec<RefreshScheduleRow> = sqlx::query_as(include_str!("../queries/refresh_schedules_list.sql"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_refresh_schedule).collect())
}

/// Lists enabled schedules due for refresh on or before `as_of`.
///
/// # Errors
/// Database error.
pub async fn list_due_refresh_schedules(
    pool: &PgPool,
    as_of: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<DatasetRefreshSchedule>, Error> {
    let rows: Vec<RefreshScheduleRow> = sqlx::query_as(include_str!("../queries/refresh_schedules_due.sql"))
        .bind(as_of)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_refresh_schedule).collect())
}

/// Updates the execution status of a schedule after a run.
///
/// # Errors
/// Database error.
pub async fn update_refresh_schedule_run(
    pool: &PgPool,
    id: Uuid,
    last_run_at: DateTime<Utc>,
    next_run_at: DateTime<Utc>,
) -> Result<bool, Error> {
    let res: Option<(Uuid,)> = sqlx::query_as(include_str!("../queries/refresh_schedule_update_run.sql"))
        .bind(id)
        .bind(last_run_at)
        .bind(next_run_at)
        .fetch_optional(pool)
        .await?;
    Ok(res.is_some())
}

/// Deletes a refresh schedule by `(workspace_id, id)`.
///
/// # Errors
/// Database error.
pub async fn delete_refresh_schedule(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<bool, Error> {
    let res: Option<(Uuid,)> = sqlx::query_as(include_str!("../queries/refresh_schedule_delete.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(res.is_some())
}
