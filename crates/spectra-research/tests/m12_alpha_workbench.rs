//! Tests for M12 Alpha Workbench (Tasks 1-5, Leakage, Known Answers, Determinism).

use spectra_research::alpha::*;

/// Task 1 Test: Leakage Prevention.
/// Changing a bar after date t must not change the signal or its forward return evaluated from t.
#[test]
fn test_leakage_prevention_signal_and_forward_returns() {
    let dates = ["2026-01-01", "2026-01-02", "2026-01-03", "2026-01-04", "2026-01-05", "2026-01-06"];

    let make_bars = |alter_future: bool| -> Vec<PriceBar> {
        let mut bars = Vec::new();
        let ts: i64 = 1_700_000_000_000_000_000;
        let day_ns: i64 = 86_400_000_000_000;

        for (idx, date) in dates.iter().enumerate() {
            let t = ts + (idx as i64) * day_ns;
            // Baseline closes
            let mut close_aapl = 100.0 + (idx as f64) * 2.0;
            let mut close_msft = 200.0 + (idx as f64) * 1.5;

            // Alter bars after date t = 2026-01-03 (idx > 2)
            if alter_future && idx > 2 {
                close_aapl *= 1.5;
                close_msft *= 0.5;
            }

            bars.push(PriceBar::new("AAPL", *date, close_aapl, t));
            bars.push(PriceBar::new("MSFT", *date, close_msft, t));
        }
        bars
    };

    let bars_original = make_bars(false);
    let bars_altered_after_t = make_bars(true);

    // 1. Signal at date t = "2026-01-03" (idx = 2) with 2-day lookback
    let mom_sig = BuiltinSignal::Momentum { lookback_days: 2 };
    let sigs_orig = generate_builtin_signal(&mom_sig, &bars_original).unwrap();
    let sigs_alt = generate_builtin_signal(&mom_sig, &bars_altered_after_t).unwrap();

    let sig_t_orig: Vec<_> = sigs_orig.iter().filter(|s| s.date == "2026-01-03").collect();
    let sig_t_alt: Vec<_> = sigs_alt.iter().filter(|s| s.date == "2026-01-03").collect();

    assert_eq!(sig_t_orig.len(), 2);
    assert_eq!(sig_t_alt.len(), 2);
    for (o, a) in sig_t_orig.iter().zip(sig_t_alt.iter()) {
        assert_eq!(o.symbol, a.symbol);
        assert!((o.value - a.value).abs() < 1e-12, "signal at date t changed when altering future bars!");
    }

    // 2. Forward returns evaluated at date t for 1-day horizon (uses t and t+1):
    // Altering bar at t+2, t+3 must not change forward return from t for horizon = 1.
    let fwd_orig = compute_forward_returns(&bars_original, &[1]);
    let fwd_alt = compute_forward_returns(&bars_altered_after_t, &[1]);

    // For date 2026-01-01 (horizon 1 uses only 2026-01-01 and 2026-01-02, strictly before idx 2 alteration)
    let ret_orig = fwd_orig.get(&("AAPL".to_string(), "2026-01-01".to_string())).unwrap().get(&1).unwrap();
    let ret_alt = fwd_alt.get(&("AAPL".to_string(), "2026-01-01".to_string())).unwrap().get(&1).unwrap();
    assert!((ret_orig - ret_alt).abs() < 1e-12, "forward return from t leaked future changes!");
}

/// Task 2 Test: Information Coefficient on synthetic dataset with known rank correlation.
#[test]
fn test_known_information_coefficient() {
    // Exact Spearman correlation test
    // Perfect rank agreement: [10, 20, 30, 40, 50] and [1.1, 2.2, 3.3, 4.4, 5.5] -> IC = 1.0
    let x = [10.0, 20.0, 30.0, 40.0, 50.0];
    let y = [1.1, 2.2, 3.3, 4.4, 5.5];
    let ic_perfect = spearman_correlation(&x, &y).unwrap();
    assert!((ic_perfect - 1.0).abs() < 1e-12);

    // Exact inverse rank agreement: -> IC = -1.0
    let y_inv = [5.5, 4.4, 3.3, 2.2, 1.1];
    let ic_inv = spearman_correlation(&x, &y_inv).unwrap();
    assert!((ic_inv - (-1.0)).abs() < 1e-12);

    // Known synthetic correlation with ties
    let x_ties = [1.0, 2.0, 2.0, 4.0, 5.0];
    let y_ties = [10.0, 25.0, 25.0, 40.0, 50.0];
    let ic_ties = spearman_correlation(&x_ties, &y_ties).unwrap();
    assert!((ic_ties - 1.0).abs() < 1e-12);

    // Synthetic universe across 20 dates where signals and forward returns have designed rank correlation
    let symbols = ["S1", "S2", "S3", "S4", "S5"];
    let mut signals = Vec::new();
    let mut bars = Vec::new();
    let day_ns: i64 = 86_400_000_000_000;

    for d in 0..20 {
        let date_str = format!("2026-01-{:02}", d + 1);
        let t = 1_700_000_000_000_000_000 + i64::from(d) * day_ns;

        // Symbols sorted by rank 1..5
        for (i, sym) in symbols.iter().enumerate() {
            let sig_val = (i + 1) as f64 * 10.0;
            signals.push(SignalValue::new(*sym, &date_str, sig_val));

            // Price series such that 1-day forward return is monotonic in i
            let close = 100.0 + (i as f64) * f64::from(d);
            bars.push(PriceBar::new(*sym, &date_str, close, t));
        }
    }

    // Next day bar for day 20 to allow 1-day forward return for all 20 days
    let t_next = 1_700_000_000_000_000_000 + 20 * day_ns;
    for (i, sym) in symbols.iter().enumerate() {
        let close = 100.0 + (i as f64) * 20.0 + (i as f64 + 1.0) * 5.0;
        bars.push(PriceBar::new(*sym, "2026-01-21", close, t_next));
    }

    let report = run_signal_test(
        "synthetic_alpha",
        &signals,
        &bars,
        &[1],
        1,
        5,
        None,
        Some("dv-synthetic-01".into()),
        1,
    )
    .unwrap();

    assert!(report.primary_ic.mean_ic > 0.0);
    assert!(report.primary_ic.date_count > 0);
}

/// Task 3 Test: Decay Analysis with Designed Half-Life.
#[test]
fn test_decay_half_life_known_answer() {
    // Design a synthetic scenario where IC decays exponentially with half-life = 5.0 trading days.
    // IC(h) = IC_0 * 2^(-h / 5.0)
    let designed_half_life = 5.0;
    let ic_0 = 0.10;

    let horizons = [1, 5, 10, 20];
    let mut horizon_metrics = Vec::new();

    for &h in &horizons {
        let expected_ic = ic_0 * (2.0f64).powf(-(h as f64) / designed_half_life);
        horizon_metrics.push(IcSummary {
            horizon: h,
            mean_ic: expected_ic,
            ic_stdev: 0.02,
            t_stat: 3.5,
            date_count: 50,
            daily_ics: Vec::new(),
        });
    }

    let estimated_half_life = fit_signal_half_life(&horizon_metrics);
    assert!(
        (estimated_half_life - designed_half_life).abs() < 1e-4,
        "estimated half-life {estimated_half_life} did not match designed half-life {designed_half_life}"
    );
}

/// Task 4 Test: Quantile Spread and Turnover Known Answers.
#[test]
fn test_quantile_spread_and_turnover_known_answers() {
    let day_ns: i64 = 86_400_000_000_000;

    let mut signals = Vec::new();
    let mut bars = Vec::new();

    // Day 1: Order A < B < C < D < E (Q_top = E, Q_bottom = A)
    // Day 2: Order A < B < C < E < D (Q_top = D, Q_bottom = A) -> 100% turnover in top quantile!
    let day1_date = "2026-02-01";
    let day2_date = "2026-02-02";
    let day3_date = "2026-02-03";

    let t1 = 1_700_000_000_000_000_000;
    let t2 = t1 + day_ns;
    let t3 = t2 + day_ns;

    // Day 1 signals
    signals.push(SignalValue::new("A", day1_date, 1.0));
    signals.push(SignalValue::new("B", day1_date, 2.0));
    signals.push(SignalValue::new("C", day1_date, 3.0));
    signals.push(SignalValue::new("D", day1_date, 4.0));
    signals.push(SignalValue::new("E", day1_date, 5.0));

    // Day 2 signals: D and E swapped
    signals.push(SignalValue::new("A", day2_date, 1.0));
    signals.push(SignalValue::new("B", day2_date, 2.0));
    signals.push(SignalValue::new("C", day2_date, 3.0));
    signals.push(SignalValue::new("E", day2_date, 4.0));
    signals.push(SignalValue::new("D", day2_date, 5.0));

    // Bars for Day 1
    bars.push(PriceBar::new("A", day1_date, 100.0, t1));
    bars.push(PriceBar::new("B", day1_date, 100.0, t1));
    bars.push(PriceBar::new("C", day1_date, 100.0, t1));
    bars.push(PriceBar::new("D", day1_date, 100.0, t1));
    bars.push(PriceBar::new("E", day1_date, 100.0, t1));

    // Bars for Day 2:
    // A forward return on day 1: (95 - 100)/100 = -0.05
    // E forward return on day 1: (110 - 100)/100 = +0.10
    // Spread day 1 = 0.10 - (-0.05) = 0.15
    bars.push(PriceBar::new("A", day2_date, 95.0, t2));
    bars.push(PriceBar::new("B", day2_date, 100.0, t2));
    bars.push(PriceBar::new("C", day2_date, 100.0, t2));
    bars.push(PriceBar::new("D", day2_date, 105.0, t2));
    bars.push(PriceBar::new("E", day2_date, 110.0, t2));

    // Bars for Day 3:
    // A forward return on day 2: (90 - 95)/95 = -0.0526315
    // D forward return on day 2: (115 - 105)/105 = +0.095238
    bars.push(PriceBar::new("A", day3_date, 90.0, t3));
    bars.push(PriceBar::new("B", day3_date, 100.0, t3));
    bars.push(PriceBar::new("C", day3_date, 100.0, t3));
    bars.push(PriceBar::new("D", day3_date, 115.0, t3));
    bars.push(PriceBar::new("E", day3_date, 108.0, t3));

    let report = run_signal_test("test_spread", &signals, &bars, &[1], 1, 5, None, None, 1).unwrap();

    assert_eq!(report.quantiles.quantile_count, 5);
    // Top-bottom spread should be positive and close to expected
    assert!(report.quantiles.top_bottom_spread > 0.10);

    // Turnover between Day 1 (top={E}) and Day 2 (top={D}) is exactly 1.0 (100%)
    assert!(
        (report.quantiles.top_quantile_turnover - 1.0).abs() < 1e-12,
        "expected 100% turnover, got {}",
        report.quantiles.top_quantile_turnover
    );
}

/// Task 5 Test: Determinism (Same inputs give exact same JSON).
#[test]
fn test_determinism_same_inputs_same_outputs() {
    let symbols = ["SYM1", "SYM2", "SYM3", "SYM4", "SYM5"];
    let mut signals = Vec::new();
    let mut bars = Vec::new();
    let day_ns: i64 = 86_400_000_000_000;

    for d in 0..10 {
        let date_str = format!("2026-03-{:02}", d + 1);
        let t = 1_700_000_000_000_000_000 + i64::from(d) * day_ns;
        for (i, sym) in symbols.iter().enumerate() {
            signals.push(SignalValue::new(*sym, &date_str, (i as f64) * 2.5));
            bars.push(PriceBar::new(*sym, &date_str, 50.0 + (i as f64) + f64::from(d), t));
        }
    }

    let report1 =
        run_signal_test("det_test", &signals, &bars, &[1, 5], 1, 5, None, Some("v1".into()), 2).unwrap();
    let report2 =
        run_signal_test("det_test", &signals, &bars, &[1, 5], 1, 5, None, Some("v1".into()), 2).unwrap();

    let json1 = report1.to_json().dump();
    let json2 = report2.to_json().dump();

    assert_eq!(json1, json2, "reports are not deterministic across runs!");
}
