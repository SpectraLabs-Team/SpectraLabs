//! Spectra Labs Data Studio: exact CSV ingest, data-quality reports and a
//! content-addressed dataset store. Ported from the C++ reference
//! (`quantforge` v0.6.0 `data` and `datastore`); dataset ids, quality reports
//! and the on-disk format are identical.

pub mod dataset;
pub mod datastore;
pub mod time;

pub use dataset::{
    assess_quality, compute_dataset_id, derive_dataset, format_quality_report, load_csv, load_csv_file,
    CsvOptions, DataQualityReport, Dataset, DatasetInfo, InvalidRowPolicy, RowIssue, SymbolStats,
    SymbolTable,
};
pub use datastore::DataStore;
pub use time::{format_timestamp, parse_fixed, parse_timestamp, ParsedTime, TimeFormat, TimeOptions};
