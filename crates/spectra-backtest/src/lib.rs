//! Backtests as data, for the backtest job (M3 phase M3a, tasks 1-4).
//!
//!  * [`config`]: the user-facing configuration form, presets, validation that
//!    reports every problem, and a JSON Schema for the Studio form.
//!  * [`strategies`]: built-in strategies by name, with parameter schemas and
//!    source hashes (the slice's stand-in for M2 strategy versions).
//!  * [`run`]: a request document (strategy + params + config) and running it
//!    with limits, cancellation and progress.
//!  * [`output`]: CSV artifacts, the summary document and downsampled chart series.
//!  * [`job`]: the worker's `backtest` job: payload validation, loading the
//!    dataset, running, and the files to store.
//!
//! The engine never runs inside an API request: the API validates a request
//! with [`BacktestRequest::from_json`] and queues a job; the worker calls
//! [`run_request`] and stores [`output::artifacts`] and [`output::summary_json`].
//! A run started this way has the same fingerprints as the same configuration
//! and strategy run directly with [`spectra_engine::run_backtest`].

pub mod config;
pub mod corporate;
pub mod instruments;
pub mod job;
pub mod output;
pub mod run;
pub mod strategies;

pub use config::{
    config_from_stored_json, config_schema, config_to_user_json, parse_config, presets_json,
    stored_config_json, ConfigError, FieldError, Preset,
};
pub use corporate::{ActionTarget, RequestAction};
pub use instruments::{parse_curve_history, parse_instrument, InstrumentRef, RequestInstruments};
pub use job::{
    load_bars, load_dataset, run_job, run_job_from_store, BacktestJob, JobFailure, JobFile, JobProducts,
    JOB_KIND,
};
pub use output::{
    artifacts, downsample, series, series_json, strategies_csv, summary_json, Artifact, SeriesKind,
    SeriesPoint, DEFAULT_SERIES_POINTS, SUMMARY_SCHEMA,
};
pub use run::{
    run_request, run_request_on, BacktestOutput, BacktestRequest, StrategyAllocation, MAX_REQUEST_STRATEGIES,
};
pub use strategies::{
    builtin, builtin_factory, builtin_strategies, builtins_json, find_builtin, BuiltinStrategy, ParamKind,
    ParamSpec,
};
