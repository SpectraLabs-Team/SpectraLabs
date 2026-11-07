//! Alpha Workbench signal test database operations (M12).

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Stored signal test record row.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct SignalTestRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub dataset_version_id: Option<Uuid>,
    pub signal_name: String,
    pub universe: serde_json::Value,
    pub horizons: serde_json::Value,
    pub primary_horizon: i32,
    pub period_start: Option<chrono::DateTime<chrono::Utc>>,
    pub period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub mean_ic: f64,
    pub ic_stdev: f64,
    pub ic_t_stat: f64,
    pub half_life_days: f64,
    pub top_quantile_turnover: f64,
    pub top_bottom_spread: f64,
    pub threshold: Option<serde_json::Value>,
    pub passed_gate: bool,
    pub tests_run_count: i32,
    pub report_json: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Parameters for creating a signal test record.
#[derive(Debug, Clone)]
pub struct NewSignalTest {
    pub workspace_id: Uuid,
    pub dataset_version_id: Option<Uuid>,
    pub signal_name: String,
    pub universe: serde_json::Value,
    pub horizons: serde_json::Value,
    pub primary_horizon: i32,
    pub period_start: Option<chrono::DateTime<chrono::Utc>>,
    pub period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub mean_ic: f64,
    pub ic_stdev: f64,
    pub ic_t_stat: f64,
    pub half_life_days: f64,
    pub top_quantile_turnover: f64,
    pub top_bottom_spread: f64,
    pub threshold: Option<serde_json::Value>,
    pub passed_gate: bool,
    pub tests_run_count: i32,
    pub report_json: serde_json::Value,
    pub created_by: Option<Uuid>,
}

/// Insert a new signal test run.
///
/// # Errors
/// Returns an error if the database query fails.
pub async fn insert_signal_test(pool: &PgPool, t: &NewSignalTest) -> Result<SignalTestRecord, sqlx::Error> {
    sqlx::query_as::<_, SignalTestRecord>(
        r"
        INSERT INTO signal_tests (
            workspace_id, dataset_version_id, signal_name, universe, horizons,
            primary_horizon, period_start, period_end, mean_ic, ic_stdev,
            ic_t_stat, half_life_days, top_quantile_turnover, top_bottom_spread,
            threshold, passed_gate, tests_run_count, report_json, created_by
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19
        )
        RETURNING *
        ",
    )
    .bind(t.workspace_id)
    .bind(t.dataset_version_id)
    .bind(&t.signal_name)
    .bind(&t.universe)
    .bind(&t.horizons)
    .bind(t.primary_horizon)
    .bind(t.period_start)
    .bind(t.period_end)
    .bind(t.mean_ic)
    .bind(t.ic_stdev)
    .bind(t.ic_t_stat)
    .bind(t.half_life_days)
    .bind(t.top_quantile_turnover)
    .bind(t.top_bottom_spread)
    .bind(&t.threshold)
    .bind(t.passed_gate)
    .bind(t.tests_run_count)
    .bind(&t.report_json)
    .bind(t.created_by)
    .fetch_one(pool)
    .await
}

/// Fetch a single signal test by id.
///
/// # Errors
/// Returns an error if querying the database fails.
pub async fn get_signal_test(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<SignalTestRecord>, sqlx::Error> {
    sqlx::query_as::<_, SignalTestRecord>(
        r"
        SELECT * FROM signal_tests
        WHERE workspace_id = $1 AND id = $2
        ",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// List signal tests for a workspace.
///
/// # Errors
/// Returns an error if querying the database fails.
pub async fn list_signal_tests(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<SignalTestRecord>, sqlx::Error> {
    sqlx::query_as::<_, SignalTestRecord>(
        r"
        SELECT * FROM signal_tests
        WHERE workspace_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        ",
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Count signal tests run on a dataset version (for overfitting transparency).
///
/// # Errors
/// Returns an error if querying the database fails.
pub async fn count_signal_tests_for_dataset(
    pool: &PgPool,
    workspace_id: Uuid,
    dataset_version_id: Option<Uuid>,
) -> Result<i64, sqlx::Error> {
    let count: i64 = if let Some(dv_id) = dataset_version_id {
        sqlx::query_scalar(
            r"
            SELECT count(*) FROM signal_tests
            WHERE workspace_id = $1 AND dataset_version_id = $2
            ",
        )
        .bind(workspace_id)
        .bind(dv_id)
        .fetch_one(pool)
        .await?
    } else {
        sqlx::query_scalar(
            r"
            SELECT count(*) FROM signal_tests
            WHERE workspace_id = $1
            ",
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?
    };

    Ok(count)
}

/// Fetch latest passed signal test for a signal name (for consumer gates like M42 sentiment score).
///
/// # Errors
/// Returns an error if querying the database fails.
pub async fn get_latest_passed_signal_test(
    pool: &PgPool,
    workspace_id: Uuid,
    signal_name: &str,
) -> Result<Option<SignalTestRecord>, sqlx::Error> {
    sqlx::query_as::<_, SignalTestRecord>(
        r"
        SELECT * FROM signal_tests
        WHERE workspace_id = $1 AND signal_name = $2 AND passed_gate = true
        ORDER BY created_at DESC
        LIMIT 1
        ",
    )
    .bind(workspace_id)
    .bind(signal_name)
    .fetch_optional(pool)
    .await
}
