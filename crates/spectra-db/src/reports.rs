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
    /// M12 alpha workbench signal test report.
    pub const SIGNAL_TEST: &str = "signal_test";
    /// M15 portfolio backtest report.
    pub const PORTFOLIO: &str = "portfolio";
}

/// What a report's `run_id` points at.
pub mod run_kind {
    /// An optimization sweep (`optimization_sweeps.id`).
    pub const SWEEP: &str = "sweep";
    /// A backtest run (`runs.id`).
    pub const BACKTEST: &str = "backtest";
    /// A backtest-to-paper gap report (`backtest_paper_gap_reports.id`).
    pub const PAPER_GAP: &str = "paper_gap";
    /// An M12 signal test (`signal_tests.id`).
    pub const SIGNAL_TEST: &str = "signal_test";
    /// An M15 portfolio backtest run (`portfolio_runs.id`).
    pub const PORTFOLIO_BACKTEST: &str = "portfolio_backtest";
}

/// The largest report content, in bytes, that [`create_version`] stores: 5 MiB. Every stored
/// report (sweep validation, tear sheet and gap exports, PDF or CSV) is checked against it before
/// anything is written; content of exactly this size is stored. The table's own check (10 MiB) is
/// a backstop behind it.
pub const REPORT_MAX_BYTES: usize = 5 * 1024 * 1024;

/// Why a report version could not be stored.
#[derive(Debug)]
pub enum CreateError {
    /// The content is larger than [`REPORT_MAX_BYTES`].
    TooLarge {
        /// Bytes in the refused content.
        size: usize,
    },
    /// The database failed or refused the row.
    Db(Error),
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::TooLarge { size } => {
                write!(f, "report content is {size} bytes, over the {REPORT_MAX_BYTES}-byte limit")
            }
            CreateError::Db(e) => write!(f, "report database: {e}"),
        }
    }
}

impl std::error::Error for CreateError {}

impl From<Error> for CreateError {
    fn from(e: Error) -> Self {
        CreateError::Db(e)
    }
}

/// Refuses content larger than [`REPORT_MAX_BYTES`].
///
/// # Errors
/// [`CreateError::TooLarge`] when `content` is over the limit.
pub fn check_size(content: &[u8]) -> Result<(), CreateError> {
    if content.len() > REPORT_MAX_BYTES {
        return Err(CreateError::TooLarge { size: content.len() });
    }
    Ok(())
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
/// [`CreateError::TooLarge`] for content over [`REPORT_MAX_BYTES`], refused before the database
/// is touched. Otherwise [`CreateError::Db`] for any database error, including a check violation
/// for an unknown type, kind or format.
pub async fn create_version(
    pool: &PgPool,
    workspace_id: Uuid,
    report: &NewReport<'_>,
) -> Result<ReportVersion, CreateError> {
    check_size(report.content)?;
    let mut tx = pool.begin().await?;
    sqlx::query(include_str!("../queries/report_lock.sql"))
        .bind(workspace_id)
        .bind(report.report_type)
        .bind(report.run_kind)
        .bind(report.run_id)
        .execute(&mut *tx)
        .await?;
    // Tear sheets and portfolio reports: an export identical to the newest version reuses it.
    // Sweep exports keep every version (see the M16 decisions).
    if report.report_type == report_type::TEAR_SHEET || report.report_type == report_type::PORTFOLIO {
        let latest: Option<ReportVersion> = sqlx::query_as(include_str!("../queries/report_latest.sql"))
            .bind(workspace_id)
            .bind(report.report_type)
            .bind(report.run_kind)
            .bind(report.run_id)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(latest) = latest {
            let candidate: String = sqlx::query_scalar("SELECT encode(sha256($1), 'hex')")
                .bind(report.content)
                .fetch_one(&mut *tx)
                .await?;
            if latest.sha256 == candidate {
                tx.commit().await?;
                return Ok(latest);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_up_to_the_limit_passes_and_one_byte_more_is_refused() {
        assert_eq!(REPORT_MAX_BYTES, 5_242_880);
        assert!(check_size(b"").is_ok());
        assert!(check_size(&vec![0u8; REPORT_MAX_BYTES]).is_ok(), "exactly 5 MiB is stored");
        match check_size(&vec![0u8; REPORT_MAX_BYTES + 1]) {
            Err(CreateError::TooLarge { size }) => assert_eq!(size, REPORT_MAX_BYTES + 1),
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }
}
