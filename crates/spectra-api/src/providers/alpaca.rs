//! Market data connector for Alpaca Market Data API v2 (M1b task 13).
//!
//! Fetches historical equities bars with authentication, rate limiting handling,
//! and pagination over the Alpaca v2 data endpoint.

use std::collections::HashMap;
use std::fmt;

use chrono::DateTime;
use serde::Deserialize;
use spectra_core::fixed::{Price, Qty};
use spectra_core::ledger::InstrumentId;
use spectra_core::Bar;

use super::{BarResolution, FetchBarsRequest, MarketDataConnector, ProviderError};

/// Default Alpaca market data base URL.
pub const ALPACA_DATA_URL: &str = "https://data.alpaca.markets/v2";

/// Market data connector for the Alpaca API.
#[derive(Clone)]
pub struct AlpacaConnector {
    client: reqwest::Client,
    base_url: String,
    key_id: String,
    secret_key: String,
    feed: String,
}

impl fmt::Debug for AlpacaConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlpacaConnector")
            .field("base_url", &self.base_url)
            .field("feed", &self.feed)
            .field("key_id", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl AlpacaConnector {
    /// Creates a new Alpaca market data connector.
    #[must_use]
    pub fn new(
        base_url: impl Into<String>,
        key_id: impl Into<String>,
        secret_key: impl Into<String>,
        feed: impl Into<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .no_proxy()
            .build()
            .unwrap_or_default();

        let base = base_url.into();
        let base_url = if base.trim().is_empty() { ALPACA_DATA_URL.to_string() } else { base };

        let f = feed.into();
        let feed = if f.trim().is_empty() { "iex".to_string() } else { f };

        Self { client, base_url, key_id: key_id.into(), secret_key: secret_key.into(), feed }
    }

    /// Overrides the HTTP client (useful for tests or custom proxies).
    #[must_use]
    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }
}

#[derive(Deserialize)]
struct AlpacaBarsResponse {
    #[serde(default)]
    bars: Option<HashMap<String, Vec<AlpacaRawBar>>>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct AlpacaRawBar {
    t: String,
    o: f64,
    h: f64,
    l: f64,
    c: f64,
    v: f64,
}

impl MarketDataConnector for AlpacaConnector {
    fn name(&self) -> &'static str {
        "alpaca"
    }

    async fn validate_credentials(&self) -> Result<(), ProviderError> {
        let url = format!("{}/stocks/bars", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .query(&[("symbols", "SPY"), ("limit", "1"), ("feed", self.feed.as_str())])
            .header("APCA-API-KEY-ID", &self.key_id)
            .header("APCA-API-SECRET-KEY", &self.secret_key)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthenticationFailed("invalid Alpaca API credentials".to_string()));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after_secs = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            return Err(ProviderError::RateLimited { retry_after_secs });
        }
        if !status.is_success() {
            return Err(ProviderError::Http(format!(
                "Alpaca credential validation failed with status {status}"
            )));
        }

        Ok(())
    }

    async fn fetch_bars(&self, req: &FetchBarsRequest) -> Result<Vec<Bar>, ProviderError> {
        if req.symbols.is_empty() {
            return Ok(Vec::new());
        }

        let timeframe = match req.resolution {
            BarResolution::Minute => "1Min",
            BarResolution::Hour => "1Hour",
            BarResolution::Day => "1Day",
        };

        let start_dt = DateTime::from_timestamp(
            req.start_ns / 1_000_000_000,
            (req.start_ns % 1_000_000_000).unsigned_abs() as u32,
        )
        .unwrap_or_default();
        let end_dt = DateTime::from_timestamp(
            req.end_ns / 1_000_000_000,
            (req.end_ns % 1_000_000_000).unsigned_abs() as u32,
        )
        .unwrap_or_default();

        let start_str = start_dt.to_rfc3339();
        let end_str = end_dt.to_rfc3339();

        let mut page_token: Option<String> = None;
        let mut all_bars = Vec::new();
        let symbols_param = req.symbols.join(",");

        loop {
            let mut query_params: Vec<(&str, &str)> = vec![
                ("symbols", symbols_param.as_str()),
                ("timeframe", timeframe),
                ("start", start_str.as_str()),
                ("end", end_str.as_str()),
                ("feed", self.feed.as_str()),
                ("limit", "10000"),
                ("sort", "asc"),
            ];
            if let Some(tok) = &page_token {
                query_params.push(("page_token", tok.as_str()));
            }

            let url = format!("{}/stocks/bars", self.base_url.trim_end_matches('/'));
            let resp = self
                .client
                .get(&url)
                .query(&query_params)
                .header("APCA-API-KEY-ID", &self.key_id)
                .header("APCA-API-SECRET-KEY", &self.secret_key)
                .send()
                .await
                .map_err(|e| ProviderError::Http(e.to_string()))?;

            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                return Err(ProviderError::AuthenticationFailed("Alpaca authentication failed".to_string()));
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after_secs = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                return Err(ProviderError::RateLimited { retry_after_secs });
            }
            if !status.is_success() {
                return Err(ProviderError::Http(format!("Alpaca bars request failed with status {status}")));
            }

            let body: AlpacaBarsResponse =
                resp.json().await.map_err(|e| ProviderError::InvalidData(e.to_string()))?;

            if let Some(bars_map) = body.bars {
                for (sym_idx, sym) in req.symbols.iter().enumerate() {
                    let inst = sym_idx as InstrumentId;
                    if let Some(raw_bars) = bars_map.get(sym) {
                        for b in raw_bars {
                            let dt = DateTime::parse_from_rfc3339(&b.t).map_err(|e| {
                                ProviderError::InvalidData(format!("invalid bar timestamp {}: {e}", b.t))
                            })?;
                            let ts_ns = dt.timestamp_nanos_opt().unwrap_or(0);
                            let open = Price::from_f64(b.o)
                                .map_err(|e| ProviderError::InvalidData(e.to_string()))?;
                            let high = Price::from_f64(b.h)
                                .map_err(|e| ProviderError::InvalidData(e.to_string()))?;
                            let low = Price::from_f64(b.l)
                                .map_err(|e| ProviderError::InvalidData(e.to_string()))?;
                            let close = Price::from_f64(b.c)
                                .map_err(|e| ProviderError::InvalidData(e.to_string()))?;
                            let volume =
                                Qty::from_f64(b.v).map_err(|e| ProviderError::InvalidData(e.to_string()))?;

                            all_bars.push(Bar {
                                ts_ns,
                                known_at_ns: ts_ns,
                                instrument: inst,
                                open,
                                high,
                                low,
                                close,
                                volume,
                            });
                        }
                    }
                }
            }

            match body.next_page_token {
                Some(tok) if !tok.trim().is_empty() => page_token = Some(tok),
                _ => break,
            }
        }

        all_bars.sort_by(|a, b| a.ts_ns.cmp(&b.ts_ns).then_with(|| a.instrument.cmp(&b.instrument)));
        Ok(all_bars)
    }
}
