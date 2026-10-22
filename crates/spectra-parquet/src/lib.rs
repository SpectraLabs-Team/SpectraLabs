//! Spectra Labs Parquet ingestion and export service (M1b task 9 part B).
//!
//! Provides streaming row-group based ingestion and export of market data datasets
//! in Apache Parquet format while preserving exact-decimal rules and dataset identity parity.

#![deny(unsafe_code)]

pub mod reader;
pub mod writer;

pub use reader::{
    load_parquet, load_parquet_file, parquet_total_rows, preview_parquet, report_parquet, report_parquet_file,
};
pub use writer::export_parquet;
