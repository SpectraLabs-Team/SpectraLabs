//! M26 task 7: pure, bars-based volatility function reference tests.

use spectra_core::{Bar, Error, Price, Qty};
use spectra_risk::volatility_from_bars;

#[test]
fn test_volatility_from_bars_daily_61_bars_hand_computed() {
    // Hand calculation for 61 bars (60 log returns):
    // 30 log returns of +0.02 and 30 log returns of -0.02.
    // Number of returns N = 60.
    // Sum of returns = 30 * (+0.02) + 30 * (-0.02) = 0.0.
    // Mean return r_bar = 0.0 / 60 = 0.0.
    // For each return r_i, (r_i - r_bar)^2 = (+/- 0.02)^2 = 0.0004.
    // Sum of squared deviations SS = 60 * 0.0004 = 0.024.
    // Sample variance (Bessel's correction, N - 1 = 59):
    //   s^2 = SS / (N - 1) = 0.024 / (60 - 1) = 0.024 / 59 ≈ 0.00040677966101694915.
    // Sample standard deviation:
    //   s = sqrt(0.024 / 59) ≈ 0.02016877936358455.
    // Annualized volatility for daily bars using sqrt(252):
    //   sigma_ann = s * sqrt(252)
    //             = sqrt(0.024 * 252 / 59)
    //             = sqrt(6.048 / 59)
    //             ≈ 0.32016944666265584.

    let mut prices = vec![100.0];
    for i in 0..60 {
        let r: f64 = if i < 30 { 0.02 } else { -0.02 };
        let last_px = *prices.last().unwrap();
        prices.push(last_px * r.exp());
    }
    assert_eq!(prices.len(), 61);

    let day_ns = 86_400_000_000_000_i64;
    let bars: Vec<Bar> = prices
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let px = Price::from_f64(p).unwrap();
            Bar::new(day_ns * (i as i64 + 1), 0, px, px, px, px, Qty::ZERO)
        })
        .collect();

    let vol = volatility_from_bars("AAPL", &bars, "daily").unwrap();
    let expected_vol = (6.048 / 59.0_f64).sqrt();
    assert!((vol - expected_vol).abs() < 1e-8, "vol {vol} must match expected {expected_vol}");

    // Also verify "1d" alias works identically
    let vol_1d = volatility_from_bars("AAPL", &bars, "1d").unwrap();
    assert_eq!(vol, vol_1d);
}

#[test]
fn test_volatility_from_bars_under_60_bars_returns_exact_error() {
    let day_ns = 86_400_000_000_000_i64;
    let bars: Vec<Bar> = (0..59)
        .map(|i| {
            let px = Price::from_f64(100.0).unwrap();
            Bar::new(day_ns * (i + 1), 0, px, px, px, px, Qty::ZERO)
        })
        .collect();
    assert_eq!(bars.len(), 59);

    let err = volatility_from_bars("AAPL", &bars, "daily").unwrap_err();
    match err {
        Error::Data(msg) => {
            assert_eq!(msg, "insufficient price history for AAPL: need 60 bars, have 59");
        }
        other => panic!("expected Error::Data, got {other:?}"),
    }
}

#[test]
fn test_volatility_from_bars_unknown_interval_returns_error() {
    let day_ns = 86_400_000_000_000_i64;
    let bars: Vec<Bar> = (0..61)
        .map(|i| {
            let px = Price::from_f64(100.0).unwrap();
            Bar::new(day_ns * (i + 1), 0, px, px, px, px, Qty::ZERO)
        })
        .collect();

    let err = volatility_from_bars("AAPL", &bars, "unknown_interval").unwrap_err();
    match err {
        Error::InvalidArgument(msg) => {
            assert!(
                msg.contains("unknown bar interval"),
                "expected 'unknown bar interval' in message, got: {msg}"
            );
        }
        other => panic!("expected Error::InvalidArgument, got {other:?}"),
    }
}
