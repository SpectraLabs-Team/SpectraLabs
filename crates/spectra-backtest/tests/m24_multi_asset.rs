//! M24 FX and multiplier tests through the engine and the request path.

use spectra_backtest::{run_request, run_request_on, BacktestRequest};
use spectra_core::{Bar, Instrument, InstrumentType, Json, Price, Qty, Ratio, Reject, Side, SCALE};
use spectra_engine::{
    run_backtest_with_setup, BacktestConfig, Context, FxRate, InstrumentSetup, OrderState, RunControl,
    Strategy, FX_SCALE,
};

const DAY: i64 = 86_400_000_000_000;

fn px(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}

/// One instrument, closes as given, open = previous close.
fn bars(closes: &[f64]) -> Vec<Bar> {
    let mut prev = closes[0];
    closes
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let (o, c2) = (prev, c);
            prev = c;
            Bar::new(
                (i as i64 + 1) * DAY,
                0,
                px(o),
                px(o.max(c2)),
                px(o.min(c2)),
                px(c2),
                Qty::from_raw(1_000_000 * SCALE),
            )
        })
        .collect()
}

/// Buys `qty` once at the start.
struct BuyOnce(Qty);
impl Strategy for BuyOnce {
    fn on_start(&mut self, ctx: &mut Context<'_>) {
        ctx.market(0, Side::Buy, self.0);
    }
    fn on_bar(&mut self, _: &mut Context<'_>, _: &[Bar]) {}
}

/// Buys `qty` on the given bar index.
struct BuyAt(usize, Qty, usize);
impl Strategy for BuyAt {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _: &[Bar]) {
        if self.2 == self.0 {
            ctx.market(0, Side::Buy, self.1);
        }
        self.2 += 1;
    }
}

fn cfg() -> BacktestConfig {
    BacktestConfig {
        initial_cash: spectra_core::Money::from_units(1_000_000).unwrap(),
        ..BacktestConfig::default()
    }
}

fn future(multiplier: i64, expiry_ns: i64) -> Instrument {
    let mut f = Instrument::equity("ESZ6", "USD", px(0.25), Qty::ONE);
    f.kind = InstrumentType::Future;
    f.multiplier = Ratio::from_units(multiplier).unwrap();
    f.expiry_ns = Some(expiry_ns);
    f
}

fn run(b: &[Bar], setup: &InstrumentSetup, s: &mut dyn Strategy) -> spectra_engine::BacktestResult {
    run_backtest_with_setup(b, &[], setup, s, &cfg(), &mut RunControl::default()).unwrap()
}

#[test]
fn future_pnl_is_fifty_times_the_equity_pnl() {
    let b = bars(&[100.0, 101.0, 103.5, 102.0, 104.25]);
    let eq = run(&b, &InstrumentSetup::default(), &mut BuyOnce(Qty::ONE));
    let mut setup = InstrumentSetup::default();
    setup.instruments.insert(0, future(50, 100 * DAY));
    let fut = run(&b, &setup, &mut BuyOnce(Qty::ONE));
    let pnl =
        |r: &spectra_engine::BacktestResult| r.ledger.equity().unwrap().raw() - cfg().initial_cash.raw();
    assert_ne!(pnl(&eq), 0);
    assert_eq!(pnl(&fut), 50 * pnl(&eq));
    assert_ne!(eq.run.config_sha256, fut.run.config_sha256);
    fut.ledger.check_invariants().unwrap();
}

#[test]
fn foreign_stock_converts_at_the_rate_known_at_each_bar() {
    // Price flat at 100 EUR; EUR moves from 1.10 to 1.20 USD on day 3.
    let b = bars(&[100.0, 100.0, 100.0, 100.0]);
    let mut setup = InstrumentSetup::default();
    setup.instruments.insert(0, Instrument::equity("SAP", "EUR", px(0.01), Qty::ONE));
    setup.fx.insert(
        "EUR".into(),
        vec![
            FxRate { as_of_ns: 0, rate: 11 * FX_SCALE / 10 },
            FxRate { as_of_ns: 3 * DAY, rate: 12 * FX_SCALE / 10 },
        ],
    );
    let r = run(&b, &setup, &mut BuyOnce(Qty::from_units(10).unwrap()));
    // Bought 10 at 110 USD on day 1; worth 10 * 120 USD from day 3.
    let fill = r.ledger.journal().iter().find(|e| e.kind == spectra_core::EntryType::Fill).unwrap();
    assert_eq!(fill.fill.price, px(110.0));
    let eq: Vec<f64> = r.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
    assert_eq!(eq[1], 1_000_000.0); // day 2: rate still 1.10, no change
    assert_eq!(eq[2], 1_000_100.0); // day 3: FX alone moved it by 10 * (120 - 110)
    r.ledger.check_invariants().unwrap();
}

#[test]
fn missing_fx_rate_is_refused_not_guessed() {
    let b = bars(&[100.0, 100.0]);
    let mut setup = InstrumentSetup::default();
    setup.instruments.insert(0, Instrument::equity("SAP", "EUR", px(0.01), Qty::ONE));
    let e =
        run_backtest_with_setup(&b, &[], &setup, &mut BuyOnce(Qty::ONE), &cfg(), &mut RunControl::default())
            .unwrap_err();
    assert!(e.to_string().contains("no FX rates from EUR"), "{e}");
    // A series that starts after the first bar is refused at that bar.
    setup.fx.insert("EUR".into(), vec![FxRate { as_of_ns: 2 * DAY, rate: FX_SCALE }]);
    let e =
        run_backtest_with_setup(&b, &[], &setup, &mut BuyOnce(Qty::ONE), &cfg(), &mut RunControl::default())
            .unwrap_err();
    assert!(e.to_string().contains("rate known at or before"), "{e}");
}

#[test]
fn expired_contract_cannot_trade() {
    let b = bars(&[100.0, 101.0, 102.0, 103.0]);
    let mut setup = InstrumentSetup::default();
    setup.instruments.insert(0, future(50, 3 * DAY)); // expires at the third bar
    let r = run(&b, &setup, &mut BuyAt(1, Qty::ONE, 0)); // order fills at bar 3
    assert_eq!(r.orders[0].state, OrderState::Rejected);
    assert_eq!(r.orders[0].reject_reason, Some(Reject::ContractExpired));
}

#[test]
fn types_without_a_model_are_refused_with_a_reason() {
    let b = bars(&[100.0, 101.0]);
    let mut setup = InstrumentSetup::default();
    let mut swap = Instrument::equity("IRS5Y", "USD", px(0.0001), Qty::ONE);
    swap.kind = InstrumentType::Swap;
    setup.instruments.insert(0, swap);
    let e =
        run_backtest_with_setup(&b, &[], &setup, &mut BuyOnce(Qty::ONE), &cfg(), &mut RunControl::default())
            .unwrap_err();
    assert!(e.to_string().contains("swap"), "{e}");
}

// ------------------------------------------------------------ request path

fn request(doc: &str) -> BacktestRequest {
    BacktestRequest::from_json(&Json::parse(doc).unwrap()).unwrap()
}

#[test]
fn plain_equity_records_change_nothing() {
    let b = bars(&[100.0, 101.0, 99.0, 102.0, 104.0, 103.0]);
    let syms = vec!["AAPL".to_string()];
    let plain =
        run_request_on(&b, &syms, &request(r#"{"strategy":"buy_and_hold"}"#), &mut RunControl::default())
            .unwrap();
    let with = run_request_on(
        &b,
        &syms,
        &request(r#"{"strategy":"buy_and_hold","base_currency":"USD","instruments":[{"symbol":"AAPL","tick_size":0.01}]}"#),
        &mut RunControl::default(),
    )
    .unwrap();
    assert_eq!(plain.result.run, with.result.run);
}

#[test]
fn stored_request_reproduces_its_fingerprint() {
    let b = bars(&[100.0, 101.0, 99.0, 102.0, 104.0, 103.0]);
    let syms = vec!["SAP".to_string()];
    let doc = r#"{"strategy":"buy_and_hold","instruments":[{"symbol":"SAP","currency":"EUR","tick_size":0.01}],
        "fx_rates":{"EUR":[{"as_of":"1970-01-01","rate":"1.0845"},{"as_of":"1970-01-04","rate":1.1}]}}"#;
    let req = request(doc);
    let a = run_request_on(&b, &syms, &req, &mut RunControl::default()).unwrap();
    let stored = req.to_json().unwrap();
    let again = BacktestRequest::from_json(&strip(&stored)).unwrap();
    let b2 = run_request_on(&b, &syms, &again, &mut RunControl::default()).unwrap();
    assert_eq!(a.result.run, b2.result.run);
    // and the summary says which currencies were converted
    let s = spectra_backtest::summary_json(&a).unwrap();
    assert_eq!(s.at("run").unwrap().at("base_currency").unwrap().as_str().unwrap(), "USD");
    let inst = s.at("instruments").unwrap().items().unwrap();
    assert_eq!(inst[0].at("currency").unwrap().as_str().unwrap(), "EUR");
}

/// The stored request without the fields a request does not accept (the
/// strategy hash and the exact config, which the job writes for the record).
fn strip(j: &Json) -> Json {
    let mut out = Json::object();
    for (k, v) in j.entries().unwrap() {
        if k != "strategy_sha256" && k != "config_exact" {
            out.set(k.clone(), v.clone()).unwrap();
        }
    }
    out
}

#[test]
fn request_without_symbols_names_instruments_by_id() {
    let b = bars(&[100.0, 101.0, 103.0]);
    let req = request(
        r#"{"strategy":"buy_and_hold","instruments":[{"instrument":0,"type":"future","multiplier":"50","tick_size":"0.25","expiry":100000000000000000}]}"#,
    );
    let out = run_request(&b, &req, &mut RunControl::default()).unwrap();
    assert_eq!(out.result.ledger.multiplier(0), Ratio::from_units(50).unwrap());
}
