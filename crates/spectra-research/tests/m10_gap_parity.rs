//! M10 Task 6: Parity fixture for backtest-to-paper gap attribution buckets.
//!
//! One fixed backtest and paper pair with known gaps, asserting each attribution bucket:
//! - cost_gap (fees & borrow delta)
//! - slippage_gap (realized vs modeled spread)
//! - timing_delay_gap (price drift before fill)
//! - missed_fills_gap (unmatched fills and unexpected trades)
//! - total_pnl_gap (net discrepancy)
//!
//! Confirms that each saved report stores the thresholds it was judged against (M10 Task 3).

use spectra_research::gap::{
    compute_backtest_to_paper_gap, BacktestFillRecord, GapAlertStatus, GapThresholds, GapTradeSide,
    PaperFillRecord,
};

#[test]
fn test_m10_backtest_to_paper_gap_parity_fixture() {
    let paper = vec![
        PaperFillRecord {
            symbol: "AAPL".into(),
            side: GapTradeSide::Buy,
            qty: 100.0,
            price: 150.50,
            timestamp_ns: 250_000_000, // 250 ms execution latency
            fee: 3.00,
            slippage_bps: 12.0,
            order_id: Some("p-aapl-1".into()),
        },
        PaperFillRecord {
            symbol: "TSLA".into(),
            side: GapTradeSide::Buy,
            qty: 10.0,
            price: 200.00,
            timestamp_ns: 200_000_000,
            fee: 1.00,
            slippage_bps: 3.0,
            order_id: Some("p-tsla-1".into()),
        },
    ];

    let backtest = vec![
        BacktestFillRecord {
            symbol: "AAPL".into(),
            side: GapTradeSide::Buy,
            qty: 100.0,
            price: 150.00,
            timestamp_ns: 0,
            fee: 1.00,
            slippage_bps: 2.0,
            bar_price: 150.00,
            order_id: Some("b-aapl-1".into()),
        },
        BacktestFillRecord {
            symbol: "MSFT".into(),
            side: GapTradeSide::Buy,
            qty: 50.0,
            price: 300.00,
            timestamp_ns: 100_000_000,
            fee: 1.50,
            slippage_bps: 1.0,
            bar_price: 300.00,
            order_id: Some("b-msft-1".into()),
        },
    ];

    let thresholds = GapThresholds {
        max_slippage_drift_bps: 10.0,
        max_timing_latency_ms: 1000.0,
        max_tracking_error_pct: 3.0,
        min_fill_ratio: 0.95,
    };

    let report = compute_backtest_to_paper_gap(paper, backtest, &thresholds);

    // Assert each attribution bucket matches exact known fixture values
    assert_eq!(report.attribution.cost_gap, 2.0, "cost_gap attribution bucket");
    assert_eq!(report.attribution.slippage_gap, 15.0, "slippage_gap attribution bucket");
    assert_eq!(report.attribution.timing_delay_gap, 35.0, "timing_delay_gap attribution bucket");
    assert_eq!(report.attribution.missed_fills_gap, -13000.0, "missed_fills_gap attribution bucket");
    assert_eq!(report.attribution.total_pnl_gap, -13052.0, "total_pnl_gap attribution bucket");

    // Metrics
    assert_eq!(report.metrics.matched_trades, 1);
    assert_eq!(report.metrics.unmatched_paper_trades, 1);
    assert_eq!(report.metrics.unmatched_backtest_trades, 1);
    assert_eq!(report.metrics.mean_slippage_drift_bps, 10.0);
    assert_eq!(report.metrics.mean_timing_latency_ms, 250.0);
    assert!((report.metrics.fill_ratio - (2.0 / 3.0)).abs() < 1e-12);

    // Thresholds recorded in the report (Task 3)
    assert_eq!(report.thresholds.max_slippage_drift_bps, 10.0);
    assert_eq!(report.thresholds.max_timing_latency_ms, 1000.0);
    assert_eq!(report.thresholds.max_tracking_error_pct, 3.0);
    assert_eq!(report.thresholds.min_fill_ratio, 0.95);
    assert_eq!(report.thresholds, thresholds);

    // Breach status due to fill ratio 66.7% < 95%
    assert_eq!(report.alert_status, GapAlertStatus::Breach);

    // Serialization verification
    let json_dump = report.to_json().dump();
    assert!(json_dump.contains(r#""thresholds":{"max_slippage_drift_bps":10"#));
    assert!(json_dump.contains(r#""cost_gap":2"#));
    assert!(json_dump.contains(r#""slippage_gap":15"#));
    assert!(json_dump.contains(r#""timing_delay_gap":35"#));
    assert!(json_dump.contains(r#""missed_fills_gap":-13000"#));
    assert!(json_dump.contains(r#""total_pnl_gap":-13052"#));
}
