//! Several strategies on one account (M3 task 13): capital allocation per
//! strategy, orders and fills tagged with their strategy, per-strategy books
//! that add up to the account, and unchanged single-strategy runs.

use spectra_core::{
    Bar, EntryType, InstrumentId, Ledger, LedgerConfig, Money, Price, Qty, Reject, Side, SCALE,
};
use spectra_engine::{
    run_backtest, run_multi_backtest, Allocation, BacktestConfig, BacktestResult, Context, FinancingConfig,
    OrderState, RebalanceConfig, Rebalancer, RunControl, RunError, Strategy, WeightStrategy,
};

/// Daily bars for instruments 1 and 2; `px(step, instrument)` gives the price (open = close).
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

/// Prices that move, so the books have something to disagree about.
fn wavy(n: usize) -> Vec<Bar> {
    bars(n, |i, id| {
        let t = i as f64;
        if id == 1 {
            100.0 + 8.0 * (t / 5.0).sin() + t * 0.2
        } else {
            50.0 + 4.0 * (t / 3.0).cos() - t * 0.05
        }
    })
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() }
}

/// Fixed target weights on every bar.
struct Fixed(Vec<(InstrumentId, f64)>);
impl WeightStrategy for Fixed {
    fn weights(&mut self, _ctx: &Context<'_>, _bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
        Some(self.0.clone())
    }
}

fn hold(w: Vec<(InstrumentId, f64)>) -> Rebalancer<Fixed> {
    Rebalancer::new(Fixed(w), RebalanceConfig::default())
}

/// Places one market order on the first bar, and counts the fills it is told about.
#[derive(Default)]
struct Once {
    side: Option<Side>,
    instrument: InstrumentId,
    units: i64,
    sent: bool,
    fills: usize,
    seen_strategy: Option<usize>,
}

impl Once {
    fn new(instrument: InstrumentId, side: Side, units: i64) -> Self {
        Self { side: Some(side), instrument, units, ..Self::default() }
    }
}

impl Strategy for Once {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        self.seen_strategy = Some(ctx.strategy());
        if !self.sent {
            self.sent = true;
            if let Some(side) = self.side {
                ctx.market(self.instrument, side, Qty::from_raw(self.units * SCALE));
            }
        }
    }
    fn on_fill(&mut self, _ctx: &mut Context<'_>, _fill: &spectra_core::Fill) {
        self.fills += 1;
    }
}

fn multi(data: &[Bar], allocations: &mut [Allocation<'_>], c: &BacktestConfig) -> BacktestResult {
    let r = run_multi_backtest(data, allocations, c, &mut RunControl::default()).unwrap();
    r.ledger.check_invariants().unwrap();
    for s in &r.strategies {
        s.ledger.check_invariants().unwrap();
    }
    r
}

fn units(l: &Ledger, id: InstrumentId) -> f64 {
    l.position(id).qty.to_f64()
}

#[test]
fn each_strategy_sizes_from_its_own_capital() {
    // Both hold instrument 1 fully (98% of the 1x limit) on 70% and 30% of 100,000 at 100.
    let (mut a, mut b) = (hold(vec![(1, 1.0)]), hold(vec![(1, 1.0)]));
    let r = multi(
        &bars(5, |_, id| if id == 1 { 100.0 } else { 50.0 }),
        &mut [Allocation { strategy: &mut a, weight: 0.7 }, Allocation { strategy: &mut b, weight: 0.3 }],
        &cfg(),
    );
    assert_eq!(r.strategies[0].allocation, Money::from_raw(70_000 * SCALE));
    assert_eq!(r.strategies[1].allocation, Money::from_raw(30_000 * SCALE));
    assert_eq!((units(&r.strategies[0].ledger, 1), units(&r.strategies[1].ledger, 1)), (686.0, 294.0));
    assert_eq!(units(&r.ledger, 1), 980.0);
    assert!(r.unallocated.is_none());
}

#[test]
fn the_account_is_the_sum_of_the_strategies_books() {
    // A long-only rebalancer, a short with a borrow fee, and 20% left unallocated earning interest.
    let c = BacktestConfig {
        ledger: LedgerConfig { allow_short: true, ..cfg().ledger },
        financing: FinancingConfig { borrow_rate: 0.03, cash_rate: 0.02, margin_rate: 0.0 },
        ..cfg()
    };
    let mut long = hold(vec![(1, 0.6), (2, 0.4)]);
    let mut short = Once::new(2, Side::Sell, 200);
    let data = wavy(60);
    let r = multi(
        &data,
        &mut [
            Allocation { strategy: &mut long, weight: 0.5 },
            Allocation { strategy: &mut short, weight: 0.3 },
        ],
        &c,
    );
    let idle = r.unallocated.as_ref().expect("20% unallocated");
    assert_eq!(idle.net_deposits(), Money::from_raw(20_000 * SCALE));
    assert!(idle.financing().raw() > 0, "unallocated cash earns interest");

    for (step, p) in r.equity_curve.iter().enumerate() {
        let books: i64 = r.strategies.iter().map(|s| s.equity_curve[step].equity.raw()).sum();
        let cash: i64 = r.strategies.iter().map(|s| s.equity_curve[step].cash.raw()).sum();
        // The unallocated cash only ever changes by interest; compare the totals at the end.
        if step + 1 == r.equity_curve.len() {
            assert_eq!(p.equity.raw(), books + idle.cash().raw(), "equity at the end");
            assert_eq!(p.cash.raw(), cash + idle.cash().raw(), "cash at the end");
        }
        assert!(p.equity.raw() >= books, "step {step}: the idle cash is never negative");
    }
    for id in [1, 2] {
        let sum: f64 = r.strategies.iter().map(|s| units(&s.ledger, id)).sum();
        assert_eq!(units(&r.ledger, id), sum, "instrument {id}");
    }
    let fees: i64 = r.strategies.iter().map(|s| s.ledger.total_fees().raw()).sum();
    assert_eq!(r.ledger.total_fees().raw(), fees);
    let financing: i64 =
        r.strategies.iter().map(|s| s.ledger.financing().raw()).sum::<i64>() + idle.financing().raw();
    assert_eq!(r.ledger.financing().raw(), financing);
    assert!(
        r.strategies[1].ledger.journal().iter().any(|e| e.kind == EntryType::BorrowFee),
        "the short pays"
    );
    assert!(
        !r.strategies[0].ledger.journal().iter().any(|e| e.kind == EntryType::BorrowFee),
        "the long does not"
    );

    // Every book, the account's included, re-derives from its journal.
    let replayed = Ledger::replay(c.ledger, r.ledger.journal()).unwrap();
    assert_eq!(replayed.cash(), r.ledger.cash());
    for s in &r.strategies {
        assert_eq!(Ledger::replay(c.ledger, s.ledger.journal()).unwrap().cash(), s.ledger.cash());
    }
}

#[test]
fn a_strategy_cannot_spend_another_strategys_cash() {
    // 500 units at 100 is 50,000: more than the 20,000 this strategy has, less than the account's 100,000.
    let mut greedy = Once::new(1, Side::Buy, 500);
    let mut quiet = Once::default();
    let r = multi(
        &bars(3, |_, _| 100.0),
        &mut [
            Allocation { strategy: &mut greedy, weight: 0.2 },
            Allocation { strategy: &mut quiet, weight: 0.8 },
        ],
        &BacktestConfig { ledger: LedgerConfig { max_gross_leverage: None, ..cfg().ledger }, ..cfg() },
    );
    assert_eq!(r.orders[0].state, OrderState::Rejected);
    assert_eq!(r.orders[0].reject_reason, Some(Reject::InsufficientCash));
    assert!(r.ledger.positions().values().all(|p| p.qty.is_zero()));

    // The same order alone on the account fills.
    let alone = run_backtest(&bars(3, |_, _| 100.0), &mut Once::new(1, Side::Buy, 500), &cfg()).unwrap();
    assert_eq!(alone.orders[0].state, OrderState::Filled);
}

#[test]
fn orders_and_fills_are_tagged_with_their_strategy() {
    let mut a = Once::new(1, Side::Buy, 10);
    let mut b = Once::new(2, Side::Buy, 20);
    let mut c = Once::new(1, Side::Buy, 5);
    let r = multi(
        &bars(4, |_, id| if id == 1 { 100.0 } else { 50.0 }),
        &mut [
            Allocation { strategy: &mut a, weight: 0.4 },
            Allocation { strategy: &mut b, weight: 0.4 },
            Allocation { strategy: &mut c, weight: 0.2 },
        ],
        &cfg(),
    );
    // Strategies are called in order, so their orders are numbered in that order.
    let tags: Vec<usize> = r.orders.iter().map(|o| o.strategy).collect();
    assert_eq!(tags, [0, 1, 2]);
    assert_eq!(r.fill_orders, [1, 2, 3]);
    assert_eq!((a.fills, b.fills, c.fills), (1, 1, 1), "each fill reaches only its owner");
    assert_eq!((a.seen_strategy, b.seen_strategy, c.seen_strategy), (Some(0), Some(1), Some(2)));
    assert_eq!(units(&r.strategies[0].ledger, 1), 10.0);
    assert_eq!(units(&r.strategies[2].ledger, 1), 5.0);
    assert_eq!(units(&r.strategies[1].ledger, 1), 0.0);
    assert_eq!(units(&r.ledger, 1), 15.0);
}

/// Places a resting limit order far below the market.
struct Rest(bool);
impl Strategy for Rest {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        if !self.0 {
            self.0 = true;
            ctx.limit(1, Side::Buy, Qty::from_raw(SCALE), Price::from_raw(SCALE));
        }
    }
}

/// Tries to cancel order 1, which another strategy placed.
struct Meddle(Option<bool>);
impl Strategy for Meddle {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        if self.0.is_none() {
            self.0 = Some(ctx.cancel(1));
        }
    }
}

#[test]
fn a_strategy_can_only_cancel_its_own_orders() {
    let (mut rest, mut meddle) = (Rest(false), Meddle(None));
    let r = multi(
        &bars(3, |_, _| 100.0),
        &mut [
            Allocation { strategy: &mut rest, weight: 0.5 },
            Allocation { strategy: &mut meddle, weight: 0.5 },
        ],
        &cfg(),
    );
    assert_eq!(meddle.0, Some(false));
    assert_eq!(r.orders[0].state, OrderState::Pending);
}

#[test]
fn one_strategy_with_everything_trades_like_a_single_run() {
    let data = wavy(40);
    let single = run_backtest(&data, &mut hold(vec![(1, 0.5), (2, 0.5)]), &cfg()).unwrap();
    let mut s = hold(vec![(1, 0.5), (2, 0.5)]);
    let r = multi(&data, &mut [Allocation { strategy: &mut s, weight: 1.0 }], &cfg());
    assert_eq!(r.equity_curve, single.equity_curve);
    assert_eq!(r.strategies[0].equity_curve, single.equity_curve);
    assert_eq!(r.ledger.journal(), single.ledger.journal());
    // A multi-strategy run is a different configuration, so its fingerprints differ.
    assert_ne!(r.run.config_sha256, single.run.config_sha256);
    assert_ne!(r.run.result_sha256, single.run.result_sha256);
    assert_eq!(r.run.dataset_sha256, single.run.dataset_sha256);
    // A single run has no per-strategy books.
    assert!(single.strategies.is_empty() && single.unallocated.is_none());
    assert_eq!(single.fill_orders.len(), single.fill_count());
}

#[test]
fn fingerprints_are_deterministic_and_depend_on_the_weights() {
    let data = wavy(30);
    let run = |w: [f64; 2]| {
        let (mut a, mut b) = (hold(vec![(1, 1.0)]), hold(vec![(2, 1.0)]));
        multi(
            &data,
            &mut [
                Allocation { strategy: &mut a, weight: w[0] },
                Allocation { strategy: &mut b, weight: w[1] },
            ],
            &cfg(),
        )
        .run
    };
    assert_eq!(run([0.6, 0.4]), run([0.6, 0.4]));
    assert_ne!(run([0.6, 0.4]).config_sha256, run([0.4, 0.6]).config_sha256);
    assert_ne!(run([0.6, 0.4]).result_sha256, run([0.4, 0.6]).result_sha256);
}

#[test]
fn bad_allocations_are_refused() {
    let data = bars(3, |_, _| 100.0);
    let err = |weights: &[f64]| {
        let mut strategies: Vec<Once> = weights.iter().map(|_| Once::default()).collect();
        let mut allocations: Vec<Allocation<'_>> = strategies
            .iter_mut()
            .zip(weights)
            .map(|(s, &weight)| Allocation { strategy: s, weight })
            .collect();
        match run_multi_backtest(&data, &mut allocations, &cfg(), &mut RunControl::default()) {
            Err(RunError::Engine(e)) => e.to_string(),
            other => panic!("expected an error, got {:?}", other.map(|r| r.run)),
        }
    };
    assert!(err(&[]).contains("1 to 32 strategies"));
    assert!(err(&[0.7, 0.4]).contains("add up to more than 1"));
    assert!(err(&[0.5, 0.0]).contains("strategy 1: weight"));
    assert!(err(&[f64::NAN]).contains("strategy 0: weight"));
    assert!(err(&[1.5]).contains("strategy 0: weight"));
}
