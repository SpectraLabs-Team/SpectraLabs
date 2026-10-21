//! C++ <-> Rust parity for the research layer: the same scenarios as
//! `parity/cpp/research_golden.cpp` must print exactly what the frozen C++
//! engine printed (`parity/golden/research.golden`): every trial fingerprint,
//! PBO, validation reports, walk-forward folds, the full Survival Lab report
//! and the statistics behind it.

// Bit-exact parity relies on glibc's libm (exp, log, pow): the golden files were produced on
// Linux x86-64 with glibc, and other C libraries may round transcendental functions differently.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use spectra_core::cfmt::fmt_g17;
use spectra_core::*;
use spectra_engine::*;
use spectra_research::*;

fn random_walk(n: usize, seed: u64, drift: f64, instruments: usize) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = vec![100.0f64; instruments];
    let mut bars = Vec::new();
    for i in 0..n {
        for (k, p) in px.iter_mut().enumerate() {
            let open = *p;
            let close = (open * (1.0 + drift + 0.01 * rng.normal())).max(1.0);
            bars.push(Bar {
                ts_ns: i as i64 + 1,
                known_at_ns: i as i64 + 1,
                instrument: k as InstrumentId,
                open: Price::from_f64(open).unwrap(),
                close: Price::from_f64(close).unwrap(),
                high: Price::from_f64(open.max(close) * 1.002).unwrap(),
                low: Price::from_f64(open.min(close) * 0.998).unwrap(),
                volume: Qty::from_f64(1_000_000.0).unwrap(),
            });
            *p = close;
        }
    }
    bars
}

/// Long/short SMA crossover on every instrument.
struct SmaCross {
    fast: usize,
    slow: usize,
    shorts: bool,
}

impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            let h = ctx.history(b.instrument);
            if h.len() < self.slow {
                continue;
            }
            let sma = |n: usize| {
                h[h.len() - n..].iter().map(|x| i128::from(x.close.raw())).sum::<i128>() / n as i128
            };
            let bullish = sma(self.fast) > sma(self.slow);
            let pos = ctx.position(b.instrument);
            let unit = Qty::from_f64(100.0).unwrap();
            if bullish && pos.raw() <= 0 {
                let q = if pos.raw() < 0 { unit + pos.checked_abs().unwrap() } else { unit };
                ctx.market(b.instrument, Side::Buy, q);
            } else if !bullish && pos.raw() > 0 {
                ctx.market(b.instrument, Side::Sell, if self.shorts { pos + unit } else { pos });
            } else if !bullish && self.shorts && pos.is_zero() {
                ctx.market(b.instrument, Side::Sell, unit);
            }
        }
    }
}

fn g(out: &mut String, label: &str, v: f64) {
    writeln!(out, "{label} {}", fmt_g17(v)).unwrap();
}

#[allow(clippy::too_many_lines)]
fn scenario(
    out: &mut String,
    title: &str,
    bars: &[Bar],
    shorts: bool,
    wf_train: usize,
    wf_test: usize,
    embargo: usize,
    mode: WalkForwardMode,
) {
    writeln!(out, "==== {title}").unwrap();
    let grid = ParamGrid::new()
        .add("fast", vec![2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0])
        .unwrap()
        .add("slow", vec![5.0, 15.0, 20.0, 25.0, 30.0, 40.0, 60.0, 80.0])
        .unwrap();
    let factory = move |p: &ParamSet| -> Result<Box<dyn Strategy>> {
        let fast = p.get_int("fast")?;
        let slow = p.get_int("slow")?;
        if fast >= slow {
            return Err(Error::Strategy("fast must be below slow".into()));
        }
        Ok(Box::new(SmaCross { fast: fast as usize, slow: slow as usize, shorts }))
    };
    let valid: Vec<ParamSet> =
        grid.all().into_iter().filter(|p| p.get_int("fast").unwrap() < p.get_int("slow").unwrap()).collect();
    let mut cfg = BacktestConfig {
        initial_cash: Money::from_f64(1_000_000.0).unwrap(),
        ledger: LedgerConfig::reference(shorts, false),
        ..BacktestConfig::default()
    };
    cfg.costs.slippage = Bps::from_f64(2.0).unwrap();
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0).unwrap();

    let mut ledger = TrialLedger::default();
    let sweep = run_sweep(
        bars,
        &factory,
        &cfg,
        &valid,
        &objective_sharpe,
        SweepOptions { threads: 3 },
        ledger.next_trial_id(),
    )
    .unwrap();
    ledger.add("full-sample sweep", &sweep);
    let best = sweep.best_trial().unwrap();
    writeln!(out, "trials {} best {} {}", sweep.trials.len(), sweep.best, best.params.canonical()).unwrap();
    for t in &sweep.trials {
        writeln!(
            out,
            "trial {} {} {} {} {}",
            t.id,
            t.params.canonical(),
            t.run.result_sha256,
            fmt_g17(t.objective),
            t.returns.len()
        )
        .unwrap();
    }
    g(out, "sharpe_variance", ledger.sharpe_variance());
    let all: Vec<Vec<f64>> = sweep.trials.iter().map(|t| t.returns.clone()).collect();
    for blocks in [8, 16] {
        let p = cscv_pbo(&all, blocks).unwrap();
        writeln!(
            out,
            "pbo {blocks} {} {} {} {} {} {}",
            fmt_g17(p.pbo),
            fmt_g17(p.prob_oos_loss),
            fmt_g17(p.mean_logit),
            p.combinations,
            p.strategies,
            p.observations_used
        )
        .unwrap();
    }
    let pbo = cscv_pbo(&all, 16).unwrap();
    let rep = validate_selection(best, &ledger, cfg.periods_per_year, Some(&pbo), 0.95);
    out.push_str(&format_report(&rep));
    g(out, "dsr", rep.dsr);
    g(out, "psr", rep.psr_vs_zero);
    g(out, "mintrl", rep.min_track_record_obs);

    let wf = WalkForwardConfig { train: wf_train, test: wf_test, embargo, mode, step: 0 };
    let w = run_walk_forward(
        bars,
        &factory,
        &cfg,
        &valid,
        &wf,
        &objective_sharpe,
        SweepOptions { threads: 2 },
        Some(&mut ledger),
    )
    .unwrap();
    writeln!(out, "walk-forward folds {} trials {} ledger {}", w.folds.len(), w.total_trials, ledger.count())
        .unwrap();
    for f in &w.folds {
        writeln!(
            out,
            "fold {} {} {} {} {} {} {} {}",
            f.fold.train_begin,
            f.fold.train_end,
            f.fold.test_begin,
            f.fold.test_end,
            f.best_params.canonical(),
            fmt_g17(f.in_sample_objective),
            fmt_g17(f.out_of_sample_objective),
            f.oos_run.result_sha256
        )
        .unwrap();
    }
    g(out, "wf_mean_is", w.mean_in_sample_objective);
    g(out, "wf_mean_oos", w.mean_out_of_sample_objective);
    g(out, "wf_oos_sharpe", w.oos_metrics.sharpe);
    g(out, "wf_oos_return", w.oos_metrics.total_return);
    let rep2 = validate_selection(best, &ledger, cfg.periods_per_year, None, 0.9);
    out.push_str(&format_report(&rep2));

    let sc = SurvivalConfig {
        integer_params: BTreeSet::from(["fast".to_string(), "slow".to_string()]),
        bootstrap_resamples: 500,
        ..SurvivalConfig::default()
    };
    let sr = run_survival_lab(bars, &factory, &cfg, &best.params, &sc).unwrap();
    out.push_str(&format_survival_report(&sr));
    let ts = &sr.trades.stats;
    writeln!(
        out,
        "tradestats {} {} {} {}",
        ts.n,
        ts.wins,
        ts.losses,
        [
            ts.win_rate,
            ts.avg_win,
            ts.avg_loss,
            ts.payoff_ratio,
            ts.expectancy,
            ts.profit_factor,
            ts.total_net,
            ts.best,
            ts.worst,
            ts.top_trade_share,
            ts.avg_holding_ns
        ]
        .map(fmt_g17)
        .join(" ")
    )
    .unwrap();
    let rs = &sr.trades.resampling;
    writeln!(
        out,
        "resampling {} {}",
        u8::from(rs.valid),
        [
            rs.observed_max_drawdown,
            rs.bootstrap_terminal_pnl.p05,
            rs.bootstrap_terminal_pnl.p95,
            rs.bootstrap_max_drawdown.p50,
            rs.bootstrap_prob_loss,
            rs.reshuffle_max_drawdown.p95,
            rs.reshuffle_prob_worse_drawdown
        ]
        .map(fmt_g17)
        .join(" ")
    )
    .unwrap();
    let bm = &sr.benchmark.metrics;
    writeln!(
        out,
        "benchmark {} {}",
        u8::from(bm.valid),
        [
            bm.beta,
            bm.alpha_annualized,
            bm.correlation,
            bm.tracking_error,
            bm.information_ratio,
            bm.up_capture,
            bm.down_capture,
            sr.benchmark.average_net_exposure
        ]
        .map(fmt_g17)
        .join(" ")
    )
    .unwrap();
    for b in &sr.regimes.buckets {
        writeln!(
            out,
            "regime {} {} {} {} {}",
            b.name,
            b.periods,
            fmt_g17(b.share),
            fmt_g17(b.mean_return),
            fmt_g17(b.sharpe)
        )
        .unwrap();
    }
    writeln!(
        out,
        "risk {} {} {} {} {} {}",
        fmt_g17(sr.risk.drawdown.max_drawdown),
        fmt_g17(sr.risk.drawdown.ulcer_index),
        sr.risk.drawdown.longest_underwater,
        fmt_g17(sr.risk.tail.var),
        fmt_g17(sr.risk.tail.es),
        fmt_g17(sr.risk.tail.worst)
    )
    .unwrap();
    writeln!(out, "stability skipped {} neighbours {}", sr.stability.skipped, sr.stability.neighbours.len())
        .unwrap();
}

#[test]
fn research_layer_matches_cpp_reference() {
    let mut out = String::new();
    let big = ParamGrid::new()
        .add("a", vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
        .unwrap()
        .add("b", vec![0.5, 1.5, 2.5])
        .unwrap()
        .add("c", vec![10.0, 20.0, 30.0, 40.0, 50.0])
        .unwrap();
    for seed in [1u64, 2, 99] {
        write!(out, "sample {seed}:").unwrap();
        for p in big.sample(9, seed) {
            write!(out, " {}", p.canonical()).unwrap();
        }
        out.push('\n');
    }
    scenario(
        &mut out,
        "noise, long only",
        &random_walk(1000, 12345, 0.0, 1),
        false,
        300,
        100,
        0,
        WalkForwardMode::Rolling,
    );
    scenario(
        &mut out,
        "drift, long/short, two instruments",
        &random_walk(600, 777, 0.001, 2),
        true,
        200,
        80,
        5,
        WalkForwardMode::Anchored,
    );
    scenario(
        &mut out,
        "short sample",
        &random_walk(160, 4242, 0.0005, 1),
        true,
        60,
        30,
        0,
        WalkForwardMode::Rolling,
    );

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden/research.golden");
    let golden = std::fs::read_to_string(path).unwrap();
    if out != golden {
        let (n, (a, g)) = out
            .lines()
            .zip(golden.lines())
            .enumerate()
            .find(|(_, (a, g))| a != g)
            .unwrap_or((0, ("<length>", "")));
        panic!("research parity: first difference at line {}\n  rust: {a}\n  c++ : {g}", n + 1);
    }
}
