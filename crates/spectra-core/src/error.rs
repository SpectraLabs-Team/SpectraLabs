//! The error type shared by the Spectra Labs crates above the ledger.
//!
//! The C++ reference used several exception classes (`std::invalid_argument`,
//! `DataError`, `RegistryError`, `VaultError`, `JsonError`, ...). Rust code
//! returns one enum instead; the variant says which layer refused and the
//! message is the same text the C++ exception carried, so log output and error
//! strings stay comparable.

use std::fmt;

use crate::fixed::ArithmeticError;
use crate::ledger::LedgerError;

/// Any failure reported by a Spectra Labs library call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A caller passed an invalid argument or configuration.
    InvalidArgument(String),
    /// An exact fixed-point result did not fit the representable range.
    Overflow,
    /// Market data could not be read, parsed, stored or verified.
    Data(String),
    /// A JSON document could not be parsed or a value had the wrong type.
    Json(String),
    /// The experiment registry refused an operation or found its log broken.
    Registry(String),
    /// The holdout vault refused an operation.
    Vault(String),
    /// A strategy factory or strategy reported a problem.
    Strategy(String),
    /// A filesystem or network operation failed.
    Io(String),
    /// The requested computation is not supported by the current model.
    Unsupported(String),
    /// A backend's result disagreed with the CPU reference beyond tolerance.
    /// Never downgraded to a warning.
    NumericalValidation(String),
    /// An internal consistency check failed (a bug, never user error).
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => f.write_str("fixed-point overflow"),
            Self::InvalidArgument(s)
            | Self::Data(s)
            | Self::Json(s)
            | Self::Registry(s)
            | Self::Vault(s)
            | Self::Strategy(s)
            | Self::Io(s)
            | Self::Unsupported(s)
            | Self::NumericalValidation(s)
            | Self::Internal(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

impl From<ArithmeticError> for Error {
    fn from(e: ArithmeticError) -> Self {
        match e {
            ArithmeticError::Overflow => Self::Overflow,
            other => Self::InvalidArgument(other.to_string()),
        }
    }
}

impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        match e {
            LedgerError::Arithmetic(a) => a.into(),
            LedgerError::Rejected(r) => {
                Self::InvalidArgument(format!("ledger rejected the operation: {}", r.as_str()))
            }
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Result alias used across the workspace.
pub type Result<T> = std::result::Result<T, Error>;
