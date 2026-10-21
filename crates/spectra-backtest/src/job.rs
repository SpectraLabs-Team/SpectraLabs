//! The `backtest` job: everything the worker does except talk to Postgres.
//!
//! Payload (what `POST /api/v1/jobs` takes with `kind: "backtest"`):
//!
//! ```json
//! {
//!   "dataset_version_id": "0192f0c4-…",
//!   "strategy": "sma_cross",
//!   "params": { "fast": 10, "slow": 30 },
//!   "config": { "preset": "default" }
//! }
//! ```
//!
//! A backtest started from a saved strategy version (M2a) also carries
//! `"strategy_version_id"`: the API fills `strategy` and `params` from the
//! version, and the worker records the id on the run.
//!
//! The worker resolves `dataset_version_id` to a dataset-store checksum in the
//! caller's workspace, then calls [`run_job_from_store`] on a blocking thread
//! with a cancel flag and a progress callback, and stores [`JobProducts`].

use std::path::Path;

use spectra_core::{Bar, Error, Json, Result};
use spectra_data::DataStore;
use spectra_engine::{RunControl, RunError, RunRecord};

use crate::config::{ConfigError, FieldError};
use crate::output::{artifacts, series_json, summary_json, SeriesKind, DEFAULT_SERIES_POINTS};
use crate::run::{run_request_on, BacktestRequest};

/// The job kind name.
pub const JOB_KIND: &str = "backtest";

/// A validated backtest job payload.
#[derive(Debug, Clone)]
pub struct BacktestJob {
    /// Dataset version to run on (a UUID in the caller's workspace).
    pub dataset_version_id: String,
    /// The saved strategy version this run was started from (lower-case UUID), if any.
    pub strategy_version_id: Option<String>,
    /// Strategy, parameters and configuration.
    pub request: BacktestRequest,
}

/// True for the canonical lowercase or uppercase 8-4-4-4-12 hex UUID form.
fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices()
            .all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

impl BacktestJob {
    /// Parses and validates a payload, reporting every problem at once.
    ///
    /// # Errors
    /// [`ConfigError`]: `dataset_version_id`, `strategy`, `params.*`, `config.*`.
    pub fn from_payload(payload: &Json) -> std::result::Result<Self, ConfigError> {
        let Ok(entries) = payload.entries() else {
            return Err(ConfigError {
                errors: vec![FieldError {
                    field: String::new(),
                    message: "payload must be a JSON object".into(),
                }],
            });
        };
        let mut errors = Vec::new();
        let dataset = match payload.find("dataset_version_id") {
            Some(Json::String(s)) if is_uuid(s.trim()) => Some(s.trim().to_ascii_lowercase()),
            Some(Json::String(_)) => {
                errors.push(FieldError {
                    field: "dataset_version_id".into(),
                    message: "must be a UUID".into(),
                });
                None
            }
            Some(_) => {
                errors.push(FieldError {
                    field: "dataset_version_id".into(),
                    message: "must be a string".into(),
                });
                None
            }
            None => {
                errors.push(FieldError { field: "dataset_version_id".into(), message: "is required".into() });
                None
            }
        };
        let strategy_version = match payload.find("strategy_version_id") {
            None | Some(Json::Null) => None,
            Some(Json::String(s)) if is_uuid(s.trim()) => Some(s.trim().to_ascii_lowercase()),
            Some(_) => {
                errors.push(FieldError {
                    field: "strategy_version_id".into(),
                    message: "must be a UUID".into(),
                });
                None
            }
        };
        let mut rest = Json::object();
        for (k, v) in entries {
            if k != "dataset_version_id" && k != "strategy_version_id" {
                // Keys are unique in a parsed object, so this cannot fail.
                let _ = rest.set(k.clone(), v.clone());
            }
        }
        let request = match BacktestRequest::from_json(&rest) {
            Ok(r) => Some(r),
            Err(mut e) => {
                errors.append(&mut e.errors);
                None
            }
        };
        match (dataset, request) {
            (Some(dataset_version_id), Some(request)) if errors.is_empty() => {
                Ok(Self { dataset_version_id, strategy_version_id: strategy_version, request })
            }
            _ => Err(ConfigError { errors }),
        }
    }
}

/// One file to store with the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFile {
    /// Artifact kind recorded in `run_artifacts.kind`.
    pub kind: &'static str,
    /// Suggested file name for downloads.
    pub file_name: &'static str,
    /// Media type.
    pub media_type: &'static str,
    /// Contents.
    pub bytes: Vec<u8>,
}

/// Everything a finished backtest job stores.
#[derive(Debug, Clone)]
pub struct JobProducts {
    /// The summary document (also the run's `metrics`).
    pub summary: Json,
    /// What was run (the run's `params`): the stored request plus the dataset.
    pub params: Json,
    /// Files for the artifact store: summary, chart series, CSVs and the request.
    pub files: Vec<JobFile>,
    /// The engine's run record (fingerprints).
    pub run: RunRecord,
}

/// Runs a job on bars already loaded.
///
/// # Errors
/// [`RunError`] as from [`run_request_on`].
pub fn run_job(
    bars: &[Bar],
    symbols: &[String],
    job: &BacktestJob,
    dataset_checksum: &str,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<JobProducts, RunError> {
    let out = run_request_on(bars, symbols, &job.request, ctl)?;
    let summary = summary_json(&out)?;
    let params = job
        .request
        .to_json()?
        .with("dataset_version_id", Json::string(&job.dataset_version_id))
        .with("dataset_checksum", Json::string(dataset_checksum));

    let json_file = |kind, file_name, j: &Json| JobFile {
        kind,
        file_name,
        media_type: "application/json",
        bytes: j.dump().into_bytes(),
    };
    let mut files = vec![
        json_file("summary", "summary.json", &summary),
        json_file(
            "equity_series",
            "equity_series.json",
            &series_json(&out.result.equity_curve, SeriesKind::Equity, DEFAULT_SERIES_POINTS)?,
        ),
        json_file(
            "drawdown_series",
            "drawdown_series.json",
            &series_json(&out.result.equity_curve, SeriesKind::Drawdown, DEFAULT_SERIES_POINTS)?,
        ),
        json_file("request", "request.json", &params),
    ];
    for a in artifacts(&out.result) {
        let kind = match a.name {
            "equity.csv" => "equity_csv",
            "orders.csv" => "orders_csv",
            "fills.csv" => "fills_csv",
            "strategies.csv" => "strategies_csv",
            _ => "returns_csv",
        };
        files.push(JobFile { kind, file_name: a.name, media_type: a.media_type, bytes: a.bytes });
    }
    Ok(JobProducts { summary, params, files, run: out.result.run })
}

/// Loads the bars of dataset `checksum` from the dataset store at `store_root`.
///
/// # Errors
/// [`Error::Data`] if the dataset is missing or fails verification, [`Error::Io`]
/// if the store cannot be read.
pub fn load_bars(store_root: &Path, checksum: &str) -> Result<Vec<Bar>> {
    Ok(load_dataset(store_root, checksum)?.bars)
}

/// Loads dataset `checksum` (bars and symbol table) from the dataset store.
///
/// # Errors
/// As [`load_bars`].
pub fn load_dataset(store_root: &Path, checksum: &str) -> Result<spectra_data::Dataset> {
    let store = DataStore::new(store_root.to_path_buf())?;
    if !store.contains(checksum) {
        return Err(Error::Data(format!(
            "dataset {checksum} is not in the dataset store; it may have been registered on another machine"
        )));
    }
    store.get(checksum)
}

/// Why a job did not produce results, sorted the way the queue needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobFailure {
    /// The cancel flag stopped it.
    Cancelled,
    /// It failed. Retryable failures (I/O) go back to the queue; the rest
    /// (bad data, bad strategy, limits) would fail the same way again.
    Failed {
        /// Worth another attempt.
        retryable: bool,
        /// For the job's `error` field, in words a user can act on.
        message: String,
    },
}

impl JobFailure {
    fn from_engine(e: &Error) -> Self {
        Self::Failed { retryable: matches!(e, Error::Io(_)), message: e.to_string() }
    }
}

impl From<RunError> for JobFailure {
    fn from(e: RunError) -> Self {
        match e {
            RunError::Cancelled(_) => Self::Cancelled,
            RunError::TimedOut(p) => Self::Failed {
                retryable: false,
                message: format!(
                    "The backtest hit its time limit after {} of {} bars. Use a shorter date range or fewer \
                     instruments.",
                    p.bars_done, p.bars_total
                ),
            },
            RunError::TooLarge(m) => Self::Failed { retryable: false, message: m },
            RunError::Engine(e) => Self::from_engine(&e),
        }
    }
}

/// Loads the dataset and runs the job: the whole blocking part of a worker's job.
///
/// # Errors
/// [`JobFailure`], already classified for the queue.
pub fn run_job_from_store(
    store_root: &Path,
    dataset_checksum: &str,
    job: &BacktestJob,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<JobProducts, JobFailure> {
    if ctl.cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
        return Err(JobFailure::Cancelled);
    }
    let d = load_dataset(store_root, dataset_checksum).map_err(|e| JobFailure::from_engine(&e))?;
    let mut ctl = std::panic::AssertUnwindSafe(ctl);
    let run_res =
        std::panic::catch_unwind(move || run_job(&d.bars, &d.symbols.symbols, job, dataset_checksum, *ctl));
    match run_res {
        Ok(res) => res.map_err(JobFailure::from),
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "an unexpected panic occurred".to_string()
            };
            Err(JobFailure::Failed { retryable: false, message: format!("The strategy panicked: {msg}") })
        }
    }
}
