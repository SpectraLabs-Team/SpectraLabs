//! Deterministic mock market data connector for tests and offline development.

use std::collections::HashSet;

use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty, Rng, SCALE};

use super::{FetchBarsRequest, MarketDataConnector, ProviderError};

/// A mock connector with deterministic bar generation and simulated failures.
#[derive(Debug, Clone, Default)]
pub struct FakeConnector {
    /// When true, `validate_credentials` and `fetch_bars` fail with `AuthenticationFailed`.
    pub fail_auth: bool,
    /// When true, operations fail with `RateLimited`.
    pub rate_limit: bool,
    /// Symbols that return `SymbolNotFound`.
    pub missing_symbols: HashSet<String>,
    /// Explicit bars to return instead of generated ones, paired with their symbol.
    pub custom_bars: Vec<(String, Bar)>,
}

impl FakeConnector {
    /// Creates a new default fake connector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configures the connector to simulate an authentication failure.
    #[must_use]
    pub fn with_auth_failure(mut self) -> Self {
        self.fail_auth = true;
        self
    }

    /// Configures the connector to simulate a rate-limit error.
    #[must_use]
    pub fn with_rate_limit(mut self) -> Self {
        self.rate_limit = true;
        self
    }

    /// Marks a symbol as missing/unknown.
    #[must_use]
    pub fn with_missing_symbol(mut self, sym: impl Into<String>) -> Self {
        self.missing_symbols.insert(sym.into());
        self
    }

    /// Supplies explicit bars to return for matching symbols.
    #[must_use]
    pub fn with_custom_bars(mut self, bars: Vec<(String, Bar)>) -> Self {
        self.custom_bars = bars;
        self
    }
}

impl MarketDataConnector for FakeConnector {
    fn name(&self) -> &'static str {
        "fake"
    }

    #[allow(clippy::unused_async_trait_impl)]
    async fn validate_credentials(&self) -> Result<(), ProviderError> {
        if self.fail_auth {
            return Err(ProviderError::AuthenticationFailed("invalid fake API key".into()));
        }
        if self.rate_limit {
            return Err(ProviderError::RateLimited { retry_after_secs: Some(30) });
        }
        Ok(())
    }

    async fn fetch_bars(&self, req: &FetchBarsRequest) -> Result<Vec<Bar>, ProviderError> {
        self.validate_credentials().await?;

        for sym in &req.symbols {
            if self.missing_symbols.contains(sym) {
                return Err(ProviderError::SymbolNotFound(sym.clone()));
            }
        }

        if !self.custom_bars.is_empty() {
            let symbol_set: HashSet<&str> = req.symbols.iter().map(String::as_str).collect();
            let mut matched: Vec<Bar> = self
                .custom_bars
                .iter()
                .filter(|(s, b)| {
                    symbol_set.contains(s.as_str()) && b.ts_ns >= req.start_ns && b.ts_ns <= req.end_ns
                })
                .map(|(_, b)| *b)
                .collect();
            matched.sort_by(|a, b| a.ts_ns.cmp(&b.ts_ns).then_with(|| a.instrument.cmp(&b.instrument)));
            return Ok(matched);
        }

        let step_ns = req.resolution.step_ns();
        let mut out = Vec::new();

        for (inst_id, sym) in req.symbols.iter().enumerate() {
            // Seed RNG based on symbol name
            let seed =
                sym.bytes().fold(104_729_u64, |acc, b| acc.wrapping_mul(31).wrapping_add(u64::from(b)));
            let mut rng = Rng::new(seed);
            let mut current_price = (50 + (seed % 100) as i64) * SCALE;

            let mut t = req.start_ns;
            while t <= req.end_ns {
                let open = current_price;
                let shock = rng.range(-SCALE, SCALE);
                let close = (open + shock).max(SCALE);
                let high = open.max(close) + rng.range(0, SCALE / 4);
                let low = (open.min(close) - rng.range(0, SCALE / 4)).max(1);
                let volume = rng.range(10_000, 100_000) * SCALE;
                current_price = close;

                out.push(Bar {
                    ts_ns: t,
                    known_at_ns: t,
                    instrument: inst_id as u32,
                    open: Price::from_raw(open),
                    high: Price::from_raw(high),
                    low: Price::from_raw(low),
                    close: Price::from_raw(close),
                    volume: Qty::from_raw(volume),
                });

                t += step_ns;
            }
        }

        out.sort_by(|a, b| a.ts_ns.cmp(&b.ts_ns).then_with(|| a.instrument.cmp(&b.instrument)));
        Ok(out)
    }
}
