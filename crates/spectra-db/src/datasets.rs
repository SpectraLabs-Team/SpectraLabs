//! Dataset versions and backtest runs (M3 task 5, with the minimal M1 piece).
//!
//! A dataset version names an entry in the content-addressed dataset store
//! (`spectra-data`'s `DataStore`, at the worker's `SPECTRA_DATA_DIR`) by its
//! checksum. Versions belong to a workspace: a job can only run on a version
//! of its own workspace, which [`resolve_version`] enforces. Every statement
//! lives in `queries/`, exercised by `tests/backtest_checks.sql`.

use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// A dataset version as a job needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetVersion {
    /// Version id.
    pub id: Uuid,
    /// Dataset-store checksum (64 hex characters).
    pub checksum: String,
    /// Number of bars.
    pub row_count: i64,
    /// Name of the dataset it belongs to.
    pub dataset_name: String,
    /// Version number within the dataset (1, 2, ...).
    pub version: i32,
    /// SHA-256 of its bars.qfb in the artifact store, or `None` when it is
    /// only in the local dataset store (registered from the command line).
    pub blob_sha256: Option<String>,
}

/// The dataset version `id`, if it belongs to `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn resolve_version(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<DatasetVersion>, Error> {
    let row: Option<(Uuid, String, i64, String, i32, Option<String>)> =
        sqlx::query_as(include_str!("../queries/dataset_version_resolve.sql"))
            .bind(workspace_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id, checksum, row_count, dataset_name, version, blob_sha256)| DatasetVersion {
        id,
        checksum,
        row_count,
        dataset_name,
        version,
        blob_sha256,
    }))
}

/// A dataset version as the Studio lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetVersionInfo {
    /// Version id (what a backtest's `dataset_version_id` names).
    pub id: Uuid,
    /// Name of the dataset it belongs to.
    pub dataset_name: String,
    /// Version number within the dataset (1, 2, ...).
    pub version: i32,
    /// Dataset-store checksum (64 hex characters).
    pub checksum: String,
    /// Number of bars.
    pub row_count: i64,
    /// Stored size in bytes.
    pub byte_size: i64,
    /// Registered at.
    pub created_at: DateTime<Utc>,
}

type VersionRow = (Uuid, String, i32, String, i64, i64, DateTime<Utc>);

/// A workspace's dataset versions, newest first (at most `limit`, 1 to 500).
///
/// # Errors
/// Any database error.
pub async fn list_versions(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: u32,
) -> Result<Vec<DatasetVersionInfo>, Error> {
    let rows: Vec<VersionRow> = sqlx::query_as(include_str!("../queries/dataset_version_list.sql"))
        .bind(workspace_id)
        .bind(i32::try_from(limit.clamp(1, 500)).unwrap_or(500))
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, dataset_name, version, checksum, row_count, byte_size, created_at)| DatasetVersionInfo {
            id,
            dataset_name,
            version,
            checksum,
            row_count,
            byte_size,
            created_at,
        })
        .collect())
}

/// A dataset-store entry to register.
#[derive(Debug, Clone)]
pub struct NewDatasetVersion<'a> {
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// Dataset name (created if new; surrounding spaces are trimmed).
    pub dataset_name: &'a str,
    /// Dataset-store checksum.
    pub checksum: &'a str,
    /// Number of bars.
    pub row_count: i64,
    /// Size of the stored data, in bytes.
    pub byte_size: i64,
    /// The data-quality report.
    pub quality_report: JsonValue,
    /// Who registered it.
    pub created_by: Option<Uuid>,
    /// SHA-256 of the bars.qfb file in the artifact store; `None` when it is
    /// only in a local dataset store (the command line).
    pub blob_sha256: Option<&'a str>,
    /// How it was made from an upload (`upload_id`, `file_sha256`,
    /// `filename`, `settings`); `None` from the command line.
    pub source: Option<JsonValue>,
}

/// A registered version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredVersion {
    /// Version id.
    pub id: Uuid,
    /// Version number within the dataset.
    pub version: i32,
    /// True if this checksum was already a version of the dataset.
    pub existed: bool,
}

/// Registers a dataset-store entry as the next version of a dataset, or
/// returns the existing version with the same checksum.
///
/// # Errors
/// Any database error (including a concurrent registration of the same
/// dataset, which can be retried).
pub async fn register_version(pool: &PgPool, v: &NewDatasetVersion<'_>) -> Result<RegisteredVersion, Error> {
    let (id, version, existed): (Uuid, i32, bool) =
        sqlx::query_as(include_str!("../queries/dataset_version_register.sql"))
            .bind(v.workspace_id)
            .bind(v.dataset_name)
            .bind(v.checksum)
            .bind(v.row_count)
            .bind(v.byte_size)
            .bind(&v.quality_report)
            .bind(v.created_by)
            .bind(v.blob_sha256)
            .bind(&v.source)
            .fetch_one(pool)
            .await?;
    Ok(RegisteredVersion { id, version, existed })
}

/// A workspace by id or slug.
///
/// # Errors
/// Any database error.
pub async fn resolve_workspace(pool: &PgPool, id_or_slug: &str) -> Result<Option<Uuid>, Error> {
    sqlx::query_scalar(include_str!("../queries/workspace_resolve.sql"))
        .bind(id_or_slug.trim())
        .fetch_optional(pool)
        .await
}

/// A finished backtest run.
#[derive(Debug, Clone)]
pub struct NewBacktestRun {
    /// UUIDv7.
    pub id: Uuid,
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// The job that produced it.
    pub job_id: Uuid,
    /// What was run: strategy, parameters, configuration and dataset.
    pub params: JsonValue,
    /// The summary document.
    pub metrics: JsonValue,
    /// The configuration's seed, when it fits a `bigint`.
    pub seed: Option<i64>,
    /// Engine version.
    pub engine_version: String,
    /// Who queued the job.
    pub created_by: Option<Uuid>,
    /// When the job's attempt started.
    pub started_at: DateTime<Utc>,
    /// The dataset version it ran on.
    pub dataset_version_id: Uuid,
}

/// Records a backtest run.
///
/// # Errors
/// Any database error.
pub async fn insert_backtest_run(pool: &PgPool, r: &NewBacktestRun) -> Result<Uuid, Error> {
    sqlx::query_scalar(include_str!("../queries/run_insert_backtest.sql"))
        .bind(r.id)
        .bind(r.workspace_id)
        .bind(r.job_id)
        .bind(&r.params)
        .bind(&r.metrics)
        .bind(r.seed)
        .bind(&r.engine_version)
        .bind(r.created_by)
        .bind(r.started_at)
        .bind(r.dataset_version_id)
        .fetch_one(pool)
        .await
}
