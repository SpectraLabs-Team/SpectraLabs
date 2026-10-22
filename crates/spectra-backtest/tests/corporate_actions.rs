//! Corporate actions in backtest requests (M3 task 10): parsing with field
//! paths, resolving symbols against the dataset, the summary's dividend and
//! action counts, and trades that span a split.

use spectra_backtest::*;
use spectra_core::{Bar, Json, Money, Price, Qty, Side, SCALE};
use spectra_engine::{run_backtest_with_actions, Context, CorporateAction, RunControl, Strategy};

const DAY: i64 = 86_400_000_000_000;

/// Daily bars for instruments 0 (AAA) and 1 (BBB) with open = close; AAA follows `aaa`.
fn bars(aaa: &[f64]) -> Vec<Bar> {
    let mut out = Vec::new();
    for (i, &p) in aaa.iter().enumerate() {
        for (id, px) in [(0, p), (1, 20.0)] {
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

fn symbols() -> Vec<String> {
    vec!["AAA".into(), "BBB".into()]
}

fn json(s: &str) -> Json {
    Json::parse(s).unwrap()
}

/// Day 3 of `bars` is 1970-01-04 (bars start at day 1 = 1970-01-02).
const REQUEST: &str = r#"{
    "strategy": "buy_and_hold",
    "config": { "preset": "zero_cost" },
    "corporate_actions": [
        { "symbol": "AAA", "ex_date": "1970-01-04", "type": "split", "from": 1, "to": 2 },
        { "symbol": "AAA", "ex_date": "1970-01-05", "type": "dividend", "amount": 0.5 }
    ]
}"#;

#[test]
fn actions_parse_and_are_stored_with_the_request() {
    let req = BacktestRequest::from_json(&json(REQUEST)).unwrap();
    assert_eq!(req.corporate_actions.len(), 2);
    assert_eq!(req.corporate_actions[0].target, ActionTarget::Symbol("AAA".into()));
    assert_eq!(req.corporate_actions[0].ex_ts_ns, 3 * DAY);
    let stored = req.to_json().unwrap();
    let list = stored.find("corporate_actions").unwrap().items().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].find("type").unwrap().as_str().unwrap(), "dividend");
    assert_eq!(list[0].find("ex_date").unwrap().as_str().unwrap(), "1970-01-04");

    // Without actions the stored request has no such key (unchanged from before).
    let plain = BacktestRequest::from_json(&json(r#"{"strategy":"buy_and_hold"}"#)).unwrap();
    assert!(plain.to_json().unwrap().find("corporate_actions").is_none());

    // By instrument id, with nanoseconds.
    let by_id = BacktestRequest::from_json(&json(
        r#"{"strategy":"buy_and_hold","corporate_actions":[{"instrument":0,"ex_date":259200000000000,"type":"split","from":1,"to":2}]}"#,
    ))
    .unwrap();
    assert_eq!(by_id.corporate_actions[0].target, ActionTarget::Instrument(0));
}

#[test]
fn every_problem_in_the_actions_is_reported() {
    let err = BacktestRequest::from_json(&json(
        r#"{"strategy":"buy_and_hold","corporate_actions":[
            {"symbol":"AAA","ex_date":"not a date","type":"split","from":0,"to":2},
            {"ex_date":"2020-01-01","type":"dividend","amount":-1,"colour":"red"},
            {"symbol":"AAA","instrument":1,"ex_date":"2020-01-01","type":"merger"},
            {"symbol":"AAA","ex_date":"2020-01-01","type":"split","from":2,"to":2},
            {"symbol":"AAA","type":"dividend","from":1}
        ]}"#,
    ))
    .unwrap_err();
    let fields: Vec<&str> = err.errors.iter().map(|e| e.field.as_str()).collect();
    for expected in [
        "corporate_actions[0].ex_date",
        "corporate_actions[0].from",
        "corporate_actions[1].symbol",
        "corporate_actions[1].amount",
        "corporate_actions[1].colour",
        "corporate_actions[2].symbol",
        "corporate_actions[2].type",
        "corporate_actions[3].to",
        "corporate_actions[4].ex_date",
        "corporate_actions[4].from",
        "corporate_actions[4].amount",
    ] {
        assert!(fields.contains(&expected), "{expected} missing from {fields:?}");
    }
    let not_list = BacktestRequest::from_json(&json(r#"{"strategy":"buy_and_hold","corporate_actions":{}}"#))
        .unwrap_err();
    assert_eq!(not_list.errors[0].field, "corporate_actions");
}

#[test]
fn a_run_resolves_symbols_and_reports_dividends() {
    let data = bars(&[100.0, 100.0, 50.0, 49.5, 49.5]);
    let req = BacktestRequest::from_json(&json(REQUEST)).unwrap();
    let out = run_request_on(&data, &symbols(), &req, &mut RunControl::default()).unwrap();
    let r = &out.result;
    r.ledger.check_invariants().unwrap();
    let held = r.ledger.position(0).qty;
    assert!(held.raw() > 0, "buy_and_hold holds AAA");
    // The dividend was paid on the post-split position.
    assert_eq!(r.ledger.dividends(), spectra_core::notional(Price::from_f64(0.5).unwrap(), held).unwrap());

    let summary = summary_json(&out).unwrap();
    let activity = summary.find("activity").unwrap();
    assert_eq!(activity.find("corporate_actions_applied").unwrap().as_int().unwrap(), 2);
    assert_eq!(activity.find("dividends").unwrap().as_str().unwrap(), r.ledger.dividends().to_string());
    assert_eq!(summary.find("run").unwrap().find("corporate_actions").unwrap().items().unwrap().len(), 2);
    let aaa = summary
        .find("instruments")
        .unwrap()
        .items()
        .unwrap()
        .iter()
        .find(|i| i.find("symbol").and_then(|s| s.as_str().ok()) == Some("AAA"))
        .unwrap()
        .clone();
    assert_eq!(aaa.find("dividends").unwrap().as_str().unwrap(), r.ledger.dividends().to_string());

    // A symbol the dataset does not have fails the run with the field named.
    let bad = BacktestRequest::from_json(&json(
        &REQUEST.replace("\"AAA\", \"ex_date\": \"1970-01-05\"", "\"ZZZ\", \"ex_date\": \"1970-01-05\""),
    ))
    .unwrap();
    let e = run_request_on(&data, &symbols(), &bad, &mut RunControl::default()).unwrap_err();
    assert!(e.to_string().contains("corporate_actions[1].symbol: ZZZ is not in this dataset"), "{e}");
    // Without symbols, actions by symbol cannot be resolved.
    assert!(run_request(&data, &req, &mut RunControl::default()).is_err());
}

#[test]
fn a_job_runs_corporate_actions_with_the_datasets_symbols() {
    let payload = json(&format!(
        r#"{{"dataset_version_id":"0192f0c4-7a3b-7c1d-8e2f-0123456789ab",{}"#,
        REQUEST.trim_start().trim_start_matches('{')
    ));
    let job = BacktestJob::from_payload(&payload).unwrap();
    let products =
        run_job(&bars(&[100.0, 100.0, 50.0, 49.5]), &symbols(), &job, "abc", &mut RunControl::default())
            .unwrap();
    assert!(products.params.find("corporate_actions").is_some());
    let activity = products.summary.find("activity").unwrap();
    assert_eq!(activity.find("corporate_actions_applied").unwrap().as_int().unwrap(), 2);
}

/// Trades on the given bars (by index).
struct Script(Vec<(usize, Side, f64)>, usize);
impl Strategy for Script {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        for &(at, side, units) in &self.0 {
            if at == self.1 {
                ctx.market(0, side, Qty::from_f64(units).unwrap());
            }
        }
        self.1 += 1;
    }
}

#[test]
fn a_trade_across_a_split_is_one_trade_with_its_dividend() {
    // Buy 10 at 100, 2-for-1 split, a 1.0 dividend on the 20 shares, sell the 20 at 55.
    let data = bars(&[100.0, 100.0, 50.0, 54.0, 55.0, 55.0]);
    let actions = [
        CorporateAction::split(0, 3 * DAY, 1, 2),
        CorporateAction::dividend(0, 4 * DAY, Price::from_f64(1.0).unwrap()),
    ];
    let mut s = Script(vec![(0, Side::Buy, 10.0), (3, Side::Sell, 20.0)], 0);
    let cfg = Preset::ZeroCost.config();
    let r = run_backtest_with_actions(&data, &actions, &mut s, &cfg, &mut RunControl::default()).unwrap();
    r.ledger.check_invariants().unwrap();
    assert!(r.ledger.position(0).qty.is_zero());
    assert_eq!(r.ledger.realized_pnl(), Money::from_raw(100 * SCALE));
    let t = spectra_research::extract_trades(&r.ledger).unwrap();
    assert_eq!(t.trades.len(), 1);
    assert_eq!(t.trades[0].gross, Money::from_raw(120 * SCALE), "realized 100 plus the 20 dividend");
    assert_eq!(t.open_positions, 0);
}
