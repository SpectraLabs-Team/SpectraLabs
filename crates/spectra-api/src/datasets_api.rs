//! Data Studio routes (M1): `POST /api/v1/datasets/preview` (task 3) and
//! `POST /api/v1/datasets/ingest` (task 5).
//!
//! A preview parses the first rows of a file with the caller's ingest settings
//! (task 2, `spectra_data::settings`) exactly as ingesting would, and returns
//! the parsed rows, every rejected row in the sample, and the sample's quality
//! report. It stores nothing and needs no database, so the Studio can call it
//! while the person adjusts the settings. Without settings it only returns
//! suggestions (delimiter, columns, timestamp format) for them to confirm.
//!
//! The sample is sent inline and kept under the proxy's body limit
//! ([`MAX_PREVIEW_SAMPLE`]); whole files go through uploads (task 4).

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::response::Response;
use axum::Extension;
use serde::{Deserialize, Serialize};
use spectra_core::Json;
use utoipa::ToSchema;

use crate::jobs_api::{context, db_error, to_json, JobView, MAX_ATTEMPTS};
use crate::{
    engine, error_response, error_with_code, invalid_fields, json_response, AppState, Identity, RequestId,
};

/// Largest preview sample, in bytes (the dashboard proxy accepts 2 MB bodies).
pub const MAX_PREVIEW_SAMPLE: usize = 1024 * 1024;
/// Parsed rows a preview returns. Every row of the sample is checked; only
/// these are sent back for display.
pub const PREVIEW_ROWS: usize = 200;

/// `POST /api/v1/datasets/preview` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    /// The start of the file as UTF-8 text, header included (at most 1 MB).
    pub sample: String,
    /// `true` when `sample` was cut from a longer file: its last line may be
    /// incomplete, so it is ignored.
    #[serde(default)]
    pub truncated: bool,
    /// Ingest settings. Omit them to get only `suggestions`.
    #[schema(value_type = Option<IngestSettings>)]
    pub settings: Option<serde_json::Value>,
}

/// Ingest settings (the JSON form of the engine's CSV options). Every field
/// below without "optional" must be given: nothing is guessed.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestSettings {
    /// One punctuation character (not `"`) or a tab.
    pub delimiter: String,
    /// `iso8601`, `epoch_s`, `epoch_ms`, `epoch_us` or `epoch_ns`.
    pub time_format: String,
    /// Minutes east of UTC for ISO timestamps written without a zone
    /// (-720 to 840). Optional if `timezone` is provided.
    pub utc_offset_minutes: Option<i32>,
    /// Optional DST-aware timezone name (e.g. `America/New_York`, `Europe/London`, `UTC`, `Asia/Tokyo`).
    pub timezone: Option<String>,
    /// Optional exchange calendar for session enforcement and calendar-aware checks (e.g. `XNYS`, `XCME`, `XLON`, `XTKS`, `XNSE`, `24/7`).
    pub exchange: Option<String>,
    /// Which header name holds each field.
    pub columns: IngestColumns,
    /// The symbol for every row when `columns.symbol` is null; not allowed otherwise.
    pub default_symbol: Option<String>,
    /// Optional, default `false`: bad rows abort the load. `true` skips and counts them.
    pub skip_bad_rows: Option<bool>,
    /// Optional quality-check thresholds.
    pub quality: Option<QualityThresholds>,
}

/// Header names (matched without regard to case).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestColumns {
    /// Timestamp column.
    pub timestamp: String,
    /// Open price column.
    pub open: String,
    /// High price column.
    pub high: String,
    /// Low price column.
    pub low: String,
    /// Close price column.
    pub close: String,
    /// Volume column, or null if the file has none (volume is then zero).
    pub volume: Option<String>,
    /// Symbol column, or null for a single-symbol file (then `default_symbol`).
    pub symbol: Option<String>,
}

/// Quality-check thresholds.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityThresholds {
    /// A gap is an interval longer than this many median intervals (default 5).
    pub gap_factor: Option<f64>,
    /// Robust z-score of a close-to-close return that counts as an outlier (default 10).
    pub outlier_z: Option<f64>,
    /// Frozen bars in a row that count as a stale run (default 5).
    pub stale_run: Option<u64>,
}

/// `POST /api/v1/datasets/preview` response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewResponse {
    /// Proposals from the sample, for the person to confirm. Never applied.
    pub suggestions: Suggestions,
    /// The sample parsed with `settings`; null when no settings were sent.
    pub preview: Option<Preview>,
}

/// Suggested settings; null where there is no confident guess.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Suggestions {
    /// Delimiter that splits every sampled row into the same number of fields.
    pub delimiter: Option<String>,
    /// The header split with that delimiter, as written.
    pub header: Vec<String>,
    /// Header names matched by common aliases.
    pub columns: SuggestedColumns,
    /// Likely `time_format`.
    pub time_format: Option<String>,
    /// For ISO timestamps: whether they carry a zone (then the UTC offset does not apply to them).
    pub timestamps_have_zone: Option<bool>,
    /// Suggested exchange calendar (e.g. `XNYS`).
    pub exchange: Option<String>,
    /// Suggested timezone identifier (e.g. `America/New_York`).
    pub timezone: Option<String>,
}

/// Suggested header name per field.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SuggestedColumns {
    pub timestamp: Option<String>,
    pub open: Option<String>,
    pub high: Option<String>,
    pub low: Option<String>,
    pub close: Option<String>,
    pub volume: Option<String>,
    pub symbol: Option<String>,
}

/// The sample, parsed.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    /// The header, trimmed and lower-cased (how columns are matched).
    pub header: Vec<String>,
    /// The first 200 parsed rows, sorted by timestamp then symbol. Prices are exact decimal text.
    pub rows: Vec<PreviewRow>,
    /// Data rows in the sample that were not checked. Every row of the
    /// sample (at most 1 MB) is checked, so this is 0.
    pub rows_not_read: u64,
    /// Why ingesting would stop with these settings (the first bad row when
    /// bad rows abort, or "no valid data rows"); null if these rows would load.
    pub would_abort: Option<String>,
    /// The quality report of the parsed rows. Every bad row in the sample is
    /// counted here, whatever `skip_bad_rows` says.
    pub quality: QualityReport,
}

/// One parsed row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct PreviewRow {
    /// UTC, RFC 3339.
    pub timestamp: String,
    pub symbol: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

/// A dataset's quality report (also what a dataset version stores).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityReport {
    /// Data rows read (header excluded).
    pub rows_total: u64,
    /// Rows that became bars.
    pub rows_accepted: u64,
    /// Rows dropped (only when bad rows are skipped).
    pub rows_rejected: u64,
    /// Things worth a human look.
    pub warnings: Vec<String>,
    /// Symbol names.
    pub symbols: Vec<String>,
    /// First bar, nanoseconds since the epoch.
    pub first_ts_ns: i64,
    /// Last bar, nanoseconds since the epoch.
    pub last_ts_ns: i64,
    /// Fingerprint of the bars (recorded in backtest results).
    pub bars_sha256: String,
    /// First bar, RFC 3339.
    pub first_ts: String,
    /// Last bar, RFC 3339.
    pub last_ts: String,
    /// Rejected rows by category (`bad_number`, `bad_timestamp`, `duplicate`, ...).
    pub rejected_by_category: BTreeMap<String, u64>,
    /// The first rejected rows (at most 50).
    pub sample_issues: Vec<RowIssue>,
    /// Whether rows arrived sorted by timestamp and symbol.
    pub input_was_sorted: bool,
    /// Standing limitations of the processing.
    pub notes: Vec<String>,
    /// Per-symbol checks.
    pub symbol_stats: Vec<SymbolStats>,
    /// Thresholds the checks used.
    pub thresholds: QualityThresholds,
    /// Exchange calendar used, if any.
    pub exchange: Option<String>,
    /// Timezone used, if any.
    pub timezone: Option<String>,
}

/// A rejected row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RowIssue {
    /// 1-based line in the file.
    pub line: u64,
    /// Rejection category.
    pub category: String,
    /// What was wrong.
    pub message: String,
}

/// Quality checks for one symbol.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolStats {
    /// Symbol.
    pub symbol: String,
    /// Bars.
    pub bars: u64,
    /// First bar, RFC 3339.
    pub first_ts: String,
    /// Last bar, RFC 3339.
    pub last_ts: String,
    /// Median interval between bars, nanoseconds.
    pub median_interval_ns: i64,
    /// Intervals longer than `gap_factor` median intervals.
    pub gaps: u64,
    /// Longest gap, nanoseconds.
    pub largest_gap_ns: i64,
    /// Bars with zero volume.
    pub zero_volume_bars: u64,
    /// Returns beyond `outlier_z` robust sigma.
    pub outlier_returns: u64,
    /// Runs of `stale_run` or more frozen bars.
    pub stale_runs: u64,
    /// Bars recorded outside regular exchange sessions.
    pub out_of_session_bars: Option<u64>,
    /// Bars recorded on non-trading days/holidays.
    pub non_trading_day_bars: Option<u64>,
}

/// Validates a settings document, prefixing field paths with `prefix`
/// (`settings`): every problem at once.
///
/// # Errors
/// The 422 response listing each invalid field.
#[allow(clippy::result_large_err)] // the error is the finished response
pub(crate) fn parse_settings(
    doc: &serde_json::Value,
    prefix: &str,
) -> Result<spectra_data::CsvOptions, Response> {
    let doc = Json::parse(&doc.to_string()).map_err(|e| error_response(422, format!("{prefix}: {e}")))?;
    spectra_data::parse_settings(&doc).map_err(|errors| {
        let errors: Vec<(String, String)> = errors
            .into_iter()
            .map(|e| {
                (
                    if e.field.is_empty() { prefix.to_string() } else { format!("{prefix}.{}", e.field) },
                    e.message,
                )
            })
            .collect();
        invalid_fields(&errors)
    })
}

/// Preview a file with ingest settings, or get suggested settings.
#[utoipa::path(post, path = "/api/v1/datasets/preview", tag = "datasets", request_body = PreviewRequest,
    responses(
        (status = 200, description = "Suggestions, and the parsed sample when settings were sent", body = PreviewResponse),
        (status = 413, description = "The sample is over 1 MB", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid settings (`details.errors` lists every field), or the file does not match them (a missing column)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn preview(State(st): State<AppState>, body: Result<Bytes, BytesRejection>) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: PreviewRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid preview request: {e}")),
    };
    if req.sample.len() > MAX_PREVIEW_SAMPLE {
        return error_response(413, "sample: send at most the first 1 MB of the file.");
    }
    let opt = match req.settings.as_ref().filter(|s| !s.is_null()) {
        None => None,
        Some(doc) => match parse_settings(doc, "settings") {
            Ok(o) => Some(o),
            Err(r) => return r,
        },
    };
    let mut sample = req.sample.into_bytes();
    let is_parquet = sample.starts_with(b"PAR1");
    if req.truncated && !is_parquet {
        // Keep whole lines: the cut may have split the last one.
        let keep = sample.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        sample.truncate(keep);
    }
    crate::compute_with(&st, move || {
        let suggestions = if is_parquet {
            Json::object()
                .with("delimiter", Json::Null)
                .with("header", Json::array())
                .with("columns", Json::object())
                .with("time_format", Json::Null)
                .with("timestamps_have_zone", Json::Null)
        } else {
            spectra_data::suggest(&sample).to_json()
        };
        let preview = match &opt {
            None => Json::Null,
            // Check every row of the sample (it is at most 1 MB); send back the first PREVIEW_ROWS.
            Some(opt) => {
                if is_parquet {
                    match spectra_parquet::preview_parquet(&sample, opt, usize::MAX - 1) {
                        Ok(p) => spectra_data::preview_json(&p, opt, PREVIEW_ROWS),
                        Err(e) => return invalid_fields(&[("sample".into(), file_problem(&e))]),
                    }
                } else {
                    match spectra_data::preview_csv(&sample, opt, usize::MAX - 1) {
                        Ok(p) => spectra_data::preview_json(&p, opt, PREVIEW_ROWS),
                        Err(e) => return invalid_fields(&[("sample".into(), file_problem(&e))]),
                    }
                }
            }
        };
        let body = Json::object().with("suggestions", suggestions).with("preview", preview);
        json_response(200, body.dump())
    })
    .await
}

/// A file-level problem, worded for the person choosing the settings.
fn file_problem(e: &spectra_core::Error) -> String {
    let text = e.to_string();
    if text.starts_with("missing") {
        format!("{text} (check the column names and the delimiter against the header)")
    } else {
        text
    }
}

/// Longest dataset name.
pub const MAX_DATASET_NAME: usize = 200;

/// `POST /api/v1/datasets/ingest` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestRequest {
    /// From `POST /api/v1/uploads`.
    pub upload_id: uuid::Uuid,
    /// The dataset to add a version to (created if new). Different content
    /// becomes its next version; the same content changes nothing.
    pub dataset_name: String,
    /// How to read the file (as previewed).
    #[schema(value_type = IngestSettings)]
    pub settings: serde_json::Value,
}

/// A dataset name, trimmed: 1 to [`MAX_DATASET_NAME`] characters, no control characters.
fn check_dataset_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("required".into());
    }
    if name.chars().count() > MAX_DATASET_NAME || name.chars().any(char::is_control) {
        return Err(format!("must be 1 to {MAX_DATASET_NAME} characters, without control characters"));
    }
    Ok(name.to_string())
}

/// Turn an upload into a dataset version (queues an `ingest` job).
#[utoipa::path(post, path = "/api/v1/datasets/ingest", tag = "datasets", request_body = IngestRequest,
    responses(
        (status = 202, description = "Queued: follow it with GET /api/v1/jobs/{id}. A rejected file ends as a failed job whose result holds the quality report", body = JobView),
        (status = 422, description = "Every problem with the upload, name and settings (`details.errors`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn ingest(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Extension(request_id): Extension<RequestId>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: IngestRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid ingest request: {e}")),
    };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };

    // Every problem at once: the name, the settings and the upload.
    let mut errors: Vec<(String, String)> = Vec::new();
    let name =
        check_dataset_name(&req.dataset_name).map_err(|m| errors.push(("dataset_name".into(), m))).ok();
    let settings = match Json::parse(&req.settings.to_string()).map(|d| spectra_data::parse_settings(&d)) {
        Ok(Ok(opt)) => Some(opt),
        Ok(Err(list)) => {
            for e in list {
                let field =
                    if e.field.is_empty() { "settings".to_string() } else { format!("settings.{}", e.field) };
                errors.push((field, e.message));
            }
            None
        }
        Err(e) => {
            errors.push(("settings".into(), e.to_string()));
            None
        }
    };
    let upload = match spectra_db::uploads::get(&pool, ws, req.upload_id).await {
        Ok(Some(u)) => Some(u),
        Ok(None) => {
            errors.push((
                "upload_id".into(),
                "no such upload in this workspace (uploads expire after 24 hours)".into(),
            ));
            None
        }
        Err(e) => return db_error(&e),
    };
    let (Some(name), Some(settings), Some(upload)) = (name, settings, upload) else {
        return invalid_fields(&errors);
    };

    let payload = serde_json::json!({
        "upload_id": upload.id,
        "dataset_name": name,
        "file_sha256": upload.sha256,
        "filename": upload.filename,
        // Canonical form: every field explicit, so the job records exactly what was used.
        "settings": serde_json::from_str::<serde_json::Value>(&spectra_data::settings_json(&settings).dump())
            .unwrap_or(serde_json::Value::Null),
        "request_id": request_id.0,
    });
    let new = spectra_db::jobs::NewJob {
        workspace_id: ws,
        kind: engine::INGEST_JOB_KIND.to_string(),
        payload,
        priority: 0,
        max_attempts: MAX_ATTEMPTS,
        created_by: uuid::Uuid::parse_str(&id.user).ok(),
    };
    match spectra_db::jobs::enqueue(&pool, &new).await {
        Ok(job) => to_json(202, &JobView::from(job)),
        Err(e) => db_error(&e),
    }
}

// ---------------------------------------------------------------------------
// Browsing datasets (M1 task 6)
// ---------------------------------------------------------------------------

/// Nanoseconds since the epoch as RFC 3339 (UTC).
fn ts(ns: Option<i64>) -> Option<String> {
    ns.map(spectra_data::format_timestamp)
}

/// A dataset, with its latest version, for the dataset list.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatasetSummaryView {
    /// Dataset id.
    pub id: uuid::Uuid,
    /// Name (unique in the workspace).
    pub name: String,
    /// Free text.
    pub description: String,
    /// Versions.
    pub version_count: i64,
    /// Created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the latest version was added.
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The latest version's id.
    pub latest_version_id: Option<uuid::Uuid>,
    /// The latest version's number.
    pub latest_version: Option<i32>,
    /// Bars in the latest version.
    pub row_count: Option<i64>,
    /// Bytes stored for the latest version.
    pub byte_size: Option<i64>,
    /// Symbols in the latest version.
    pub symbols: Vec<String>,
    /// First bar of the latest version (RFC 3339).
    pub first_ts: Option<String>,
    /// Last bar of the latest version (RFC 3339).
    pub last_ts: Option<String>,
}

impl From<spectra_db::datasets::DatasetSummary> for DatasetSummaryView {
    fn from(d: spectra_db::datasets::DatasetSummary) -> Self {
        let l = d.latest.as_ref();
        Self {
            id: d.id,
            name: d.name,
            description: d.description,
            version_count: d.version_count,
            created_at: d.created_at,
            updated_at: l.map(|l| l.created_at),
            latest_version_id: l.map(|l| l.id),
            latest_version: l.map(|l| l.version),
            row_count: l.map(|l| l.row_count),
            byte_size: l.map(|l| l.byte_size),
            symbols: l.map(|l| l.symbols.clone()).unwrap_or_default(),
            first_ts: ts(l.and_then(|l| l.first_ts_ns)),
            last_ts: ts(l.and_then(|l| l.last_ts_ns)),
        }
    }
}

/// One version in a dataset's history.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VersionEntryView {
    /// Version id (a backtest's `dataset_version_id`).
    pub id: uuid::Uuid,
    /// Version number.
    pub version: i32,
    /// The dataset id (content hash of bars and symbols).
    pub checksum: String,
    /// Bars.
    pub row_count: i64,
    /// Bytes stored.
    pub byte_size: i64,
    /// Registered at.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Who registered it.
    pub created_by: Option<uuid::Uuid>,
    /// Symbols.
    pub symbols: Vec<String>,
    /// First bar (RFC 3339).
    pub first_ts: Option<String>,
    /// Last bar (RFC 3339).
    pub last_ts: Option<String>,
    /// Rows dropped at ingest.
    pub rows_rejected: i64,
    /// Warnings in its quality report.
    pub warning_count: i64,
    /// Uploaded file name; null when registered from the command line.
    pub filename: Option<String>,
    /// Runs that used it (a version with runs can't be deleted).
    pub run_count: i64,
    /// Base version id if derived.
    pub derived_from_id: Option<uuid::Uuid>,
    /// Derivation kind (e.g. "adjusted") if derived.
    pub derivation_kind: Option<String>,
}

/// A dataset and its versions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatasetDetailView {
    /// Dataset id.
    pub id: uuid::Uuid,
    /// Name.
    pub name: String,
    /// Free text.
    pub description: String,
    /// Who created it.
    pub created_by: Option<uuid::Uuid>,
    /// Created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Newest first.
    pub versions: Vec<VersionEntryView>,
}

/// A dataset version in full.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VersionDetailView {
    /// Version id.
    pub id: uuid::Uuid,
    /// Its dataset.
    pub dataset_id: uuid::Uuid,
    /// The dataset's name.
    pub dataset_name: String,
    /// Version number.
    pub version: i32,
    /// The dataset id (content hash of bars and symbols).
    pub checksum: String,
    /// Bars.
    pub row_count: i64,
    /// Bytes stored.
    pub byte_size: i64,
    /// Registered at.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Who registered it.
    pub created_by: Option<uuid::Uuid>,
    /// Runs that used it.
    pub run_count: i64,
    /// Whether its bars can be charted here (`/bars`).
    pub chartable: bool,
    /// The quality report. Versions ingested through the Studio have every
    /// field of `QualityReport`; command-line versions only some
    /// (`rows_total`, `rows_accepted`, `rows_rejected`, `warnings`, `symbols`, ...).
    #[schema(value_type = Object)]
    pub quality_report: serde_json::Value,
    /// How it was made from an upload (`upload_id`, `file_sha256`, `filename`,
    /// `settings`); null when registered from the command line.
    #[schema(value_type = Option<Object>)]
    pub source: Option<serde_json::Value>,
    /// Base version id if derived.
    pub derived_from_id: Option<uuid::Uuid>,
    /// Derivation kind (e.g. "adjusted") if derived.
    pub derivation_kind: Option<String>,
    /// Derivation parameters.
    #[schema(value_type = Option<Object>)]
    pub derivation_params: Option<serde_json::Value>,
}

/// A run that used a dataset version.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VersionRunView {
    /// Run id.
    pub id: uuid::Uuid,
    /// The job; for a backtest, its id in `/api/v1/backtests/{id}`.
    pub job_id: Option<uuid::Uuid>,
    /// `backtest`, ...
    pub kind: String,
    /// Run status.
    pub status: String,
    /// The strategy, or `multi-strategy`.
    pub strategy: Option<String>,
    /// Started.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Finished.
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Total return (a fraction) for finished backtests.
    pub total_return: Option<f64>,
}

/// Runs of a version, newest first (at most 100).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VersionRunList {
    /// The runs.
    pub items: Vec<VersionRunView>,
}

const NO_DATASET: &str = "No such dataset.";
const NO_VERSION: &str = "No such dataset version.";

/// A dataset and its version history.
#[utoipa::path(get, path = "/api/v1/datasets/{id}", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset id")),
    responses((status = 200, body = DatasetDetailView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_dataset(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(dataset_id)) = path else { return error_response(404, NO_DATASET) };
    let d = match spectra_db::datasets::get_dataset(&pool, ws, dataset_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return error_response(404, NO_DATASET),
        Err(e) => return db_error(&e),
    };
    let versions = match spectra_db::datasets::dataset_versions(&pool, ws, dataset_id).await {
        Ok(v) => v,
        Err(e) => return db_error(&e),
    };
    to_json(
        200,
        &DatasetDetailView {
            id: d.id,
            name: d.name,
            description: d.description,
            created_by: d.created_by,
            created_at: d.created_at,
            versions: versions
                .into_iter()
                .map(|v| VersionEntryView {
                    id: v.id,
                    version: v.version,
                    checksum: v.checksum,
                    row_count: v.row_count,
                    byte_size: v.byte_size,
                    created_at: v.created_at,
                    created_by: v.created_by,
                    symbols: v.symbols,
                    first_ts: ts(v.first_ts_ns),
                    last_ts: ts(v.last_ts_ns),
                    rows_rejected: v.rows_rejected,
                    warning_count: v.warning_count,
                    filename: v.filename,
                    run_count: v.run_count,
                    derived_from_id: v.derived_from_id,
                    derivation_kind: v.derivation_kind,
                })
                .collect(),
        },
    )
}

/// `PATCH /api/v1/datasets/{id}` body: what to change.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDataset {
    /// New name (unique in the workspace).
    pub name: Option<String>,
    /// New description (at most 2000 characters; empty clears it).
    pub description: Option<String>,
}

/// Longest description.
pub const MAX_DESCRIPTION: usize = 2000;

/// Rename a dataset or change its description.
#[utoipa::path(patch, path = "/api/v1/datasets/{id}", tag = "datasets", request_body = UpdateDataset,
    params(("id" = uuid::Uuid, Path, description = "Dataset id")),
    responses(
        (status = 200, body = DatasetDetailView),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "Another dataset has that name", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every invalid field (`details.errors`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn update_dataset(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Extension(request_id): Extension<RequestId>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(dataset_id)) = path else { return error_response(404, NO_DATASET) };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: UpdateDataset = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid update: {e}")),
    };
    let mut errors = Vec::new();
    let name = match req.name.as_deref().map(check_dataset_name) {
        None => None,
        Some(Ok(n)) => Some(n),
        Some(Err(m)) => {
            errors.push(("name".to_string(), m));
            None
        }
    };
    if let Some(d) = &req.description {
        if d.chars().count() > MAX_DESCRIPTION {
            errors.push(("description".into(), format!("at most {MAX_DESCRIPTION} characters")));
        }
    }
    if req.name.is_none() && req.description.is_none() {
        errors.push((String::new(), "give a name or a description to change".into()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    let before = match spectra_db::datasets::get_dataset(&pool, ws, dataset_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return error_response(404, NO_DATASET),
        Err(e) => return db_error(&e),
    };
    let description = req.description.as_deref().map(str::trim);
    match spectra_db::datasets::update_dataset(&pool, ws, dataset_id, name.as_deref(), description).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, NO_DATASET),
        Err(e) if e.as_database_error().is_some_and(|d| d.code().as_deref() == Some("23505")) => {
            return crate::error_with_code(
                409,
                "conflict",
                "Another dataset in this workspace has that name.",
            );
        }
        Err(e) => return db_error(&e),
    }
    let audit = spectra_db::uploads::audit(
        &pool,
        &spectra_db::uploads::AuditEvent {
            workspace_id: ws,
            actor_id: uuid::Uuid::parse_str(&id.user).ok(),
            action: "dataset.updated",
            target_type: "dataset",
            target_id: dataset_id.to_string(),
            request_id: &request_id.0,
            details: serde_json::json!({
                "name": { "from": before.name, "to": name },
                "description_changed": description.is_some(),
            }),
        },
    )
    .await;
    if let Err(e) = audit {
        tracing::error!(error = %e, "dataset update not written to the audit log");
    }
    get_dataset(State(st), Extension(id), Ok(axum::extract::Path(dataset_id))).await
}

/// One dataset version: metadata, symbols, date range and the full quality report.
#[utoipa::path(get, path = "/api/v1/datasets/versions/{id}", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset version id")),
    responses((status = 200, body = VersionDetailView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    let v = match spectra_db::datasets::get_version(&pool, ws, version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    match spectra_db::optimization::is_holdout_sealed(&pool, ws, version_id).await {
        Ok(true) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return error_with_code(
                403,
                "holdout_sealed",
                "This dataset version is a sealed holdout; direct bar access is prohibited. Use /api/v1/optimization/holdout/{id}/evaluate.",
            );
        }
        Ok(false) => {}
        Err(e) => return db_error(&e),
    }
    let chartable = v.blob_sha256.is_some()
        || st.cfg.data_dir.as_ref().is_some_and(|dir| dir.join(&v.checksum).join("bars.qfb").exists());
    to_json(
        200,
        &VersionDetailView {
            id: v.id,
            dataset_id: v.dataset_id,
            dataset_name: v.dataset_name,
            version: v.version,
            checksum: v.checksum,
            row_count: v.row_count,
            byte_size: v.byte_size,
            created_at: v.created_at,
            created_by: v.created_by,
            run_count: v.run_count,
            chartable,
            quality_report: v.quality_report,
            source: v.source,
            derived_from_id: v.derived_from_id,
            derivation_kind: v.derivation_kind,
            derivation_params: v.derivation_params,
        },
    )
}

/// Runs that used a dataset version, newest first.
#[utoipa::path(get, path = "/api/v1/datasets/versions/{id}/runs", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset version id")),
    responses((status = 200, body = VersionRunList), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn version_runs(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    match spectra_db::datasets::get_version(&pool, ws, version_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    }
    match spectra_db::datasets::version_runs(&pool, ws, version_id, 100).await {
        Ok(runs) => to_json(
            200,
            &VersionRunList {
                items: runs
                    .into_iter()
                    .map(|r| {
                        let strategy = if r.params.get("strategies").is_some_and(serde_json::Value::is_array)
                        {
                            Some("multi-strategy".to_string())
                        } else {
                            r.params.get("strategy").and_then(serde_json::Value::as_str).map(str::to_string)
                        };
                        VersionRunView {
                            id: r.id,
                            job_id: r.job_id,
                            kind: r.kind,
                            status: r.status,
                            strategy,
                            created_at: r.created_at,
                            finished_at: r.finished_at,
                            total_return: r.total_return,
                        }
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// What deleting a version did.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeletedVersionView {
    /// The version was deleted.
    pub deleted: bool,
    /// It was the dataset's last version, so the dataset was deleted too.
    pub dataset_deleted: bool,
}

/// Delete a dataset version that no run uses.
#[utoipa::path(delete, path = "/api/v1/datasets/versions/{id}", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset version id")),
    responses(
        (status = 200, body = DeletedVersionView),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "A run or a queued backtest uses it (code `in_use`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Extension(request_id): Extension<RequestId>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    let before = match spectra_db::datasets::get_version(&pool, ws, version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    let out = match spectra_db::datasets::delete_version(&pool, ws, version_id).await {
        Ok(o) => o,
        Err(e) => return db_error(&e),
    };
    if !out.found {
        return error_response(404, NO_VERSION);
    }
    if out.in_use {
        return crate::error_with_code(
            409,
            "in_use",
            "This version is used by a backtest, so it is kept: runs must stay reproducible.",
        );
    }
    if let (Some(sha), Some(store)) = (&out.orphaned_blob, &st.cfg.artifacts) {
        if let Err(e) = store.delete(sha).await {
            tracing::warn!(error = %e, %sha, "dataset file not removed from the artifact store");
        }
    }
    let audit = spectra_db::uploads::audit(
        &pool,
        &spectra_db::uploads::AuditEvent {
            workspace_id: ws,
            actor_id: uuid::Uuid::parse_str(&id.user).ok(),
            action: "dataset.version_deleted",
            target_type: "dataset_version",
            target_id: version_id.to_string(),
            request_id: &request_id.0,
            details: serde_json::json!({
                "dataset_name": before.dataset_name,
                "version": before.version,
                "checksum": before.checksum,
                "dataset_deleted": out.dataset_deleted,
            }),
        },
    )
    .await;
    if let Err(e) = audit {
        tracing::error!(error = %e, "version delete not written to the audit log");
    }
    to_json(200, &DeletedVersionView { deleted: out.deleted, dataset_deleted: out.dataset_deleted })
}

/// `GET /api/v1/datasets/versions/{id}/bars` query.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct BarsQuery {
    /// Symbol; default: the version's first.
    pub symbol: Option<String>,
    /// From (inclusive), RFC 3339 or a date (`2024-01-02`).
    pub from: Option<String>,
    /// To (inclusive), RFC 3339 or a date.
    pub to: Option<String>,
    /// Most points to return, 10 to 5000 (default 1000).
    pub points: Option<usize>,
    /// Point-in-time date or RFC 3339 timestamp (e.g. `2024-01-05`). Revisions after this date are ignored.
    pub as_of: Option<String>,
}

/// A symbol's bars for a chart.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BarsView {
    /// The symbol charted.
    pub symbol: String,
    /// Every symbol in the version.
    pub symbols: Vec<String>,
    /// Bars in the range, before reducing them to points.
    pub total_bars: u64,
    /// At most `points` points; each groups consecutive bars (first open,
    /// highest high, lowest low, last close, summed volume).
    pub points: Vec<BarPoint>,
}

/// One chart point. Prices are decimal numbers (for drawing only).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[allow(missing_docs)]
pub struct BarPoint {
    /// Milliseconds since the epoch, UTC.
    pub t: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Datasets kept decoded for charts.
const CHART_CACHE: usize = 3;

/// The time `text` names: RFC 3339, or a date (midnight UTC; `end` for its last nanosecond).
fn parse_time(text: &str, end: bool) -> Option<i64> {
    let text = text.trim();
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(text) {
        return t.timestamp_nanos_opt();
    }
    let day = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    let start = day.and_hms_opt(0, 0, 0)?.and_utc().timestamp_nanos_opt()?;
    Some(if end { start + 86_400_000_000_000 - 1 } else { start })
}

/// The decoded dataset of a version: from the cache, the API's local store,
/// or the artifact store (verified either way).
#[allow(clippy::result_large_err)] // the error is the finished response
async fn load_for_chart(
    st: &AppState,
    checksum: &str,
    blob: Option<&str>,
) -> Result<std::sync::Arc<spectra_data::Dataset>, Response> {
    if let Some(d) = st
        .charted
        .lock()
        .ok()
        .and_then(|c| c.iter().find(|(k, _)| k == checksum).map(|(_, d)| std::sync::Arc::clone(d)))
    {
        return Ok(d);
    }
    let local = st.cfg.data_dir.clone().filter(|dir| dir.join(checksum).join("bars.qfb").exists());
    let id = checksum.to_string();
    let decoded = if let Some(dir) = local {
        tokio::task::spawn_blocking(move || spectra_data::DataStore::new(dir).and_then(|s| s.get(&id))).await
    } else if let (Some(blob), Some(store)) = (blob, st.cfg.artifacts.as_ref()) {
        let raw = store.get(blob).await.map_err(|e| {
            tracing::error!(error = %e, "dataset file unreadable");
            error_response(503, "Could not read the dataset's file.")
        })?;
        tokio::task::spawn_blocking(move || spectra_data::datastore::decode_dataset(&raw, &id)).await
    } else {
        return Err(crate::error_with_code(
            409,
            "not_chartable",
            "This version was registered from the command line and its bars aren't reachable from here.",
        ));
    };
    let d = match decoded {
        Ok(Ok(d)) => std::sync::Arc::new(d),
        Ok(Err(e)) => {
            tracing::error!(error = %e, checksum, "dataset failed verification");
            return Err(error_response(503, "The dataset's file failed verification."));
        }
        Err(_) => return Err(error_response(500, "Reading the dataset crashed.")),
    };
    if let Ok(mut c) = st.charted.lock() {
        c.retain(|(k, _)| k != checksum);
        c.insert(0, (checksum.to_string(), std::sync::Arc::clone(&d)));
        c.truncate(CHART_CACHE);
    }
    Ok(d)
}

/// A symbol's bars, reduced to at most `points` points, for a price chart.
#[utoipa::path(get, path = "/api/v1/datasets/versions/{id}/bars", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset version id"), BarsQuery),
    responses(
        (status = 200, body = BarsView),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "Bars not reachable (code `not_chartable`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every invalid parameter (`details.errors`)", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn bars(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
    query: Result<axum::extract::Query<BarsQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    let Ok(axum::extract::Query(q)) = query else {
        return error_response(422, "points must be a whole number.");
    };
    let mut errors = Vec::new();
    let from = match q.from.as_deref().map(|t| parse_time(t, false)) {
        None => i64::MIN,
        Some(Some(t)) => t,
        Some(None) => {
            errors.push(("from".to_string(), "use RFC 3339 or YYYY-MM-DD".to_string()));
            0
        }
    };
    let to = match q.to.as_deref().map(|t| parse_time(t, true)) {
        None => i64::MAX,
        Some(Some(t)) => t,
        Some(None) => {
            errors.push(("to".to_string(), "use RFC 3339 or YYYY-MM-DD".to_string()));
            0
        }
    };
    let as_of = match q.as_of.as_deref().map(|t| parse_time(t, true)) {
        None => None,
        Some(Some(t)) => Some(t),
        Some(None) => {
            errors.push(("as_of".to_string(), "use RFC 3339 or YYYY-MM-DD".to_string()));
            None
        }
    };
    let points = q.points.unwrap_or(1000);
    if !(10..=5000).contains(&points) {
        errors.push(("points".into(), "must be from 10 to 5000".into()));
    }
    if errors.is_empty() && from > to {
        errors.push(("to".into(), "must not be before from".into()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    // The bars come from the checksum the database hands out, which it refuses (and logs) for a
    // sealed holdout.
    let access = spectra_db::datasets::Access { purpose: "bars", user: uuid::Uuid::parse_str(&id.user).ok() };
    let data = match spectra_db::datasets::resolve_version(&pool, ws, version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(d)) => d,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return crate::optimization_api::sealed_response(&holdout, "reading bars");
        }
        Ok(spectra_db::datasets::Resolved::Missing) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    let d = match load_for_chart(&st, &data.checksum, data.blob_sha256.as_deref()).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    let resolved = match as_of {
        Some(as_of_ns) => d.as_of(as_of_ns),
        None => (*d).clone(),
    };
    let symbol = match q.symbol {
        Some(s) => s,
        None => resolved.symbols.symbols.first().cloned().unwrap_or_default(),
    };
    let Some(instrument) = resolved.symbols.id(&symbol) else {
        return invalid_fields(&[(
            "symbol".into(),
            format!("not in this version (it has {})", resolved.symbols.symbols.join(", ")),
        )]);
    };
    let (pts, total) = spectra_data::chart_points(&resolved, instrument, from, to, points);
    let num = |raw: i64| raw as f64 / spectra_core::fixed::SCALE as f64;
    to_json(
        200,
        &BarsView {
            symbol,
            symbols: d.symbols.symbols.clone(),
            total_bars: total as u64,
            points: pts
                .into_iter()
                .map(|p| BarPoint {
                    t: p.ts_ns.div_euclid(1_000_000),
                    open: num(p.open),
                    high: num(p.high),
                    low: num(p.low),
                    close: num(p.close),
                    volume: num(p.volume),
                })
                .collect(),
        },
    )
}

/// An exchange calendar supported by the platform.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarView {
    /// MIC or short code (e.g. `XNYS`).
    pub code: String,
    /// Exchange name.
    pub name: String,
    /// Primary operating timezone.
    pub timezone: String,
    /// Regular session hours.
    pub sessions: String,
}

/// Supported calendars and timezones.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarsResponse {
    /// Supported exchanges.
    pub calendars: Vec<CalendarView>,
    /// Common IANA timezones.
    pub timezones: Vec<String>,
}

/// `GET /api/v1/calendars` (M1 task 10): lists supported exchange calendars and timezones.
#[utoipa::path(
    get,
    path = "/api/v1/calendars",
    tag = "datasets",
    responses(
        (status = 200, description = "Supported exchange calendars and timezones.", body = CalendarsResponse),
        (status = 401, description = "Missing or invalid signature.", body = crate::openapi::ErrorDoc),
    )
)]
pub async fn list_calendars() -> Response {
    let calendars = vec![
        CalendarView {
            code: "XNYS".into(),
            name: "New York Stock Exchange / NASDAQ".into(),
            timezone: "America/New_York".into(),
            sessions: "09:30-16:00 ET (early close 13:00 ET)".into(),
        },
        CalendarView {
            code: "XCME".into(),
            name: "Chicago Mercantile Exchange (US Futures)".into(),
            timezone: "America/Chicago".into(),
            sessions: "Sun 17:00 - Fri 16:00 CT (daily pause 16:00-17:00 CT)".into(),
        },
        CalendarView {
            code: "XLON".into(),
            name: "London Stock Exchange".into(),
            timezone: "Europe/London".into(),
            sessions: "08:00-16:30 London (early close 12:30 London)".into(),
        },
        CalendarView {
            code: "XTKS".into(),
            name: "Tokyo Stock Exchange".into(),
            timezone: "Asia/Tokyo".into(),
            sessions: "09:00-11:30, 12:30-15:30 JST".into(),
        },
        CalendarView {
            code: "XNSE".into(),
            name: "National Stock Exchange of India".into(),
            timezone: "Asia/Kolkata".into(),
            sessions: "09:15-15:30 IST".into(),
        },
        CalendarView {
            code: "24/7".into(),
            name: "Continuous 24/7 (Crypto)".into(),
            timezone: "UTC".into(),
            sessions: "00:00-24:00 (continuous)".into(),
        },
    ];
    let timezones = vec![
        "UTC".into(),
        "America/New_York".into(),
        "America/Chicago".into(),
        "Europe/London".into(),
        "Europe/Berlin".into(),
        "Asia/Tokyo".into(),
        "Asia/Hong_Kong".into(),
        "Asia/Kolkata".into(),
    ];
    to_json(200, &CalendarsResponse { calendars, timezones })
}

/// `POST /api/v1/datasets/versions/{id}/derive` body: corporate actions derivation options.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeriveDatasetRequest {
    /// Target dataset name (defaults to source dataset name if omitted or null).
    pub target_dataset_name: Option<String>,
    /// Corporate action set id to apply.
    pub corporate_action_set_id: uuid::Uuid,
    /// Whether to adjust splits (default: true).
    pub adjust_splits: Option<bool>,
    /// Whether to adjust dividends (default: true).
    pub adjust_dividends: Option<bool>,
}

/// Derive an adjusted dataset version from an existing version using a corporate action set.
#[utoipa::path(post, path = "/api/v1/datasets/versions/{id}/derive", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Source dataset version id")),
    request_body = DeriveDatasetRequest,
    responses(
        (status = 201, body = VersionDetailView),
        (status = 404, description = "Source dataset version or action set not found", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Derivation or adjustment math error", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn derive_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: DeriveDatasetRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid derive request: {e}")),
    };

    let v = match spectra_db::datasets::get_version(&pool, ws, version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    // The bars come from the checksum the database hands out, which it refuses (and logs) for a
    // sealed holdout.
    let access =
        spectra_db::datasets::Access { purpose: "derive", user: uuid::Uuid::parse_str(&id.user).ok() };
    let data = match spectra_db::datasets::resolve_version(&pool, ws, version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(d)) => d,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return crate::optimization_api::sealed_response(&holdout, "reading bars");
        }
        Ok(spectra_db::datasets::Resolved::Missing) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };

    let act_set =
        match spectra_db::corporate_actions::get_action_set(&pool, ws, req.corporate_action_set_id).await {
            Ok(Some(s)) => s,
            Ok(None) => return error_response(404, "No such corporate action set."),
            Err(e) => return db_error(&e),
        };

    let action_rows = match spectra_db::corporate_actions::list_actions_by_set(&pool, ws, act_set.id).await {
        Ok(a) => a,
        Err(e) => return db_error(&e),
    };

    let d = match load_for_chart(&st, &data.checksum, data.blob_sha256.as_deref()).await {
        Ok(d) => d,
        Err(r) => return r,
    };

    let adjust_splits = req.adjust_splits.unwrap_or(true);
    let adjust_dividends = req.adjust_dividends.unwrap_or(true);
    let mut actions = Vec::new();
    for act in action_rows {
        let kind = match act.action_kind.as_str() {
            "split" => {
                if !adjust_splits {
                    continue;
                }
                spectra_data::ActionKind::Split
            }
            "dividend" => {
                if !adjust_dividends {
                    continue;
                }
                spectra_data::ActionKind::Dividend
            }
            _ => continue,
        };
        let div_price = if kind == spectra_data::ActionKind::Dividend {
            match spectra_data::parse_fixed(&act.dividend_amount) {
                Some(raw) => spectra_core::Price::from_raw(raw),
                None => {
                    return error_response(
                        422,
                        format!("Invalid dividend amount {} for {}", act.dividend_amount, act.symbol),
                    );
                }
            }
        } else {
            spectra_core::Price::ZERO
        };
        actions.push(spectra_data::CorporateAction {
            symbol: act.symbol,
            ex_date_ns: act.ex_date.timestamp_nanos_opt().unwrap_or(0),
            kind,
            split_from: act.split_from as u32,
            split_to: act.split_to as u32,
            dividend_amount: div_price,
        });
    }

    let adjusted = match spectra_data::adjust_dataset(&d, &actions) {
        Ok(a) => a,
        Err(e) => return error_response(422, format!("Adjustment error: {e}")),
    };

    let raw = spectra_data::datastore::encode_dataset(&adjusted);
    let blob_sha256 = spectra_core::sha256_hex(&raw);

    if let Some(store) = st.cfg.artifacts.as_ref() {
        if let Err(e) = store.put(raw.clone()).await {
            tracing::error!(error = %e, "failed to store derived dataset bytes");
            return error_response(503, "Could not store derived dataset.");
        }
    }
    if let Some(dir) = st.cfg.data_dir.as_ref() {
        if let Ok(store) = spectra_data::DataStore::new(dir) {
            let _ = store.put(&adjusted);
        }
    }

    let target_name = req
        .target_dataset_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&v.dataset_name);

    let quality_json = serde_json::from_str(
        &spectra_data::quality_report_json(&adjusted, &spectra_data::CsvOptions::default()).dump(),
    )
    .unwrap_or(serde_json::Value::Null);
    let source_meta = serde_json::json!({
        "type": "derived",
        "derived_from_id": version_id,
        "action_set_id": req.corporate_action_set_id,
    });
    let derivation_params = serde_json::json!({
        "action_set_id": req.corporate_action_set_id,
        "adjust_splits": adjust_splits,
        "adjust_dividends": adjust_dividends,
    });

    let user_id = uuid::Uuid::parse_str(&id.user).ok();
    let reg = match spectra_db::datasets::register_derived_version(
        &pool,
        &spectra_db::datasets::NewDerivedDatasetVersion {
            workspace_id: ws,
            dataset_name: target_name,
            checksum: &adjusted.info.id,
            row_count: adjusted.bars.len() as i64,
            byte_size: raw.len() as i64,
            quality_report: quality_json.clone(),
            created_by: user_id,
            blob_sha256: Some(&blob_sha256),
            source: Some(source_meta.clone()),
            derived_from_id: Some(version_id),
            derivation_kind: Some("adjusted"),
            derivation_params: Some(derivation_params.clone()),
        },
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return db_error(&e),
    };

    // Update chart cache with new dataset
    if let Ok(mut c) = st.charted.lock() {
        c.retain(|(k, _)| k != &adjusted.info.id);
        c.insert(0, (adjusted.info.id.clone(), std::sync::Arc::new(adjusted.clone())));
        c.truncate(CHART_CACHE);
    }

    to_json(
        201,
        &VersionDetailView {
            id: reg.id,
            dataset_id: v.dataset_id,
            dataset_name: target_name.to_string(),
            version: reg.version,
            checksum: adjusted.info.id,
            row_count: adjusted.bars.len() as i64,
            byte_size: raw.len() as i64,
            created_at: chrono::Utc::now(),
            created_by: user_id,
            run_count: 0,
            chartable: true,
            quality_report: quality_json,
            source: Some(source_meta),
            derived_from_id: Some(version_id),
            derivation_kind: Some("adjusted".to_string()),
            derivation_params: Some(derivation_params),
        },
    )
}

/// `GET /api/v1/datasets/versions/{id}/export` query.
#[derive(Debug, Deserialize, utoipa::IntoParams, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportQuery {
    /// Format to export: currently only `parquet` is supported (default: `parquet`).
    pub format: Option<String>,
}

/// Export a dataset version as Apache Parquet.
#[utoipa::path(get, path = "/api/v1/datasets/versions/{id}/export", tag = "datasets",
    params(("id" = uuid::Uuid, Path, description = "Dataset version id"), ExportQuery),
    responses(
        (status = 200, description = "Parquet binary payload", content_type = "application/vnd.apache.parquet"),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid query parameter", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn export_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<axum::extract::Path<uuid::Uuid>, axum::extract::rejection::PathRejection>,
    query: Result<axum::extract::Query<ExportQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(axum::extract::Path(version_id)) = path else { return error_response(404, NO_VERSION) };
    let Ok(axum::extract::Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };
    if let Some(ref fmt) = q.format {
        if fmt != "parquet" {
            return invalid_fields(&[("format".into(), "only 'parquet' format is supported".into())]);
        }
    }
    let v = match spectra_db::datasets::get_version(&pool, ws, version_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    // The bars come from the checksum the database hands out, which it refuses (and logs) for a
    // sealed holdout.
    let access =
        spectra_db::datasets::Access { purpose: "export", user: uuid::Uuid::parse_str(&id.user).ok() };
    let data = match spectra_db::datasets::resolve_version(&pool, ws, version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(d)) => d,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return crate::optimization_api::sealed_response(&holdout, "reading bars");
        }
        Ok(spectra_db::datasets::Resolved::Missing) => return error_response(404, NO_VERSION),
        Err(e) => return db_error(&e),
    };
    let d = match load_for_chart(&st, &data.checksum, data.blob_sha256.as_deref()).await {
        Ok(d) => d,
        Err(r) => return r,
    };
    let mut buf = Vec::new();
    if let Err(e) = spectra_parquet::export_parquet(&d, &mut buf) {
        tracing::error!(error = %e, "parquet export failed");
        return error_response(500, format!("Could not export Parquet: {e}"));
    }
    let filename = format!("{}-v{}.parquet", v.dataset_name, v.version);
    Response::builder()
        .status(200)
        .header("content-type", "application/vnd.apache.parquet")
        .header("content-disposition", format!("attachment; filename=\"{filename}\""))
        .body(axum::body::Body::from(buf))
        .unwrap_or_else(|_| error_response(500, "Could not build response."))
}
