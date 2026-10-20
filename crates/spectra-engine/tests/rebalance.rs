//! Target-weight mode (M3 task 8): weights become orders through the normal
//! engine, with rebalance rules, lot rounding, minimum trades and a gross cap.

use spectra_core::{Bar, InstrumentId, LedgerConfig, Money, Price, Qty, Ratio, Side, SCALE};
use spectra_engine::{
    run_backtest, BacktestConfig, BacktestResult, Context, ExecutionConfig, OrderState, RebalanceConfig,
    RebalanceRule, Rebalancer, WeightStrategy,
};

/// Bars for two instruments; `px(step, instrument)` gives the price (open = close).
fn bars(n: usize, px: impl Fn(usize, InstrumentId) -> f64) -> Vec<Bar> {
    let mut out = Vec::new();
    for i in 0..n {
        for id in [1, 2] {
            let p = Price::from_f64(px(i, id)).unwrap();
            out.push(Bar {
                ts_ns: (i as i64 + 1) * 86_400_000_000_000,
                instrument: id,
                open: p,
                high: p,
                low: p,
                close: p,
                volume: Qty::from_raw(10_000_000 * SCALE),
            });
        }
    }
    out
}

fn flat(n: usize) -> Vec<Bar> {
    bars(n, |_, id| if id == 1 { 100.0 } else { 50.0 })
}

/// Fixed weights, returned on every bar.
struct Fixed(Vec<(InstrumentId, f64)>);
impl WeightStrategy for Fixed {
    fn weights(&mut self, _ctx: &Context<'_>, _bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
        Some(self.0.clone())
    }
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() }
}

fn run(data: &[Bar], w: Vec<(InstrumentId, f64)>, rc: RebalanceConfig, c: &BacktestConfig) -> BacktestResult {
    let r = run_backtest(data, &mut Rebalancer::new(Fixed(w), rc), c).unwrap();
    r.ledger.check_invariants().unwrap();
    r
}

fn units(r: &BacktestResult, id: InstrumentId) -> f64 {
    r.ledger.position(id).qty.to_f64()
}

#[test]
fn weights_become_whole_lots_within_the_gross_cap() {
    // 0.5 / 0.5 of 100,000 at 98% of the 1x limit: 0.49 * 100,000 / 100 = 490 and / 50 = 980.
    let r = run(&flat(5), vec![(1, 0.5), (2, 0.5)], RebalanceConfig::default(), &cfg());
    assert_eq!((units(&r, 1), units(&r, 2)), (490.0, 980.0));
    // One buy each, then nothing: already on target.
    assert_eq!(r.orders.len(), 2);
    assert!(r.orders.iter().all(|o| o.state == OrderState::Filled && o.side == Side::Buy));
    assert_eq!(r.ledger.cash(), Money::from_raw(2_000 * SCALE));
}

#[test]
fn over_allocated_weights_are_scaled_down() {
    // 0.8 + 0.8 = 1.6 gross, scaled to 0.98: 0.49 each.
    let r = run(&flat(4), vec![(1, 0.8), (2, 0.8)], RebalanceConfig::default(), &cfg());
    assert_eq!((units(&r, 1), units(&r, 2)), (490.0, 980.0));
}

#[test]
fn lot_size_rounds_towards_zero() {
    let rc = RebalanceConfig { lot_size: Qty::from_raw(100 * SCALE), ..RebalanceConfig::default() };
    let r = run(&flat(4), vec![(1, 0.5), (2, 0.5)], rc, &cfg());
    assert_eq!((units(&r, 1), units(&r, 2)), (400.0, 900.0));
}

#[test]
fn short_weights_need_shorting() {
    let w = vec![(1, 0.5), (2, -0.3)];
    let long_only = run(&flat(4), w.clone(), RebalanceConfig::default(), &cfg());
    // The negative weight is dropped; 0.5 alone is under the cap, so it is not scaled: 500.
    assert_eq!(
        (units(&long_only, 1), units(&long_only, 2)),
        (500.0, 0.0),
        "negative weight dropped, the rest kept"
    );

    let shorts = BacktestConfig {
        ledger: LedgerConfig {
            allow_short: true,
            max_gross_leverage: Some(Ratio::from_raw(2 * SCALE)),
            ..LedgerConfig::default()
        },
        ..cfg()
    };
    let r = run(&flat(4), w, RebalanceConfig::default(), &shorts);
    // 0.8 gross is under 0.98 * 2: unscaled. 0.5 * 100,000 / 100 = 500; -0.3 * 100,000 / 50 = -600.
    assert_eq!((units(&r, 1), units(&r, 2)), (500.0, -600.0));
}

#[test]
fn every_n_bars_trades_only_on_schedule() {
    // Instrument 1 doubles over time, so weights drift every bar.
    let data = bars(25, |i, id| if id == 1 { 100.0 + 4.0 * i as f64 } else { 50.0 });
    let rc = RebalanceConfig { rule: RebalanceRule::EveryNBars(10), ..RebalanceConfig::default() };
    let r = run(&data, vec![(1, 0.5), (2, 0.5)], rc, &cfg());
    // Submitted on bars 0, 10 and 20 (step indices of the submitting bar).
    let steps: std::collections::BTreeSet<i64> = r.orders.iter().map(|o| o.submit_step).collect();
    assert_eq!(steps.into_iter().collect::<Vec<_>>(), vec![0, 10, 20]);
}

#[test]
fn drift_rule_waits_for_the_threshold() {
    let data = bars(30, |i, id| if id == 1 { 100.0 + 2.0 * i as f64 } else { 50.0 });
    let tight = run(
        &data,
        vec![(1, 0.5), (2, 0.5)],
        RebalanceConfig { rule: RebalanceRule::Drift(0.01), ..RebalanceConfig::default() },
        &cfg(),
    );
    let loose = run(
        &data,
        vec![(1, 0.5), (2, 0.5)],
        RebalanceConfig { rule: RebalanceRule::Drift(0.10), ..RebalanceConfig::default() },
        &cfg(),
    );
    let rebalances = |r: &BacktestResult| {
        r.orders.iter().map(|o| o.submit_step).collect::<std::collections::BTreeSet<_>>().len()
    };
    assert!(rebalances(&tight) > rebalances(&loose), "{} vs {}", rebalances(&tight), rebalances(&loose));
    assert!(rebalances(&loose) >= 2, "the initial allocation and at least one drift rebalance");
}

#[test]
fn minimum_trade_skips_small_adjustments() {
    let data = bars(12, |i, id| if id == 1 { 100.0 + i as f64 } else { 50.0 });
    let every = run(&data, vec![(1, 0.5), (2, 0.5)], RebalanceConfig::default(), &cfg());
    let min = RebalanceConfig { min_trade_notional: 5_000.0, ..RebalanceConfig::default() };
    let skipped = run(&data, vec![(1, 0.5), (2, 0.5)], min, &cfg());
    assert!(every.orders.len() > 2, "small drifts are traded by default");
    assert_eq!(skipped.orders.len(), 2, "only the initial allocation is big enough");
}

#[test]
fn orders_do_not_stack_while_waiting_to_fill() {
    let delayed =
        BacktestConfig { exec: ExecutionConfig { delay_bars: 3, ..ExecutionConfig::default() }, ..cfg() };
    let r = run(&flat(8), vec![(1, 0.5), (2, 0.5)], RebalanceConfig::default(), &delayed);
    assert_eq!(r.orders.len(), 2, "one allocation, not one per bar of the delay");
    assert_eq!((units(&r, 1), units(&r, 2)), (490.0, 980.0));
}

#[test]
fn dropped_instruments_are_closed() {
    /// Holds both, then only instrument 1 from bar 5.
    struct Switch(usize);
    impl WeightStrategy for Switch {
        fn weights(&mut self, _ctx: &Context<'_>, _bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
            self.0 += 1;
            Some(if self.0 < 5 { vec![(1, 0.5), (2, 0.5)] } else { vec![(1, 0.5)] })
        }
    }
    let r =
        run_backtest(&flat(10), &mut Rebalancer::new(Switch(0), RebalanceConfig::default()), &cfg()).unwrap();
    r.ledger.check_invariants().unwrap();
    assert_eq!(units(&r, 2), 0.0);
    // 0.5 alone is under the cap: topped up from 490 to 500.
    assert_eq!(units(&r, 1), 500.0);
}
