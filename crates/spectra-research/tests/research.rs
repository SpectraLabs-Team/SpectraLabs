//! Behavioural tests for sweeps, walk-forward and the Survival Lab (the
//! numbers themselves are pinned by the C++ parity test).

use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

use spectra_core::*;
use spectra_engine::*;
use spectra_research::*;

fn walk(n: usize, seed: u64, drift: f64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = 100.0f64;
    (0..n)
        .map(|i| {
            let open = px;
            let close = (open * (1.0 + drift + 0.01 * rng.normal())).max(1.0);
            px = close;
            Bar {
                ts_ns: i as i64 + 1,
                instrument: 1,
                open: Price::from_f64(open).unwrap(),
                close: Price::from_f64(close).unwrap(),
                high: Price::from_f64(open.max(close) * 1.002).unwrap(),
                low: Price::from_f64(open.min(close) * 0.998).unwrap(),
                volume: Qty::from_f64(1e6).unwrap(),
            }
        })
        .collect()
}

struct Sma(usize, usize);
impl Strategy for Sma {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let h = ctx.history(bars[0].instrument);
        if h.len() < self.1 {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let pos = ctx.position(1);
        if sma(self.0) > sma(self.1) && pos.is_zero() {
            ctx.market(1, Side::Buy, Qty::from_f64(100.0).unwrap());
        } else if sma(self.0) <= sma(self.1) && pos.raw() > 0 {
            ctx.market(1, Side::Sell, pos);
        }
    }
}

fn factory(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    let (f, s) = (p.get_int("fast")?, p.get_int("slow")?);
    if f >= s {
        return Err(Error::Strategy("fast must be below slow".into()));
    }
    Ok(Box::new(Sma(f as usize, s as usize)))
}

fn grid() -> Vec<ParamSet> {
    ParamGrid::new()
        .add("fast", vec![3.0, 5.0, 8.0, 13.0])
        .unwrap()
        .add("slow", vec![20.0, 30.0, 50.0, 80.0])
        .unwrap()
        .all()
}

#[test]
fn sweep_output_is_identical_for_any_thread_count() {
    let bars = walk(600, 3, 0.0005);
    let cfg = BacktestConfig::default();
    let one =
        run_sweep(&bars, &factory, &cfg, &grid(), &objective_sharpe, SweepOptions { threads: 1 }, 1).unwrap();
    for threads in [2, 4, 16] {
        let many = run_sweep(&bars, &factory, &cfg, &grid(), &objective_sharpe, SweepOptions { threads }, 1)
            .unwrap();
        assert_eq!(one, many);
    }
    assert_eq!(one.trials.len(), 16);
    assert_eq!(one.trials[15].id, 16);
}

#[test]
fn sweep_reports_the_lowest_index_error_deterministically() {
    let bars = walk(100, 3, 0.0);
    let mut params = grid();
    params.insert(5, ParamSet::from_pairs([("fast", 50.0), ("slow", 10.0)]));
    params.insert(9, ParamSet::from_pairs([("fast", 60.0), ("slow", 10.0)]));
    for threads in [1, 4] {
        let err = run_sweep(
            &bars,
            &factory,
            &BacktestConfig::default(),
            &params,
            &objective_sharpe,
            SweepOptions { threads },
            1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("fast must be below slow"));
    }
}

#[test]
fn walk_forward_never_trains_on_test_data_and_records_every_trial() {
    let bars = walk(800, 9, 0.0);
    let mut ledger = TrialLedger::default();
    let wf = WalkForwardConfig { train: 200, test: 100, embargo: 10, ..WalkForwardConfig::default() };
    let r = run_walk_forward(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &grid(),
        &wf,
        &objective_sharpe,
        SweepOptions { threads: 2 },
        Some(&mut ledger),
    )
    .unwrap();
    assert_eq!(r.folds.len(), 5);
    for f in &r.folds {
        assert_eq!(f.fold.train_end + 10, f.fold.test_begin);
    }
    assert_eq!(ledger.count(), r.total_trials);
    assert_eq!(r.total_trials, 16 * 5);
}

#[test]
fn survival_lab_handles_a_strategy_that_never_trades() {
    struct Idle;
    impl Strategy for Idle {
        fn on_bar(&mut self, _: &mut Context<'_>, _: &[Bar]) {}
    }
    let idle = |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(Idle)) };
    let r = run_survival_lab(
        &walk(300, 1, 0.0),
        &idle,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();
    assert_eq!(r.costs.verdict, Verdict::Insufficient);
    assert!(r.costs.finding.contains("closed no trades"));
    assert_eq!(r.worst, Verdict::Insufficient);
    assert!(format_survival_report(&r).contains("NOT a score"));
    let json = r.to_json().unwrap();
    assert_eq!(json.at("schema_version").unwrap().as_int().unwrap(), 1);
    assert!(json.at("config").is_ok());
    for key in ["costs", "delay", "stability", "time", "trades", "sample", "regimes", "benchmark", "risk"] {
        assert!(json.at(key).is_ok(), "missing report component {key}");
    }
    let serialized = format!("{}\n", json.dump());
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/survival-report.json");
    if std::env::var_os("UPDATE_SURVIVAL_GOLDEN").is_some() {
        std::fs::write(golden, &serialized).unwrap();
    }
    assert_eq!(serialized, include_str!("golden/survival-report.json"));
}

#[test]
fn survival_lab_reports_progress_and_honours_cancellation() {
    struct Idle;
    impl Strategy for Idle {
        fn on_bar(&mut self, _: &mut Context<'_>, _: &[Bar]) {}
    }
    let idle = |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(Idle)) };
    let bars = walk(300, 1, 0.0);
    let cancelled = AtomicBool::new(true);
    let result = run_survival_lab_controlled(
        &bars,
        &idle,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
        Some(&cancelled),
        None,
    );
    assert!(matches!(result, Err(Error::Cancelled)));

    let mut events = Vec::new();
    let mut report_progress = |event| events.push(event);
    run_survival_lab_controlled(
        &bars,
        &idle,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
        None,
        Some(&mut report_progress),
    )
    .unwrap();
    assert_eq!(events.iter().map(|event| event.test_number).collect::<Vec<_>>(), (1..=9).collect::<Vec<_>>());
    assert!(events.iter().all(|event| event.step > 0 && event.total_steps >= event.step));
}

#[test]
fn survival_lab_flags_a_lucky_noise_winner_for_validation_not_stress() {
    // On pure noise the best of a sweep can still pass the fragility tests;
    // DSR is what rejects it. Both are reported; neither is collapsed.
    let bars = walk(1000, 12345, 0.0);
    let cfg = BacktestConfig::default();
    let mut ledger = TrialLedger::default();
    let sweep =
        run_sweep(&bars, &factory, &cfg, &grid(), &objective_sharpe, SweepOptions { threads: 4 }, 1).unwrap();
    ledger.add("noise", &sweep);
    let best = sweep.best_trial().unwrap();
    let v = validate_selection(best, &ledger, 252.0, None, 0.95);
    assert!(v.dsr < 0.95, "DSR {}", v.dsr);
    let sc = SurvivalConfig {
        integer_params: BTreeSet::from(["fast".into(), "slow".into()]),
        ..SurvivalConfig::default()
    };
    let r = run_survival_lab(&bars, &factory, &cfg, &best.params, &sc).unwrap();
    assert!(r.stability.neighbours.len() + r.stability.skipped >= 4);
}
