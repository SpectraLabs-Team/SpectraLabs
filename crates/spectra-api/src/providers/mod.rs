//! Market data provider connectors (M1b task 13).
//!
//! Service-layer connectors that fetch historical bars from external or mock providers
//! into datasets, supporting automated scheduled refreshes.

use std::fmt;

use spectra_core::Bar;

pub mod alpaca;
pub mod fake;
pub mod runner;
pub mod vault;

pub use runner::{
    run_refresh, sweep_due_refresh_schedules, RefreshEnv, RefreshOutcome, PROVIDER_REFRESH_JOB_KIND,
};

/// Errors returned by market data provider connectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// Authentication or API key rejected.
    AuthenticationFailed(String),
    /// Rate limit exceeded; optional retry delay in seconds.
    RateLimited { retry_after_secs: Option<u64> },
    /// Network or transport level failure.
    Http(String),
    /// Payload or response parsing failure.
    InvalidData(String),
    /// One or more requested symbols were unknown or not found.
    SymbolNotFound(String),
    /// Internal connector failure.
    Internal(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationFailed(msg) => write!(f, "authentication failed: {msg}"),
            Self::RateLimited { retry_after_secs } => match retry_after_secs {
                Some(s) => write!(f, "rate limit exceeded; retry after {s}s"),
                None => write!(f, "rate limit exceeded"),
            },
            Self::Http(msg) => write!(f, "HTTP/transport error: {msg}"),
            Self::InvalidData(msg) => write!(f, "invalid data from provider: {msg}"),
            Self::SymbolNotFound(sym) => write!(f, "symbol not found: {sym}"),
            Self::Internal(msg) => write!(f, "internal provider error: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Bar interval or resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarResolution {
    /// 1-minute bars.
    Minute,
    /// 1-hour bars.
    Hour,
    /// 1-day bars (default).
    #[default]
    Day,
}

impl BarResolution {
    /// Step size in nanoseconds.
    #[must_use]
    pub const fn step_ns(self) -> i64 {
        match self {
            Self::Minute => 60_000_000_000,
            Self::Hour => 3_600_000_000_000,
            Self::Day => 86_400_000_000_000,
        }
    }
}

/// Request to fetch bars for given symbols and time range.
#[derive(Debug, Clone)]
pub struct FetchBarsRequest {
    /// Symbols to fetch (e.g. `["AAPL", "MSFT"]`).
    pub symbols: Vec<String>,
    /// Start timestamp in nanoseconds since Unix epoch (inclusive).
    pub start_ns: i64,
    /// End timestamp in nanoseconds since Unix epoch (inclusive).
    pub end_ns: i64,
    /// Bar resolution (daily, hourly, minute).
    pub resolution: BarResolution,
}

/// Interface for market data provider connectors.
pub trait MarketDataConnector: Send + Sync {
    /// Machine-readable provider name (e.g. "fake", "alpaca").
    fn name(&self) -> &'static str;

    /// Verifies provider credentials and connectivity.
    fn validate_credentials(&self) -> impl std::future::Future<Output = Result<(), ProviderError>> + Send;

    /// Fetches historical bars for requested symbols and time range.
    ///
    /// Returned bars must be validated with increasing timestamps per instrument,
    /// and have `known_at_ns >= ts_ns`.
    fn fetch_bars(
        &self,
        req: &FetchBarsRequest,
    ) -> impl std::future::Future<Output = Result<Vec<Bar>, ProviderError>> + Send;
}
