//! End-to-end research workflow on a market with NO real edge:
//! ingest -> store -> register an experiment -> sweep 64 variants (all
//! recorded) -> validate the best with the TRUE trial count -> try more
//! variants and watch the bar rise -> survival tests -> a metered look at a
//! sealed holdout -> verify the audit log -> reproduce a recorded run.
//!
//! Run with: `cargo run --release -p spectra-cli --example research_workflow [work_dir]`

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use spectra_core::{sha256_hex, Bar, Bps, Json, Money, Price, Qty, Result, Rng, Side};
use spectra_data::{format_timestamp, load_csv, CsvOptions, DataStore};
use spectra_engine::{BacktestConfig, Context, Strategy};
use spectra_registry::{reproduce_run, Registry, RunFilter, Vault};
use spectra_research::{
    format_survival_report, objective_sharpe, run_survival_lab, run_sweep, validate_selection, ParamGrid,
    ParamSet, SurvivalConfig, SweepOptions,
};

fn noise_csv(n: usize, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut csv = String::from("timestamp,symbol,open,high,low,close,volume\n");
    let mut px = 100.0f64;
    for i in 0..n {
        let open = px;
        let close = (open * (1.0 + 0.01 * rng.normal())).max(1.0);
        let ts = (1_577_836_800i64 + i as i64 * 86_400) * 1_000_000_000;
        let p = |v: f64| Price::from_f64(v).expect("finite").to_string();
        let _ = writeln!(
            csv,
            "{},DEMO,{},{},{},{},1000000",
            format_timestamp(ts),
            p(open),
            p(open.max(close) * 1.002),
            p(open.min(close) * 0.998),
            p(close)
        );
        px = close;
    }
    csv
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

fn heading(text: &str) {
    println!("\n--- {text}");
}

#[allow(clippy::too_many_lines)]
fn run(work: &std::path::Path) -> Result<bool> {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work)?;
    let store = DataStore::new(work.join("data"))?;
    let mut registry = Registry::open(work.join("registry"))?;
    let strategy = "sma-cross";
    let source = sha256_hex(b"SmaCross: long while SMA(fast) > SMA(slow), fixed 100 units, v1");
    let mut cfg = BacktestConfig {
        initial_cash: Money::from_f64(1_000_000.0)?,
        seed: 2026,
        ..BacktestConfig::default()
    };
    cfg.costs.slippage = Bps::from_f64(2.0)?;
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0)?;
    let opts = SweepOptions { threads: 4 };

    heading("1. Ingest and store data (a market with no real edge)");
    let research = load_csv(noise_csv(1000, 31337).as_bytes(), &CsvOptions::default())?;
    let hold = load_csv(noise_csv(500, 424_242).as_bytes(), &CsvOptions::default())?;
    store.put(&research)?;
    store.put(&hold)?;
    println!("research dataset {}... ({} bars)", &research.info.id[..16], research.bars.len());
    println!("holdout  dataset {}... ({} bars)", &hold.info.id[..16], hold.bars.len());

    heading("2. Seal the holdout with a budget of 2 evaluations");
    {
        let mut vault = Vault::new(&mut registry, &store);
        vault.seal("final-check", &hold.info.id, 2, "one honest look before any paper trading")?;
        println!("sealed 'final-check': budget {}", vault.status("final-check")?.budget);
    }

    heading("3. Register an experiment and sweep 64 variants (every trial is recorded)");
    let exp1 = registry.create_experiment(
        "sma-premium",
        "A simple moving-average crossover earns a risk premium",
        &["trend", "sma"],
        "",
    )?;
    let grid = ParamGrid::new()
        .add("fast", vec![2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0])?
        .add("slow", vec![15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 80.0])?;
    let sweep1 = run_sweep(&research.bars, &factory, &cfg, &grid.all(), &objective_sharpe, opts, 1)?;
    registry.record_sweep(&exp1, &research.info.id, strategy, &source, &cfg, &sweep1)?;
    let best1 = sweep1.best_trial().expect("non-empty sweep").clone();
    println!(
        "best of {}: {}  Sharpe {:.2} (in-sample)",
        sweep1.trials.len(),
        best1.params.canonical(),
        best1.metrics.sharpe
    );

    heading("4. Validate that winner using the TRUE number of trials in the registry");
    let v1 = validate_selection(&best1, &registry.trial_ledger()?, cfg.periods_per_year, None, 0.95);
    println!(
        "trials recorded {} | PSR {:.3} | DSR {:.3} (needs >= 0.95)",
        v1.n_trials, v1.psr_vs_zero, v1.dsr
    );

    heading("5. A second round: fork the experiment and try 64 more variants");
    let exp2 =
        registry.fork_experiment(&exp1, "sma-premium-v2", "Maybe a different grid works", &["trend"])?;
    let grid2 = ParamGrid::new()
        .add("fast", vec![1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 9.0, 11.0])?
        .add("slow", vec![10.0, 14.0, 18.0, 22.0, 28.0, 35.0, 45.0, 70.0])?;
    let sweep2 = run_sweep(&research.bars, &factory, &cfg, &grid2.all(), &objective_sharpe, opts, 1)?;
    registry.record_sweep(&exp2, &research.info.id, strategy, &source, &cfg, &sweep2)?;
    let v1_again = validate_selection(&best1, &registry.trial_ledger()?, cfg.periods_per_year, None, 0.95);
    println!(
        "the SAME winner, now judged against {} recorded trials: DSR {:.3} (was {:.3})",
        v1_again.n_trials, v1_again.dsr, v1.dsr
    );
    let names: Vec<String> = registry
        .lineage(&exp2)?
        .iter()
        .map(|id| registry.experiment(id).map(|x| x.name))
        .collect::<Result<_>>()?;
    println!("lineage: {}", names.join(" -> "));

    heading("6. Try to break the winner (Survival Lab) and record the report");
    let sc = SurvivalConfig {
        integer_params: BTreeSet::from(["fast".to_string(), "slow".to_string()]),
        ..SurvivalConfig::default()
    };
    let survival = run_survival_lab(&research.bars, &factory, &cfg, &best1.params, &sc)?;
    let summary = Json::object().with("worst_verdict", Json::string(survival.worst.as_str()));
    registry.record_artifact(
        &exp1,
        "",
        "survival_report",
        format_survival_report(&survival).as_bytes(),
        "(not saved)",
        summary,
    )?;
    println!(
        "most severe component verdict: {} | time-period: {} | sample size: {}",
        survival.worst.as_str(),
        survival.time.verdict.as_str(),
        survival.sample.verdict.as_str()
    );

    heading("7. One metered look at the sealed holdout");
    let h = {
        let mut vault = Vault::new(&mut registry, &store);
        if let Err(e) = vault.checked_get(&hold.info.id) {
            println!("direct access refused: {e}");
        }
        vault.evaluate("final-check", &factory, strategy, &source, &best1.params, &cfg)?
    };
    println!(
        "holdout evaluation {} of {}: Sharpe {:.2}, total return {:.2}%, PSR {:.3}, DSR {:.3}",
        h.evaluation_no,
        h.budget,
        h.metrics.sharpe,
        h.metrics.total_return * 100.0,
        h.psr_vs_zero,
        h.dsr
    );
    println!(
        "in-sample Sharpe was {:.2}; out-of-sample on the holdout is {:.2}",
        best1.metrics.sharpe, h.metrics.sharpe
    );

    heading("8. Audit: verify the log, reproduce a recorded run, anchor the head");
    let vr = registry.verify();
    println!("log verified: {} ({} events)", if vr.ok { "yes" } else { "NO" }, vr.events);
    let best_run_id = registry
        .runs(&RunFilter { experiment_id: Some(exp1.clone()), ..RunFilter::default() })?
        .into_iter()
        .find(|r| r.result_sha256 == best1.run.result_sha256)
        .map(|r| r.id)
        .unwrap_or_default();
    let rr = reproduce_run(&mut registry, &store, &best_run_id, &factory, &source)?;
    println!(
        "re-running {best_run_id} from the stored data: {}",
        if rr.reproduced { "result fingerprint matches" } else { "DOES NOT MATCH" }
    );
    println!("chain head (anchor this): {}", vr.last_hash);
    println!("\nregistry: {}", work.join("registry").join("registry.jsonl").display());
    Ok(vr.ok && rr.reproduced)
}

fn main() -> ExitCode {
    let work = std::env::args()
        .nth(1)
        .map_or_else(|| std::env::temp_dir().join("spectra_workflow_demo"), PathBuf::from);
    match run(&work) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
