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

/// What [`resolve_version`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The version, with the checksum that unlocks its bars.
    Found(DatasetVersion),
    /// The version is a sealed holdout. The database did not hand out its checksum and logged the
    /// attempt; the only way to its bars is a holdout evaluation
    /// ([`crate::optimization::grant_holdout_evaluation`]).
    Sealed {
        /// Name of the holdout that seals it.
        holdout: String,
    },
    /// No such version in this workspace.
    Missing,
}

impl Resolved {
    /// The version, if it was found and is not sealed.
    #[must_use]
    pub fn found(self) -> Option<DatasetVersion> {
        match self {
            Self::Found(v) => Some(v),
            Self::Sealed { .. } | Self::Missing => None,
        }
    }
}

/// Who is asking [`resolve_version`] for a version's data, recorded when it is a sealed holdout.
#[derive(Debug, Clone, Copy)]
pub struct Access<'a> {
    /// What the data is for: `"job"`, `"sweep"`, `"bars"`, `"worker"`, ...
    pub purpose: &'a str,
    /// The user, or `None` for the engine itself.
    pub user: Option<Uuid>,
}

impl<'a> Access<'a> {
    /// An engine-side access (no user).
    #[must_use]
    pub const fn engine(purpose: &'a str) -> Self {
        Self { purpose, user: None }
    }
}

/// The dataset version `id` of `workspace_id`, as a job needs it.
///
/// The database never returns the checksum of a sealed holdout: such a version comes back as
/// [`Resolved::Sealed`], and the attempt is written to `holdout_access_log`.
///
/// # Errors
/// Any database error.
pub async fn resolve_version(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    access: Access<'_>,
) -> Result<Resolved, Error> {
    type Row = (Uuid, Option<String>, i64, String, i32, Option<String>, Option<String>);
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/dataset_version_resolve.sql"))
        .bind(workspace_id)
        .bind(id)
        .bind(access.purpose)
        .bind(access.user)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some((_, _, _, _, _, _, Some(holdout))) => Resolved::Sealed { holdout },
        Some((id, Some(checksum), row_count, dataset_name, version, blob_sha256, None)) => {
            Resolved::Found(DatasetVersion { id, checksum, row_count, dataset_name, version, blob_sha256 })
        }
        // No row, or (impossible: the column is NOT NULL) an unsealed version without a checksum.
        None | Some((_, None, ..)) => Resolved::Missing,
    })
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
    /// The saved strategy version it ran, if it was started from one (M2a).
    pub strategy_version_id: Option<Uuid>,
    /// The strategy's name (`""` when not recorded).
    pub strategy: String,
    /// The strategy's source hash (`""` when not recorded).
    pub strategy_sha256: String,
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
        .bind(r.strategy_version_id)
        .bind(&r.strategy)
        .bind(&r.strategy_sha256)
        .fetch_one(pool)
        .await
}

// ---------------------------------------------------------------------------
// The Studio's dataset browser (M1 task 6)
// ---------------------------------------------------------------------------

/// A dataset with its latest version, as the dataset list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct DatasetSummary {
    /// Dataset id.
    pub id: Uuid,
    /// Name (unique in the workspace).
    pub name: String,
    /// Free text.
    pub description: String,
    /// When the dataset was created.
    pub created_at: DateTime<Utc>,
    /// Number of versions.
    pub version_count: i64,
    /// The latest version, if any.
    pub latest: Option<LatestVersion>,
}

/// The newest version of a dataset, in a [`DatasetSummary`].
#[derive(Debug, Clone, PartialEq)]
pub struct LatestVersion {
    /// Version id.
    pub id: Uuid,
    /// Version number.
    pub version: i32,
    /// Bars.
    pub row_count: i64,
    /// Bytes stored.
    pub byte_size: i64,
    /// When it was registered (the dataset's "last updated").
    pub created_at: DateTime<Utc>,
    /// Symbol names, from its quality report.
    pub symbols: Vec<String>,
    /// First and last bar, nanoseconds since the epoch (from its quality report).
    pub first_ts_ns: Option<i64>,
    /// See `first_ts_ns`.
    pub last_ts_ns: Option<i64>,
}

fn strings(v: &JsonValue) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The workspace's datasets, most recently updated first.
///
/// # Errors
/// Any database error.
pub async fn list_datasets(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: i32,
) -> Result<Vec<DatasetSummary>, Error> {
    type Row = (
        Uuid,
        String,
        String,
        DateTime<Utc>,
        i64,
        Option<Uuid>,
        Option<i32>,
        Option<i64>,
        Option<i64>,
        Option<DateTime<Utc>>,
        JsonValue,
        Option<i64>,
        Option<i64>,
    );
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/dataset_list_grouped.sql"))
        .bind(workspace_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                description,
                created_at,
                version_count,
                lid,
                lver,
                rows,
                size,
                updated,
                symbols,
                first,
                last,
            )| {
                let latest = match (lid, lver, rows, size, updated) {
                    (Some(id), Some(version), Some(row_count), Some(byte_size), Some(created_at)) => {
                        Some(LatestVersion {
                            id,
                            version,
                            row_count,
                            byte_size,
                            created_at,
                            symbols: strings(&symbols),
                            first_ts_ns: first,
                            last_ts_ns: last,
                        })
                    }
                    _ => None,
                };
                DatasetSummary { id, name, description, created_at, version_count, latest }
            },
        )
        .collect())
}

/// A dataset's own fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetRow {
    /// Dataset id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Free text.
    pub description: String,
    /// Who created it.
    pub created_by: Option<Uuid>,
    /// When.
    pub created_at: DateTime<Utc>,
}

type DatasetTuple = (Uuid, String, String, Option<Uuid>, DateTime<Utc>);

fn dataset_row((id, name, description, created_by, created_at): DatasetTuple) -> DatasetRow {
    DatasetRow { id, name, description, created_by, created_at }
}

/// The dataset `id` of `workspace_id`.
///
/// # Errors
/// Any database error.
pub async fn get_dataset(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<Option<DatasetRow>, Error> {
    let row: Option<DatasetTuple> = sqlx::query_as(include_str!("../queries/dataset_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(dataset_row))
}

/// Renames a dataset and/or sets its description (`None` keeps a field).
/// `Ok(None)` if it is not in the workspace.
///
/// # Errors
/// Any database error; a name already used in the workspace is a unique
/// violation (code 23505).
pub async fn update_dataset(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<Option<DatasetRow>, Error> {
    let row: Option<DatasetTuple> = sqlx::query_as(include_str!("../queries/dataset_update.sql"))
        .bind(workspace_id)
        .bind(id)
        .bind(name)
        .bind(description)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(dataset_row))
}

/// One version in a dataset's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionEntry {
    /// Version id.
    pub id: Uuid,
    /// Version number.
    pub version: i32,
    /// Dataset-store checksum (the dataset id).
    pub checksum: String,
    /// Bars.
    pub row_count: i64,
    /// Bytes stored.
    pub byte_size: i64,
    /// Who registered it.
    pub created_by: Option<Uuid>,
    /// When.
    pub created_at: DateTime<Utc>,
    /// Symbol names.
    pub symbols: Vec<String>,
    /// First bar, ns since the epoch.
    pub first_ts_ns: Option<i64>,
    /// Last bar, ns since the epoch.
    pub last_ts_ns: Option<i64>,
    /// Rows dropped at ingest.
    pub rows_rejected: i64,
    /// Warnings in its quality report.
    pub warning_count: i64,
    /// Uploaded file name (`None` from the command line).
    pub filename: Option<String>,
    /// Runs that used it.
    pub run_count: i64,
    /// Version it was derived from, if derived.
    pub derived_from_id: Option<Uuid>,
    /// Derivation kind (e.g. "adjusted"), if derived.
    pub derivation_kind: Option<String>,
}

/// Every version of a dataset, newest first.
///
/// # Errors
/// Any database error.
pub async fn dataset_versions(
    pool: &PgPool,
    workspace_id: Uuid,
    dataset_id: Uuid,
) -> Result<Vec<VersionEntry>, Error> {
    type Row = (
        Uuid,
        i32,
        String,
        i64,
        i64,
        Option<Uuid>,
        DateTime<Utc>,
        JsonValue,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<String>,
        i64,
        Option<Uuid>,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/dataset_versions_of.sql"))
        .bind(workspace_id)
        .bind(dataset_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                version,
                checksum,
                row_count,
                byte_size,
                created_by,
                created_at,
                symbols,
                first_ts_ns,
                last_ts_ns,
                rows_rejected,
                warning_count,
                filename,
                run_count,
                derived_from_id,
                derivation_kind,
            )| VersionEntry {
                id,
                version,
                checksum,
                row_count,
                byte_size,
                created_by,
                created_at,
                symbols: strings(&symbols),
                first_ts_ns,
                last_ts_ns,
                rows_rejected,
                warning_count,
                filename,
                run_count,
                derived_from_id,
                derivation_kind,
            },
        )
        .collect())
}

/// A dataset version in full.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionDetail {
    /// Version id.
    pub id: Uuid,
    /// Its dataset.
    pub dataset_id: Uuid,
    /// The dataset's name.
    pub dataset_name: String,
    /// Version number.
    pub version: i32,
    /// Dataset-store checksum (the dataset id).
    pub checksum: String,
    /// Bars.
    pub row_count: i64,
    /// Bytes stored.
    pub byte_size: i64,
    /// The full quality report.
    pub quality_report: JsonValue,
    /// How it was made from an upload, if it was.
    pub source: Option<JsonValue>,
    /// Its file in the artifact store, if there.
    pub blob_sha256: Option<String>,
    /// Who registered it.
    pub created_by: Option<Uuid>,
    /// When.
    pub created_at: DateTime<Utc>,
    /// Runs that used it.
    pub run_count: i64,
    /// Version it was derived from, if derived.
    pub derived_from_id: Option<Uuid>,
    /// Derivation kind (e.g. "adjusted"), if derived.
    pub derivation_kind: Option<String>,
    /// Derivation parameters.
    pub derivation_params: Option<JsonValue>,
}

/// The dataset version `id` of `workspace_id`, in full.
///
/// # Errors
/// Any database error.
pub async fn get_version(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<VersionDetail>, Error> {
    type Row = (
        Uuid,
        Uuid,
        String,
        i32,
        String,
        i64,
        i64,
        JsonValue,
        Option<JsonValue>,
        Option<String>,
        Option<Uuid>,
        DateTime<Utc>,
        i64,
        Option<Uuid>,
        Option<String>,
        Option<JsonValue>,
    );
    let row: Option<Row> = sqlx::query_as(include_str!("../queries/dataset_version_get.sql"))
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(
        |(
            id,
            dataset_id,
            dataset_name,
            version,
            checksum,
            row_count,
            byte_size,
            quality_report,
            source,
            blob_sha256,
            created_by,
            created_at,
            run_count,
            derived_from_id,
            derivation_kind,
            derivation_params,
        )| VersionDetail {
            id,
            dataset_id,
            dataset_name,
            version,
            checksum,
            row_count,
            byte_size,
            quality_report,
            source,
            blob_sha256,
            created_by,
            created_at,
            run_count,
            derived_from_id,
            derivation_kind,
            derivation_params,
        },
    ))
}

/// A derived dataset-store entry to register.
#[derive(Debug, Clone)]
pub struct NewDerivedDatasetVersion<'a> {
    /// Owning workspace.
    pub workspace_id: Uuid,
    /// Dataset name.
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
    /// SHA-256 of the bars.qfb file in the artifact store.
    pub blob_sha256: Option<&'a str>,
    /// Source metadata.
    pub source: Option<JsonValue>,
    /// Base version id.
    pub derived_from_id: Option<Uuid>,
    /// Kind of derivation ("adjusted").
    pub derivation_kind: Option<&'a str>,
    /// Derivation parameters.
    pub derivation_params: Option<JsonValue>,
}

/// Registers a derived dataset version with lineage.
///
/// # Errors
/// Any database error.
pub async fn register_derived_version(
    pool: &PgPool,
    v: &NewDerivedDatasetVersion<'_>,
) -> Result<RegisteredVersion, Error> {
    let (id, version, existed): (Uuid, i32, bool) =
        sqlx::query_as(include_str!("../queries/dataset_version_derive.sql"))
            .bind(v.workspace_id)
            .bind(v.dataset_name)
            .bind(v.checksum)
            .bind(v.row_count)
            .bind(v.byte_size)
            .bind(&v.quality_report)
            .bind(v.created_by)
            .bind(v.blob_sha256)
            .bind(&v.source)
            .bind(v.derived_from_id)
            .bind(v.derivation_kind)
            .bind(v.derivation_params.as_ref().unwrap_or(&serde_json::json!({})))
            .fetch_one(pool)
            .await?;
    Ok(RegisteredVersion { id, version, existed })
}

/// A run that used a dataset version.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionRun {
    /// Run id.
    pub id: Uuid,
    /// Its job (a backtest's id in the API).
    pub job_id: Option<Uuid>,
    /// `backtest`, ...
    pub kind: String,
    /// Run status.
    pub status: String,
    /// What ran (strategy, parameters, configuration).
    pub params: JsonValue,
    /// When.
    pub created_at: DateTime<Utc>,
    /// When it finished.
    pub finished_at: Option<DateTime<Utc>>,
    /// Total return, for finished backtests.
    pub total_return: Option<f64>,
}

/// Runs that used the dataset version `id`, newest first.
///
/// # Errors
/// Any database error.
pub async fn version_runs(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    limit: i32,
) -> Result<Vec<VersionRun>, Error> {
    type Row =
        (Uuid, Option<Uuid>, String, String, JsonValue, DateTime<Utc>, Option<DateTime<Utc>>, Option<f64>);
    let rows: Vec<Row> = sqlx::query_as(include_str!("../queries/dataset_version_runs.sql"))
        .bind(workspace_id)
        .bind(id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, job_id, kind, status, params, created_at, finished_at, total_return)| VersionRun {
            id,
            job_id,
            kind,
            status,
            params,
            created_at,
            finished_at,
            total_return,
        })
        .collect())
}

/// What [`delete_version`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedVersion {
    /// The version exists in the workspace.
    pub found: bool,
    /// A run or a queued or running backtest uses it, so it was kept.
    pub in_use: bool,
    /// It was deleted.
    pub deleted: bool,
    /// Its dataset had no other version and was deleted too.
    pub dataset_deleted: bool,
    /// Its file in the artifact store, now referenced by nothing: delete it.
    pub orphaned_blob: Option<String>,
}

/// Deletes a dataset version unless something uses it (see the query).
///
/// # Errors
/// Any database error.
pub async fn delete_version(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> Result<DeletedVersion, Error> {
    let (found, in_use, deleted, dataset_deleted, orphaned_blob): (bool, bool, bool, bool, Option<String>) =
        sqlx::query_as(include_str!("../queries/dataset_version_delete.sql"))
            .bind(workspace_id)
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(DeletedVersion { found, in_use, deleted, dataset_deleted, orphaned_blob })
}
