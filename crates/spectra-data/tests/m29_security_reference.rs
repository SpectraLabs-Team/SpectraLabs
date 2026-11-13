//! M29 Input & File Security Reference Tests.

use spectra_data::security::*;

#[test]
fn test_m29_csv_injection_prevention() {
    let dangerous_inputs = [
        "=SUM(A1:A10)",
        "+12345",
        "-5+5",
        "@IMPORTXML()",
        "\t=2+2",
        "\r=1+1",
        "%windir%",
        "   =cmd|' /C calc'!A0",
    ];

    for input in &dangerous_inputs {
        let escaped = escape_csv_cell(input);
        assert!(
            escaped.starts_with('\''),
            "Input '{input}' must be escaped with single quote, got '{escaped}'"
        );
    }

    let safe_inputs = ["AAPL", "2024-01-01", "100.50", "John Doe"];
    for input in &safe_inputs {
        let escaped = escape_csv_cell(input);
        assert_eq!(&escaped, input);
    }

    // Row escaping test
    let row = vec!["AAPL", "=2+2", "150.00", "+cmd"];
    let escaped_row = escape_csv_row(row);
    assert_eq!(escaped_row, vec!["AAPL", "'=2+2", "150.00", "'+cmd"]);
}

#[test]
fn test_m29_export_escaping_boundaries() {
    // Tests formula trigger characters across report and trade exports
    assert_eq!(escape_csv_cell("=1+1"), "'=1+1");
    assert_eq!(escape_csv_cell("+441234"), "'+441234");
    assert_eq!(escape_csv_cell("-20.50"), "'-20.50");
    assert_eq!(escape_csv_cell("@username"), "'@username");
    assert_eq!(escape_csv_cell("\tcommand"), "'\tcommand");
    assert_eq!(escape_csv_cell("\rreturn"), "'\rreturn");
    assert_eq!(escape_csv_cell("%0a"), "'%0a");
    assert_eq!(escape_csv_cell("  =HYPERLINK()"), "'  =HYPERLINK()");
    assert_eq!(escape_csv_cell("normal"), "normal");
}

#[test]
fn test_m29_upload_size_and_content_checks() {
    let max_size = 100;
    let small_content = b"symbol,price\nAAPL,150.00";
    let valid = validate_text_upload(small_content, max_size);
    assert!(valid.is_ok());

    let oversized = vec![b'a'; 150];
    let err = validate_text_upload(&oversized, max_size);
    assert!(matches!(err, Err(SecurityError::FileSizeExceeded { .. })));

    let null_byte_attack = b"AAPL\0MALICIOUS";
    let err2 = validate_text_upload(null_byte_attack, max_size);
    assert_eq!(err2, Err(SecurityError::IllegalControlCharacters));
}
