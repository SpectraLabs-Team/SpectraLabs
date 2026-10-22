//! Demo: why the best backtest of many is not trustworthy.
//!
//! Two synthetic markets, same strategy family (SMA crossover), same 64-point
//! parameter grid: A. pure noise (nothing to find) and B. real drift
//! (something to find). For each we pick the best parameters in-sample and
//! then ask what can actually be claimed: DSR, PBO, walk-forward and the
//! Survival Lab.
//!
//! Run with: `cargo run --release -p spectra-cli --example overfitting`

use std::collections::BTreeSet;

use spectra_core::{Bar, Bps, Money, Price, Qty, Result, Rng, Side, ENGINE_VERSION};
use spectra_engine::{BacktestConfig, Context, Strategy};
use spectra_research::{
    cscv_pbo, format_report, objective_sharpe, run_survival_lab, run_sweep, run_walk_forward,
    validate_selection, ParamGrid, ParamSet, SurvivalConfig, SweepOptions, TrialLedger, Verdict,
    WalkForwardConfig,
};

fn random_walk(n: usize, seed: u64, drift: f64) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut px = 100.0f64;
    (0..n)
        .map(|i| {
            let open = px;
            let close = (open * (1.0 + drift + 0.01 * rng.normal())).max(1.0);
            px = close;
            let p = |v: f64| Price::from_f64(v).expect("finite price");
            Bar {
                ts_ns: i as i64 + 1,
                known_at_ns: i as i64 + 1,
                instrument: 1,
                open: p(open),
                close: p(close),
                high: p(open.max(close) * 1.002),
                low: p(open.min(close) * 0.998),
                volume: Qty::from_f64(1_000_000.0).expect("finite"),
            }
        })
        .collect()
}

struct SmaCross(usize, usize);
impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.1.max(self.0) {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let bullish = sma(self.0) > sma(self.1);
        let pos = ctx.position(id);
        if bullish && pos.is_zero() {
            ctx.market(id, Side::Buy, Qty::from_f64(100.0).expect("finite"));
        } else if !bullish && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn factory(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(SmaCross(p.get_int("fast")? as usize, p.get_int("slow")? as usize)))
}

fn run_scenario(title: &str, bars: &[Bar]) -> Result<()> {
    let rule = "=".repeat(66);
    println!("\n{rule}\n{title}\n{rule}");
    let params = ParamGrid::new()
        .add("fast", vec![2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0])?
        .add("slow", vec![15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 80.0])?
        .all();
    let mut cfg = BacktestConfig { initial_cash: Money::from_f64(1_000_000.0)?, ..BacktestConfig::default() };
    cfg.costs.slippage = Bps::from_f64(2.0)?;
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0)?;
    let opts = SweepOptions { threads: 4 };

    let mut ledger = TrialLedger::default();
    let sweep = run_sweep(bars, &factory, &cfg, &params, &objective_sharpe, opts, ledger.next_trial_id())?;
    ledger.add("full-sample sweep", &sweep);
    let best = sweep.best_trial().expect("non-empty sweep");
    println!("\n1) Best of {} parameter sets, in-sample", sweep.trials.len());
    println!(
        "   params {}  ->  Sharpe {:.2}, total return {:.1}%, max drawdown {:.1}%",
        best.params.canonical(),
        best.metrics.sharpe,
        best.metrics.total_return * 100.0,
        best.metrics.max_drawdown * 100.0
    );

    let all: Vec<Vec<f64>> = sweep.trials.iter().map(|t| t.returns.clone()).collect();
    let pbo = cscv_pbo(&all, 16)?;
    let report = validate_selection(best, &ledger, cfg.periods_per_year, Some(&pbo), 0.95);
    print!("\n2) Validation of that selection\n{}", format_report(&report));

    let wf = WalkForwardConfig { train: 300, test: 100, ..WalkForwardConfig::default() };
    let w = run_walk_forward(bars, &factory, &cfg, &params, &wf, &objective_sharpe, opts, None)?;
    println!("\n3) Walk-forward ({} folds, train {} / test {})", w.folds.len(), wf.train, wf.test);
    println!("   mean in-sample Sharpe       {:6.2}", w.mean_in_sample_objective);
    println!("   mean out-of-sample Sharpe   {:6.2}", w.mean_out_of_sample_objective);
    println!(
        "   stitched OOS Sharpe         {:6.2}   total return {:.1}%",
        w.oos_metrics.sharpe,
        w.oos_metrics.total_return * 100.0
    );

    let sc = SurvivalConfig {
        integer_params: BTreeSet::from(["fast".to_string(), "slow".to_string()]),
        ..SurvivalConfig::default()
    };
    let sr = run_survival_lab(bars, &factory, &cfg, &best.params, &sc)?;
    println!("\n4) Survival Lab on {}", best.params.canonical());
    let line =
        |name: &str, v: Verdict, finding: &str| println!("   [{:<12}] {name:<28} {finding}", v.as_str());
    line("Cost sensitivity", sr.costs.verdict, &sr.costs.finding);
    line("Execution delay", sr.delay.verdict, &sr.delay.finding);
    line("Parameter stability", sr.stability.verdict, &sr.stability.finding);
    line("Time-period robustness", sr.time.verdict, &sr.time.finding);
    line("Trade bootstrap", sr.trades.verdict, &sr.trades.finding);
    line("Sample size", sr.sample.verdict, &sr.sample.finding);
    line("Regime sensitivity", sr.regimes.verdict, &sr.regimes.finding);
    line("Benchmark-relative", sr.benchmark.verdict, &sr.benchmark.finding);
    line("Drawdown & tail risk", sr.risk.verdict, &sr.risk.finding);
    println!("   most severe component verdict: {} (not a score)", sr.worst.as_str());
    Ok(())
}

fn main() -> Result<()> {
    println!("Spectra Labs {ENGINE_VERSION} - overfitting demo");
    run_scenario(
        "Scenario A: PURE NOISE (zero drift). There is nothing to find.",
        &random_walk(1000, 12345, 0.0),
    )?;
    run_scenario(
        "Scenario B: market WITH real drift (+0.10% per bar). There is something to find.",
        &random_walk(1000, 12345, 0.001),
    )
}
