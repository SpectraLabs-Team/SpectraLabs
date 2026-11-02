//! M26 task 7: reference tests for strategy error reporting channel via Context::fail.

use spectra_core::{Bar, Error, InstrumentId, Price, Qty};
use spectra_engine::{run_backtest, BacktestConfig, Context, Strategy};

fn make_bar(ts: i64, id: InstrumentId, price: f64) -> Bar {
    let p = Price::from_f64(price).unwrap();
    let q = Qty::from_f64(100.0).unwrap();
    Bar { ts_ns: ts, known_at_ns: ts, instrument: id, open: p, high: p, low: p, close: p, volume: q }
}

/// Strategy that fails on the first bar and tracks how many times on_bar was invoked.
#[derive(Default)]
struct FailOnBarStrategy {
    bar_calls: usize,
}

impl Strategy for FailOnBarStrategy {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        self.bar_calls += 1;
        if self.bar_calls == 1 {
            ctx.fail(Error::InvalidArgument("test failure".into()));
        }
    }
}

/// Strategy that fails in on_start and tracks if on_bar was ever reached.
#[derive(Default)]
struct FailOnStartStrategy {
    start_calls: usize,
    bar_calls: usize,
}

impl Strategy for FailOnStartStrategy {
    fn on_start(&mut self, ctx: &mut Context<'_>) {
        self.start_calls += 1;
        ctx.fail(Error::InvalidArgument("start failure".into()));
    }

    fn on_bar(&mut self, _ctx: &mut Context<'_>, _bars: &[Bar]) {
        self.bar_calls += 1;
    }
}

#[test]
fn strategy_fail_on_bar_halts_engine_and_returns_error() {
    let bars = vec![make_bar(10, 1, 100.0), make_bar(20, 1, 101.0), make_bar(30, 1, 102.0)];
    let mut strategy = FailOnBarStrategy::default();
    let cfg = BacktestConfig::default();

    let result = run_backtest(&bars, &mut strategy, &cfg);

    match result {
        Err(err) => {
            assert_eq!(err, Error::InvalidArgument("test failure".into()));
        }
        Ok(_) => panic!("expected run_backtest to return Err, got Ok"),
    }
    assert_eq!(strategy.bar_calls, 1, "strategy must not receive later bars");
}

#[test]
fn strategy_fail_on_start_halts_engine_before_any_bar() {
    let bars = vec![make_bar(10, 1, 100.0), make_bar(20, 1, 101.0), make_bar(30, 1, 102.0)];
    let mut strategy = FailOnStartStrategy::default();
    let cfg = BacktestConfig::default();

    let result = run_backtest(&bars, &mut strategy, &cfg);

    match result {
        Err(err) => {
            assert_eq!(err, Error::InvalidArgument("start failure".into()));
        }
        Ok(_) => panic!("expected run_backtest to return Err, got Ok"),
    }
    assert_eq!(strategy.start_calls, 1, "on_start called once");
    assert_eq!(strategy.bar_calls, 0, "no bar should be processed");
}
