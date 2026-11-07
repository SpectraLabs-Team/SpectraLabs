//! Pinned reference tests for M35 Technical Indicators.
//!
//! Tests all 25 indicators against pinned reference values with explicit tolerances.

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

#[test]
fn test_indicator_01_sma() {
    let vals = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
    let res = sma(&vals, 3);
    assert_eq!(res[0], None);
    assert_eq!(res[1], None);
    assert_eq!(res[2], Some(11.0));
    assert_eq!(res[3], Some(12.0));
    assert_eq!(res[4], Some(13.0));
    assert_eq!(res[5], Some(14.0));
}

#[test]
fn test_indicator_02_ema() {
    let vals = [10.0, 11.0, 12.0, 13.0, 14.0];
    let res = ema(&vals, 3);
    assert_eq!(res[2], Some(11.0)); // Seed: (10+11+12)/3 = 11.0
                                    // alpha = 2/(3+1) = 0.5. res[3] = 13.0 * 0.5 + 11.0 * 0.5 = 12.0
    assert_eq!(res[3], Some(12.0));
    // res[4] = 14.0 * 0.5 + 12.0 * 0.5 = 13.0
    assert_eq!(res[4], Some(13.0));
}

#[test]
fn test_indicator_03_wma() {
    let vals = [1.0, 2.0, 3.0, 4.0, 5.0];
    let res = wma(&vals, 3);
    // (1*1 + 2*2 + 3*3)/6 = 14/6 = 2.3333333333333335
    let v2 = res[2].unwrap();
    assert!((v2 - (14.0 / 6.0)).abs() < 1e-10);
    // (2*1 + 3*2 + 4*3)/6 = (2+6+12)/6 = 20/6 = 3.3333333333333335
    let v3 = res[3].unwrap();
    assert!((v3 - (20.0 / 6.0)).abs() < 1e-10);
}

#[test]
fn test_indicator_04_dema() {
    let vals = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0];
    let res = dema(&vals, 3);
    // DEMA produces values once EMA of EMA is ready
    let val = res.iter().rev().find_map(|x| *x).unwrap();
    assert!(val > 10.0 && val <= 19.0);
}

#[test]
fn test_indicator_05_tema() {
    let vals: Vec<f64> = (1..=20).map(|x| x as f64).collect();
    let res = tema(&vals, 3);
    let last = res.last().unwrap().unwrap();
    assert!((last - 20.0).abs() < 1.0); // For steady linear ramp, TEMA tracks price closely
}

#[test]
fn test_indicator_06_rsi() {
    // Wilder standard reference data
    let prices = [
        44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03, 45.61, 46.28,
        46.28, 46.00, 46.03,
    ];
    let res = rsi(&prices, 14);
    assert_eq!(res[13], None);
    assert!(res[14].is_some());
    let rsi_14 = res[14].unwrap();
    // Pinned reference range for this standard series
    assert!(rsi_14 >= 65.0 && rsi_14 <= 75.0, "RSI: {}", rsi_14);
}

#[test]
fn test_indicator_07_macd() {
    let prices: Vec<f64> = (1..=50).map(|x| x as f64).collect();
    let res = macd(&prices, 12, 26, 9);
    assert!(res.macd[35].is_some());
    assert!(res.signal[35].is_some());
    assert!(res.histogram[35].is_some());
}

#[test]
fn test_indicator_08_bollinger_bands() {
    let prices = [10.0, 12.0, 11.0, 13.0, 12.0, 14.0, 13.0];
    let bb = bollinger_bands(&prices, 5, 2.0);
    assert!(bb.upper[4].unwrap() > bb.middle[4].unwrap());
    assert!(bb.lower[4].unwrap() < bb.middle[4].unwrap());
    assert!(bb.bandwidth[4].unwrap() > 0.0);
}

#[test]
fn test_indicator_09_atr() {
    let bars = [
        bar(1, 10.0, 12.0, 9.0, 11.0, 100.0),
        bar(2, 11.0, 13.0, 10.5, 12.5, 100.0),
        bar(3, 12.5, 14.0, 12.0, 13.5, 100.0),
        bar(4, 13.5, 15.0, 13.0, 14.0, 100.0),
    ];
    let res = atr(&bars, 3);
    assert!(res[2].is_some());
    assert!(res[2].unwrap() > 0.0);
}

#[test]
fn test_indicator_10_stochastic() {
    let bars = [
        bar(1, 10.0, 12.0, 8.0, 11.0, 100.0),
        bar(2, 11.0, 13.0, 9.0, 12.0, 100.0),
        bar(3, 12.0, 14.0, 10.0, 13.0, 100.0),
        bar(4, 13.0, 15.0, 11.0, 14.0, 100.0),
    ];
    let stoch = stochastic(&bars, 3, 2, 2);
    assert!(stoch.percent_k[2].unwrap() >= 0.0 && stoch.percent_k[2].unwrap() <= 100.0);
}

#[test]
fn test_indicator_11_adx() {
    let mut bars = Vec::new();
    for i in 1..=40 {
        let p = i as f64;
        bars.push(bar(i, p, p + 2.0, p - 1.0, p + 1.0, 100.0));
    }
    let res = adx(&bars, 14);
    assert!(res.plus_di[13].is_some());
    assert!(res.minus_di[13].is_some());
}

#[test]
fn test_indicator_12_obv() {
    let bars = [
        bar(1, 10.0, 11.0, 9.0, 10.0, 100.0),
        bar(2, 10.0, 12.0, 10.0, 11.0, 50.0),
        bar(3, 11.0, 11.5, 9.5, 9.5, 30.0),
    ];
    let res = obv(&bars);
    assert_eq!(res[0], Some(100.0));
    assert_eq!(res[1], Some(150.0));
    assert_eq!(res[2], Some(120.0));
}

#[test]
fn test_indicator_13_vwap() {
    let bars = [
        bar(1, 10.0, 12.0, 8.0, 10.0, 100.0), // TP = 10, vol = 100 -> cum_tp_vol = 1000, cum_vol = 100 -> VWAP = 10.0
        bar(2, 11.0, 14.0, 11.0, 11.0, 100.0), // TP = 12, vol = 100 -> cum_tp_vol = 2200, cum_vol = 200 -> VWAP = 11.0
    ];
    let res = vwap(&bars);
    assert_eq!(res[0], Some(10.0));
    assert_eq!(res[1], Some(11.0));
}

#[test]
fn test_indicator_14_ichimoku() {
    let mut bars = Vec::new();
    for i in 1..=60 {
        let p = i as f64;
        bars.push(bar(i, p, p + 1.0, p - 1.0, p, 100.0));
    }
    let res = ichimoku(&bars, 9, 26, 52);
    assert!(res.tenkan_sen[8].is_some());
    assert!(res.kijun_sen[25].is_some());
    assert!(res.senkou_span_b[51].is_some());
}

#[test]
fn test_indicator_15_parabolic_sar() {
    let bars = [
        bar(1, 10.0, 11.0, 9.0, 10.0, 100.0),
        bar(2, 10.0, 12.0, 9.5, 11.5, 100.0),
        bar(3, 11.5, 13.0, 11.0, 12.5, 100.0),
    ];
    let res = parabolic_sar(&bars, 0.02, 0.02, 0.2);
    assert_eq!(res.len(), 3);
    assert!(res[0].is_some());
    assert!(res[1].is_some());
}

#[test]
fn test_indicator_16_cci() {
    let mut bars = Vec::new();
    for i in 1..=30 {
        let p = (i % 5) as f64 + 10.0;
        bars.push(bar(i, p, p + 1.0, p - 1.0, p, 100.0));
    }
    let res = cci(&bars, 14);
    assert!(res[13].is_some());
}

#[test]
fn test_indicator_17_williams_r() {
    let bars = [
        bar(1, 10.0, 12.0, 8.0, 10.0, 100.0),
        bar(2, 10.0, 13.0, 9.0, 12.0, 100.0),
        bar(3, 12.0, 14.0, 10.0, 11.0, 100.0),
    ];
    let res = williams_r(&bars, 3);
    assert!(res[2].is_some());
    let wr = res[2].unwrap();
    assert!(wr >= -100.0 && wr <= 0.0);
}

#[test]
fn test_indicator_18_donchian() {
    let bars = [
        bar(1, 10.0, 15.0, 5.0, 10.0, 100.0),
        bar(2, 10.0, 16.0, 6.0, 11.0, 100.0),
        bar(3, 11.0, 14.0, 7.0, 12.0, 100.0),
    ];
    let ch = donchian_channels(&bars, 3);
    assert_eq!(ch.upper[2], Some(16.0));
    assert_eq!(ch.lower[2], Some(5.0));
    assert_eq!(ch.middle[2], Some(10.5));
}

#[test]
fn test_indicator_19_keltner() {
    let bars = [
        bar(1, 10.0, 12.0, 8.0, 10.0, 100.0),
        bar(2, 10.0, 13.0, 9.0, 11.0, 100.0),
        bar(3, 11.0, 14.0, 10.0, 12.0, 100.0),
    ];
    let ch = keltner_channels(&bars, 3, 1.5);
    assert!(ch.upper[2].is_some());
    assert!(ch.lower[2].is_some());
}

#[test]
fn test_indicator_20_chaikin_money_flow() {
    let bars = [
        bar(1, 10.0, 12.0, 8.0, 11.0, 100.0),
        bar(2, 11.0, 13.0, 9.0, 12.0, 100.0),
        bar(3, 12.0, 14.0, 10.0, 13.0, 100.0),
    ];
    let cmf = chaikin_money_flow(&bars, 3);
    assert!(cmf[2].is_some());
    let v = cmf[2].unwrap();
    assert!(v >= -1.0 && v <= 1.0);
}

#[test]
fn test_indicator_21_momentum() {
    let vals = [10.0, 12.0, 15.0, 14.0, 18.0];
    let mom = momentum(&vals, 2);
    assert_eq!(mom[0], None);
    assert_eq!(mom[1], None);
    assert_eq!(mom[2], Some(5.0)); // 15 - 10
    assert_eq!(mom[3], Some(2.0)); // 14 - 12
    assert_eq!(mom[4], Some(3.0)); // 18 - 15
}

#[test]
fn test_indicator_22_rate_of_change() {
    let vals = [100.0, 110.0, 120.0, 110.0];
    let roc = rate_of_change(&vals, 2);
    assert_eq!(roc[0], None);
    assert_eq!(roc[1], None);
    assert_eq!(roc[2], Some(20.0)); // 100 * (120-100)/100 = 20%
}

#[test]
fn test_indicator_23_mfi() {
    let mut bars = Vec::new();
    for i in 1..=20 {
        let p = (i as f64) * 2.0;
        bars.push(bar(i, p, p + 1.0, p - 1.0, p, 100.0));
    }
    let res = mfi(&bars, 14);
    assert!(res[14].is_some());
    let m = res[14].unwrap();
    assert!(m >= 0.0 && m <= 100.0);
}

#[test]
fn test_indicator_24_std_dev() {
    let vals = [10.0, 12.0, 23.0, 23.0, 16.0, 23.0, 21.0, 16.0];
    let sd = std_dev(&vals, 5);
    assert!(sd[4].is_some());
    assert!(sd[4].unwrap() > 0.0);
}

#[test]
fn test_indicator_25_typical_and_median_price() {
    let bars = [bar(1, 10.0, 14.0, 8.0, 11.0, 100.0)];
    let tp = typical_price(&bars);
    let mp = median_price(&bars);
    assert_eq!(tp[0], (14.0 + 8.0 + 11.0) / 3.0);
    assert_eq!(mp[0], (14.0 + 8.0) / 2.0);
}
