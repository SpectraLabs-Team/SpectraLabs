//! M24.4 options: cash settlement at expiry against the underlying, expiry
//! at zero, and the implied volatility and Greeks in the summary.

use spectra_backtest::{run_request_on, summary_json, BacktestRequest};
use spectra_core::{
    Bar, EntryType, Instrument, InstrumentType, Json, Money, OptionRight, Price, Qty, Ratio, Side,
};
use spectra_engine::{
    run_backtest_with_setup, BacktestConfig, Context, InstrumentSetup, OptionInputs, RollKind, RunControl,
    Strategy,
};

const DAY: i64 = 86_400_000_000_000;

fn px(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

fn bar(day: i64, id: u32, close: f64) -> Bar {
    Bar::new(day * DAY, id, px(close), px(close), px(close), px(close), Qty::from_units(1_000_000).unwrap())
}

/// Underlying (id 0) moving from 100 to `final_spot` over days 1..=20; option (id 1) at `premium` every day.
fn bars(final_spot: f64, premium: f64) -> Vec<Bar> {
    let mut out = Vec::new();
    for d in 1..=20 {
        let s = 100.0 + (final_spot - 100.0) * (d - 1) as f64 / 19.0;
        out.push(bar(d, 0, s));
        out.push(bar(d, 1, premium));
    }
    out
}

/// A 100-strike option on id 0 expiring at day 15, contract size 100.
fn option(right: OptionRight) -> Instrument {
    let mut o = Instrument::equity("OPT", "USD", px(0.01), Qty::ONE);
    o.kind = InstrumentType::Option;
    o.multiplier = Ratio::from_units(100).unwrap();
    o.expiry_ns = Some(15 * DAY);
    o.strike = Some(px(100.0));
    o.option_right = Some(right);
    o.underlying = Some("UND".into());
    o
}

fn setup(right: OptionRight) -> InstrumentSetup {
    let mut s = InstrumentSetup::default();
    s.instruments.insert(1, option(right));
    s.underlying_ids.insert(1, 0);
    s
}

fn cfg() -> BacktestConfig {
    let mut c =
        BacktestConfig { initial_cash: Money::from_units(100_000).unwrap(), ..BacktestConfig::default() };
    c.ledger.allow_short = true;
    c
}

/// Trades `qty` contracts of the option on the first bar (negative: sells).
struct Trade(f64, bool);
impl Strategy for Trade {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _: &[Bar]) {
        if !self.1 {
            self.1 = true;
            let side = if self.0 > 0.0 { Side::Buy } else { Side::Sell };
            ctx.market(1, side, Qty::from_f64(self.0.abs()).unwrap());
        }
    }
}

fn run(b: &[Bar], s: &InstrumentSetup, qty: f64) -> spectra_engine::BacktestResult {
    run_backtest_with_setup(b, &[], s, &mut Trade(qty, false), &cfg(), &mut RunControl::default()).unwrap()
}

#[test]
fn an_in_the_money_call_settles_at_intrinsic_value() {
    // The underlying closes at day 15 at 100 + 19 * 14/19 = 114: intrinsic 14.
    let r = run(&bars(119.0, 5.0), &setup(OptionRight::Call), 2.0);
    r.ledger.check_invariants().unwrap();
    let e = r.rolls.iter().find(|e| e.from == 1).unwrap();
    assert_eq!((e.kind, e.ts_ns), (RollKind::Exercised, 15 * DAY));
    assert_eq!(e.close_price, px(14.0));
    assert_eq!(r.ledger.position(1).qty, Qty::ZERO);
    // Paid 2 x 100 x 5 = 1,000; received 2 x 100 x 14 = 2,800.
    assert_eq!(r.ledger.realized_pnl(), Money::from_units(1_800).unwrap());
}

#[test]
fn an_out_of_the_money_option_expires_at_zero() {
    let r = run(&bars(119.0, 2.0), &setup(OptionRight::Put), 3.0);
    r.ledger.check_invariants().unwrap();
    let e = r.rolls.iter().find(|e| e.from == 1).unwrap();
    assert_eq!(e.kind, RollKind::Expired);
    assert!(r.ledger.journal().iter().any(|j| j.kind == EntryType::Expire));
    assert_eq!(r.ledger.realized_pnl(), Money::from_units(-600).unwrap()); // the premium paid
    assert_eq!(r.ledger.cash(), Money::from_units(100_000 - 600).unwrap());
}

#[test]
fn a_short_option_keeps_the_premium_when_it_expires() {
    let r = run(&bars(90.0, 1.5), &setup(OptionRight::Call), -1.0);
    r.ledger.check_invariants().unwrap();
    assert_eq!(r.ledger.realized_pnl(), Money::from_units(150).unwrap());
}

#[test]
fn the_underlying_must_be_in_the_run() {
    let mut s = setup(OptionRight::Call);
    s.underlying_ids.clear();
    let e = run_backtest_with_setup(
        &bars(110.0, 3.0),
        &[],
        &s,
        &mut Trade(1.0, false),
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("underlying"), "{e}");
}

// ------------------------------------------------------------ request path

fn request(doc: &str) -> BacktestRequest {
    BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap()
}

#[test]
fn the_summary_reports_implied_volatility_and_greeks() {
    // An option priced by the model at 25% vol must come back at 25%.
    let spot = 105.0;
    // Expiring well after the last bar (day 20), so the option is live when the run ends.
    let expiry_day = 120;
    let t = (expiry_day - 20) as f64 * 86_400e9 / (365.25 * 86_400e9);
    let model = OptionInputs {
        right: OptionRight::Call,
        spot,
        strike: 100.0,
        years: t,
        rate: (1.0f64 + 0.03).ln(),
        dividend_yield: 0.0,
        volatility: 0.25,
    };
    let premium = (model.value().unwrap().price * 1e4).round() / 1e4;
    let mut b = Vec::new();
    for d in 1..=20 {
        b.push(bar(d, 0, spot));
        b.push(bar(d, 1, premium));
    }
    let doc = format!(
        r#"{{"strategy":"buy_and_hold","config":{{"preset":"zero_cost","risk_free_rate":0.03}},
        "instruments":[{{"symbol":"OPT","type":"option","right":"call","strike":100,"multiplier":100,"tick_size":0.01,
        "expiry":{},"underlying":"UND"}}]}}"#,
        expiry_day * DAY
    );
    let out = run_request_on(&b, &["UND".into(), "OPT".into()], &request(&doc), &mut RunControl::default())
        .unwrap();
    let s = summary_json(&out).unwrap();
    let o = &s.at("options").unwrap().at("options").unwrap().items().unwrap()[0];
    let iv = o.at("implied_volatility").unwrap().as_double().unwrap();
    assert!((iv - 0.25).abs() < 1e-4, "{iv}");
    let delta = o.at("delta").unwrap().as_double().unwrap();
    assert!(delta > 0.5 && delta < 1.0, "{delta}");
    assert!(s.at("options").unwrap().at("gaps").unwrap().items().unwrap()[0]
        .as_str()
        .unwrap()
        .contains("M26"));
}

#[test]
fn a_request_with_an_unknown_underlying_is_refused() {
    let doc = r#"{"strategy":"buy_and_hold","instruments":[{"symbol":"OPT","type":"option","right":"put","strike":100,
        "multiplier":100,"tick_size":0.01,"expiry":1000000000000000000,"underlying":"NOPE"}]}"#;
    let b = bars(100.0, 1.0);
    let e = run_request_on(&b, &["UND".into(), "OPT".into()], &request(doc), &mut RunControl::default())
        .unwrap_err();
    assert!(e.to_string().contains("NOPE"), "{e}");
    // A right that is not call or put is refused when the request is read.
    let bad = r#"{"strategy":"buy_and_hold","instruments":[{"symbol":"OPT","type":"option","right":"straddle",
        "strike":100,"tick_size":0.01,"expiry":1,"underlying":"UND"}]}"#;
    let errs = BacktestRequest::from_json(&Json::parse(bad).unwrap()).unwrap_err();
    assert!(errs.errors.iter().any(|e| e.field == "instruments[0].right"));
}
