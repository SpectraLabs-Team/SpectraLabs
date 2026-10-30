//! Versioned reports (M16): list the reports of a run and download any stored version.
//!
//! Reports are stored by the endpoints that render them (the M4 sweep export today); every export
//! adds a version and never overwrites one. These handlers only read.

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_db::reports::{self, ReportVersion};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, AppState, Identity};

const NOT_FOUND: &str = "Report not found.";
const RUN_KINDS: [&str; 3] =
    [reports::run_kind::SWEEP, reports::run_kind::BACKTEST, reports::run_kind::PAPER_GAP];

/// One stored version of a report (without its content).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReportVersionView {
    /// Shared by every version of the report.
    pub report_id: Uuid,
    /// `validation`, `tear_sheet` or `gap`.
    pub report_type: String,
    /// What `run_id` points at: `sweep`, `backtest` or `paper_gap`.
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
    /// Where to download this version.
    pub download_url: String,
}

impl From<ReportVersion> for ReportVersionView {
    fn from(v: ReportVersion) -> Self {
        Self {
            download_url: format!("/api/v1/reports/{}/versions/{}", v.report_id, v.version),
            report_id: v.report_id,
            report_type: v.report_type,
            run_kind: v.run_kind,
            run_id: v.run_id,
            version: v.version,
            format: v.format,
            content_type: v.content_type,
            sha256: v.sha256,
            size_bytes: v.size_bytes,
            created_by: v.created_by,
            created_at: v.created_at,
        }
    }
}

/// Which run's reports to list.
#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct ListReportsQuery {
    /// `sweep`, `backtest` or `paper_gap`.
    pub run_kind: String,
    /// The run's id.
    pub run_id: Uuid,
}

/// The reports of a run: every version, grouped by type and newest first.
#[utoipa::path(get, path = "/api/v1/reports", tag = "reports", params(ListReportsQuery),
    responses(
        (status = 200, description = "Report versions for the run", body = Vec<ReportVersionView>),
        (status = 422, description = "Missing or unknown run_kind, or an invalid run_id", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn list_reports(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<ListReportsQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return error_response(422, "Give run_kind (sweep, backtest or paper_gap) and run_id.");
    };
    if !RUN_KINDS.contains(&q.run_kind.as_str()) {
        return error_response(422, "run_kind must be sweep, backtest or paper_gap.");
    }
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match reports::for_run(&pool, ws, &q.run_kind, q.run_id).await {
        Ok(list) => to_json(200, &list.into_iter().map(ReportVersionView::from).collect::<Vec<_>>()),
        Err(e) => db_error(&e),
    }
}

/// Every version of one report, newest first.
#[utoipa::path(get, path = "/api/v1/reports/{report_id}/versions", tag = "reports",
    params(("report_id" = Uuid, Path, description = "Report id")),
    responses(
        (status = 200, description = "Versions, newest first", body = Vec<ReportVersionView>),
        (status = 404, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn list_versions(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    report_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(report_id)) = report_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match reports::versions_of(&pool, ws, report_id).await {
        Ok(list) if list.is_empty() => error_response(404, NOT_FOUND),
        Ok(list) => to_json(200, &list.into_iter().map(ReportVersionView::from).collect::<Vec<_>>()),
        Err(e) => db_error(&e),
    }
}

/// Download one version of a report, exactly as it was stored.
#[utoipa::path(get, path = "/api/v1/reports/{report_id}/versions/{version}", tag = "reports",
    params(
        ("report_id" = Uuid, Path, description = "Report id"),
        ("version" = i32, Path, description = "Version number (1, 2, ...)"),
    ),
    responses(
        (status = 200, description = "The stored PDF or CSV; `x-report-sha256` carries its hash"),
        (status = 404, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn download_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<(Uuid, i32)>, PathRejection>,
) -> Response {
    let Ok(Path((report_id, version))) = path else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match reports::get_version(&pool, ws, report_id, version).await {
        Ok(Some(r)) => {
            let filename = format!(
                "{}-{}-{}-v{}.{}",
                r.meta.report_type, r.meta.run_kind, r.meta.run_id, r.meta.version, r.meta.format
            );
            report_response(&r.meta, &filename, r.content)
        }
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

/// A report download: its bytes, type and filename, with the stored id, version and hash as
/// headers.
pub(crate) fn report_response(meta: &ReportVersion, filename: &str, body: Vec<u8>) -> Response {
    Response::builder()
        .status(200)
        .header("content-type", &meta.content_type)
        .header("content-disposition", format!("attachment; filename=\"{filename}\""))
        .header("x-report-id", meta.report_id.to_string())
        .header("x-report-version", meta.version.to_string())
        .header("x-report-sha256", &meta.sha256)
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| error_response(500, "Could not build the response."))
}
