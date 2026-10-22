//! Spectra Labs Parquet ingestion and export service (M1b task 9 part B).
//!
//! Provides streaming row-group based ingestion and export of market data datasets
//! in Apache Parquet format while preserving exact-decimal rules and dataset identity parity.

#![deny(unsafe_code)]

/// Placeholder to verify crate setup.
#[must_use]
pub fn version() -> &'static str {
    "0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(version(), "0.1.0");
    }
}
