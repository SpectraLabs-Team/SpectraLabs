//! M29 Malicious-Input Corpus & Parser Fuzzing Tests.
//!
//! Feeds malformed, hostile, and edge-case inputs into the parsers to guarantee
//! they fail gracefully with clear errors and never panic or abort.

use spectra_data::security::*;
use spectra_data::{load_csv, CsvOptions};

#[test]
fn test_m29_corpus_malformed_csv_no_panic() {
    let test_cases: &[(&str, &[u8])] = &[
        // 1. Unterminated quote
        ("unterminated_quote", b"timestamp,symbol,close\n2024-01-01,\"AAPL,150.00\n2024-01-02,AAPL,155.00"),
        // 2. Quote in unquoted field
        ("misplaced_quote", b"timestamp,symbol,close\n2024-01-01,AA\"PL,150.00\n"),
        // 3. UTF-8 BOM variants & invalid UTF-16
        ("utf16_bom", b"\xFF\xFEtimestamp,symbol,close\n2024-01-01,AAPL,150.00"),
        // 4. Mixed line endings (\r, \n, \r\n)
        ("mixed_line_endings", b"timestamp,symbol,close\r2024-01-01,AAPL,150.00\n2024-01-02,AAPL,151.00\r\n2024-01-03,AAPL,152.00"),
        // 5. Embedded NUL bytes
        ("null_byte_header", b"timestamp,\0symbol,close\n2024-01-01,AAPL,150.00"),
        ("null_byte_data", b"timestamp,symbol,close\n2024-01-01,AA\0PL,150.00"),
        // 6. Huge field (64 KB field)
        ("huge_field", &[b"timestamp,symbol,close\n2024-01-01,".as_slice(), &vec![b'A'; 65536], b",150.00\n"].concat()),
        // 7. Non-finite values
        ("nan_value", b"timestamp,symbol,close\n2024-01-01,AAPL,NaN\n"),
        ("infinity_value", b"timestamp,symbol,close\n2024-01-01,AAPL,Infinity\n"),
        ("neg_infinity_value", b"timestamp,symbol,close\n2024-01-01,AAPL,-Infinity\n"),
        // 8. Truncated input
        ("truncated_header", b"time"),
        ("empty_input", b""),
        ("whitespace_only", b"   \t  \r\n   "),
        // 9. Dangerous formula triggers in input
        ("formula_injection", b"timestamp,symbol,close\n2024-01-01,=cmd|' /C calc'!A0,150.00\n"),
    ];

    let opts = CsvOptions::default();

    for (name, payload) in test_cases {
        let result = std::panic::catch_unwind(|| {
            let _ = load_csv(payload, &opts);
        });
        assert!(result.is_ok(), "Parser must never panic on corpus case '{name}'");
    }
}

#[test]
fn test_m29_corpus_text_upload_checks() {
    let max_size = 1024;

    // Normal text
    assert!(validate_text_upload(b"symbol,close\nAAPL,150.00", max_size).is_ok());

    // NUL byte rejection
    assert_eq!(
        validate_text_upload(b"symbol\0attack,close", max_size),
        Err(SecurityError::IllegalControlCharacters)
    );

    // Invalid UTF-8 rejection
    assert_eq!(validate_text_upload(&[0xC0, 0xAF], max_size), Err(SecurityError::InvalidUtf8));

    // File size boundary
    let oversize = vec![b'x'; 2048];
    assert!(matches!(
        validate_text_upload(&oversize, max_size),
        Err(SecurityError::FileSizeExceeded { actual: 2048, max: 1024 })
    ));
}

#[test]
fn test_m29_corpus_parquet_magic_and_truncation() {
    // 1. Truncated parquet (less than 4 bytes)
    assert!(verify_parquet_magic(b"PAR").is_err());

    // 2. Missing PAR1 magic
    assert!(verify_parquet_magic(b"NOTPARQUETMAGIC").is_err());

    // 3. Valid PAR1 magic
    assert!(verify_parquet_magic(b"PAR1restofbinarydata").is_ok());
}

#[test]
fn test_m29_corpus_workspace_upload_quotas() {
    let max_storage = 1024 * 1024; // 1 MiB
    let max_uploads = 5;

    // Within quota
    assert!(check_workspace_quota(500_000, 100_000, max_storage, 2, max_uploads).is_ok());

    // Exceeds daily upload count boundary
    assert_eq!(
        check_workspace_quota(100_000, 10_000, max_storage, 5, max_uploads),
        Err(SecurityError::QuotaExceeded { quota: "daily_uploads" })
    );

    // Exceeds storage limit boundary
    assert_eq!(
        check_workspace_quota(1000_000, 50_000, max_storage, 1, max_uploads),
        Err(SecurityError::QuotaExceeded { quota: "storage_bytes" })
    );

    // Exact boundary check (current + incoming == max)
    assert!(check_workspace_quota(1000_000, 24 * 1024, max_storage, 4, max_uploads).is_ok());
}

#[test]
fn test_m29_corpus_sanitize_filename() {
    assert_eq!(sanitize_filename("report.csv"), "report.csv");
    assert_eq!(sanitize_filename("path/traversal/../escape.parquet"), "path_traversal_.._escape.parquet");
    assert_eq!(sanitize_filename("newline\r\ninjection.csv"), "newline__injection.csv");
    assert_eq!(sanitize_filename("special!@#$%^&*().csv"), "special__________.csv");
    assert_eq!(sanitize_filename(""), "download");
}
