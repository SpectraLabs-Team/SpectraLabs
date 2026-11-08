//! Integration tests for M3: streaming backtest progress and single currency recording.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use spectra_core::{Bar, Money, Price, Qty, Rng, Side, SCALE};
use spectra_engine::{BacktestConfig, Context, Progress, RunControl, Strategy};

fn bars(n: usize, instruments: u32, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = vec![100 * SCALE; instruments as usize];
    let mut out = Vec::with_capacity(n * instruments as usize);
    for i in 0..n {
        for id in 0..instruments {
            let open = px[id as usize];
            let close = (open + rng.range(-SCALE, SCALE + SCALE / 20)).max(SCALE);
            let high = open.max(close) + rng.range(0, SCALE / 2);
            let low = (open.min(close) - rng.range(0, SCALE / 2)).max(1);
            px[id as usize] = close;
            out.push(Bar {
                ts_ns: (i as i64 + 1) * 86_400_000_000_000,
                known_at_ns: (i as i64 + 1) * 86_400_000_000_000,
                instrument: id + 1,
                open: Price::from_raw(open),
                high: Price::from_raw(high),
                low: Price::from_raw(low),
                close: Price::from_raw(close),
                volume: Qty::from_raw(500_000 * SCALE),
            });
        }
    }
    out
}

struct DummyStrategy;
impl Strategy for DummyStrategy {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            if ctx.position(b.instrument).is_zero() {
                ctx.market(b.instrument, Side::Buy, Qty::from_raw(10 * SCALE));
            }
        }
    }
}

#[test]
fn test_m3_streaming_bar_by_bar_progress() {
    let data = bars(50, 1, 42);
    let cancel = AtomicBool::new(false);
    let progress_points = Arc::new(Mutex::new(Vec::<Progress>::new()));
    let pp = Arc::clone(&progress_points);

    let mut report = move |p: Progress| {
        pp.lock().unwrap().push(p);
    };

    let cfg =
        BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), seed: 42, ..Default::default() };

    let mut ctl = RunControl {
        cancel: Some(&cancel),
        progress: Some(&mut report),
        progress_every: 1, // Bar-by-bar streaming progress
        ..Default::default()
    };

    let mut strat = DummyStrategy;
    let res = spectra_engine::run_backtest_controlled(&data, &mut strat, &cfg, &mut ctl).unwrap();
    assert!(res.fill_count() > 0);

    let points = progress_points.lock().unwrap().clone();
    assert!(!points.is_empty(), "Progress points must be captured bar by bar");
    // Verify monotonic progress and that multiple points were delivered during execution
    assert!(
        points.len() >= 40,
        "Expected at least 40 progress calls for 50 bars with progress_every=1, got {}",
        points.len()
    );
    for w in points.windows(2) {
        assert!(w[0].bars_done <= w[1].bars_done, "Bars done must be non-decreasing");
        assert!(w[0].fraction() <= w[1].fraction(), "Progress fraction must be monotonic");
    }
}

#[test]
fn test_m3_every_run_records_single_currency() {
    use spectra_backtest::{run_request_on, BacktestRequest};
    use spectra_research::ParamSet;

    let data = bars(20, 1, 123);
    let symbols = vec!["SPY".to_string()];
    let req = BacktestRequest::new("buy_and_hold", &ParamSet::default(), BacktestConfig::default()).unwrap();

    let mut ctl = RunControl::default();
    let out = run_request_on(&data, &symbols, &req, &mut ctl).unwrap();
    let summary = spectra_backtest::summary_json(&out).unwrap();

    // Verify top-level currency and run currency are both recorded and default to USD
    assert_eq!(summary.find("currency").unwrap().as_str().unwrap(), "USD");
    assert_eq!(summary.find("run").unwrap().find("currency").unwrap().as_str().unwrap(), "USD");
    assert_eq!(summary.find("run").unwrap().find("base_currency").unwrap().as_str().unwrap(), "USD");

    // Also verify request JSON serialization records currency
    let req_json = req.to_json().unwrap();
    assert_eq!(req_json.find("currency").unwrap().as_str().unwrap(), "USD");
    assert_eq!(req_json.find("base_currency").unwrap().as_str().unwrap(), "USD");
}
