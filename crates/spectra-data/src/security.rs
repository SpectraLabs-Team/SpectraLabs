//! Input & File Security (M29).
//!
//! Provides CSV formula injection protection, upload size limits, strict UTF-8/BOM
//! validation, and defense against malicious input files.

use std::fmt;

/// Errors arising from input security validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityError {
    /// File size exceeds maximum allowed bytes.
    FileSizeExceeded { actual: usize, max: usize },
    /// Input is not valid UTF-8.
    InvalidUtf8,
    /// Prohibited control characters (e.g. null bytes) found in text input.
    IllegalControlCharacters,
    /// Row count exceeded limit.
    RowCountExceeded { actual: usize, max: usize },
    /// Field length exceeded limit.
    FieldLengthExceeded { length: usize, max: usize },
    /// Input contains non-finite numerical values (NaN / Inf).
    NonFiniteNumericValue,
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSizeExceeded { actual, max } => {
                write!(f, "file size {} exceeds maximum allowed limit of {} bytes", actual, max)
            }
            Self::InvalidUtf8 => write!(f, "file content is not valid UTF-8"),
            Self::IllegalControlCharacters => {
                write!(f, "file contains illegal control characters or null bytes")
            }
            Self::RowCountExceeded { actual, max } => {
                write!(f, "row count {} exceeds maximum allowed limit of {}", actual, max)
            }
            Self::FieldLengthExceeded { length, max } => {
                write!(f, "field length {} exceeds maximum allowed limit of {}", length, max)
            }
            Self::NonFiniteNumericValue => write!(f, "numeric value is NaN or infinite"),
        }
    }
}

impl std::error::Error for SecurityError {}

/// Default limits for input ingestion.
pub const DEFAULT_MAX_FILE_SIZE: usize = 100 * 1024 * 1024; // 100 MiB
pub const DEFAULT_MAX_ROWS: usize = 5_000_000;
pub const DEFAULT_MAX_FIELD_LENGTH: usize = 4_096;

/// Escapes a CSV cell value to neutralize spreadsheet formula injection (OWASP CSV Injection).
///
/// If the trimmed string starts with `=`, `+`, `-`, `@`, `\t`, `\r`, or `%`,
/// it is prepended with a single quote `'` so spreadsheet applications treat it as text.
pub fn escape_csv_cell(cell: &str) -> String {
    let trimmed = cell.trim_start();
    if trimmed.is_empty() {
        return cell.to_string();
    }

    let first = trimmed.as_bytes()[0];
    if matches!(first, b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r' | b'%') {
        format!("'{}", cell)
    } else {
        cell.to_string()
    }
}

/// Sanitizes an entire CSV row against formula injection.
pub fn escape_csv_row<'a>(fields: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    fields.into_iter().map(escape_csv_cell).collect()
}

/// Validates raw text file bytes for UTF-8 validity, BOM, and null-byte injection.
pub fn validate_text_upload(bytes: &[u8], max_size: usize) -> Result<&str, SecurityError> {
    if bytes.len() > max_size {
        return Err(SecurityError::FileSizeExceeded { actual: bytes.len(), max: max_size });
    }

    // Strip UTF-8 BOM if present (EF BB BF)
    let payload = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) { &bytes[3..] } else { bytes };

    let text = std::str::from_utf8(payload).map_err(|_| SecurityError::InvalidUtf8)?;

    // Disallow null bytes (common in binary injection/tampering)
    if text.contains('\0') {
        return Err(SecurityError::IllegalControlCharacters);
    }

    Ok(text)
}

/// Validates a parsed floating-point number, rejecting NaNs and infinities.
pub fn validate_finite(val: f64) -> Result<f64, SecurityError> {
    if val.is_finite() {
        Ok(val)
    } else {
        Err(SecurityError::NonFiniteNumericValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_formula_injection_escaping() {
        // Starts with dangerous formula characters
        assert_eq!(escape_csv_cell("=1+1"), "'=1+1");
        assert_eq!(escape_csv_cell("+cmd|' /C calc'!A0"), "'+cmd|' /C calc'!A0");
        assert_eq!(escape_csv_cell("-2+3*cmd"), "'-2+3*cmd");
        assert_eq!(escape_csv_cell("@SUM(A1:A10)"), "'@SUM(A1:A10)");
        assert_eq!(escape_csv_cell("\t=HYPERLINK()"), "'\t=HYPERLINK()");
        assert_eq!(escape_csv_cell("\r=2+2"), "'\r=2+2");
        assert_eq!(escape_csv_cell("%0Acmd"), "'%0Acmd");

        // Leading whitespace before trigger char must also be neutralized
        assert_eq!(escape_csv_cell("  =1+1"), "'  =1+1");

        // Normal strings and numbers remain unescaped
        assert_eq!(escape_csv_cell("AAPL"), "AAPL");
        assert_eq!(escape_csv_cell("123.45"), "123.45");
        assert_eq!(escape_csv_cell("Regular text"), "Regular text");
        assert_eq!(escape_csv_cell(""), "");
    }

    #[test]
    fn test_text_upload_validation() {
        // Valid plain text
        let ok = b"timestamp,symbol,close\n2024-01-01,AAPL,150.00";
        assert!(validate_text_upload(ok, 1024).is_ok());

        // Valid with UTF-8 BOM
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(ok);
        let res = validate_text_upload(&with_bom, 1024).unwrap();
        assert_eq!(res, "timestamp,symbol,close\n2024-01-01,AAPL,150.00");

        // Exceeds max size
        assert_eq!(
            validate_text_upload(ok, 10),
            Err(SecurityError::FileSizeExceeded { actual: ok.len(), max: 10 })
        );

        // Null byte injection
        let malicious = b"timestamp\0malicious";
        assert_eq!(validate_text_upload(malicious, 1024), Err(SecurityError::IllegalControlCharacters));

        // Invalid UTF-8
        let bad_utf8 = vec![0xFF, 0xFE, 0x00, 0x3C];
        assert_eq!(validate_text_upload(&bad_utf8, 1024), Err(SecurityError::InvalidUtf8));
    }

    #[test]
    fn test_finite_validation() {
        assert!(validate_finite(123.45).is_ok());
        assert_eq!(validate_finite(f64::NAN), Err(SecurityError::NonFiniteNumericValue));
        assert_eq!(validate_finite(f64::INFINITY), Err(SecurityError::NonFiniteNumericValue));
        assert_eq!(validate_finite(f64::NEG_INFINITY), Err(SecurityError::NonFiniteNumericValue));
    }
}
