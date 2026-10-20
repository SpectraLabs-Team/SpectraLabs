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
use crate::{engine, error_response, invalid_fields, json_response, AppState, Identity, RequestId};

/// Largest preview sample, in bytes (the dashboard proxy accepts 2 MB bodies).
pub const MAX_PREVIEW_SAMPLE: usize = 1024 * 1024;
/// Data rows a preview parses.
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
    /// (-720 to 840). A fixed offset: daylight saving is not modelled.
    pub utc_offset_minutes: i32,
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
    /// Up to 200 parsed rows, sorted by timestamp then symbol. Prices are exact decimal text.
    pub rows: Vec<PreviewRow>,
    /// Data rows in the sample beyond the first 200, not parsed.
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
    if req.truncated {
        // Keep whole lines: the cut may have split the last one.
        let keep = sample.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        sample.truncate(keep);
    }
    crate::compute_with(&st, move || {
        let suggestions = spectra_data::suggest(&sample).to_json();
        let preview = match &opt {
            None => Json::Null,
            Some(opt) => match spectra_data::preview_csv(&sample, opt, PREVIEW_ROWS) {
                Ok(p) => spectra_data::preview_json(&p, opt, PREVIEW_ROWS),
                Err(e) => return invalid_fields(&[("sample".into(), file_problem(&e))]),
            },
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
