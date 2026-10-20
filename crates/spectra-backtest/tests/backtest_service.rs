//! Tests for spectra-backtest: configuration as data, the built-in strategy
//! registry, requests, and result serialization.

use std::collections::BTreeSet;

use spectra_backtest::*;
use spectra_core::{Bar, Bps, Json, Money, Price, Qty, Ratio, Rng, Side, SCALE};
use spectra_engine::{config_fingerprint, run_backtest, BacktestConfig, Context, RunControl, Strategy};
use spectra_research::ParamSet;

// ---------------------------------------------------------------- helpers

/// The bar generator of the `sma_cross` demo (crates/spectra-cli/examples/sma_cross.rs).
fn demo_bars(n: usize, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = 100 * SCALE;
    (0..n)
        .map(|i| {
            let open = px;
            let close = (open + rng.range(-SCALE * 3 / 2, SCALE * 3 / 2 + SCALE / 20)).max(SCALE);
            let high = open.max(close) + rng.range(0, SCALE / 2);
            let low = (open.min(close) - rng.range(0, SCALE / 2)).max(1);
            let volume = rng.range(100_000, 1_000_000) * SCALE;
            px = close;
            Bar {
                ts_ns: (i as i64 + 1) * 86_400_000_000_000,
                instrument: 1,
                open: Price::from_raw(open),
                high: Price::from_raw(high),
                low: Price::from_raw(low),
                close: Price::from_raw(close),
                volume: Qty::from_raw(volume),
            }
        })
        .collect()
}

/// Several instruments with mean-reverting noise, so every strategy trades.
fn multi_bars(n: usize, instruments: u32, seed: u64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = vec![50 * SCALE; instruments as usize];
    let mut out = Vec::new();
    for i in 0..n {
        for id in 0..instruments {
            let k = id as usize;
            let open = px[k];
            let pull = (50 * SCALE - open) / 20;
            let close = (open + pull + rng.range(-SCALE, SCALE)).max(SCALE);
            px[k] = close;
            out.push(Bar {
                ts_ns: (i as i64 + 1) * 3_600_000_000_000,
                instrument: id + 1,
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

/// The demo's strategy, copied verbatim, as the reference for the built-in.
struct DemoSmaCross {
    fast: usize,
    slow: usize,
    size: Qty,
}
impl Strategy for DemoSmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.slow.max(self.fast) {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let bullish = sma(self.fast) > sma(self.slow);
        let pos = ctx.position(id);
        if bullish && pos.is_zero() {
            ctx.market(id, Side::Buy, self.size);
        } else if !bullish && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn demo_config() -> BacktestConfig {
    let mut cfg = BacktestConfig {
        initial_cash: Money::from_raw(100_000 * SCALE),
        seed: 2026,
        ..BacktestConfig::default()
    };
    cfg.costs.slippage = Bps::from_f64(2.0).unwrap();
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0).unwrap();
    cfg
}

fn json(text: &str) -> Json {
    Json::parse(text).unwrap()
}

fn fields(e: &ConfigError) -> BTreeSet<String> {
    e.errors.iter().map(|f| f.field.clone()).collect()
}

// ---------------------------------------------------------------- task 1: config

#[test]
fn empty_config_is_the_default_preset() {
    let c = parse_config(&json("{}")).unwrap();
    assert_eq!(c, Preset::Default.config());
    assert_eq!(c.costs.commission.bps_of_notional, Bps::from_f64(1.0).unwrap());
    assert_eq!(c.costs.slippage, Bps::from_f64(2.0).unwrap());
    assert_eq!(c.ledger.max_gross_leverage, Some(Ratio::ONE));
    // The engine's own default is untouched: it is the zero-cost preset.
    assert_eq!(Preset::ZeroCost.config(), BacktestConfig::default());
}

#[test]
fn presets_are_named_and_distinct() {
    let configs: Vec<String> = Preset::ALL.iter().map(|p| config_fingerprint(&p.config())).collect();
    assert_eq!(configs.iter().collect::<BTreeSet<_>>().len(), 3);
    for p in Preset::ALL {
        assert_eq!(Preset::from_name(p.name()), Some(p));
        let c = parse_config(&json(&format!(r#"{{"preset":"{}"}}"#, p.name()))).unwrap();
        assert_eq!(c, p.config());
    }
    let cons = Preset::ConservativeCosts.config();
    assert_eq!(cons.costs.commission.min_fee, Money::ONE);
    assert_eq!(cons.exec.max_participation, Qty::from_f64(0.1).unwrap());
    assert_eq!(presets_json().unwrap().items().unwrap().len(), 3);
}

#[test]
fn fields_override_the_preset() {
    let c = parse_config(&json(
        r#"{"preset":"zero_cost","initial_cash":250000.5,"allow_short":true,"allow_negative_cash":true,
            "max_gross_leverage":2.5,"commission_per_unit":0.005,"commission_bps":0.5,"commission_min":1,
            "slippage_bps":3,"delay_bars":2,"max_participation":0.25,"seed":42,
            "periods_per_year":98280,"risk_free_rate":0.04,
            "borrow_fee_rate":0.03,"cash_interest_rate":0.02,"margin_interest_rate":0.06}"#,
    ))
    .unwrap();
    assert_eq!(c.initial_cash, Money::from_f64(250_000.5).unwrap());
    assert!(c.ledger.allow_short && c.ledger.allow_negative_cash);
    assert_eq!(c.ledger.max_gross_leverage, Some(Ratio::from_f64(2.5).unwrap()));
    assert_eq!(c.costs.commission.per_unit, Money::from_f64(0.005).unwrap());
    assert_eq!(c.costs.commission.bps_of_notional, Bps::from_f64(0.5).unwrap());
    assert_eq!(c.costs.commission.min_fee, Money::ONE);
    assert_eq!(c.costs.slippage, Bps::from_f64(3.0).unwrap());
    assert_eq!(c.exec.delay_bars, 2);
    assert_eq!(c.exec.max_participation, Qty::from_f64(0.25).unwrap());
    assert_eq!(c.seed, 42);
    assert_eq!(c.periods_per_year, 98_280.0);
    assert_eq!(c.risk_free_rate, 0.04);
    assert_eq!((c.financing.borrow_rate, c.financing.cash_rate, c.financing.margin_rate), (0.03, 0.02, 0.06));
    // The stored form keeps the rates, and they change the fingerprint.
    assert_eq!(config_from_stored_json(&stored_config_json(&c).unwrap()).unwrap(), c);
    let no_fin = BacktestConfig { financing: spectra_engine::FinancingConfig::default(), ..c };
    assert_ne!(config_fingerprint(&no_fin), config_fingerprint(&c));

    // User form round trip.
    assert_eq!(parse_config(&config_to_user_json(&c).unwrap()).unwrap(), c);
}

#[test]
fn leverage_null_disables_the_limit_and_big_seeds_travel_as_strings() {
    let c = parse_config(&json(r#"{"max_gross_leverage":null,"seed":"18446744073709551615"}"#)).unwrap();
    assert_eq!(c.ledger.max_gross_leverage, None);
    assert_eq!(c.seed, u64::MAX);
    let back = config_to_user_json(&c).unwrap();
    assert!(back.find("max_gross_leverage").unwrap().is_null());
    assert_eq!(back.find("seed").unwrap().as_str().unwrap(), "18446744073709551615");
    assert_eq!(parse_config(&back).unwrap(), c);
}

#[test]
fn every_problem_is_reported_with_its_field() {
    let err = parse_config(&json(
        r#"{"preset":"nope","initial_cash":-5,"max_gross_leverage":-1,"bogus":1,"seed":"x",
            "delay_bars":1.5,"allow_short":"yes","max_participation":2,"periods_per_year":0,
            "borrow_fee_rate":-0.01}"#,
    ))
    .unwrap_err();
    let expected: BTreeSet<String> = [
        "preset",
        "initial_cash",
        "max_gross_leverage",
        "bogus",
        "seed",
        "delay_bars",
        "allow_short",
        "max_participation",
        "periods_per_year",
        "borrow_fee_rate",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(fields(&err), expected, "{err}");
    assert_eq!(err.to_json().items().unwrap().len(), expected.len());
    assert!(err.to_string().starts_with("invalid backtest configuration: "));

    assert!(parse_config(&json("[1,2]")).is_err());
    assert!(parse_config(&json(r#"{"initial_cash":0}"#)).is_err(), "cash must be positive");
    assert!(parse_config(&json(r#"{"seed":-1}"#)).is_err());
}

#[test]
fn stored_form_round_trips_and_fingerprints_match() {
    for c in Preset::ALL.map(Preset::config).into_iter().chain([demo_config()]) {
        let stored = stored_config_json(&c).unwrap();
        let back = config_from_stored_json(&Json::parse(&stored.dump()).unwrap()).unwrap();
        assert_eq!(back, c);
        assert_eq!(config_fingerprint(&back), config_fingerprint(&c));
        // A config that went through the user form fingerprints the same, too.
        let via_user = parse_config(&config_to_user_json(&c).unwrap()).unwrap();
        assert_eq!(config_fingerprint(&via_user), config_fingerprint(&c));
    }
}

#[test]
fn schema_describes_every_field() {
    let s = config_schema().unwrap();
    assert!(!s.find("additionalProperties").unwrap().as_bool().unwrap());
    let props = s.find("properties").unwrap().entries().unwrap();
    let names: Vec<&str> = props.iter().map(|(k, _)| k.as_str()).collect();
    for name in ["preset", "initial_cash", "max_gross_leverage", "slippage_bps", "seed", "risk_free_rate"] {
        assert!(names.contains(&name), "{name} missing");
    }
    // Every schema property is accepted by the parser, and its default parses back to the default preset.
    let mut defaults = Json::object();
    for (k, v) in props {
        defaults.set(k.clone(), v.find("default").cloned().unwrap()).unwrap();
    }
    assert_eq!(parse_config(&defaults).unwrap(), Preset::Default.config());
}

// ---------------------------------------------------------------- task 2: strategies

#[test]
fn builtins_are_listed_with_schemas_and_distinct_source_hashes() {
    let names: Vec<&str> = builtin_strategies().iter().map(|s| s.name).collect();
    assert_eq!(names, ["buy_and_hold", "equal_weight", "sma_cross", "zscore_reversion"]);
    let mut hashes = BTreeSet::new();
    for s in builtin_strategies() {
        let h = s.source_sha256();
        assert_eq!(h.len(), 64);
        hashes.insert(h);
        let j = s.to_json().unwrap();
        assert_eq!(j.find("name").unwrap().as_str().unwrap(), s.name);
        assert_eq!(j.find("params").unwrap().items().unwrap().len(), s.params.len());
    }
    assert_eq!(hashes.len(), 4);
    assert_eq!(builtins_json().unwrap().items().unwrap().len(), 4);
    assert!(builtin("nope").unwrap_err().to_string().contains("sma_cross"));
}

#[test]
fn params_get_defaults_in_canonical_order() {
    let s = builtin("sma_cross").unwrap();
    let p = s.resolve_params(&ParamSet::from_pairs([("slow", 50.0)])).unwrap();
    assert_eq!(p.canonical(), "fast=10;slow=50;qty=100;allow_short=0");
    let from_json = s.resolve_params_json(Some(&json(r#"{"allow_short":true,"slow":50}"#))).unwrap();
    assert_eq!(from_json.canonical(), "fast=10;slow=50;qty=100;allow_short=1");
    assert_eq!(s.resolve_params_json(None).unwrap().canonical(), "fast=10;slow=30;qty=100;allow_short=0");
}

#[test]
fn bad_params_are_all_reported() {
    let s = builtin("sma_cross").unwrap();
    let err = s
        .resolve_params_json(Some(&json(r#"{"fast":2.5,"slow":0,"qty":"x","extra":1,"allow_short":3}"#)))
        .unwrap_err();
    let expected: BTreeSet<String> =
        ["params.fast", "params.slow", "params.qty", "params.extra", "params.allow_short"]
            .map(String::from)
            .into();
    assert_eq!(fields(&err), expected, "{err}");

    let err = s.resolve_params(&ParamSet::from_pairs([("fast", 30.0), ("slow", 20.0)])).unwrap_err();
    assert_eq!(fields(&err), ["params.fast".to_string()].into());
    let err = builtin("zscore_reversion")
        .unwrap()
        .resolve_params(&ParamSet::from_pairs([("entry_z", 1.0), ("exit_z", 1.5)]))
        .unwrap_err();
    assert_eq!(fields(&err), ["params.exit_z".to_string()].into());
    assert!(s.build(&ParamSet::from_pairs([("fast", 0.0)])).is_err());
}

#[test]
fn builtin_sma_cross_reproduces_the_demo_exactly() {
    let bars = demo_bars(1000, 2026);
    let cfg = demo_config();
    let demo =
        run_backtest(&bars, &mut DemoSmaCross { fast: 10, slow: 30, size: Qty::from_raw(100 * SCALE) }, &cfg)
            .unwrap();
    let mut built = builtin("sma_cross").unwrap().build(&ParamSet::default()).unwrap();
    let ours = run_backtest(&bars, built.as_mut(), &cfg).unwrap();
    assert!(demo.fill_count() > 10);
    assert_eq!(demo.run, ours.run);
}

#[test]
fn factory_plugs_into_research_sweeps() {
    let f = builtin_factory("sma_cross").unwrap();
    let factory: &spectra_research::StrategyFactory<'_> = &f;
    let bars = demo_bars(300, 1);
    let mut s = factory(&ParamSet::from_pairs([("fast", 5.0), ("slow", 20.0)])).unwrap();
    assert!(run_backtest(&bars, s.as_mut(), &demo_config()).is_ok());
    assert!(factory(&ParamSet::from_pairs([("fast", 20.0), ("slow", 5.0)])).is_err());
}

#[test]
fn every_builtin_trades_multi_instrument_data_cleanly() {
    let bars = multi_bars(800, 3, 9);
    let long_short = parse_config(&json(r#"{"allow_short":true,"max_gross_leverage":2}"#)).unwrap();
    let cases = [
        ("buy_and_hold", r"{}", Preset::Default.config()),
        ("equal_weight", r#"{"rebalance_every":20}"#, Preset::Default.config()),
        (
            "equal_weight",
            r#"{"rebalance_every":0,"drift":0.02,"min_trade":500}"#,
            Preset::ConservativeCosts.config(),
        ),
        ("sma_cross", r#"{"fast":5,"slow":20,"qty":50}"#, Preset::Default.config()),
        ("sma_cross", r#"{"fast":5,"slow":20,"qty":50,"allow_short":true}"#, long_short),
        ("zscore_reversion", r#"{"lookback":30,"qty":50}"#, Preset::ConservativeCosts.config()),
        ("zscore_reversion", r#"{"lookback":30,"qty":50,"allow_short":true}"#, long_short),
    ];
    for (name, params, config) in cases {
        let s = builtin(name).unwrap();
        let p = s.resolve_params_json(Some(&json(params))).unwrap();
        let req = BacktestRequest::new(name, &p, config).unwrap();
        let out = run_request(&bars, &req, &mut RunControl::default()).unwrap();
        let r = &out.result;
        assert!(r.fill_count() > 0, "{name} {params} never traded");
        assert!(r.ledger.check_invariants().is_ok(), "{name} {params}");
        let rejected = r.orders.iter().filter(|o| o.state == spectra_engine::OrderState::Rejected).count();
        assert_eq!(rejected, 0, "{name} {params}: rejected orders");
        if params.contains("allow_short") {
            // Replay positions from the fills: some instrument must have gone net short.
            let mut pos = std::collections::BTreeMap::<u32, i64>::new();
            let mut went_short = false;
            for e in r.ledger.journal().iter().filter(|e| e.kind == spectra_core::EntryType::Fill) {
                let q = pos.entry(e.fill.instrument).or_default();
                *q += if e.fill.side == Side::Buy { e.fill.qty.raw() } else { -e.fill.qty.raw() };
                went_short |= *q < 0;
            }
            assert!(went_short, "{name} {params} never went short");
        }
    }
    // Buy and hold: one order per instrument, all filled, never sells.
    let out = run_request(
        &bars,
        &BacktestRequest::new("buy_and_hold", &ParamSet::default(), Preset::Default.config()).unwrap(),
        &mut RunControl::default(),
    )
    .unwrap();
    assert_eq!(out.result.orders.len(), 3);
    assert!(out
        .result
        .orders
        .iter()
        .all(|o| o.side == Side::Buy && o.state == spectra_engine::OrderState::Filled));
}

// ---------------------------------------------------------------- requests

#[test]
fn request_documents_parse_and_report_every_problem() {
    let req = BacktestRequest::from_json(&json(
        r#"{"strategy":"sma_cross","params":{"fast":5,"slow":20},"config":{"preset":"zero_cost","seed":9}}"#,
    ))
    .unwrap();
    assert_eq!(req.strategy.name, "sma_cross");
    assert_eq!(req.params.canonical(), "fast=5;slow=20;qty=100;allow_short=0");
    assert_eq!(req.config.seed, 9);

    let minimal = BacktestRequest::from_json(&json(r#"{"strategy":"buy_and_hold"}"#)).unwrap();
    assert_eq!(minimal.config, Preset::Default.config());

    let err = BacktestRequest::from_json(&json(
        r#"{"strategy":"sma_cross","params":{"fast":50,"slow":20},"config":{"initial_cash":-1,"x":1},"dataset":1}"#,
    ))
    .unwrap_err();
    let expected: BTreeSet<String> =
        ["dataset", "params.fast", "config.initial_cash", "config.x"].map(String::from).into();
    assert_eq!(fields(&err), expected, "{err}");

    let err = BacktestRequest::from_json(&json(r#"{"params":{}}"#)).unwrap_err();
    assert_eq!(fields(&err), ["strategy".to_string()].into());
    let err = BacktestRequest::from_json(&json(r#"{"strategy":"nope"}"#)).unwrap_err();
    assert!(err.to_string().contains("built-in strategies are"), "{err}");
}

#[test]
fn stored_request_carries_hashes_and_exact_config() {
    let req = BacktestRequest::from_json(&json(r#"{"strategy":"sma_cross","config":{"slippage_bps":1.25}}"#))
        .unwrap();
    let j = req.to_json().unwrap();
    assert_eq!(j.find("strategy_sha256").unwrap().as_str().unwrap(), req.strategy.source_sha256());
    let exact = config_from_stored_json(j.find("config_exact").unwrap()).unwrap();
    assert_eq!(exact, req.config);
    let again = BacktestRequest::from_json(
        &Json::object()
            .with("strategy", Json::string("sma_cross"))
            .with("params", j.find("params").unwrap().clone())
            .with("config", j.find("config").unwrap().clone()),
    )
    .unwrap();
    assert_eq!(config_fingerprint(&again.config), config_fingerprint(&req.config));
    assert_eq!(again.params, req.params);
}

#[test]
fn request_path_matches_direct_engine_call() {
    let bars = demo_bars(700, 77);
    let req = BacktestRequest::from_json(&json(r#"{"strategy":"sma_cross","params":{"fast":8,"slow":21}}"#))
        .unwrap();
    let via_request = run_request(&bars, &req, &mut RunControl::default()).unwrap();
    let mut s = builtin("sma_cross").unwrap().build(&req.params).unwrap();
    let direct = run_backtest(&bars, s.as_mut(), &Preset::Default.config()).unwrap();
    assert_eq!(via_request.result.run, direct.run);
}

// ---------------------------------------------------------------- task 4: output

fn sample_output() -> BacktestOutput {
    let bars = demo_bars(1500, 2026);
    let req = BacktestRequest::new("sma_cross", &ParamSet::default(), demo_config()).unwrap();
    run_request(&bars, &req, &mut RunControl::default()).unwrap()
}

fn data_rows(a: &Artifact) -> Vec<String> {
    let text = String::from_utf8(a.bytes.clone()).unwrap();
    text.lines().skip(1).map(String::from).collect()
}

#[test]
fn artifacts_have_one_row_per_record() {
    let out = sample_output();
    let r = &out.result;
    let arts = artifacts(r);
    let names: Vec<&str> = arts.iter().map(|a| a.name).collect();
    assert_eq!(names, ["equity.csv", "orders.csv", "fills.csv", "returns.csv"]);
    assert!(arts.iter().all(|a| a.media_type == "text/csv"));
    assert_eq!(data_rows(&arts[0]).len(), r.equity_curve.len());
    assert_eq!(data_rows(&arts[1]).len(), r.orders.len());
    assert_eq!(data_rows(&arts[2]).len(), r.fill_count());
    assert_eq!(data_rows(&arts[3]).len(), r.equity_curve.len() - 1);

    // Exact values: the first equity row is the starting cash, written by the ledger's own formatter.
    let first = data_rows(&arts[0])[0].clone();
    assert_eq!(first, format!("{},100000.00000000,100000.00000000,0", r.equity_curve[0].ts_ns));
    // Fills: the fees column adds up to the ledger's total fees exactly.
    let fees: i64 = data_rows(&arts[2])
        .iter()
        .map(|row| {
            let f = row.split(',').nth(6).unwrap();
            let (whole, frac) = f.split_once('.').unwrap();
            whole.parse::<i64>().unwrap() * SCALE + frac.parse::<i64>().unwrap()
        })
        .sum();
    assert_eq!(fees, r.ledger.total_fees().raw());
    // Every column count is the header's.
    for a in &arts {
        let text = String::from_utf8(a.bytes.clone()).unwrap();
        let cols = text.lines().next().unwrap().split(',').count();
        assert!(text.lines().all(|l| l.split(',').count() == cols), "{}", a.name);
    }
}

#[test]
fn summary_has_metrics_fingerprints_and_trades() {
    let out = sample_output();
    let s = summary_json(&out).unwrap();
    let s = Json::parse(&s.dump()).unwrap(); // survives serialization
    assert_eq!(s.find("schema").unwrap().as_str().unwrap(), SUMMARY_SCHEMA);
    let run = s.find("run").unwrap();
    assert_eq!(run.find("result_sha256").unwrap().as_str().unwrap(), out.result.run.result_sha256);
    assert_eq!(run.find("strategy").unwrap().as_str().unwrap(), "sma_cross");
    let m = s.find("metrics").unwrap();
    assert_eq!(m.find("sharpe").unwrap().as_double().unwrap(), out.result.metrics.sharpe);
    assert_eq!(m.find("max_drawdown").unwrap().as_double().unwrap(), out.result.metrics.max_drawdown);
    let a = s.find("activity").unwrap();
    assert_eq!(a.find("fill_count").unwrap().as_int().unwrap() as usize, out.result.fill_count());
    assert_eq!(a.find("total_fees").unwrap().as_str().unwrap(), out.result.ledger.total_fees().to_string());
    let t = s.find("trades").unwrap();
    assert!(t.find("count").unwrap().as_int().unwrap() > 5);
    let wr = t.find("win_rate").unwrap().as_double().unwrap();
    assert!((0.0..=1.0).contains(&wr));
}

#[test]
fn summary_writes_null_for_non_finite_ratios() {
    // Steadily rising prices and a single round trip: one winner, no losers,
    // so the profit factor is +inf and must be written as null.
    let bars: Vec<Bar> = (0..60)
        .map(|i| {
            let px = Price::from_units(100 + i).unwrap();
            Bar {
                ts_ns: (i + 1) * 1_000,
                instrument: 1,
                open: px,
                high: px,
                low: px,
                close: px,
                volume: Qty::from_units(1_000_000).unwrap(),
            }
        })
        .collect();
    struct OneTrade(usize);
    impl Strategy for OneTrade {
        fn on_bar(&mut self, ctx: &mut Context<'_>, b: &[Bar]) {
            self.0 += 1;
            if self.0 == 1 {
                ctx.market(b[0].instrument, Side::Buy, Qty::ONE);
            }
            if self.0 == 50 {
                let pos = ctx.position(b[0].instrument);
                ctx.market(b[0].instrument, Side::Sell, pos);
            }
        }
    }
    let req = BacktestRequest::new("buy_and_hold", &ParamSet::default(), BacktestConfig::default()).unwrap();
    let result = run_backtest(&bars, &mut OneTrade(0), &req.config).unwrap();
    let out = BacktestOutput { request: req, result };
    let s = summary_json(&out).unwrap();
    let t = s.find("trades").unwrap();
    assert_eq!(t.find("wins").unwrap().as_int().unwrap(), 1);
    assert_eq!(t.find("losses").unwrap().as_int().unwrap(), 0);
    assert!(t.find("profit_factor").unwrap().is_null());
    assert!(Json::parse(&s.dump()).is_ok());
}

#[test]
fn downsampled_drawdown_keeps_the_exact_maximum() {
    let out = sample_output();
    let curve = &out.result.equity_curve;
    for max_points in [4, 10, 101, 500] {
        let pts = downsample(&series(curve, SeriesKind::Drawdown), max_points);
        assert!(pts.len() <= max_points);
        let worst = pts.iter().map(|p| p.value).fold(0.0, f64::max);
        assert_eq!(worst.to_bits(), out.result.metrics.max_drawdown.to_bits(), "max_points {max_points}");
        assert!(pts.windows(2).all(|w| w[0].ts_ns < w[1].ts_ns));
        assert_eq!(pts.first().unwrap().ts_ns, curve.first().unwrap().ts_ns);
        assert_eq!(pts.last().unwrap().ts_ns, curve.last().unwrap().ts_ns);
    }
    let j = series_json(curve, SeriesKind::Equity, DEFAULT_SERIES_POINTS).unwrap();
    assert_eq!(j.find("total_points").unwrap().as_int().unwrap() as usize, curve.len());
    assert!(j.find("downsampled").unwrap().as_bool().unwrap());
    assert!(j.find("points").unwrap().items().unwrap().len() <= DEFAULT_SERIES_POINTS);
}

#[test]
fn downsampling_keeps_global_extremes_of_any_series() {
    let mut rng = Rng::new(3);
    for n in [0usize, 1, 5, 37, 1000, 4321] {
        let pts: Vec<SeriesPoint> = (0..n)
            .map(|i| SeriesPoint { ts_ns: i as i64, value: rng.range(-1_000_000, 1_000_000) as f64 })
            .collect();
        for max_points in [1, 4, 7, 64, 1000] {
            let d = downsample(&pts, max_points);
            assert!(d.len() <= max_points.max(4) || d.len() == n);
            if n == 0 {
                assert!(d.is_empty());
                continue;
            }
            let lo = pts.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
            let hi = pts.iter().map(|p| p.value).fold(f64::NEG_INFINITY, f64::max);
            assert!(
                d.iter().any(|p| p.value == lo) && d.iter().any(|p| p.value == hi),
                "n {n} max {max_points}"
            );
            assert!(d.windows(2).all(|w| w[0].ts_ns < w[1].ts_ns));
        }
    }
    assert_eq!(SeriesKind::parse("drawdown").unwrap(), SeriesKind::Drawdown);
    assert!(SeriesKind::parse("pnl").is_err());
}
