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

    let vol = volatility_from_bars("AAPL", &bars, 252.0).unwrap();
    let expected_vol = (6.048 / 59.0_f64).sqrt();
    assert!((vol - expected_vol).abs() < 1e-8, "vol {vol} must match expected {expected_vol}");
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

    let err = volatility_from_bars("AAPL", &bars, 252.0).unwrap_err();
    match err {
        Error::Data(msg) => {
            assert_eq!(msg, "insufficient price history for AAPL: need 60 bars, have 59");
        }
        other => panic!("expected Error::Data, got {other:?}"),
    }
}

#[test]
fn test_volatility_from_bars_invalid_periods_per_year_returns_error() {
    let day_ns = 86_400_000_000_000_i64;
    let bars: Vec<Bar> = (0..61)
        .map(|i| {
            let px = Price::from_f64(100.0).unwrap();
            Bar::new(day_ns * (i + 1), 0, px, px, px, px, Qty::ZERO)
        })
        .collect();

    for bad in [0.0, -252.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let err = volatility_from_bars("AAPL", &bars, bad).unwrap_err();
        match err {
            Error::InvalidArgument(msg) => {
                assert!(
                    msg.contains("periods_per_year must be positive and finite"),
                    "expected 'periods_per_year must be positive and finite' in message, got: {msg}"
                );
            }
            other => panic!("expected Error::InvalidArgument for {bad}, got {other:?}"),
        }
    }
}

#[test]
fn test_volatility_from_bars_rejects_non_positive_price_naming_symbol_and_bar_index() {
    let day_ns = 86_400_000_000_000_i64;
    let mut bars: Vec<Bar> = (0..61)
        .map(|i| {
            let px = Price::from_f64(100.0).unwrap();
            Bar::new(day_ns * (i + 1), 0, px, px, px, px, Qty::ZERO)
        })
        .collect();

    // Zero price at index 10
    bars[10].close = Price::ZERO;
    let err = volatility_from_bars("MSFT", &bars, 252.0).unwrap_err();
    match err {
        Error::Data(msg) => {
            assert!(
                msg.contains("MSFT") && msg.contains("10"),
                "expected symbol 'MSFT' and bar index '10' in error, got: {msg}"
            );
        }
        other => panic!("expected Error::Data, got {other:?}"),
    }

    // Zero price at index 0
    bars[10].close = Price::from_f64(100.0).unwrap();
    bars[0].close = Price::ZERO;
    let err_0 = volatility_from_bars("TSLA", &bars, 252.0).unwrap_err();
    match err_0 {
        Error::Data(msg) => {
            assert!(
                msg.contains("TSLA") && msg.contains('0'),
                "expected symbol 'TSLA' and bar index '0' in error, got: {msg}"
            );
        }
        other => panic!("expected Error::Data, got {other:?}"),
    }
}
