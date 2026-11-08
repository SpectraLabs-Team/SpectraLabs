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
