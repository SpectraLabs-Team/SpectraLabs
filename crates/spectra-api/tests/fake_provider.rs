//! Tests for the market data connector interface and FakeConnector (M1b task 13).

use spectra_api::providers::fake::FakeConnector;
use spectra_api::providers::{BarResolution, FetchBarsRequest, MarketDataConnector, ProviderError};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty, SCALE};

const DAY: i64 = 86_400_000_000_000;

#[tokio::test]
async fn fake_connector_credentials_and_errors() {
    let conn = FakeConnector::new();
    assert_eq!(conn.name(), "fake");
    assert!(conn.validate_credentials().await.is_ok());

    let auth_fail = FakeConnector::new().with_auth_failure();
    assert!(matches!(
        auth_fail.validate_credentials().await.unwrap_err(),
        ProviderError::AuthenticationFailed(_)
    ));

    let rate_limited = FakeConnector::new().with_rate_limit();
    assert!(matches!(
        rate_limited.validate_credentials().await.unwrap_err(),
        ProviderError::RateLimited { retry_after_secs: Some(30) }
    ));
}

#[tokio::test]
async fn fake_connector_fetches_deterministic_bars_with_invariants() {
    let conn = FakeConnector::new();
    let req = FetchBarsRequest {
        symbols: vec!["AAPL".into(), "MSFT".into()],
        start_ns: DAY,
        end_ns: 5 * DAY,
        resolution: BarResolution::Day,
    };

    let bars = conn.fetch_bars(&req).await.unwrap();
    // 5 days * 2 symbols = 10 bars
    assert_eq!(bars.len(), 10);

    // Verify ordering by ts_ns, then instrument
    for w in bars.windows(2) {
        assert!(w[0].ts_ns <= w[1].ts_ns);
        if w[0].ts_ns == w[1].ts_ns {
            assert!(w[0].instrument < w[1].instrument);
        }
    }

    // Verify invariants
    for b in &bars {
        assert_eq!(b.known_at_ns, b.ts_ns);
        assert!(b.high >= b.open.max(b.close));
        assert!(b.low <= b.open.min(b.close));
        assert!(b.volume.raw() > 0);
    }

    // Same request is 100% deterministic
    let bars2 = conn.fetch_bars(&req).await.unwrap();
    assert_eq!(bars, bars2);
}

#[tokio::test]
async fn fake_connector_handles_missing_symbols_and_custom_bars() {
    let conn = FakeConnector::new().with_missing_symbol("INVALID");
    let req = FetchBarsRequest {
        symbols: vec!["AAPL".into(), "INVALID".into()],
        start_ns: DAY,
        end_ns: 2 * DAY,
        resolution: BarResolution::Day,
    };
    let err = conn.fetch_bars(&req).await.unwrap_err();
    assert!(matches!(err, ProviderError::SymbolNotFound(sym) if sym == "INVALID"));

    let custom = vec![(
        "CUSTOM".into(),
        Bar {
            ts_ns: DAY,
            known_at_ns: DAY,
            instrument: 0,
            open: Price::from_raw(100 * SCALE),
            high: Price::from_raw(105 * SCALE),
            low: Price::from_raw(95 * SCALE),
            close: Price::from_raw(102 * SCALE),
            volume: Qty::from_raw(50_000 * SCALE),
        },
    )];
    let custom_conn = FakeConnector::new().with_custom_bars(custom);
    let req_custom = FetchBarsRequest {
        symbols: vec!["CUSTOM".into()],
        start_ns: DAY,
        end_ns: DAY,
        resolution: BarResolution::Day,
    };
    let fetched = custom_conn.fetch_bars(&req_custom).await.unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].close, Price::from_raw(102 * SCALE));
}

#[tokio::test]
async fn connector_dispatch_and_payload_serialization() {
    use spectra_api::providers::runner::{Connector, RefreshJobPayload, PROVIDER_REFRESH_JOB_KIND};
    use uuid::Uuid;

    let conn = Connector::Fake(FakeConnector::new());
    let req = FetchBarsRequest {
        symbols: vec!["AAPL".into()],
        start_ns: DAY,
        end_ns: DAY,
        resolution: BarResolution::Day,
    };
    let bars = conn.fetch_bars(&req).await.unwrap();
    assert_eq!(bars.len(), 1);

    let payload = RefreshJobPayload {
        dataset_id: Uuid::now_v7(),
        schedule_id: Some(Uuid::now_v7()),
        provider_config_id: Uuid::now_v7(),
        symbols: vec!["AAPL".into(), "MSFT".into()],
        lookback_bars: 5,
    };
    let json_val = serde_json::to_value(&payload).unwrap();
    let deserialized: RefreshJobPayload = serde_json::from_value(json_val).unwrap();
    assert_eq!(deserialized.dataset_id, payload.dataset_id);
    assert_eq!(deserialized.symbols, vec!["AAPL", "MSFT"]);
    assert_eq!(deserialized.lookback_bars, 5);
    assert_eq!(PROVIDER_REFRESH_JOB_KIND, "provider_refresh");
}
