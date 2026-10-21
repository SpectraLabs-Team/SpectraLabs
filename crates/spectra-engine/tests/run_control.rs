//! Tests for `run_backtest_controlled`: limits, cancellation and progress must
//! never change a completed run's fingerprints.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use spectra_core::{Bar, Money, Price, Qty, Rng, Side, SCALE};
use spectra_engine::{
    run_backtest, run_backtest_controlled, BacktestConfig, Context, Progress, RunControl, RunError,
    RunLimits, Strategy,
};

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

/// Flips between long and flat every few bars, so runs have fills to fingerprint.
struct Flipper(usize);
impl Strategy for Flipper {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        self.0 += 1;
        for b in bars {
            let pos = ctx.position(b.instrument);
            if self.0.is_multiple_of(7) && pos.is_zero() {
                ctx.market(b.instrument, Side::Buy, Qty::from_raw(10 * SCALE));
            } else if self.0.is_multiple_of(11) && pos.raw() > 0 {
                ctx.market(b.instrument, Side::Sell, pos);
            }
        }
    }
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), seed: 7, ..BacktestConfig::default() }
}

#[test]
fn controlled_run_matches_plain_run_exactly() {
    let data = bars(600, 3, 11);
    let plain = run_backtest(&data, &mut Flipper(0), &cfg()).unwrap();

    let seen = RefCell::new(Vec::<Progress>::new());
    let mut cb = |p: Progress| seen.borrow_mut().push(p);
    let flag = AtomicBool::new(false);
    let mut ctl = RunControl {
        limits: RunLimits {
            max_bars: Some(data.len()),
            max_instruments: Some(3),
            max_wall_time: Some(Duration::from_secs(600)),
        },
        cancel: Some(&flag),
        progress: Some(&mut cb),
        progress_every: 50,
    };
    let controlled = run_backtest_controlled(&data, &mut Flipper(0), &cfg(), &mut ctl).unwrap();
    assert_eq!(plain.run, controlled.run);
    assert!(plain.fill_count() > 0);

    let seen = seen.into_inner();
    // Every 50 timestamps (50, 100, ... 550) plus the final call.
    assert_eq!(seen.len(), 11 + 1);
    assert!(seen.windows(2).all(|w| w[0].bars_done < w[1].bars_done));
    let last = seen.last().unwrap();
    assert_eq!(last.bars_done, data.len());
    assert_eq!(last.timestamps_done, 600);
    assert!((last.fraction() - 1.0).abs() < f64::EPSILON);
    assert_eq!(seen[0].bars_done, 150); // 50 timestamps x 3 instruments
}

#[test]
fn cancel_stops_at_the_next_timestamp() {
    let data = bars(400, 2, 3);
    let flag = AtomicBool::new(false);
    let mut cb = |p: Progress| {
        if p.timestamps_done >= 100 {
            flag.store(true, Ordering::Relaxed);
        }
    };
    let mut ctl =
        RunControl { cancel: Some(&flag), progress: Some(&mut cb), progress_every: 10, ..Default::default() };
    match run_backtest_controlled(&data, &mut Flipper(0), &cfg(), &mut ctl) {
        Err(RunError::Cancelled(p)) => {
            assert_eq!(p.timestamps_done, 100);
            assert_eq!(p.bars_done, 200);
            assert_eq!(p.bars_total, 800);
        }
        other => panic!("expected cancellation, got {other:?}"),
    }
}

#[test]
fn cancel_set_before_start_runs_nothing() {
    let data = bars(50, 1, 3);
    let flag = AtomicBool::new(true);
    let mut ctl = RunControl { cancel: Some(&flag), ..Default::default() };
    let err = run_backtest_controlled(&data, &mut Flipper(0), &cfg(), &mut ctl).unwrap_err();
    assert!(matches!(err, RunError::Cancelled(Progress { bars_done: 0, .. })), "{err:?}");
}

#[test]
fn size_limits_refuse_before_simulating() {
    let data = bars(100, 3, 5);
    let mut ctl = RunControl {
        limits: RunLimits { max_bars: Some(299), ..RunLimits::default() },
        ..Default::default()
    };
    let err = run_backtest_controlled(&data, &mut Flipper(0), &cfg(), &mut ctl).unwrap_err();
    assert!(matches!(&err, RunError::TooLarge(m) if m.contains("300 bars")), "{err}");

    let mut ctl = RunControl {
        limits: RunLimits { max_instruments: Some(2), ..RunLimits::default() },
        ..Default::default()
    };
    let err = run_backtest_controlled(&data, &mut Flipper(0), &cfg(), &mut ctl).unwrap_err();
    assert!(matches!(&err, RunError::TooLarge(m) if m.contains("3 instruments")), "{err}");
}

/// Sleeps on every bar, so a run of a few hundred bars takes measurable time.
struct Slow;
impl Strategy for Slow {
    fn on_bar(&mut self, _ctx: &mut Context<'_>, _bars: &[Bar]) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn time_limit_stops_a_slow_run() {
    let data = bars(600, 1, 5);
    let mut ctl = RunControl {
        limits: RunLimits { max_wall_time: Some(Duration::from_millis(50)), ..RunLimits::default() },
        ..Default::default()
    };
    // The clock is read every 256 timestamps; by timestamp 256 at least 256 ms have passed.
    match run_backtest_controlled(&data, &mut Slow, &cfg(), &mut ctl) {
        Err(RunError::TimedOut(p)) => assert_eq!(p.timestamps_done, 256),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

#[test]
fn engine_errors_pass_through() {
    let data = bars(10, 1, 5);
    let bad = BacktestConfig { periods_per_year: 0.0, ..cfg() };
    let err = run_backtest_controlled(&data, &mut Flipper(0), &bad, &mut RunControl::default()).unwrap_err();
    assert!(matches!(&err, RunError::Engine(e) if e.to_string().contains("periods_per_year")), "{err}");
}
