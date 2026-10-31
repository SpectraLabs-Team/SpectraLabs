//! M24.3 bonds: accrued interest at the fill, coupons, redemption at par,
//! clean and dirty quotes, and what the run records.

use spectra_backtest::{run_request_on, summary_json, BacktestRequest};
use spectra_core::{
    Bar, DayCount, EntryType, Instrument, InstrumentType, Json, Money, Price, Qty, Ratio, Reject, Side,
};
use spectra_engine::{
    run_backtest_with_setup, BacktestConfig, BondQuote, Context, InstrumentSetup, OrderState, RollKind,
    RunControl, Strategy,
};

const DAY: i64 = 86_400_000_000_000;

fn px(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

/// One bond (id 0) quoted at `price` every day 1..=days.
fn bars(days: i64, price: f64) -> Vec<Bar> {
    (1..=days)
        .map(|d| {
            Bar::new(
                d * DAY,
                0,
                px(price),
                px(price),
                px(price),
                px(price),
                Qty::from_units(1_000_000).unwrap(),
            )
        })
        .collect()
}

/// 5% semiannual Act/Act bond maturing 1971-01-01 (day 365): coupons on 1970-07-01
/// (day 181) and at maturity. Face 1,000 per unit: multiplier 10.
fn bond() -> Instrument {
    let mut b = Instrument::equity("UST", "USD", px(0.007_812_5), Qty::ONE);
    b.kind = InstrumentType::Bond;
    b.multiplier = Ratio::from_units(10).unwrap();
    b.maturity_ns = Some(365 * DAY);
    b.coupon_rate = Some(Ratio::from_f64(0.05).unwrap());
    b.coupon_frequency = Some(2);
    b.day_count = Some(DayCount::ActActIcma);
    b
}

fn setup(quote: BondQuote) -> InstrumentSetup {
    let mut s = InstrumentSetup { bond_quote: quote, ..InstrumentSetup::default() };
    s.instruments.insert(0, bond());
    s
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_units(1_000_000).unwrap(), ..BacktestConfig::default() }
}

/// Buys `qty` (or sells, when negative) on the first bar; on `late`, tries once more.
struct Trade(f64, Option<i64>, i64);
impl Strategy for Trade {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _: &[Bar]) {
        self.2 += 1;
        let side = if self.0 > 0.0 { Side::Buy } else { Side::Sell };
        let q = Qty::from_f64(self.0.abs()).unwrap();
        if self.2 == 1 || Some(self.2) == self.1 {
            ctx.market(0, side, q);
        }
    }
}

fn run(b: &[Bar], s: &InstrumentSetup, qty: f64, late: Option<i64>) -> spectra_engine::BacktestResult {
    let mut c = cfg();
    c.ledger.allow_short = true;
    run_backtest_with_setup(b, &[], s, &mut Trade(qty, late, 0), &c, &mut RunControl::default()).unwrap()
}

#[test]
fn a_clean_quoted_purchase_pays_accrued_interest() {
    let r = run(&bars(400, 100.0), &setup(BondQuote::Clean), 10.0, None);
    r.ledger.check_invariants().unwrap();
    // Filled on day 2 (1970-01-03): two days into the Jan 1 - Jul 1 period (181 days).
    let fill = r.ledger.journal().iter().find(|e| e.kind == EntryType::Fill).unwrap();
    assert_eq!(fill.fill.price, px(100.0 + 2.5 * 2.0 / 181.0));
}

#[test]
fn coupons_are_paid_and_equity_stays_continuous() {
    let r = run(&bars(400, 100.0), &setup(BondQuote::Clean), 10.0, None);
    let coupons: Vec<_> = r.ledger.journal().iter().filter(|e| e.kind == EntryType::Coupon).collect();
    assert_eq!(coupons.len(), 2, "one mid-life coupon, one at maturity");
    assert_eq!(coupons[0].ts_ns, 181 * DAY);
    assert_eq!(coupons[0].amount, Money::from_units(250).unwrap()); // 10 units x face 1,000 x 2.5%
    assert_eq!(r.ledger.coupons(), Money::from_units(500).unwrap());
    // Across the coupon date equity moves by one day of accrual, not by the coupon.
    let eq = |day: i64| r.equity_curve.iter().find(|p| p.ts_ns == day * DAY).unwrap().equity.to_f64();
    let one_day = 10.0 * 10.0 * 2.5 / 181.0;
    assert!((eq(181) - eq(180) - one_day).abs() < 1e-4, "{} vs {one_day}", eq(181) - eq(180));
}

#[test]
fn a_bond_is_redeemed_at_par_at_maturity() {
    let r = run(&bars(400, 100.0), &setup(BondQuote::Clean), 10.0, Some(370));
    r.ledger.check_invariants().unwrap();
    let e = r.rolls.iter().find(|e| e.kind == RollKind::Redeemed).unwrap();
    assert_eq!((e.ts_ns, e.close_price, e.qty), (365 * DAY, px(100.0), Qty::from_units(10).unwrap()));
    assert_eq!(r.ledger.position(0).qty, Qty::ZERO);
    // Bought at par plus two days' accrual, received both coupons, repaid at par.
    let pnl = r.ledger.equity().unwrap().to_f64() - 1_000_000.0;
    let paid_accrued = 10.0 * 10.0 * 2.5 * 2.0 / 181.0;
    assert!((pnl - (500.0 - paid_accrued)).abs() < 1e-4, "{pnl}");
    // After maturity it cannot trade.
    let late = r.orders.last().unwrap();
    assert_eq!((late.state, late.reject_reason), (OrderState::Rejected, Some(Reject::ContractExpired)));
}

#[test]
fn dirty_quotes_are_used_as_they_are() {
    let r = run(&bars(400, 100.0), &setup(BondQuote::Dirty), 10.0, None);
    let fill = r.ledger.journal().iter().find(|e| e.kind == EntryType::Fill).unwrap();
    assert_eq!(fill.fill.price, px(100.0));
    let clean = run(&bars(400, 100.0), &setup(BondQuote::Clean), 10.0, None);
    assert_ne!(r.run.config_sha256, clean.run.config_sha256, "the convention is part of the fingerprint");
}

#[test]
fn a_short_position_pays_the_coupon() {
    let r = run(&bars(300, 100.0), &setup(BondQuote::Clean), -4.0, None);
    r.ledger.check_invariants().unwrap();
    assert_eq!(r.ledger.coupons(), Money::from_units(-100).unwrap()); // 4 x 1,000 x 2.5%
}

// ------------------------------------------------------------ request path

fn request(doc: &str) -> BacktestRequest {
    BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap()
}

const BOND: &str = r#""instruments":[{"symbol":"UST","type":"bond","multiplier":10,"tick_size":"0.0078125",
    "maturity":31536000000000000,"coupon_rate":"0.05","coupon_frequency":2,"day_count":"act_act_icma"}]"#;

#[test]
fn the_summary_reports_yield_and_risk() {
    let doc = format!(
        r#"{{"strategy":"buy_and_hold","config":{{"preset":"zero_cost"}},"bond_quote":"clean",{BOND}}}"#
    );
    let out = run_request_on(&bars(200, 98.0), &["UST".into()], &request(&doc), &mut RunControl::default())
        .unwrap();
    let s = summary_json(&out).unwrap();
    let b = s.at("bonds").unwrap();
    assert_eq!(b.at("quote").unwrap().as_str().unwrap(), "clean");
    let first = &b.at("bonds").unwrap().items().unwrap()[0];
    // Priced below par with ~half a year left: the yield is above the coupon.
    let y = first.at("yield_to_maturity").unwrap().as_double().unwrap();
    assert!(y > 0.05 && y < 0.15, "{y}");
    let d = first.at("modified_duration").unwrap().as_double().unwrap();
    assert!(d > 0.0 && d < 1.0, "{d}");
    assert!(b.at("gaps").unwrap().items().unwrap()[0].as_str().unwrap().contains("Ex-coupon"));
    let inst = &s.at("instruments").unwrap().items().unwrap()[0];
    assert!(inst.at("coupons").unwrap().as_str().is_ok());
}

#[test]
fn requests_need_the_coupon_terms() {
    let doc = r#"{"strategy":"buy_and_hold","instruments":[{"symbol":"UST","type":"bond","tick_size":0.01,
        "maturity":31536000000000000,"coupon_rate":0.05}],"bond_quote":"mid"}"#;
    let errs = BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap_err();
    let fields: Vec<&str> = errs.errors.iter().map(|e| e.field.as_str()).collect();
    assert!(fields.contains(&"bond_quote"), "{fields:?}");
    assert!(fields.contains(&"instruments[0]"), "{fields:?}"); // no coupon frequency
}

#[test]
fn a_swap_record_is_accepted_but_not_backtested() {
    // Swap terms parse and validate; the run is refused because datasets carry no curve history.
    let doc = r#"{"strategy":"buy_and_hold","instruments":[{"symbol":"UST","type":"swap","tick_size":"0.0001",
        "maturity":31536000000000000,"coupon_rate":"-0.001","coupon_frequency":2}]}"#;
    let e = run_request_on(&bars(10, 100.0), &["UST".into()], &request(doc), &mut RunControl::default())
        .unwrap_err();
    assert!(e.to_string().contains("curve"), "{e}");
}
