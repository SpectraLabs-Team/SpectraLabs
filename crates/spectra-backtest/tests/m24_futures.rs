//! M24.2 futures: the roll rule, rolls on real contracts, the continuous
//! series, settlement at expiry, and what the run records.

use spectra_backtest::{artifacts, run_request_on, summary_json, BacktestRequest};
use spectra_core::{Bar, EntryType, Instrument, InstrumentType, Json, Money, Price, Qty, Ratio, Side};
use spectra_engine::{
    run_backtest_with_setup, run_multi_backtest_with_setup, Allocation, BacktestConfig, Context,
    InstrumentSetup, RollAdjust, RollKind, RollRule, RunControl, Strategy,
};

const DAY: i64 = 86_400_000_000_000;

fn px(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

fn bar(day: i64, id: u32, open: f64, close: f64) -> Bar {
    Bar::new(
        day * DAY,
        id,
        px(open),
        px(open.max(close)),
        px(open.min(close)),
        px(close),
        Qty::from_units(1_000_000).unwrap(),
    )
}

/// Two contracts of chain ES on every day 1..=days: A (id 0) around 100, B (id 1) 10 higher.
fn chain_bars(days: i64) -> Vec<Bar> {
    let mut out = Vec::new();
    for d in 1..=days {
        let a = 100.0 + d as f64;
        out.push(bar(d, 0, a - 0.5, a));
        out.push(bar(d, 1, a + 9.5, a + 10.0));
    }
    out
}

fn future(sym: &str, expiry_day: i64) -> Instrument {
    let mut f = Instrument::equity(sym, "USD", px(0.25), Qty::ONE);
    f.kind = InstrumentType::Future;
    f.multiplier = Ratio::from_units(50).unwrap();
    f.expiry_ns = Some(expiry_day * DAY);
    f.underlying = Some("ES".into());
    f
}

/// A expires on day 12 (a Tuesday: 1970-01-13), B on day 40; roll two business days before.
fn setup(days_before: u32) -> InstrumentSetup {
    let mut s = InstrumentSetup {
        roll: RollRule { business_days_before_expiry: days_before, adjust: RollAdjust::Ratio },
        ..InstrumentSetup::default()
    };
    s.instruments.insert(0, future("ESA", 12));
    s.instruments.insert(1, future("ESB", 40));
    s
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_units(10_000_000).unwrap(), ..BacktestConfig::default() }
}

/// Buys one front contract on the first bar.
struct BuyFront {
    done: bool,
}
impl Strategy for BuyFront {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        if self.done {
            return;
        }
        for b in bars {
            if ctx.is_front(b.instrument) {
                ctx.market(b.instrument, Side::Buy, Qty::ONE);
                self.done = true;
            }
        }
    }
}

#[test]
fn a_held_contract_rolls_into_the_next_one() {
    let bars = chain_bars(20);
    let s = setup(2);
    let r = run_backtest_with_setup(
        &bars,
        &[],
        &s,
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    r.ledger.check_invariants().unwrap();

    // Bought ESA at day 2's open; rolled on 1970-01-09 (day 8, two weekdays before Tuesday the 13th).
    assert_eq!(r.rolls.len(), 1, "{:?}", r.rolls);
    let e = &r.rolls[0];
    assert_eq!((e.kind, e.from, e.to), (RollKind::Rolled, 0, Some(1)));
    assert_eq!(e.ts_ns, 8 * DAY);
    assert_eq!(e.qty, Qty::ONE);
    assert_eq!(e.close_price, px(107.5)); // ESA's open on day 8
    assert_eq!(e.open_price, Some(px(117.5))); // ESB's open on day 8
    assert_eq!(r.ledger.position(0).qty, Qty::ZERO);
    assert_eq!(r.ledger.position(1).qty, Qty::ONE);

    // P&L is exactly the two legs: ESA 101.5 -> 107.5, ESB 117.5 -> 130 (day 20 close), times 50.
    let pnl = r.ledger.equity().unwrap().checked_sub(cfg().initial_cash).unwrap();
    assert_eq!(pnl, Money::from_f64((107.5 - 101.5) * 50.0 + (130.0 - 117.5) * 50.0).unwrap());
    // The engine's two fills are marked with order id 0.
    let fills = r.ledger.journal().iter().filter(|j| j.kind == EntryType::Fill).count();
    assert_eq!(fills, 3);
    assert_eq!(r.fill_orders, vec![1, 0, 0]);
}

#[test]
fn the_roll_rule_moves_the_roll() {
    let bars = chain_bars(20);
    let r = run_backtest_with_setup(
        &bars,
        &[],
        &setup(0),
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    assert_eq!(r.rolls[0].ts_ns, 12 * DAY, "offset 0 rolls on the expiry day");
    let r5 = run_backtest_with_setup(
        &bars,
        &[],
        &setup(5),
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    // Five weekdays before Tuesday 1970-01-13 is Tuesday 1970-01-06 (day 5).
    assert_eq!(r5.rolls[0].ts_ns, 5 * DAY);
    assert_ne!(r.run.config_sha256, r5.run.config_sha256, "the rule is part of the fingerprint");
}

#[test]
fn continuous_series_is_back_adjusted_without_look_ahead() {
    let bars = chain_bars(20);
    let r = run_backtest_with_setup(
        &bars,
        &[],
        &setup(2),
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    let c = &r.continuous["ES"];
    assert_eq!(c.len(), 20);
    // Bars up to day 7 came from ESA, from day 8 from ESB.
    assert!(c[..7].iter().all(|b| b.instrument == 0));
    assert!(c[7..].iter().all(|b| b.instrument == 1));
    // Ratio adjustment at the roll: ESA's closes scaled by ESB / ESA at day 8 (118 / 108).
    let k = 118.0 / 108.0;
    assert!((c[0].close.to_f64() - 101.0 * k).abs() < 1e-6);
    assert_eq!(c[19].close, px(130.0));
    // Percentage moves are kept: day 1 -> 2 on the series equals ESA's own move.
    let series_move = c[1].close.to_f64() / c[0].close.to_f64();
    assert!((series_move - 102.0 / 101.0).abs() < 1e-8);
}

#[test]
fn a_contract_without_prices_is_settled_at_expiry() {
    // ESA trades only to day 6 and expires on day 12; an equity keeps the clock going.
    let mut bars = Vec::new();
    for d in 1..=15 {
        if d <= 6 {
            bars.push(bar(d, 0, 100.0 + d as f64, 100.5 + d as f64));
        }
        bars.push(bar(d, 2, 50.0, 50.0));
    }
    let mut s = setup(2);
    s.instruments.remove(&1);
    let r = run_backtest_with_setup(
        &bars,
        &[],
        &s,
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    r.ledger.check_invariants().unwrap();
    let e = &r.rolls[0];
    assert_eq!(e.kind, RollKind::Settled);
    assert_eq!(e.ts_ns, 12 * DAY);
    assert_eq!(e.close_price, px(106.5), "the last mark");
    assert_eq!(r.ledger.position(0).qty, Qty::ZERO);
}

#[test]
fn a_chain_without_a_next_contract_closes() {
    let bars: Vec<Bar> =
        chain_bars(20).into_iter().filter(|b| b.instrument == 0 || b.ts_ns > 30 * DAY).collect();
    let mut s = setup(2);
    s.instruments.remove(&1);
    let r = run_backtest_with_setup(
        &bars,
        &[],
        &s,
        &mut BuyFront { done: false },
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap();
    assert_eq!(r.rolls[0].kind, RollKind::Closed);
    assert!(r.rolls[0].note.as_deref().unwrap().contains("no later contract"));
}

#[test]
fn each_strategy_rolls_its_own_position() {
    let bars = chain_bars(20);
    let (mut a, mut b) = (BuyFront { done: false }, BuyFront { done: false });
    let mut alloc =
        [Allocation { strategy: &mut a, weight: 0.5 }, Allocation { strategy: &mut b, weight: 0.5 }];
    let r =
        run_multi_backtest_with_setup(&bars, &[], &setup(2), &mut alloc, &cfg(), &mut RunControl::default())
            .unwrap();
    assert_eq!(r.rolls.len(), 2);
    assert_eq!(r.rolls.iter().map(|e| e.strategy).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(r.ledger.position(1).qty, Qty::from_units(2).unwrap());
    for s in &r.strategies {
        assert_eq!(s.ledger.position(1).qty, Qty::ONE);
        s.ledger.check_invariants().unwrap();
    }
    r.ledger.check_invariants().unwrap();
}

// ------------------------------------------------------------ request path

fn request(doc: &str) -> BacktestRequest {
    BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap()
}

const INSTRUMENTS: &str = r#""instruments":[
    {"symbol":"ESA","type":"future","underlying":"ES","multiplier":50,"tick_size":0.25,"expiry":1036800000000000},
    {"symbol":"ESB","type":"future","underlying":"ES","multiplier":50,"tick_size":0.25,"expiry":3456000000000000}]"#;

#[test]
fn built_in_strategies_trade_the_front_contract() {
    let bars = chain_bars(30);
    let syms = vec!["ESA".to_string(), "ESB".to_string()];
    for strat in [
        r#""strategy":"buy_and_hold""#,
        r#""strategy":"sma_cross","params":{"fast":2,"slow":4,"qty":1}"#,
        r#""strategy":"equal_weight","params":{"rebalance_every":3}"#,
    ] {
        let doc = format!(
            r#"{{{strat},"config":{{"preset":"zero_cost","initial_cash":5000000}},"futures_roll":{{"business_days_before_expiry":2}},{INSTRUMENTS}}}"#
        );
        let out = run_request_on(&bars, &syms, &request(&doc), &mut RunControl::default()).unwrap();
        out.result.ledger.check_invariants().unwrap();
        // Before ESA's roll (day 8) nothing in ESB was opened by the strategy itself.
        let early_b = out.result.orders.iter().any(|o| o.instrument == 1 && o.submit_ts < 8 * DAY);
        assert!(!early_b, "{strat}: entered the back contract");
        assert!(
            out.result.orders.iter().all(|o| o.instrument == 1 || o.submit_ts < 8 * DAY),
            "{strat}: traded ESA after its roll"
        );
        // Anything that would have added to ESA on or after its roll date was refused.
        for o in out
            .result
            .orders
            .iter()
            .filter(|o| o.instrument == 0 && o.state == spectra_engine::OrderState::Rejected)
        {
            assert_eq!(o.reject_reason, Some(spectra_core::Reject::ContractRolled), "{strat}");
        }
    }
}

#[test]
fn summary_and_artifacts_record_the_rolls() {
    let bars = chain_bars(20);
    let syms = vec!["ESA".to_string(), "ESB".to_string()];
    let doc = format!(
        r#"{{"strategy":"buy_and_hold","config":{{"preset":"zero_cost"}},"futures_roll":{{"business_days_before_expiry":2,"adjust":"difference"}},{INSTRUMENTS}}}"#
    );
    let req = request(&doc);
    let out = run_request_on(&bars, &syms, &req, &mut RunControl::default()).unwrap();
    let s = summary_json(&out).unwrap();
    let f = s.at("futures").unwrap();
    assert_eq!(f.at("roll_rule").unwrap().at("business_days_before_expiry").unwrap().as_int().unwrap(), 2);
    assert_eq!(f.at("roll_rule").unwrap().at("adjust").unwrap().as_str().unwrap(), "difference");
    let rolls = f.at("rolls").unwrap().items().unwrap();
    assert_eq!(rolls[0].at("from").unwrap().as_str().unwrap(), "ESA");
    assert_eq!(rolls[0].at("to").unwrap().as_str().unwrap(), "ESB");
    assert!(f.at("gaps").unwrap().items().unwrap()[0].as_str().unwrap().contains("Margin is not modelled"));
    let names: Vec<&str> = artifacts(&out.result).iter().map(|a| a.name).collect();
    assert!(names.contains(&"rolls.csv") && names.contains(&"continuous.csv"));

    // The stored request re-runs to the same fingerprint, roll rule included.
    let stored = req.to_json().unwrap();
    let mut clean = Json::object();
    for (k, v) in stored.entries().unwrap() {
        if k != "strategy_sha256" && k != "config_exact" {
            clean.set(k.clone(), v.clone()).unwrap();
        }
    }
    let again = BacktestRequest::from_json(&clean).unwrap();
    let out2 = run_request_on(&bars, &syms, &again, &mut RunControl::default()).unwrap();
    assert_eq!(out.result.run, out2.result.run);

    // Bad rules are refused with their field.
    let bad = format!(
        r#"{{"strategy":"buy_and_hold","futures_roll":{{"business_days_before_expiry":99,"adjust":"x"}},{INSTRUMENTS}}}"#
    );
    let errs = BacktestRequest::from_json(&Json::parse(&bad).unwrap()).unwrap_err();
    let fields: Vec<&str> = errs.errors.iter().map(|e| e.field.as_str()).collect();
    assert!(
        fields.contains(&"futures_roll.business_days_before_expiry")
            && fields.contains(&"futures_roll.adjust"),
        "{fields:?}"
    );
}

#[test]
fn an_equity_run_has_no_futures_section() {
    let bars: Vec<Bar> = (1..=10).map(|d| bar(d, 0, 100.0, 101.0)).collect();
    let out = run_request_on(
        &bars,
        &["AAPL".into()],
        &request(r#"{"strategy":"buy_and_hold"}"#),
        &mut RunControl::default(),
    )
    .unwrap();
    assert!(summary_json(&out).unwrap().find("futures").is_none());
    assert!(out.result.rolls.is_empty() && out.result.continuous.is_empty());
}

#[test]
fn a_fully_invested_position_rolls_into_a_pricier_contract() {
    // Futures are marked to market: rolling needs no cash for the notional,
    // so the whole position moves even though ESB costs more than ESA.
    let bars = chain_bars(20);
    let syms = vec!["ESA".to_string(), "ESB".to_string()];
    let doc = format!(
        r#"{{"strategy":"buy_and_hold","config":{{"preset":"zero_cost"}},"futures_roll":{{"business_days_before_expiry":2}},{INSTRUMENTS}}}"#
    );
    let out = run_request_on(&bars, &syms, &request(&doc), &mut RunControl::default()).unwrap();
    let r = &out.result;
    r.ledger.check_invariants().unwrap();
    let held = r.rolls[0].qty;
    assert!(held > Qty::from_units(10).unwrap(), "sized to the account: {held}");
    assert_eq!(r.rolls[0].kind, RollKind::Rolled, "{:?}", r.rolls[0]);
    assert_eq!(r.ledger.position(1).qty, held);
    // Only realized P&L moved cash (no costs in this run).
    assert_eq!(
        r.ledger.cash(),
        BacktestConfig::default().initial_cash.checked_add(r.ledger.realized_pnl()).unwrap()
    );
}

#[test]
fn adding_to_a_rolled_contract_is_refused() {
    /// Buys ESA on the first bar, then on day 7 asks for one more.
    struct Late(i64);
    impl Strategy for Late {
        fn on_bar(&mut self, ctx: &mut Context<'_>, _: &[Bar]) {
            self.0 += 1;
            if self.0 == 1 {
                ctx.market(0, Side::Buy, Qty::from_units(3).unwrap());
            }
            if self.0 == 7 {
                ctx.market(0, Side::Buy, Qty::ONE);
            }
        }
    }
    // ESA has no bar on day 8 (its roll date), so the engine cannot roll it then and
    // the add order meets the rule at its fill on day 9.
    let bars: Vec<Bar> =
        chain_bars(12).into_iter().filter(|b| !(b.instrument == 0 && b.ts_ns == 8 * DAY)).collect();
    let r = run_backtest_with_setup(&bars, &[], &setup(2), &mut Late(0), &cfg(), &mut RunControl::default())
        .unwrap();
    assert_eq!(r.orders[1].reject_reason, Some(spectra_core::Reject::ContractRolled), "{:?}", r.orders[1]);
    r.ledger.check_invariants().unwrap();
}
