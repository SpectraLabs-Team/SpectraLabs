//! M24.4 swaps: marks from a dated curve history, net coupons, maturity,
//! and what the run records.

use spectra_backtest::{run_request_on, summary_json, BacktestRequest};
use spectra_core::{Bar, EntryType, Instrument, InstrumentType, Json, Money, Price, Qty, Ratio, Side};
use spectra_engine::{
    run_backtest_with_setup, swap_bars, BacktestConfig, Context, CurvePoint, CurveSnapshot, InstrumentSetup,
    QuoteKind, RollKind, RunControl, Strategy,
};

const DAY: i64 = 86_400_000_000_000;

fn px(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

/// SPY (id 0) at 100 on every day 1..=days.
fn spy(days: i64) -> Vec<Bar> {
    (1..=days)
        .map(|d| {
            Bar::new(
                d * DAY,
                0,
                px(100.0),
                px(100.0),
                px(100.0),
                px(100.0),
                Qty::from_units(1_000_000).unwrap(),
            )
        })
        .collect()
}

fn snap(day: i64, rate: f64) -> CurveSnapshot {
    let r = Ratio::from_f64(rate).unwrap();
    let swap = |y: i64| CurvePoint {
        kind: QuoteKind::Swap { frequency: 2 },
        years: Ratio::from_units(y).unwrap(),
        rate: r,
    };
    CurveSnapshot {
        as_of_ns: day * DAY,
        quotes: vec![
            CurvePoint { kind: QuoteKind::Deposit, years: Ratio::from_f64(0.5).unwrap(), rate: r },
            swap(2),
            swap(5),
        ],
    }
}

/// A 2-year semiannual receiver at 4% maturing 1972-01-01 (day 730); 1,000,000 notional a unit.
fn swap() -> Instrument {
    let mut s = Instrument::equity("USD2Y", "USD", px(0.0001), Qty::ONE);
    s.kind = InstrumentType::Swap;
    s.multiplier = Ratio::from_units(10_000).unwrap();
    s.maturity_ns = Some(730 * DAY);
    s.coupon_rate = Some(Ratio::from_f64(0.04).unwrap());
    s.coupon_frequency = Some(2);
    s.underlying = Some("USD".into());
    s
}

/// Rates at 4% until day 300, then 3%.
fn setup() -> InstrumentSetup {
    let mut s = InstrumentSetup::default();
    s.instruments.insert(1, swap());
    s.curves.insert("USD".into(), vec![snap(0, 0.04), snap(300, 0.03)]);
    s
}

fn with_swaps(setup: &InstrumentSetup, bars: Vec<Bar>) -> Vec<Bar> {
    let generated = swap_bars(setup, &bars).unwrap();
    let mut all = bars;
    all.extend(generated);
    all.sort_by_key(|b| (b.ts_ns, b.instrument));
    all
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_units(5_000_000).unwrap(), ..BacktestConfig::default() }
}

/// Trades `qty` units of the swap on the first bar (negative: pays fixed).
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

fn run(qty: f64) -> spectra_engine::BacktestResult {
    let s = setup();
    let bars = with_swaps(&s, spy(800));
    let mut c = cfg();
    c.ledger.allow_short = true;
    run_backtest_with_setup(&bars, &[], &s, &mut Trade(qty, false), &c, &mut RunControl::default()).unwrap()
}

#[test]
fn bars_are_generated_only_while_the_swap_is_alive() {
    let s = setup();
    let gen = swap_bars(&s, &spy(800)).unwrap();
    assert_eq!(gen.first().unwrap().ts_ns, DAY);
    assert_eq!(gen.last().unwrap().ts_ns, 730 * DAY); // nothing after maturity
    assert_eq!(gen.last().unwrap().close, px(100.0));
    assert!(gen.iter().all(|b| b.instrument == 1 && b.volume == Qty::ZERO));
}

#[test]
fn a_receiver_is_paid_net_coupons_gains_when_rates_fall_and_matures_at_100() {
    let r = run(2.0);
    r.ledger.check_invariants().unwrap();
    // Net coupons on every payment date it held the swap through (day 181, 365, 546, 730).
    let coupons: Vec<_> = r.ledger.journal().iter().filter(|e| e.kind == EntryType::Coupon).collect();
    let days: Vec<i64> = coupons.iter().map(|e| e.ts_ns / DAY).collect();
    assert_eq!(days, vec![181, 365, 546, 730]);
    // Fixed 4% against fixings off a 4% curve: small; then against 3% fixings: about +0.5% a period.
    let late = coupons[2].amount.to_f64();
    assert!(late > 0.0045 * 1e6 * 2.0 && late < 0.0055 * 1e6 * 2.0, "{late}");
    // The rate fall on day 300 shows up as a gain on that day.
    let eq = |d: i64| r.equity_curve.iter().find(|p| p.ts_ns == d * DAY).unwrap().equity.to_f64();
    assert!(eq(300) - eq(299) > 10_000.0, "{}", eq(300) - eq(299));
    // Across a payment date equity moves by one day's carry, not by the coupon.
    assert!((eq(365) - eq(364)).abs() < 1_000.0, "{}", eq(365) - eq(364));
    // Closed at 100 at maturity, with the event recorded.
    let m = r.rolls.iter().find(|e| e.kind == RollKind::Redeemed).unwrap();
    assert_eq!((m.ts_ns, m.close_price, m.note.as_deref()), (730 * DAY, px(100.0), Some("swap matured")));
    assert_eq!(r.ledger.position(1).qty, Qty::ZERO);
    // Everything that happened adds up: entry to 100, plus coupons, less fees.
    let entry = r.ledger.journal().iter().find(|e| e.kind == EntryType::Fill).unwrap().fill;
    let fees: f64 =
        r.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill).map(|e| e.fill.fee.to_f64()).sum();
    let expected = (100.0 - entry.price.to_f64()) * 2.0 * 10_000.0 + r.ledger.coupons().to_f64() - fees;
    let pnl = r.ledger.equity().unwrap().to_f64() - 5_000_000.0;
    assert!((pnl - expected).abs() < 1e-2, "{pnl} vs {expected}");
}

#[test]
fn a_payer_mirrors_the_receiver() {
    let rec = run(1.0);
    let pay = run(-1.0);
    pay.ledger.check_invariants().unwrap();
    let pnl = |r: &spectra_engine::BacktestResult| r.ledger.equity().unwrap().to_f64() - 5_000_000.0;
    let fees = |r: &spectra_engine::BacktestResult| -> f64 {
        r.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill).map(|e| e.fill.fee.to_f64()).sum()
    };
    // Gross of fees and the bid/ask of the entry, the two sides add up to about nothing.
    let gross = pnl(&rec) + fees(&rec) + pnl(&pay) + fees(&pay);
    assert!(gross.abs() < 0.02 * 1e6 * 1e-2, "{gross}");
    assert!(pay.ledger.coupons().to_f64() < 0.0);
}

#[test]
fn a_swap_without_its_curve_is_refused() {
    let mut s = setup();
    s.curves.clear();
    let e = run_backtest_with_setup(
        &spy(10),
        &[],
        &s,
        &mut Trade(1.0, false),
        &cfg(),
        &mut RunControl::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("curve"), "{e}");
}

#[test]
fn the_curve_history_is_part_of_the_fingerprint() {
    let a = run(1.0);
    let mut s = setup();
    s.curves.insert("USD".into(), vec![snap(0, 0.04), snap(300, 0.031)]);
    let bars = with_swaps(&s, spy(800));
    let b =
        run_backtest_with_setup(&bars, &[], &s, &mut Trade(1.0, false), &cfg(), &mut RunControl::default())
            .unwrap();
    assert_ne!(a.run.config_sha256, b.run.config_sha256);
}

// ------------------------------------------------------------ request path

const REQUEST: &str = r#"{"strategy":"buy_and_hold","config":{"preset":"zero_cost","initial_cash":5000000},
    "instruments":[{"symbol":"USD2Y","type":"swap","multiplier":10000,"tick_size":"0.0001",
        "maturity":"1972-01-01","coupon_rate":"0.04","coupon_frequency":2,"underlying":"USD-SOFR"}],
    "curves":{"USD-SOFR":[
        {"as_of":"1970-01-01","quotes":[{"type":"deposit","years":"0.5","rate":"0.04"},
            {"type":"swap","years":2,"rate":"0.04","frequency":2}]},
        {"as_of":"1970-10-28","quotes":[{"type":"deposit","years":"0.5","rate":"0.03"},
            {"type":"swap","years":2,"rate":"0.03","frequency":2}]}]}}"#;

fn request(doc: &str) -> BacktestRequest {
    BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap()
}

#[test]
fn a_request_adds_the_swap_and_reports_it() {
    let out =
        run_request_on(&spy(800), &["SPY".into()], &request(REQUEST), &mut RunControl::default()).unwrap();
    out.result.ledger.check_invariants().unwrap();
    assert_eq!(out.symbols, vec!["SPY".to_string(), "USD2Y".to_string()]);
    let s = summary_json(&out).unwrap();
    let w = s.at("swaps").unwrap();
    assert_eq!(w.at("model").unwrap().as_str().unwrap(), "single_curve");
    let first = &w.at("swaps").unwrap().items().unwrap()[0];
    assert_eq!(first.at("symbol").unwrap().as_str().unwrap(), "USD2Y");
    assert_eq!(first.at("curve").unwrap().as_str().unwrap(), "USD-SOFR");
    assert!(first.at("net_coupons").unwrap().as_str().is_ok());
    assert_eq!(w.at("maturities").unwrap().items().unwrap().len(), 1);
    assert!(w.at("gaps").unwrap().items().unwrap()[0].as_str().unwrap().contains("single-curve"));
    // The stored request reproduces the run exactly.
    let stored = BacktestRequest::from_json(&strip(&out.request.to_json().unwrap())).unwrap();
    let again = run_request_on(&spy(800), &["SPY".into()], &stored, &mut RunControl::default()).unwrap();
    assert_eq!(out.result.run.config_sha256, again.result.run.config_sha256);
    assert_eq!(out.result.run.result_sha256, again.result.run.result_sha256);
}

#[test]
fn requests_check_swaps_and_curves() {
    // A swap naming a curve the request does not give.
    let doc = REQUEST.replace("\"underlying\":\"USD-SOFR\"", "\"underlying\":\"EUR-ESTR\"");
    let e =
        run_request_on(&spy(10), &["SPY".into()], &request(&doc), &mut RunControl::default()).unwrap_err();
    assert!(e.to_string().contains("EUR-ESTR"), "{e}");
    // Bad quotes are refused when the request is read, every problem at once.
    let bad = r#"{"strategy":"buy_and_hold","curves":{"X":[{"as_of":"1970-01-01","quotes":[
        {"type":"swap","years":2,"rate":0.04},{"type":"fra","years":-1,"rate":3}]}]}}"#;
    let errs = BacktestRequest::from_json(&Json::parse(bad).unwrap()).unwrap_err();
    let fields: Vec<&str> = errs.errors.iter().map(|e| e.field.as_str()).collect();
    for f in [
        "curves.X[0].quotes[0].frequency",
        "curves.X[0].quotes[1].type",
        "curves.X[0].quotes[1].years",
        "curves.X[0].quotes[1].rate",
    ] {
        assert!(fields.contains(&f), "{f} not in {fields:?}");
    }
}

/// The stored request without the fields only the record carries.
fn strip(j: &Json) -> Json {
    let mut out = Json::object();
    for (k, v) in j.entries().unwrap() {
        if k != "strategy_sha256" && k != "config_exact" {
            out.set(k.clone(), v.clone()).unwrap();
        }
    }
    out
}
