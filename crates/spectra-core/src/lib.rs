//! Spectra Labs core: exact fixed-point money, the accounting ledger, bars,
//! hashing, deterministic random numbers and strict JSON.
//!
//! Everything here is a faithful port of the frozen C++ reference engine
//! (`quantforge` v0.6.0) and is checked against golden output produced by
//! that engine (see `parity/` at the repository root). The one deliberate
//! behavioural change is the gross leverage limit in [`ledger::LedgerConfig`].

pub mod bar;
pub mod cfmt;
pub mod error;
pub mod fixed;
pub mod instrument;
pub mod json;
pub mod ledger;
pub mod rng;
pub mod sha256;

pub use bar::{bars_sha256, timestamp_offsets, validate_bars, Bar};
pub use error::{Error, Result};
pub use fixed::{
    notional, notional_with_multiplier, prorate, ArithmeticError, Bps, Fixed, Money, Price, Qty, Ratio, SCALE,
};
pub use instrument::{
    is_currency_code, DayCount, Instrument, InstrumentType, COUPON_FREQUENCIES, DEFAULT_BASE_CURRENCY,
};
pub use json::Json;
pub use ledger::{
    EntryType, Fill, InstrumentId, JournalEntry, Ledger, LedgerConfig, LedgerError, Position, Reject, Side,
};
pub use rng::Rng;
pub use sha256::{sha256_hex, Sha256};

/// Version of the Spectra Labs engine, recorded in run records and dataset
/// metadata. The C++ reference recorded "0.6.0"; the Rust port continues the
/// numbering, so a registry can tell which engine produced each run.
pub const ENGINE_VERSION: &str = "0.7.0";
