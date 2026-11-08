//! Alpha Workbench Signal Library database operations (M12).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Base signal entity row.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct SignalRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One immutable version of a saved signal.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct SignalVersionRow {
    pub id: Uuid,
    pub signal_id: Uuid,
    pub workspace_id: Uuid,
    pub version: i32,
    pub signal_type: String,
    pub parameters: Value,
    pub threshold: Option<Value>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Signal with latest version information.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct SignalWithLatest {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: String,
    pub latest_version: i32,
    pub signal_type: String,
    pub parameters: Value,
    pub threshold: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Save a signal and a new version under the given workspace.
///
/// If a signal with `name` exists in `workspace_id`, updates its description/timestamp
/// and increments version number. Otherwise, creates the signal record with version 1.
pub async fn save_signal_version(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: &str,
    signal_type: &str,
    parameters: &Value,
    threshold: Option<&Value>,
    created_by: Option<Uuid>,
) -> Result<SignalVersionRow, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let signal = sqlx::query_as::<_, SignalRow>(
        r"
        INSERT INTO signals (workspace_id, name, description, created_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (workspace_id, name)
        DO UPDATE SET description = EXCLUDED.description, updated_at = clock_timestamp()
        RETURNING *
        ",
    )
    .bind(workspace_id)
    .bind(name)
    .bind(description)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;

    let next_version: (i32,) = sqlx::query_as(
        r"
        SELECT COALESCE(MAX(version), 0) + 1
        FROM signal_versions
        WHERE signal_id = $1
        ",
    )
    .bind(signal.id)
    .fetch_one(&mut *tx)
    .await?;

    let version_row = sqlx::query_as::<_, SignalVersionRow>(
        r"
        INSERT INTO signal_versions (
            signal_id, workspace_id, version, signal_type, parameters, threshold, created_by
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7
        )
        RETURNING *
        ",
    )
    .bind(signal.id)
    .bind(workspace_id)
    .bind(next_version.0)
    .bind(signal_type)
    .bind(parameters)
    .bind(threshold)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(version_row)
}

/// List all saved signals in the workspace with their latest version.
pub async fn list_signals(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<SignalWithLatest>, sqlx::Error> {
    sqlx::query_as::<_, SignalWithLatest>(
        r"
        SELECT DISTINCT ON (s.id)
            s.id,
            s.workspace_id,
            s.name,
            s.description,
            v.version AS latest_version,
            v.signal_type,
            v.parameters,
            v.threshold,
            s.created_at,
            s.updated_at
        FROM signals s
        JOIN signal_versions v ON v.signal_id = s.id
        WHERE s.workspace_id = $1
        ORDER BY s.id, v.version DESC
        ",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

/// Get a signal by id with its latest version.
pub async fn get_signal_by_id(
    pool: &PgPool,
    workspace_id: Uuid,
    signal_id: Uuid,
) -> Result<Option<SignalWithLatest>, sqlx::Error> {
    sqlx::query_as::<_, SignalWithLatest>(
        r"
        SELECT
            s.id,
            s.workspace_id,
            s.name,
            s.description,
            v.version AS latest_version,
            v.signal_type,
            v.parameters,
            v.threshold,
            s.created_at,
            s.updated_at
        FROM signals s
        JOIN signal_versions v ON v.signal_id = s.id
        WHERE s.workspace_id = $1 AND s.id = $2
        ORDER BY v.version DESC
        LIMIT 1
        ",
    )
    .bind(workspace_id)
    .bind(signal_id)
    .fetch_optional(pool)
    .await
}

/// List all versions for a given signal.
pub async fn list_signal_versions(
    pool: &PgPool,
    workspace_id: Uuid,
    signal_id: Uuid,
) -> Result<Vec<SignalVersionRow>, sqlx::Error> {
    sqlx::query_as::<_, SignalVersionRow>(
        r"
        SELECT *
        FROM signal_versions
        WHERE workspace_id = $1 AND signal_id = $2
        ORDER BY version DESC
        ",
    )
    .bind(workspace_id)
    .bind(signal_id)
    .fetch_all(pool)
    .await
}

/// Get a specific version of a signal.
pub async fn get_signal_version(
    pool: &PgPool,
    workspace_id: Uuid,
    signal_id: Uuid,
    version: i32,
) -> Result<Option<SignalVersionRow>, sqlx::Error> {
    sqlx::query_as::<_, SignalVersionRow>(
        r"
        SELECT *
        FROM signal_versions
        WHERE workspace_id = $1 AND signal_id = $2 AND version = $3
        ",
    )
    .bind(workspace_id)
    .bind(signal_id)
    .bind(version)
    .fetch_optional(pool)
    .await
}
