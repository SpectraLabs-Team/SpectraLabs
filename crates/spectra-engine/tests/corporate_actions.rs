//! Corporate actions during a run (M3 task 10): splits adjust positions,
//! marks and open orders without creating P&L; dividends are paid to longs and
//! charged to shorts; the books re-derive from the journal; runs without
//! actions keep their fingerprints.

use spectra_core::{Bar, EntryType, InstrumentId, Ledger, LedgerConfig, Money, Price, Qty, Side, SCALE};
use spectra_engine::{
    run_backtest_controlled, run_backtest_with_actions, run_multi_backtest_with_actions, Allocation,
    BacktestConfig, BacktestResult, Context, CorporateAction, OrderState, RunControl, RunError, Strategy,
};

const DAY: i64 = 86_400_000_000_000;

/// Daily bars of instrument 1 (and 2 at 50, so multi-instrument paths run),
/// with open = close = the given price.
fn bars(prices: &[f64]) -> Vec<Bar> {
    let mut out = Vec::new();
    for (i, &p) in prices.iter().enumerate() {
        for (id, px) in [(1, p), (2, 50.0)] {
            let px = Price::from_f64(px).unwrap();
            out.push(Bar {
                ts_ns: (i as i64 + 1) * DAY,
                known_at_ns: (i as i64 + 1) * DAY,
                instrument: id,
                open: px,
                high: px,
                low: px,
                close: px,
                volume: Qty::from_raw(1_000_000 * SCALE),
            });
        }
    }
    out
}

fn ts(day: i64) -> i64 {
    day * DAY
}

fn q(v: f64) -> Qty {
    Qty::from_f64(v).unwrap()
}

fn p(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() }
}

/// Buys (or sells) once on the first bar; optionally places a resting order
/// too; records the corporate actions it is told about.
#[derive(Default)]
struct Once {
    side: Option<Side>,
    units: f64,
    resting: Option<(Side, f64, f64)>,
    stop: Option<(Side, f64, f64)>,
    done: bool,
    seen: Vec<CorporateAction>,
    position_when_told: Vec<Qty>,
}

impl Strategy for Once {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(side) = self.side {
            ctx.market(1, side, q(self.units));
        }
        if let Some((side, units, limit)) = self.resting {
            ctx.limit(1, side, q(units), p(limit));
        }
        if let Some((side, units, stop)) = self.stop {
            ctx.stop(1, side, q(units), p(stop));
        }
    }
    fn on_corporate_action(&mut self, ctx: &mut Context<'_>, action: &CorporateAction) {
        self.seen.push(*action);
        self.position_when_told.push(ctx.position(action.instrument));
    }
}

fn buy(units: f64) -> Once {
    Once { side: Some(Side::Buy), units, ..Once::default() }
}

fn run(
    data: &[Bar],
    actions: &[CorporateAction],
    s: &mut dyn Strategy,
    c: &BacktestConfig,
) -> BacktestResult {
    let r = run_backtest_with_actions(data, actions, s, c, &mut RunControl::default()).unwrap();
    r.ledger.check_invariants().unwrap();
    r
}

fn entries(r: &BacktestResult, kind: EntryType) -> Vec<(InstrumentId, Qty, Money)> {
    r.ledger
        .journal()
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (e.fill.instrument, e.fill.qty, e.amount))
        .collect()
}

#[test]
fn a_split_changes_shares_not_value() {
    // A 2-for-1 split on day 3: the unadjusted price halves.
    let data = bars(&[100.0, 100.0, 50.0, 50.0, 50.0]);
    let split = [CorporateAction::split(1, ts(3), 1, 2)];
    let r = run(&data, &split, &mut buy(10.0), &cfg());
    assert_eq!(r.ledger.position(1).qty, q(20.0));
    assert_eq!(r.ledger.position(1).cost, Money::from_raw(1_000 * SCALE), "cost basis unchanged");
    let eq: Vec<f64> = r.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
    assert!(eq.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9), "no jump at the split: {eq:?}");
    assert_eq!(entries(&r, EntryType::Split), [(1, q(20.0), Money::ZERO)]);

    // Without the action the same prices look like a 50% loss on the position.
    let plain = run_backtest_controlled(&data, &mut buy(10.0), &cfg(), &mut RunControl::default()).unwrap();
    assert_eq!(plain.ledger.position(1).qty, q(10.0));
    assert!((plain.equity_curve.last().unwrap().equity.to_f64() - 99_500.0).abs() < 1e-9);
}

#[test]
fn open_orders_are_adjusted_like_a_broker_does() {
    // A buy limit at 90 and a sell stop at 80 rest through a 2-for-1 split;
    // afterwards the price falls to 44, which fills the (halved) limit at 45.
    let data = bars(&[100.0, 100.0, 50.0, 50.0, 44.0, 44.0]);
    let mut s = Once {
        resting: Some((Side::Buy, 10.0, 90.0)),
        stop: Some((Side::Sell, 5.0, 80.0)),
        ..Once::default()
    };
    let c = BacktestConfig { ledger: LedgerConfig { allow_short: true, ..cfg().ledger }, ..cfg() };
    let r = run(&data, &[CorporateAction::split(1, ts(3), 1, 2)], &mut s, &c);
    let limit = r.orders[0];
    assert_eq!((limit.qty, limit.limit, limit.state), (q(20.0), p(45.0), OrderState::Filled));
    let fill =
        r.ledger.journal().iter().find(|e| e.kind == EntryType::Fill && e.fill.side == Side::Buy).unwrap();
    assert_eq!(
        (fill.fill.price, fill.fill.qty, fill.ts_ns),
        (p(44.0), q(20.0), ts(5)),
        "gapped below the limit"
    );
    let stop = r.orders[1];
    assert_eq!((stop.qty, stop.stop), (q(10.0), p(40.0)), "stop halved, quantity doubled; not reached");
    assert_eq!(stop.state, OrderState::Pending);
}

#[test]
fn dividends_are_paid_to_longs_and_charged_to_shorts() {
    let data = bars(&[100.0, 100.0, 100.0, 98.5, 98.5]);
    let div = [CorporateAction::dividend(1, ts(4), p(1.5))];
    let long = run(&data, &div, &mut buy(10.0), &cfg());
    assert_eq!(long.ledger.dividends(), Money::from_raw(15 * SCALE));
    assert_eq!(entries(&long, EntryType::Dividend), [(1, q(10.0), Money::from_raw(15 * SCALE))]);
    // The price drop on the ex-date is offset by the cash.
    assert!((long.equity_curve.last().unwrap().equity.to_f64() - 100_000.0).abs() < 1e-9);

    let c = BacktestConfig { ledger: LedgerConfig { allow_short: true, ..cfg().ledger }, ..cfg() };
    let short = run(&data, &div, &mut Once { side: Some(Side::Sell), units: 10.0, ..Once::default() }, &c);
    assert_eq!(short.ledger.dividends(), Money::from_raw(-15 * SCALE));
    assert!((short.equity_curve.last().unwrap().equity.to_f64() - 100_000.0).abs() < 1e-9);

    // No position, no entry.
    let flat = run(&data, &div, &mut Once::default(), &cfg());
    assert!(entries(&flat, EntryType::Dividend).is_empty());
}

#[test]
fn splits_go_before_dividends_on_the_same_day_and_strategies_are_told() {
    let data = bars(&[100.0, 100.0, 49.0, 49.0]);
    // Listed dividend first: the engine still splits first, so the dividend is paid on 20 shares.
    let actions = [CorporateAction::dividend(1, ts(3), p(1.0)), CorporateAction::split(1, ts(3), 1, 2)];
    let mut s = buy(10.0);
    let r = run(&data, &actions, &mut s, &cfg());
    assert_eq!(r.ledger.dividends(), Money::from_raw(20 * SCALE));
    assert_eq!(s.seen, [actions[1], actions[0]]);
    assert_eq!(s.position_when_told, [q(20.0), q(20.0)], "books already adjusted when told");
}

#[test]
fn a_reverse_split_keeps_fractions_exactly() {
    // 1-for-10 reverse split of 15 shares: 1.5 shares at ten times the price.
    let data = bars(&[10.0, 10.0, 100.0, 100.0]);
    let r = run(&data, &[CorporateAction::split(1, ts(3), 10, 1)], &mut buy(15.0), &cfg());
    assert_eq!(r.ledger.position(1).qty, q(1.5));
    let eq: Vec<f64> = r.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
    assert!(eq.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9), "{eq:?}");
}

#[test]
fn each_strategy_is_adjusted_on_its_own_books() {
    let data = bars(&[100.0, 100.0, 50.0, 49.0, 49.0]);
    let actions = [CorporateAction::split(1, ts(3), 1, 2), CorporateAction::dividend(1, ts(4), p(0.5))];
    let (mut a, mut b) = (buy(10.0), buy(3.0));
    let r = run_multi_backtest_with_actions(
        &data,
        &actions,
        &mut [Allocation { strategy: &mut a, weight: 0.5 }, Allocation { strategy: &mut b, weight: 0.5 }],
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    r.ledger.check_invariants().unwrap();
    let books: Vec<&Ledger> = r.strategies.iter().map(|s| &s.ledger).collect();
    for l in &books {
        l.check_invariants().unwrap();
    }
    assert_eq!((books[0].position(1).qty, books[1].position(1).qty), (q(20.0), q(6.0)));
    assert_eq!(r.ledger.position(1).qty, q(26.0), "the account is the sum");
    assert_eq!(
        (books[0].dividends(), books[1].dividends()),
        (Money::from_raw(10 * SCALE), Money::from_raw(3 * SCALE))
    );
    assert_eq!(r.ledger.dividends(), Money::from_raw(13 * SCALE));
    let last = r.equity_curve.len() - 1;
    let sum: i64 = r.strategies.iter().map(|s| s.equity_curve[last].equity.raw()).sum();
    assert_eq!(r.equity_curve[last].equity.raw(), sum);
}

#[test]
fn fingerprints_cover_the_actions_and_nothing_else_changes_without_them() {
    let data = bars(&[100.0, 100.0, 50.0, 50.0]);
    let plain = run_backtest_controlled(&data, &mut buy(10.0), &cfg(), &mut RunControl::default()).unwrap();
    let none = run(&data, &[], &mut buy(10.0), &cfg());
    assert_eq!(plain.run, none.run, "no actions: identical run, fingerprints included");

    let a = [CorporateAction::split(1, ts(3), 1, 2), CorporateAction::dividend(2, ts(2), p(0.1))];
    let with = run(&data, &a, &mut buy(10.0), &cfg());
    let reordered = run(&data, &[a[1], a[0]], &mut buy(10.0), &cfg());
    assert_eq!(with.run, reordered.run, "the order actions are given in does not matter");
    assert_ne!(with.run.config_sha256, plain.run.config_sha256);
    assert_ne!(with.run.result_sha256, plain.run.result_sha256);
    let other = run(&data, &[CorporateAction::split(1, ts(3), 1, 3)], &mut buy(10.0), &cfg());
    assert_ne!(other.run.config_sha256, with.run.config_sha256);
}

#[test]
fn bad_actions_are_refused() {
    let data = bars(&[100.0, 100.0]);
    let err = |a: CorporateAction| match run_backtest_with_actions(
        &data,
        &[a],
        &mut buy(1.0),
        &cfg(),
        &mut RunControl::default(),
    ) {
        Err(RunError::Engine(e)) => e.to_string(),
        other => panic!("expected an error, got {:?}", other.map(|r| r.run)),
    };
    assert!(err(CorporateAction::split(9, ts(2), 1, 2)).contains("no bars"));
    assert!(err(CorporateAction::split(1, ts(2), 0, 2)).contains("share counts"));
    assert!(err(CorporateAction::split(1, ts(2), 3, 3)).contains("changes nothing"));
    assert!(err(CorporateAction::dividend(1, ts(2), Price::from_raw(-1))).contains("greater than zero"));
    let twice = [CorporateAction::split(1, ts(2), 1, 2); 2];
    match run_backtest_with_actions(&data, &twice, &mut buy(1.0), &cfg(), &mut RunControl::default()) {
        Err(RunError::Engine(e)) => assert!(e.to_string().contains("listed twice")),
        other => panic!("expected an error, got {:?}", other.map(|r| r.run)),
    }
}
