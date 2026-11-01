//! M26 task 3e (part 5a): boundary tests for the backtest-to-paper execution gap
//! alerts (`src/gap.rs`, `compute_backtest_to_paper_gap`).
//!
//! The defaults are 10 bps slippage drift, 1,000 ms latency, 3% tracking error
//! and a 0.95 fill ratio. The code fires slippage and latency alerts on a strict
//! "above" comparison and the fill-ratio alert on "below", so each threshold is
//! checked exactly at its boundary (no alert) and just past it (alert). Each case
//! uses one matched trade, so the means equal the single values.
//!
//! Not covered here: the tracking-error boundary (its definition is a
//! documented approximation, and it's the subject of a later check), and the
//! survival lab, which is a separate part.

use spectra_research::gap::{
    compute_backtest_to_paper_gap, BacktestFillRecord, GapAlert, GapAlertStatus, GapThresholds, GapTradeSide,
    PaperFillRecord,
};

const BACKTEST_TS: i64 = 10_000_000_000;

/// One matched buy: backtest qty 10 at 100, paper qty `paper_qty` at 100, paper
/// fill `latency_ns` after the backtest fill, with the given slippage in bps.
fn run(paper_qty: f64, paper_slippage_bps: f64, latency_ns: i64) -> Vec<GapAlert> {
    let paper = PaperFillRecord {
        symbol: "AAPL".into(),
        side: GapTradeSide::Buy,
        qty: paper_qty,
        price: 100.0,
        timestamp_ns: BACKTEST_TS + latency_ns,
        fee: 0.0,
        slippage_bps: paper_slippage_bps,
        order_id: None,
    };
    let backtest = BacktestFillRecord {
        symbol: "AAPL".into(),
        side: GapTradeSide::Buy,
        qty: 10.0,
        price: 100.0,
        timestamp_ns: BACKTEST_TS,
        fee: 0.0,
        slippage_bps: 0.0,
        bar_price: 100.0,
        order_id: None,
    };
    compute_backtest_to_paper_gap(vec![paper], vec![backtest], &GapThresholds::default()).alerts
}

/// Returns an owned copy, so the caller can pass a temporary `run(...)` result.
fn alert(alerts: &[GapAlert], code: &str) -> Option<GapAlert> {
    alerts.iter().find(|a| a.code == code).cloned()
}

#[test]
fn slippage_drift_fires_only_above_ten_bps() {
    // Exactly 10 bps: the alert is strict, so it does not fire.
    assert!(alert(&run(10.0, 10.0, 0), "slippage_drift_exceeded").is_none(), "10 bps is at the boundary");
    // Just above: a warning (not yet 1.5x the threshold).
    let a = alert(&run(10.0, 10.01, 0), "slippage_drift_exceeded").expect("10.01 bps fires");
    assert_eq!(a.severity, GapAlertStatus::Warning);
    assert_eq!(a.threshold, 10.0);
}

#[test]
fn slippage_drift_breaches_at_one_and_a_half_times_the_threshold() {
    // 15 bps is 1.5x the threshold: still a warning (strict comparison).
    let at_15 = alert(&run(10.0, 15.0, 0), "slippage_drift_exceeded").expect("15 bps fires");
    assert_eq!(at_15.severity, GapAlertStatus::Warning);
    let above = alert(&run(10.0, 15.5, 0), "slippage_drift_exceeded").expect("15.5 bps fires");
    assert_eq!(above.severity, GapAlertStatus::Breach);
}

#[test]
fn latency_fires_only_above_one_second() {
    // Exactly 1,000 ms: no alert.
    assert!(
        alert(&run(10.0, 0.0, 1_000_000_000), "execution_latency_high").is_none(),
        "1000 ms is at the boundary"
    );
    // 1,000.5 ms: a warning.
    let warn = alert(&run(10.0, 0.0, 1_000_500_000), "execution_latency_high").expect("1000.5 ms fires");
    assert_eq!(warn.severity, GapAlertStatus::Warning);
    // Above 2 s (2,001 ms): a breach.
    let breach = alert(&run(10.0, 0.0, 2_001_000_000), "execution_latency_high").expect("2001 ms fires");
    assert_eq!(breach.severity, GapAlertStatus::Breach);
}

#[test]
fn fill_ratio_fires_only_below_point_nine_five() {
    // Paper 9.5 of backtest 10: ratio 0.95, not below the minimum.
    assert!(alert(&run(9.5, 0.0, 0), "fill_ratio_breach").is_none(), "0.95 is at the boundary");
    // Paper 9.4: ratio 0.94, a breach.
    let a = alert(&run(9.4, 0.0, 0), "fill_ratio_breach").expect("0.94 fires");
    assert_eq!(a.severity, GapAlertStatus::Breach);
}

#[test]
fn a_clean_match_raises_no_alerts() {
    let alerts = run(10.0, 0.0, 0);
    assert!(
        alerts.is_empty(),
        "no alerts expected, got {:?}",
        alerts.iter().map(|a| &a.code).collect::<Vec<_>>()
    );
}
