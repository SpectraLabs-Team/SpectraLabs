//! Multi-strategy requests (M3 task 13): parsing and validation, running, and
//! the per-strategy output (summary section, tagged CSVs, strategies.csv).

use spectra_backtest::*;
use spectra_core::{Bar, Json, Price, Qty, Rng, SCALE};
use spectra_engine::{run_multi_backtest, Allocation, RunControl};
use spectra_research::ParamSet;

/// Three instruments with mean-reverting noise, so every built-in trades.
fn bars(n: usize, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = [50 * SCALE; 3];
    let mut out = Vec::new();
    for i in 0..n {
        for (k, p) in px.iter_mut().enumerate() {
            let open = *p;
            let close = (open + (50 * SCALE - open) / 20 + rng.range(-SCALE, SCALE)).max(SCALE);
            *p = close;
            out.push(Bar {
                ts_ns: (i as i64 + 1) * 86_400_000_000_000,
                instrument: k as u32 + 1,
                open: Price::from_raw(open),
                high: Price::from_raw(open.max(close) + rng.range(0, SCALE / 4)),
                low: Price::from_raw((open.min(close) - rng.range(0, SCALE / 4)).max(1)),
                close: Price::from_raw(close),
                volume: Qty::from_raw(1_000_000 * SCALE),
            });
        }
    }
    out
}

fn json(s: &str) -> Json {
    Json::parse(s).unwrap()
}

const TWO: &str = r#"{
    "strategies": [
        { "strategy": "equal_weight", "params": { "rebalance_every": 5 }, "weight": 0.5 },
        { "strategy": "zscore_reversion", "weight": 0.3 }
    ],
    "config": { "preset": "default", "initial_cash": 100000 }
}"#;

fn fields(err: &ConfigError) -> Vec<&str> {
    err.errors.iter().map(|e| e.field.as_str()).collect()
}

#[test]
fn a_strategies_list_parses_with_defaults_filled_in() {
    let req = BacktestRequest::from_json(&json(TWO)).unwrap();
    assert!(req.is_multi());
    let names: Vec<&str> = req.strategies.iter().map(|a| a.strategy.name).collect();
    assert_eq!(names, ["equal_weight", "zscore_reversion"]);
    assert_eq!(req.strategies.iter().map(|a| a.weight).collect::<Vec<_>>(), [0.5, 0.3]);
    assert_eq!(req.strategies[0].params.get("rebalance_every").unwrap(), 5.0);
    assert_eq!(req.strategy.name, "equal_weight", "the first strategy stands in for `strategy`");

    let stored = req.to_json().unwrap();
    assert!(stored.find("strategy").is_none());
    let list = stored.find("strategies").unwrap().items().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].find("strategy").unwrap().as_str().unwrap(), "zscore_reversion");
    assert!(list[1].find("strategy_sha256").is_some() && list[1].find("params").is_some());

    // The combined strategy hash covers every strategy and weight.
    let single = BacktestRequest::from_json(&json(r#"{"strategy":"equal_weight"}"#)).unwrap();
    assert_eq!(single.strategy_sha256(), single.strategy.source_sha256());
    assert_ne!(req.strategy_sha256(), single.strategy_sha256());
    let other_weights = BacktestRequest::from_json(&json(&TWO.replace("0.3", "0.4"))).unwrap();
    assert_ne!(req.strategy_sha256(), other_weights.strategy_sha256());
    assert_eq!(stored.find("strategy_sha256").unwrap().as_str().unwrap(), req.strategy_sha256());

    // Built in code, the same request.
    let mut p = ParamSet::default();
    p.values.push(("rebalance_every".into(), 5.0));
    let coded = BacktestRequest::multi(
        &[("equal_weight", p, 0.5), ("zscore_reversion", ParamSet::default(), 0.3)],
        req.config,
    )
    .unwrap();
    assert_eq!(coded.strategy_sha256(), req.strategy_sha256());
    assert_eq!(coded.to_json().unwrap(), stored);
}

#[test]
fn every_problem_in_a_strategies_list_is_reported() {
    let err = BacktestRequest::from_json(&json(
        r#"{
            "strategy": "sma_cross",
            "strategies": [
                { "strategy": "sma_cross", "params": { "fast": 30, "slow": 10 }, "weight": 0.7 },
                { "strategy": "nope", "weight": 0.2 },
                { "strategy": "buy_and_hold", "weight": 0, "colour": "red" },
                { "strategy": "buy_and_hold" },
                { "strategy": "buy_and_hold", "weight": "half" }
            ]
        }"#,
    ))
    .unwrap_err();
    let f = fields(&err);
    for expected in [
        "strategy",
        "strategies[0].params.fast",
        "strategies[1].strategy",
        "strategies[2].weight",
        "strategies[2].colour",
        "strategies[3].weight",
        "strategies[4].weight",
    ] {
        assert!(f.contains(&expected), "{expected} missing from {f:?}");
    }

    let over = BacktestRequest::from_json(&json(
        r#"{"strategies":[{"strategy":"buy_and_hold","weight":0.6},{"strategy":"buy_and_hold","weight":0.5}]}"#,
    ))
    .unwrap_err();
    assert_eq!(over.errors[0].field, "strategies");
    assert!(over.errors[0].message.contains("more than 1"));

    let empty = BacktestRequest::from_json(&json(r#"{"strategies":[]}"#)).unwrap_err();
    assert!(empty.errors[0].message.contains("1 to 10"));
    let not_a_list = BacktestRequest::from_json(&json(r#"{"strategies":{}}"#)).unwrap_err();
    assert_eq!(fields(&not_a_list), ["strategies"]);
    let eleven =
        format!(r#"{{"strategies":[{}]}}"#, [r#"{"strategy":"buy_and_hold","weight":0.05}"#; 11].join(","));
    assert!(BacktestRequest::from_json(&json(&eleven)).is_err());
}

#[test]
fn a_multi_request_runs_like_the_engine_and_reports_each_strategy() {
    let data = bars(300, 11);
    let req = BacktestRequest::from_json(&json(TWO)).unwrap();
    let out = run_request(&data, &req, &mut RunControl::default()).unwrap();
    let r = &out.result;

    // The same as calling the engine directly.
    let mut built: Vec<_> = req.strategies.iter().map(|a| a.strategy.build(&a.params).unwrap()).collect();
    let mut alloc: Vec<Allocation<'_>> = built
        .iter_mut()
        .zip(&req.strategies)
        .map(|(s, a)| Allocation { strategy: s.as_mut(), weight: a.weight })
        .collect();
    let direct = run_multi_backtest(&data, &mut alloc, &req.config, &mut RunControl::default()).unwrap();
    assert_eq!(r.run, direct.run);
    assert!(r.strategies.iter().all(|s| s.ledger.journal().len() > 1), "both strategies trade");

    // Summary: one entry per strategy, profits adding up to the account's.
    let summary = summary_json(&out).unwrap();
    assert_eq!(summary.find("run").unwrap().find("strategy").unwrap().as_str().unwrap(), "multi");
    let section = summary.find("strategies").unwrap();
    let list = section.find("list").unwrap().items().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].find("allocation").unwrap().as_str().unwrap(), "50000.00000000");
    assert_eq!(list[1].find("allocation").unwrap().as_str().unwrap(), "30000.00000000");
    let unallocated = section.find("unallocated").unwrap();
    assert_eq!(unallocated.find("allocation").unwrap().as_str().unwrap(), "20000.00000000");
    let profit: f64 =
        list.iter().map(|s| s.find("profit").unwrap().as_str().unwrap().parse::<f64>().unwrap()).sum();
    let account = r.ledger.equity().unwrap().to_f64() - 100_000.0;
    assert!((profit - account).abs() < 1e-6, "{profit} vs {account}");
    for s in list {
        for key in ["sharpe", "max_drawdown", "fill_count", "total_fees", "contribution", "strategy_sha256"] {
            assert!(s.find(key).is_some(), "{key}");
        }
    }

    // CSVs: tagged orders and fills, and each strategy's own curve.
    let arts = artifacts(r);
    let names: Vec<&str> = arts.iter().map(|a| a.name).collect();
    assert_eq!(names, ["equity.csv", "orders.csv", "fills.csv", "returns.csv", "strategies.csv"]);
    let text =
        |name: &str| String::from_utf8(arts.iter().find(|a| a.name == name).unwrap().bytes.clone()).unwrap();

    let orders = text("orders.csv");
    assert!(orders.lines().next().unwrap().ends_with(",reject_reason,strategy"));
    for (line, o) in orders.lines().skip(1).zip(&r.orders) {
        assert!(line.ends_with(&format!(",{}", o.strategy)), "{line}");
    }
    let fills = text("fills.csv");
    assert!(fills.lines().next().unwrap().ends_with(",cash_after,order_id,strategy"));
    let mut per_strategy = [0usize; 2];
    for line in fills.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        let id: usize = cols[9].parse().unwrap();
        let strategy: usize = cols[10].parse().unwrap();
        assert_eq!(r.orders[id - 1].strategy, strategy);
        per_strategy[strategy] += 1;
    }
    let own_fills = |i: usize| {
        r.strategies[i].ledger.journal().iter().filter(|e| e.kind == spectra_core::EntryType::Fill).count()
    };
    assert_eq!(per_strategy, [own_fills(0), own_fills(1)]);
    let curves = text("strategies.csv");
    assert_eq!(curves.lines().next().unwrap(), "ts_ns,strategy,equity,cash,gross_exposure");
    assert_eq!(curves.lines().count(), 1 + 2 * r.equity_curve.len());
}

#[test]
fn single_strategy_output_has_no_strategy_columns() {
    let data = bars(120, 3);
    let req = BacktestRequest::from_json(&json(r#"{"strategy":"zscore_reversion"}"#)).unwrap();
    let out = run_request(&data, &req, &mut RunControl::default()).unwrap();
    assert!(summary_json(&out).unwrap().find("strategies").is_none());
    let arts = artifacts(&out.result);
    assert_eq!(arts.len(), 4);
    let orders = String::from_utf8(arts[1].bytes.clone()).unwrap();
    assert!(orders.lines().next().unwrap().ends_with(",reject_reason"));
}

#[test]
fn a_multi_strategy_job_stores_the_strategy_curves() {
    let payload = json(&format!(
        r#"{{"dataset_version_id":"0192f0c4-7a3b-7c1d-8e2f-0123456789ab",{}"#,
        TWO.trim_start().trim_start_matches('{')
    ));
    let job = BacktestJob::from_payload(&payload).unwrap();
    assert!(job.request.is_multi());
    let products = run_job(&bars(200, 5), &[], &job, "abc", &mut RunControl::default()).unwrap();
    let kinds: Vec<&str> = products.files.iter().map(|f| f.kind).collect();
    assert_eq!(kinds.last(), Some(&"strategies_csv"));
    assert!(products.params.find("strategies").is_some());
    assert_eq!(products.params.find("dataset_checksum").unwrap().as_str().unwrap(), "abc");
}
