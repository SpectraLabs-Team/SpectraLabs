//! M35 Task 7: No look-ahead validation test across all 25 technical indicators.
//!
//! Verifies that for every supported indicator in `spectra_research::indicators`:
//! - Computing indicators on a historical prefix `bars[..T]` produces values
//!   that are strictly identical to computing on `bars[..T + future_bars]`, even
//!   when future bars are modified or subjected to extreme price shocks.
//! - No past indicator value (at indices `0..T`) ever changes as new bars arrive.

use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_research::indicators::*;

fn bar(ts: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
    Bar::new(
        ts,
        0,
        Price::from_f64(o).unwrap(),
        Price::from_f64(h).unwrap(),
        Price::from_f64(l).unwrap(),
        Price::from_f64(c).unwrap(),
        Qty::from_f64(v).unwrap(),
    )
}

fn generate_synthetic_bars(count: usize) -> Vec<Bar> {
    let mut bars = Vec::with_capacity(count);
    let mut current_price = 100.0;
    for i in 0..count {
        let delta = ((i as f64 * 0.17).sin() * 2.5) + ((i as f64 * 0.05).cos() * 1.2);
        let open = current_price;
        let close = (current_price + delta).max(1.0);
        let high = open.max(close) + 1.5 + (i as f64 * 0.3).sin().abs();
        let low = (open.min(close) - 1.5 - (i as f64 * 0.2).cos().abs()).max(0.5);
        let volume = 1000.0 + (i as f64 * 37.0 % 500.0);
        current_price = close;
        bars.push(bar(1_700_000_000 + i as i64 * 60, open, high, low, close, volume));
    }
    bars
}

fn closes(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| b.close.to_f64()).collect()
}

fn assert_series_equal(prefix: &[Option<f64>], full: &[Option<f64>], name: &str) {
    assert!(
        full.len() >= prefix.len(),
        "{}: full length ({}) must be >= prefix length ({})",
        name,
        full.len(),
        prefix.len()
    );
    for i in 0..prefix.len() {
        match (prefix[i], full[i]) {
            (None, None) => {}
            (Some(p), Some(f)) => {
                let diff = (p - f).abs();
                assert!(
                    diff < 1e-12,
                    "Look-ahead detected in {} at bar index {}: prefix gave {:.12}, extended gave {:.12} (diff = {})",
                    name,
                    i,
                    p,
                    f,
                    diff
                );
            }
            (p, f) => {
                panic!(
                    "Look-ahead detected in {} at bar index {}: prefix gave {:?}, extended gave {:?}",
                    name, i, p, f
                );
            }
        }
    }
}

fn assert_f64_series_equal(prefix: &[f64], full: &[f64], name: &str) {
    assert!(
        full.len() >= prefix.len(),
        "{}: full length ({}) must be >= prefix length ({})",
        name,
        full.len(),
        prefix.len()
    );
    for i in 0..prefix.len() {
        let diff = (prefix[i] - full[i]).abs();
        assert!(
            diff < 1e-12,
            "Look-ahead detected in {} at bar index {}: prefix gave {:.12}, extended gave {:.12}",
            name,
            i,
            prefix[i],
            full[i]
        );
    }
}

/// Tests that mutating future bars does not change any past indicator value across all 25 indicators.
#[test]
fn test_all_25_indicators_have_zero_lookahead() {
    let prefix_len = 70;
    let total_len = 120;
    let base_bars = generate_synthetic_bars(total_len);

    let prefix_bars = &base_bars[..prefix_len];
    let prefix_c = closes(prefix_bars);

    // Create mutated future scenarios:
    // Scenario 1: Future bars with 10x price shock and 100x volume surge
    let mut mutated_future_bars = base_bars.clone();
    for i in prefix_len..total_len {
        let b = &base_bars[i];
        let mult = 10.0;
        mutated_future_bars[i] = bar(
            b.ts_ns,
            b.open.to_f64() * mult,
            b.high.to_f64() * mult,
            b.low.to_f64() * mult,
            b.close.to_f64() * mult,
            b.volume.to_f64() * 100.0,
        );
    }
    let mutated_c = closes(&mutated_future_bars);

    // Scenario 2: Future bars with 90% price crash
    let mut crashed_future_bars = base_bars.clone();
    for i in prefix_len..total_len {
        let b = &base_bars[i];
        let mult = 0.1;
        crashed_future_bars[i] = bar(
            b.ts_ns,
            b.open.to_f64() * mult,
            b.high.to_f64() * mult,
            b.low.to_f64() * mult,
            b.close.to_f64() * mult,
            1.0,
        );
    }
    let crashed_c = closes(&crashed_future_bars);

    for (scenario_name, ext_bars, ext_c) in [
        ("Base extension", &base_bars, &closes(&base_bars)),
        ("Price shock scenario", &mutated_future_bars, &mutated_c),
        ("Price crash scenario", &crashed_future_bars, &crashed_c),
    ] {
        // 1. SMA
        let p_sma = sma(&prefix_c, 20);
        let f_sma = sma(ext_c, 20);
        assert_series_equal(&p_sma, &f_sma, &format!("SMA ({})", scenario_name));

        // 2. EMA
        let p_ema = ema(&prefix_c, 20);
        let f_ema = ema(ext_c, 20);
        assert_series_equal(&p_ema, &f_ema, &format!("EMA ({})", scenario_name));

        // 3. WMA
        let p_wma = wma(&prefix_c, 20);
        let f_wma = wma(ext_c, 20);
        assert_series_equal(&p_wma, &f_wma, &format!("WMA ({})", scenario_name));

        // 4. DEMA
        let p_dema = dema(&prefix_c, 20);
        let f_dema = dema(ext_c, 20);
        assert_series_equal(&p_dema, &f_dema, &format!("DEMA ({})", scenario_name));

        // 5. TEMA
        let p_tema = tema(&prefix_c, 20);
        let f_tema = tema(ext_c, 20);
        assert_series_equal(&p_tema, &f_tema, &format!("TEMA ({})", scenario_name));

        // 6. Bollinger Bands
        let p_bb = bollinger_bands(&prefix_c, 20, 2.0);
        let f_bb = bollinger_bands(ext_c, 20, 2.0);
        assert_series_equal(&p_bb.middle, &f_bb.middle, &format!("BB Middle ({})", scenario_name));
        assert_series_equal(&p_bb.upper, &f_bb.upper, &format!("BB Upper ({})", scenario_name));
        assert_series_equal(&p_bb.lower, &f_bb.lower, &format!("BB Lower ({})", scenario_name));
        assert_series_equal(&p_bb.percent_b, &f_bb.percent_b, &format!("BB %B ({})", scenario_name));
        assert_series_equal(&p_bb.bandwidth, &f_bb.bandwidth, &format!("BB Bandwidth ({})", scenario_name));

        // 7. Ichimoku
        let p_ich = ichimoku(prefix_bars, 9, 26, 52);
        let f_ich = ichimoku(ext_bars, 9, 26, 52);
        assert_series_equal(
            &p_ich.tenkan_sen,
            &f_ich.tenkan_sen,
            &format!("Ichimoku Tenkan ({})", scenario_name),
        );
        assert_series_equal(
            &p_ich.kijun_sen,
            &f_ich.kijun_sen,
            &format!("Ichimoku Kijun ({})", scenario_name),
        );
        assert_series_equal(
            &p_ich.senkou_span_a,
            &f_ich.senkou_span_a,
            &format!("Ichimoku Span A ({})", scenario_name),
        );
        assert_series_equal(
            &p_ich.senkou_span_b,
            &f_ich.senkou_span_b,
            &format!("Ichimoku Span B ({})", scenario_name),
        );
        assert_series_equal(
            &p_ich.chikou_span,
            &f_ich.chikou_span,
            &format!("Ichimoku Chikou ({})", scenario_name),
        );

        // 8. Parabolic SAR
        let p_sar = parabolic_sar(prefix_bars, 0.02, 0.02, 0.2);
        let f_sar = parabolic_sar(ext_bars, 0.02, 0.02, 0.2);
        assert_series_equal(&p_sar, &f_sar, &format!("Parabolic SAR ({})", scenario_name));

        // 9. Donchian Channels
        let p_don = donchian_channels(prefix_bars, 20);
        let f_don = donchian_channels(ext_bars, 20);
        assert_series_equal(&p_don.upper, &f_don.upper, &format!("Donchian Upper ({})", scenario_name));
        assert_series_equal(&p_don.middle, &f_don.middle, &format!("Donchian Middle ({})", scenario_name));
        assert_series_equal(&p_don.lower, &f_don.lower, &format!("Donchian Lower ({})", scenario_name));

        // 10. Keltner Channels
        let p_kelt = keltner_channels(prefix_bars, 20, 2.0);
        let f_kelt = keltner_channels(ext_bars, 20, 2.0);
        assert_series_equal(&p_kelt.upper, &f_kelt.upper, &format!("Keltner Upper ({})", scenario_name));
        assert_series_equal(&p_kelt.middle, &f_kelt.middle, &format!("Keltner Middle ({})", scenario_name));
        assert_series_equal(&p_kelt.lower, &f_kelt.lower, &format!("Keltner Lower ({})", scenario_name));

        // 11. VWAP
        let p_vwap = vwap(prefix_bars);
        let f_vwap = vwap(ext_bars);
        assert_series_equal(&p_vwap, &f_vwap, &format!("VWAP ({})", scenario_name));

        // 12. Typical Price
        let p_typ = typical_price(prefix_bars);
        let f_typ = typical_price(ext_bars);
        assert_f64_series_equal(&p_typ, &f_typ, &format!("Typical Price ({})", scenario_name));

        // 13. Median Price
        let p_med = median_price(prefix_bars);
        let f_med = median_price(ext_bars);
        assert_f64_series_equal(&p_med, &f_med, &format!("Median Price ({})", scenario_name));

        // 14. RSI
        let p_rsi = rsi(&prefix_c, 14);
        let f_rsi = rsi(ext_c, 14);
        assert_series_equal(&p_rsi, &f_rsi, &format!("RSI ({})", scenario_name));

        // 15. MACD
        let p_macd = macd(&prefix_c, 12, 26, 9);
        let f_macd = macd(ext_c, 12, 26, 9);
        assert_series_equal(&p_macd.macd, &f_macd.macd, &format!("MACD ({})", scenario_name));
        assert_series_equal(&p_macd.signal, &f_macd.signal, &format!("MACD Signal ({})", scenario_name));
        assert_series_equal(&p_macd.histogram, &f_macd.histogram, &format!("MACD Hist ({})", scenario_name));

        // 16. Stochastic
        let p_stoch = stochastic(prefix_bars, 14, 3, 3);
        let f_stoch = stochastic(ext_bars, 14, 3, 3);
        assert_series_equal(&p_stoch.percent_k, &f_stoch.percent_k, &format!("Stoch %K ({})", scenario_name));
        assert_series_equal(&p_stoch.percent_d, &f_stoch.percent_d, &format!("Stoch %D ({})", scenario_name));
        assert_series_equal(&p_stoch.slow_d, &f_stoch.slow_d, &format!("Stoch Slow %D ({})", scenario_name));

        // 17. CCI
        let p_cci = cci(prefix_bars, 20);
        let f_cci = cci(ext_bars, 20);
        assert_series_equal(&p_cci, &f_cci, &format!("CCI ({})", scenario_name));

        // 18. Williams %R
        let p_willr = williams_r(prefix_bars, 14);
        let f_willr = williams_r(ext_bars, 14);
        assert_series_equal(&p_willr, &f_willr, &format!("Williams %R ({})", scenario_name));

        // 19. Momentum
        let p_mom = momentum(&prefix_c, 10);
        let f_mom = momentum(ext_c, 10);
        assert_series_equal(&p_mom, &f_mom, &format!("Momentum ({})", scenario_name));

        // 20. Rate of Change
        let p_roc = rate_of_change(&prefix_c, 10);
        let f_roc = rate_of_change(ext_c, 10);
        assert_series_equal(&p_roc, &f_roc, &format!("Rate of Change ({})", scenario_name));

        // 21. ATR
        let p_atr = atr(prefix_bars, 14);
        let f_atr = atr(ext_bars, 14);
        assert_series_equal(&p_atr, &f_atr, &format!("ATR ({})", scenario_name));

        // 22. ADX
        let p_adx = adx(prefix_bars, 14);
        let f_adx = adx(ext_bars, 14);
        assert_series_equal(&p_adx.adx, &f_adx.adx, &format!("ADX ({})", scenario_name));
        assert_series_equal(&p_adx.plus_di, &f_adx.plus_di, &format!("ADX +DI ({})", scenario_name));
        assert_series_equal(&p_adx.minus_di, &f_adx.minus_di, &format!("ADX -DI ({})", scenario_name));

        // 23. OBV
        let p_obv = obv(prefix_bars);
        let f_obv = obv(ext_bars);
        assert_series_equal(&p_obv, &f_obv, &format!("OBV ({})", scenario_name));

        // 24. Chaikin Money Flow
        let p_cmf = chaikin_money_flow(prefix_bars, 20);
        let f_cmf = chaikin_money_flow(ext_bars, 20);
        assert_series_equal(&p_cmf, &f_cmf, &format!("CMF ({})", scenario_name));

        // 25. MFI
        let p_mfi = mfi(prefix_bars, 14);
        let f_mfi = mfi(ext_bars, 14);
        assert_series_equal(&p_mfi, &f_mfi, &format!("MFI ({})", scenario_name));

        // Additional: Standard Deviation
        let p_std = std_dev(&prefix_c, 20);
        let f_std = std_dev(ext_c, 20);
        assert_series_equal(&p_std, &f_std, &format!("Std Dev ({})", scenario_name));
    }
}

/// Tests that incremental bar-by-bar evaluation produces identical values at each step.
#[test]
fn test_incremental_stream_consistency() {
    let bars = generate_synthetic_bars(80);
    let full_c = closes(&bars);

    let full_sma = sma(&full_c, 15);
    let full_rsi = rsi(&full_c, 14);
    let full_atr = atr(&bars, 14);

    for t in 30..80 {
        let prefix_bars = &bars[..=t];
        let prefix_c = closes(prefix_bars);

        let step_sma = sma(&prefix_c, 15);
        let step_rsi = rsi(&prefix_c, 14);
        let step_atr = atr(prefix_bars, 14);

        assert_eq!(step_sma[t], full_sma[t], "SMA step mismatch at bar {}", t);
        assert_eq!(step_rsi[t], full_rsi[t], "RSI step mismatch at bar {}", t);
        assert_eq!(step_atr[t], full_atr[t], "ATR step mismatch at bar {}", t);
    }
}
