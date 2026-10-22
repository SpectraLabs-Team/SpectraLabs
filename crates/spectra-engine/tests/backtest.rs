//! Backtester tests: a port of the C++ reference tests (`test_backtest.cpp`,
//! `test_backtest_property.cpp`) plus tests for the three fixes.

use std::time::Instant;

use spectra_core::*;
use spectra_engine::*;

fn m(v: f64) -> Money {
    Money::from_f64(v).unwrap()
}
fn p(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}
fn q(v: f64) -> Qty {
    Qty::from_f64(v).unwrap()
}

fn mk(ts: i64, id: InstrumentId, o: f64, h: f64, l: f64, c: f64, vol: f64) -> Bar {
    Bar {
        ts_ns: ts,
        known_at_ns: ts,
        instrument: id,
        open: p(o),
        high: p(h),
        low: p(l),
        close: p(c),
        volume: q(vol),
    }
}
fn mk1(ts: i64, id: InstrumentId, o: f64, h: f64, l: f64, c: f64) -> Bar {
    mk(ts, id, o, h, l, c, 1_000_000.0)
}

type BarFn = Box<dyn FnMut(&mut Context<'_>, &[Bar])>;
type StartFn = Box<dyn FnMut(&mut Context<'_>)>;
type FillFn = Box<dyn FnMut(&mut Context<'_>, &Fill)>;

/// A strategy driven by closures, so each test scripts exactly what it needs.
#[derive(Default)]
struct FnStrategy {
    start_fn: Option<StartFn>,
    bar_fn: Option<BarFn>,
    fill_fn: Option<FillFn>,
}

impl Strategy for FnStrategy {
    fn on_start(&mut self, c: &mut Context<'_>) {
        if let Some(f) = &mut self.start_fn {
            f(c);
        }
    }
    fn on_bar(&mut self, c: &mut Context<'_>, b: &[Bar]) {
        if let Some(f) = &mut self.bar_fn {
            f(c, b);
        }
    }
    fn on_fill(&mut self, c: &mut Context<'_>, fill: &Fill) {
        if let Some(f) = &mut self.fill_fn {
            f(c, fill);
        }
    }
}

/// Submits `make` exactly once, on the first bar.
fn once(mut make: impl FnMut(&mut Context<'_>) + 'static) -> FnStrategy {
    let mut sent = false;
    FnStrategy {
        bar_fn: Some(Box::new(move |c, _| {
            if !sent {
                sent = true;
                make(c);
            }
        })),
        ..FnStrategy::default()
    }
}

fn base_config() -> BacktestConfig {
    BacktestConfig { initial_cash: m(100_000.0), ..BacktestConfig::default() }
}

fn fills_of(r: &BacktestResult) -> Vec<Fill> {
    r.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill).map(|e| e.fill).collect()
}

fn expect_invariants(r: &BacktestResult) {
    if let Err(e) = r.ledger.check_invariants() {
        panic!("invariant violated: {e}");
    }
}

const A: InstrumentId = 1;
const B: InstrumentId = 2;

#[test]
fn market_order_fills_at_next_open_not_signal_close() {
    let bars = [
        mk1(1, A, 100.0, 106.0, 99.0, 105.0),
        mk1(2, A, 110.0, 121.0, 109.0, 120.0),
        mk1(3, A, 120.0, 125.0, 118.0, 123.0),
    ];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(10.0));
    });
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    let fills = fills_of(&r);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].price, p(110.0));
    assert_eq!(fills[0].ts_ns, 2);
    assert_eq!(r.equity_curve.len(), 3);
    assert_eq!(r.equity_curve[0].equity, m(100_000.0));
    assert_eq!(r.equity_curve[1].equity, m(100_100.0));
    assert_eq!(r.orders[0].state, OrderState::Filled);
    expect_invariants(&r);
}

#[test]
fn strategy_history_never_includes_future_bars() {
    let bars: Vec<Bar> = (1..=20)
        .map(|t| {
            let px = 100.0 + t as f64;
            mk1(t, A, px, px + 1.0, px - 1.0, px + 0.5)
        })
        .collect();
    let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let c2 = calls.clone();
    let mut s = FnStrategy {
        bar_fn: Some(Box::new(move |c, group| {
            c2.set(c2.get() + 1);
            let h = c.history(A);
            assert_eq!(h.len(), c2.get());
            assert_eq!(h[h.len() - 1], group[0]);
            assert_eq!(c.now(), group[0].ts_ns);
        })),
        ..FnStrategy::default()
    };
    run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(calls.get(), 20);
}

#[test]
fn orders_from_on_start_fill_on_first_bar() {
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), mk1(2, A, 100.0, 101.0, 99.0, 100.0)];
    let mut s = FnStrategy {
        start_fn: Some(Box::new(|c| {
            c.market(A, Side::Buy, q(5.0));
        })),
        ..FnStrategy::default()
    };
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    let fills = fills_of(&r);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].ts_ns, 1);
}

#[test]
fn applies_slippage_and_commission() {
    let mut cfg = base_config();
    cfg.costs.slippage = Bps::from_f64(10.0).unwrap();
    cfg.costs.commission.per_unit = m(0.01);
    cfg.costs.commission.bps_of_notional = Bps::from_f64(5.0).unwrap();
    cfg.costs.commission.min_fee = m(1.0);
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), mk1(2, A, 100.0, 101.0, 99.0, 100.0)];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(100.0));
    });
    let r = run_backtest(&bars, &mut s, &cfg).unwrap();
    let fills = fills_of(&r);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].price, p(100.1));
    assert_eq!(fills[0].fee, m(6.005));
    assert_eq!(r.ledger.cash(), m(89_983.995));
    expect_invariants(&r);
}

fn limit_run(second: Bar, side: Side, limit: f64, cfg: &BacktestConfig) -> BacktestResult {
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), second];
    let mut s = once(move |c| {
        c.limit(A, side, q(10.0), p(limit));
    });
    run_backtest(&bars, &mut s, cfg).unwrap()
}

#[test]
fn limit_buy_fills_at_limit_or_better() {
    let cfg = base_config();
    assert_eq!(
        fills_of(&limit_run(mk1(2, A, 102.0, 103.0, 98.0, 100.0), Side::Buy, 99.0, &cfg))[0].price,
        p(99.0)
    );
    assert_eq!(
        fills_of(&limit_run(mk1(2, A, 97.0, 100.0, 96.0, 99.0), Side::Buy, 99.0, &cfg))[0].price,
        p(97.0)
    );
    let missed = limit_run(mk1(2, A, 102.0, 103.0, 100.0, 101.0), Side::Buy, 99.0, &cfg);
    assert!(fills_of(&missed).is_empty());
    assert_eq!(missed.orders[0].state, OrderState::Pending);
}

#[test]
fn limit_sell_fills_at_limit_or_better() {
    let mut cfg = base_config();
    cfg.ledger.allow_short = true;
    assert_eq!(
        fills_of(&limit_run(mk1(2, A, 100.0, 106.0, 99.0, 104.0), Side::Sell, 105.0, &cfg))[0].price,
        p(105.0)
    );
    assert_eq!(
        fills_of(&limit_run(mk1(2, A, 108.0, 109.0, 107.0, 108.0), Side::Sell, 105.0, &cfg))[0].price,
        p(108.0)
    );
    assert!(fills_of(&limit_run(mk1(2, A, 100.0, 104.0, 99.0, 103.0), Side::Sell, 105.0, &cfg)).is_empty());
}

#[test]
fn participation_cap_creates_partial_fills() {
    let mut cfg = base_config();
    cfg.exec.max_participation = q(0.1);
    let bars: Vec<Bar> = (1..=5).map(|t| mk(t, A, 100.0, 101.0, 99.0, 100.0, 100.0)).collect();
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(25.0));
    });
    let r = run_backtest(&bars, &mut s, &cfg).unwrap();
    let fills = fills_of(&r);
    assert_eq!(fills.iter().map(|f| f.qty).collect::<Vec<_>>(), vec![q(10.0), q(10.0), q(5.0)]);
    assert_eq!(r.orders[0].state, OrderState::Filled);
    assert!(r.orders[0].remaining.is_zero());
    expect_invariants(&r);
}

#[test]
fn zero_volume_bar_blocks_capped_fill() {
    let mut cfg = base_config();
    cfg.exec.max_participation = q(0.5);
    let bars = [
        mk(1, A, 100.0, 101.0, 99.0, 100.0, 100.0),
        mk(2, A, 100.0, 101.0, 99.0, 100.0, 0.0),
        mk(3, A, 100.0, 101.0, 99.0, 100.0, 100.0),
    ];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(10.0));
    });
    let r = run_backtest(&bars, &mut s, &cfg).unwrap();
    let fills = fills_of(&r);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].ts_ns, 3);
}

#[test]
fn rejects_orders_the_ledger_refuses() {
    let mut cfg = base_config();
    cfg.initial_cash = m(1_000.0);
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), mk1(2, A, 100.0, 101.0, 99.0, 100.0)];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(100.0));
    });
    let r = run_backtest(&bars, &mut s, &cfg).unwrap();
    assert_eq!(r.orders[0].state, OrderState::Rejected);
    assert_eq!(r.orders[0].reject_reason, Some(Reject::InsufficientCash));
    assert!(fills_of(&r).is_empty());
    assert_eq!(r.ledger.cash(), m(1_000.0));
    expect_invariants(&r);
}

#[test]
fn short_sale_rejected_unless_enabled() {
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), mk1(2, A, 100.0, 101.0, 99.0, 100.0)];
    let mut s = once(|c| {
        c.market(A, Side::Sell, q(1.0));
    });
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.orders[0].reject_reason, Some(Reject::ShortNotAllowed));
}

#[test]
fn malformed_orders_are_recorded_as_rejected() {
    let bars = [mk1(1, A, 100.0, 101.0, 99.0, 100.0), mk1(2, A, 100.0, 101.0, 99.0, 100.0)];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(0.0));
        c.limit(A, Side::Buy, q(1.0), p(0.0));
    });
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.orders.len(), 2);
    assert!(r.orders.iter().all(|o| o.state == OrderState::Rejected));
    assert_eq!(r.orders[0].reject_reason, Some(Reject::InvalidFill));
}

#[test]
fn cancel_prevents_fill_and_only_works_on_open_orders() {
    let bars: Vec<Bar> = (1..=3).map(|t| mk1(t, A, 100.0, 101.0, 99.0, 100.0)).collect();
    let mut call = 0;
    let mut filled = 0;
    let mut s = FnStrategy {
        bar_fn: Some(Box::new(move |c, _| {
            call += 1;
            if call == 1 {
                let cancelled = c.market(A, Side::Buy, q(5.0));
                assert!(c.cancel(cancelled));
                assert!(!c.cancel(cancelled));
                filled = c.market(A, Side::Buy, q(3.0));
            } else if call == 3 {
                assert!(!c.cancel(filled));
                assert!(!c.cancel(9999));
            }
        })),
        ..FnStrategy::default()
    };
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.orders[0].state, OrderState::Cancelled);
    assert_eq!(r.orders[1].state, OrderState::Filled);
    assert_eq!(fills_of(&r).len(), 1);
    assert_eq!(r.ledger.position(A).qty, q(3.0));
}

#[test]
fn orders_submitted_from_on_fill_are_safe_and_fill_later() {
    let bars: Vec<Bar> = (1..=4).map(|t| mk1(t, A, 100.0, 101.0, 99.0, 100.0)).collect();
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(1.0));
    });
    let mut chained = false;
    s.fill_fn = Some(Box::new(move |c, _| {
        if !chained {
            chained = true;
            c.market(A, Side::Buy, q(1.0));
        }
    }));
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.orders.len(), 2);
    assert_eq!(r.orders[1].state, OrderState::Filled);
    let fills = fills_of(&r);
    assert_eq!(fills.len(), 2);
    assert!(fills[1].ts_ns > fills[0].ts_ns);
    expect_invariants(&r);
}

#[test]
fn handles_multiple_instruments_per_timestamp() {
    let bars = [
        mk1(1, A, 100.0, 101.0, 99.0, 100.0),
        mk1(1, B, 50.0, 51.0, 49.0, 50.0),
        mk1(2, A, 101.0, 103.0, 100.0, 102.0),
        mk1(2, B, 50.0, 52.0, 49.0, 51.0),
        mk1(3, A, 102.0, 104.0, 101.0, 103.0),
        mk1(3, B, 51.0, 53.0, 50.0, 52.0),
    ];
    let mut sent = false;
    let mut s = FnStrategy {
        bar_fn: Some(Box::new(move |c, group| {
            assert_eq!(group.len(), 2);
            if !sent {
                sent = true;
                c.market(A, Side::Buy, q(10.0));
                c.market(B, Side::Buy, q(20.0));
            }
        })),
        ..FnStrategy::default()
    };
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.ledger.position(A).qty, q(10.0));
    assert_eq!(r.ledger.position(B).qty, q(20.0));
    assert_eq!(r.equity_curve.last().unwrap().equity, r.ledger.equity().unwrap());
    assert_eq!(r.equity_curve.last().unwrap().equity, m(100_000.0 - 1_010.0 - 1_000.0 + 1_030.0 + 1_040.0));
    expect_invariants(&r);
}

#[test]
fn metrics_reflect_equity_curve() {
    let bars = [
        mk1(1, A, 100.0, 101.0, 99.0, 100.0),
        mk1(2, A, 100.0, 111.0, 99.0, 110.0),
        mk1(3, A, 110.0, 121.0, 109.0, 120.0),
    ];
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(100.0));
    });
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert!(r.metrics.valid);
    assert_eq!(r.metrics.periods, 2);
    let expected = r.equity_curve.last().unwrap().equity.to_f64() / 100_000.0 - 1.0;
    assert_eq!(r.metrics.total_return, expected);
    assert!(r.metrics.total_return > 0.0);
}

#[test]
fn rejects_invalid_bars_and_config() {
    let mut s = FnStrategy::default();
    let unsorted = [mk1(2, A, 100.0, 101.0, 99.0, 100.0), mk1(1, A, 100.0, 101.0, 99.0, 100.0)];
    assert!(matches!(run_backtest(&unsorted, &mut s, &base_config()), Err(Error::InvalidArgument(_))));
    let mut bad = base_config();
    bad.initial_cash = Money::from_raw(-1);
    assert!(matches!(run_backtest(&[], &mut s, &bad), Err(Error::InvalidArgument(_))));
    bad = base_config();
    bad.periods_per_year = 0.0;
    assert!(run_backtest(&[], &mut s, &bad).is_err());
}

#[test]
fn with_no_bars_returns_empty_result() {
    let mut s = FnStrategy::default();
    let r = run_backtest(&[], &mut s, &base_config()).unwrap();
    assert!(r.equity_curve.is_empty());
    assert!(!r.metrics.valid);
    assert_eq!(r.ledger.cash(), m(100_000.0));
}

fn buy_ten(bars: &[Bar], cfg: &BacktestConfig) -> BacktestResult {
    let mut s = once(|c| {
        c.market(A, Side::Buy, q(10.0));
    });
    run_backtest(bars, &mut s, cfg).unwrap()
}

#[test]
fn run_record_is_deterministic_and_sensitive_to_inputs() {
    let bars = vec![
        mk1(1, A, 100.0, 101.0, 99.0, 100.0),
        mk1(2, A, 101.0, 103.0, 100.0, 102.0),
        mk1(3, A, 102.0, 104.0, 101.0, 103.0),
    ];
    let mut cfg = base_config();
    cfg.seed = 7;
    let a = buy_ten(&bars, &cfg);
    let b = buy_ten(&bars, &cfg);
    assert_eq!(a.run, b.run);
    assert_eq!(a.run.result_sha256.len(), 64);
    assert_eq!(a.run.engine_version, ENGINE_VERSION);
    assert_eq!(a.run.seed, 7);
    assert_eq!(a.run.bar_count, 3);

    let mut changed = bars.clone();
    changed[2].close = p(103.000_000_01);
    let c = buy_ten(&changed, &cfg);
    assert_ne!(c.run.dataset_sha256, a.run.dataset_sha256);
    assert_ne!(c.run.result_sha256, a.run.result_sha256);
    assert_eq!(c.run.config_sha256, a.run.config_sha256);

    let mut cfg2 = cfg;
    cfg2.costs.slippage = Bps::from_f64(1.0).unwrap();
    let d = buy_ten(&bars, &cfg2);
    assert_ne!(d.run.config_sha256, a.run.config_sha256);
    assert_ne!(d.run.result_sha256, a.run.result_sha256);
    assert_eq!(d.run.dataset_sha256, a.run.dataset_sha256);
}

fn bars_from_opens(opens: &[f64]) -> Vec<Bar> {
    opens.iter().enumerate().map(|(i, &o)| mk1(i as i64 + 1, 1, o, o + 1.0, o - 1.0, o)).collect()
}

#[test]
fn execution_delay_postpones_fills_by_whole_bars() {
    let bars = bars_from_opens(&[100.0, 102.0, 104.0, 108.0, 110.0, 107.0, 105.0, 103.0]);
    let run = |delay: usize| {
        let mut cfg = base_config();
        cfg.exec.delay_bars = delay;
        buy_ten(&bars, &cfg)
    };
    let d0 = run(0);
    assert_eq!((fills_of(&d0)[0].price, fills_of(&d0)[0].ts_ns), (p(102.0), 2));
    let d2 = run(2);
    assert_eq!((fills_of(&d2)[0].price, fills_of(&d2)[0].ts_ns), (p(108.0), 4));
    let never = run(50);
    assert!(fills_of(&never).is_empty());
    assert_eq!(never.orders[0].state, OrderState::Pending);
    assert_ne!(d0.run.config_sha256, d2.run.config_sha256);
    assert_ne!(d0.run.result_sha256, d2.run.result_sha256);
    expect_invariants(&d2);
}

#[test]
fn delay_applies_to_orders_placed_in_on_start_too() {
    let bars = bars_from_opens(&[100.0, 102.0, 104.0, 108.0]);
    let mut cfg = base_config();
    cfg.exec.delay_bars = 1;
    let mut s = FnStrategy {
        start_fn: Some(Box::new(|c| {
            c.market(A, Side::Buy, q(1.0));
        })),
        ..FnStrategy::default()
    };
    let r = run_backtest(&bars, &mut s, &cfg).unwrap();
    assert_eq!(fills_of(&r)[0].ts_ns, 2);
}

// --- The three fixes ----------------------------------------------------------

#[test]
fn leverage_limit_rejects_short_proceeds_funding_more_buying() {
    // The audit's probe: short $100k of A, then buy $190k of B with the proceeds.
    let bars: Vec<Bar> = (1..=4)
        .flat_map(|t| [mk1(t, 1, 100.0, 100.0, 100.0, 100.0), mk1(t, 2, 100.0, 100.0, 100.0, 100.0)])
        .collect();
    let mut n = 0;
    let make = move || {
        let mut n2 = 0;
        FnStrategy {
            bar_fn: Some(Box::new(move |c, _| {
                if n2 == 0 {
                    c.market(1, Side::Sell, q(1000.0));
                }
                if n2 == 1 {
                    c.market(2, Side::Buy, q(1900.0));
                }
                n2 += 1;
            })),
            ..FnStrategy::default()
        }
    };
    n += 1;
    let mut cfg = base_config();
    cfg.ledger.allow_short = true;
    let r = run_backtest(&bars, &mut make(), &cfg).unwrap();
    assert_eq!(r.orders[0].state, OrderState::Filled);
    assert_eq!(r.orders[1].state, OrderState::Rejected);
    assert_eq!(r.orders[1].reject_reason, Some(Reject::LeverageExceeded));
    cfg.ledger = LedgerConfig::reference(true, false);
    let old = run_backtest(&bars, &mut make(), &cfg).unwrap();
    assert_eq!(old.orders[1].state, OrderState::Filled); // the C++ behaviour
    assert_eq!(n, 1);
}

#[test]
fn resting_orders_scale_linearly() {
    // The C++ engine scanned every order ever submitted at every step:
    // 40,000 bars with one never-filling limit order per bar took about 3 s there.
    let bars: Vec<Bar> = (1..=40_000).map(|t| mk1(t, A, 100.0, 101.0, 99.0, 100.0)).collect();
    let mut s = FnStrategy {
        bar_fn: Some(Box::new(|c, _| {
            c.limit(A, Side::Buy, q(1.0), p(0.01));
        })),
        ..FnStrategy::default()
    };
    let start = Instant::now();
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(r.orders.len(), 40_000);
    assert!(r.orders.iter().all(|o| o.state == OrderState::Pending));
    // The price-indexed book never visits orders far below the market.
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}

#[test]
fn closed_orders_cost_nothing() {
    // 40,000 orders that each fill on the next bar: the C++ engine still
    // re-scanned all of them every step (quadratic); now each step only sees
    // the one open order.
    let bars: Vec<Bar> = (1..=40_000).map(|t| mk1(t, A, 100.0, 101.0, 99.0, 100.0)).collect();
    let mut buy = true;
    let mut s = FnStrategy {
        bar_fn: Some(Box::new(move |c, _| {
            c.market(A, if buy { Side::Buy } else { Side::Sell }, q(1.0));
            buy = !buy;
        })),
        ..FnStrategy::default()
    };
    let start = Instant::now();
    let r = run_backtest(&bars, &mut s, &base_config()).unwrap();
    assert_eq!(r.fill_count(), 39_999);
    assert!(start.elapsed().as_secs_f64() < 2.0, "took {:?}", start.elapsed());
    expect_invariants(&r);
}

#[test]
fn mixed_bar_intervals_are_refused() {
    // Instrument 1 daily, instrument 2 every hour: a strategy at the daily bar's
    // timestamp could read that day's close while trading hourly bars.
    let day = 86_400;
    let mut bars = Vec::new();
    for d in 0..3i64 {
        bars.push(mk1(d * day, 1, 100.0, 101.0, 99.0, 100.0));
        for h in 0..24i64 {
            bars.push(mk1(d * day + h * 3600, 2, 50.0, 51.0, 49.0, 50.0));
        }
    }
    bars.sort_by_key(|b| (b.ts_ns, b.instrument));
    let mut s = FnStrategy::default();
    let err = run_backtest(&bars, &mut s, &base_config()).unwrap_err();
    assert!(err.to_string().contains("different bar intervals"), "{err}");
}

// --- Randomised property test (port of test_backtest_property.cpp) ------------

const INSTRUMENTS: i64 = 3;
const TIMESTAMPS: i64 = 250;

fn random_market(seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut last: Vec<i64> = (0..INSTRUMENTS).map(|_| rng.range(20 * SCALE, 300 * SCALE)).collect();
    let mut bars = Vec::new();
    for t in 1..=TIMESTAMPS {
        for k in 0..INSTRUMENTS {
            if rng.range(0, 19) == 0 {
                continue;
            }
            let prev = last[k as usize];
            let open = (prev + rng.range(-SCALE, SCALE)).max(SCALE);
            let close = (open + rng.range(-2 * SCALE, 2 * SCALE)).max(SCALE);
            let high = open.max(close) + rng.range(0, SCALE);
            let low = (open.min(close) - rng.range(0, SCALE)).max(1);
            let volume = if rng.range(0, 9) == 0 { 0 } else { rng.range(1, 1000) * SCALE };
            bars.push(Bar {
                ts_ns: t,
                known_at_ns: t,
                instrument: k as InstrumentId,
                open: Price::from_raw(open),
                high: Price::from_raw(high),
                low: Price::from_raw(low),
                close: Price::from_raw(close),
                volume: Qty::from_raw(volume),
            });
            last[k as usize] = close;
        }
    }
    bars
}

struct RandomStrategy {
    rng: Rng,
    last: OrderId,
}

impl Strategy for RandomStrategy {
    fn on_bar(&mut self, ctx: &mut Context<'_>, group: &[Bar]) {
        for b in group {
            let action = self.rng.range(0, 9);
            let side = if self.rng.range(0, 1) == 0 { Side::Buy } else { Side::Sell };
            let qty = Qty::from_raw(self.rng.range(1, 50 * SCALE));
            if action < 3 {
                self.last = ctx.market(b.instrument, side, qty);
            } else if action < 6 {
                let d = (b.close.raw() / 50).max(1);
                let px = (b.close.raw() + self.rng.range(-d, d)).max(1);
                self.last = ctx.limit(b.instrument, side, qty, Price::from_raw(px));
            } else if action == 6 && self.last != 0 {
                ctx.cancel(self.last);
            }
        }
    }
}

#[test]
fn property_books_always_reconcile_and_runs_reproduce() {
    let mut total_fills = 0;
    let mut total_rejects = 0;
    for c in 0..6u64 {
        let mut cfg =
            BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() };
        cfg.ledger.allow_short = c % 2 == 1;
        cfg.ledger.allow_negative_cash = c % 3 == 2;
        cfg.ledger.max_gross_leverage = if c < 3 { None } else { Some(Ratio::from_units(2).unwrap()) };
        cfg.costs.slippage = Bps::from_raw((c as i64) * 3 * SCALE);
        cfg.costs.commission.bps_of_notional = Bps::from_raw(SCALE);
        cfg.exec.max_participation = if c % 2 == 0 { Qty::from_raw(SCALE / 10) } else { Qty::ZERO };
        cfg.exec.delay_bars = (c % 3) as usize;
        for seed in 1..=8u64 {
            let bars = random_market(seed * 31 + c);
            let r1 = run_backtest(&bars, &mut RandomStrategy { rng: Rng::new(seed), last: 0 }, &cfg).unwrap();
            let r2 = run_backtest(&bars, &mut RandomStrategy { rng: Rng::new(seed), last: 0 }, &cfg).unwrap();
            assert_eq!(r1.run, r2.run, "runs must be bit-for-bit reproducible");
            expect_invariants(&r1);
            let filled: i64 = r1.orders.iter().map(|o| o.qty.raw() - o.remaining.raw()).sum();
            let journal: i64 = fills_of(&r1).iter().map(|f| f.qty.raw()).sum();
            assert_eq!(filled, journal, "order accounting must match the ledger");
            assert_eq!(r1.equity_curve.last().unwrap().equity, r1.ledger.equity().unwrap());
            total_fills += r1.fill_count();
            total_rejects += r1.orders.iter().filter(|o| o.state == OrderState::Rejected).count();
        }
    }
    assert!(total_fills > 1000);
    assert!(total_rejects > 0);
}
