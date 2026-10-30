//! Versioned reports (M16): every server-rendered report, stored when it is created and tied to
//! the run it came from.
//!
//! The versions of a report (one report type for one run) share a `report_id` and are numbered
//! 1, 2, ... A new export always adds a version; versions are immutable (a trigger refuses
//! updates). Content is stored inline with its SHA-256, which the table re-checks.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::Error;

/// What a report describes.
pub mod report_type {
    /// M4 optimization and validation report of a sweep.
    pub const VALIDATION: &str = "validation";
    /// Backtest tear sheet.
    pub const TEAR_SHEET: &str = "tear_sheet";
    /// M10 backtest-to-paper gap report.
    pub const GAP: &str = "gap";
}

/// What a report's `run_id` points at.
pub mod run_kind {
    /// An optimization sweep (`optimization_sweeps.id`).
    pub const SWEEP: &str = "sweep";
    /// A backtest run (`runs.id`).
    pub const BACKTEST: &str = "backtest";
    /// A backtest-to-paper gap report (`backtest_paper_gap_reports.id`).
    pub const PAPER_GAP: &str = "paper_gap";
}

/// One stored version of a report, without its content.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ReportVersion {
    /// Id of this version's row.
    pub id: Uuid,
    /// Shared by every version of the report.
    pub report_id: Uuid,
    pub report_type: String,
    pub run_kind: String,
    pub run_id: Uuid,
    /// 1, 2, ... in creation order.
    pub version: i32,
    /// `pdf` or `csv`.
    pub format: String,
    pub content_type: String,
    /// Lower-case hex SHA-256 of the content.
    pub sha256: String,
    pub size_bytes: i64,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A stored version with its bytes.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ReportContent {
    #[sqlx(flatten)]
    pub meta: ReportVersion,
    pub content: Vec<u8>,
}

/// A report to store.
#[derive(Debug, Clone)]
pub struct NewReport<'a> {
    pub report_type: &'a str,
    pub run_kind: &'a str,
    pub run_id: Uuid,
    /// `pdf` or `csv`.
    pub format: &'a str,
    pub content_type: &'a str,
    pub content: &'a [u8],
    pub created_by: Option<Uuid>,
}

/// Stores `report` as the next version of its report (version 1 for a run's first report of that
/// type). Never overwrites a version.
///
/// # Errors
/// Any database error, including a check violation for an unknown type, kind or format, or
/// content over 10 MiB.
pub async fn create_version(
    pool: &PgPool,
    workspace_id: Uuid,
    report: &NewReport<'_>,
) -> Result<ReportVersion, Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(include_str!("../queries/report_lock.sql"))
        .bind(workspace_id)
        .bind(report.report_type)
        .bind(report.run_kind)
        .bind(report.run_id)
        .execute(&mut *tx)
        .await?;
    let created: ReportVersion = sqlx::query_as(include_str!("../queries/report_version_insert.sql"))
        .bind(workspace_id)
        .bind(report.report_type)
        .bind(report.run_kind)
        .bind(report.run_id)
        .bind(report.format)
        .bind(report.content_type)
        .bind(report.content)
        .bind(report.created_by)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(created)
}

/// One version of a report, with its content, if it exists in the workspace.
///
/// # Errors
/// Any database error.
pub async fn get_version(
    pool: &PgPool,
    workspace_id: Uuid,
    report_id: Uuid,
    version: i32,
) -> Result<Option<ReportContent>, Error> {
    sqlx::query_as(include_str!("../queries/report_version_get.sql"))
        .bind(workspace_id)
        .bind(report_id)
        .bind(version)
        .fetch_optional(pool)
        .await
}

/// Every version of a report, newest first (empty if the report is not in the workspace).
///
/// # Errors
/// Any database error.
pub async fn versions_of(
    pool: &PgPool,
    workspace_id: Uuid,
    report_id: Uuid,
) -> Result<Vec<ReportVersion>, Error> {
    sqlx::query_as(include_str!("../queries/report_versions_of.sql"))
        .bind(workspace_id)
        .bind(report_id)
        .fetch_all(pool)
        .await
}

/// Every report version for a run, grouped by type and newest first.
///
/// # Errors
/// Any database error.
pub async fn for_run(
    pool: &PgPool,
    workspace_id: Uuid,
    run_kind: &str,
    run_id: Uuid,
) -> Result<Vec<ReportVersion>, Error> {
    sqlx::query_as(include_str!("../queries/reports_for_run.sql"))
        .bind(workspace_id)
        .bind(run_kind)
        .bind(run_id)
        .fetch_all(pool)
        .await
}
