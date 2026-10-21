//! Spectra Labs Data Studio: exact CSV ingest, data-quality reports and a
//! content-addressed dataset store. Ported from the C++ reference
//! (`quantforge` v0.6.0 `data` and `datastore`); dataset ids, quality reports
//! and the on-disk format are identical.

pub mod chart;
pub mod dataset;
pub mod datastore;
mod records;
pub mod report;
pub mod settings;
pub mod time;

pub use chart::{chart_points, ChartPoint};
pub use dataset::{
    assess_quality, compute_dataset_id, derive_dataset, format_quality_report, load_csv, load_csv_file,
    load_csv_reader, preview_csv, report_csv_reader, CsvOptions, CsvPreview, DataQualityReport, Dataset,
    DatasetInfo, InvalidRowPolicy, RowIssue, SymbolStats, SymbolTable,
};
pub use datastore::DataStore;
pub use report::{preview_json, quality_report_json};
pub use settings::{parse_settings, settings_json, suggest, SettingsError, Suggestions};
pub use time::{format_timestamp, parse_fixed, parse_timestamp, ParsedTime, TimeFormat, TimeOptions};
