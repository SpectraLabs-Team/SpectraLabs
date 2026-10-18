//! C++ <-> Rust parity for the registry and holdout vault. Runs the same
//! workflow as `parity/cpp/registry_golden.cpp` with the same deterministic
//! clock and requires:
//! * a byte-identical `registry.jsonl` (`parity/golden/registry.jsonl`), and
//! * an identical transcript (`parity/golden/registry.golden`).
//!
//! Runs are recorded with `engine_version` "0.6.0" (the C++ engine's) so the
//! log can match byte for byte; every other field is produced by the Rust
//! engine. Reproduction therefore adds one expected note about the engine
//! version, which is checked and removed before comparing.

// Bit-exact parity relies on glibc's libm (exp, log, pow): the golden files were produced on
// Linux x86-64 with glibc, and other C libraries may round transcendental functions differently.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use spectra_core::*;
use spectra_data::{format_timestamp, load_csv, CsvOptions, DataStore};
use spectra_engine::*;
use spectra_registry::*;
use spectra_research::*;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden").join(name)
}

fn temp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("spectra-registry-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn deterministic_clock() -> Clock {
    let tick = Arc::new(AtomicI64::new(0));
    Box::new(move || 1_700_000_000_000_000_000 + (tick.fetch_add(1, Ordering::SeqCst) + 1) * 1_000_000)
}

fn noise_csv(n: usize, seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut csv = String::from("timestamp,symbol,open,high,low,close,volume\n");
    let mut px = 100.0f64;
    for i in 0..n {
        let open = px;
        let close = (open * (1.0 + 0.01 * rng.normal())).max(1.0);
        let ts = (1_577_836_800i64 + i as i64 * 86_400) * 1_000_000_000;
        let p = |v: f64| Price::from_f64(v).unwrap().to_string();
        writeln!(
            csv,
            "{},DEMO,{},{},{},{},1000000",
            format_timestamp(ts),
            p(open),
            p(open.max(close) * 1.002),
            p(open.min(close) * 0.998),
            p(close)
        )
        .unwrap();
        px = close;
    }
    csv
}

struct SmaCross(usize, usize);
impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.1 {
            return;
        }
        // The C++ demo's window start `size - n` wraps when fast > history (grid2 has fast=11,
        // slow=10), which makes its loop empty: the sum is 0. Reproduced here for parity.
        let sma = |n: usize| {
            if n > h.len() {
                return 0;
            }
            h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128
        };
        let bullish = sma(self.0) > sma(self.1);
        let pos = ctx.position(id);
        if bullish && pos.is_zero() {
            ctx.market(id, Side::Buy, Qty::from_f64(100.0).unwrap());
        } else if !bullish && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn factory(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(SmaCross(p.get_int("fast")? as usize, p.get_int("slow")? as usize)))
}

fn as_reference_engine(mut s: SweepResult) -> SweepResult {
    for t in &mut s.trials {
        t.run.engine_version = "0.6.0".into();
    }
    s
}

fn b(v: bool) -> u8 {
    u8::from(v)
}

#[allow(clippy::too_many_lines)]
fn workflow(work: &Path) -> String {
    let mut out = String::new();
    let store = DataStore::new(work.join("data")).unwrap();
    let mut registry = Registry::with_clock(work.join("registry"), deterministic_clock()).unwrap();
    let strategy = "sma-cross";
    let source = sha256_hex(b"SmaCross: long while SMA(fast) > SMA(slow), fixed 100 units, v1");
    let mut cfg = BacktestConfig {
        initial_cash: Money::from_f64(1_000_000.0).unwrap(),
        ledger: LedgerConfig::reference(false, false),
        seed: 2026,
        ..BacktestConfig::default()
    };
    cfg.costs.slippage = Bps::from_f64(2.0).unwrap();
    cfg.costs.commission.bps_of_notional = Bps::from_f64(1.0).unwrap();

    let research = load_csv(noise_csv(1000, 31337).as_bytes(), &CsvOptions::default()).unwrap();
    let hold = load_csv(noise_csv(500, 424_242).as_bytes(), &CsvOptions::default()).unwrap();
    store.put(&research).unwrap();
    store.put(&hold).unwrap();
    writeln!(out, "datasets {} {}", research.info.id, hold.info.id).unwrap();

    {
        let mut vault = Vault::new(&mut registry, &store);
        vault.seal("final-check", &hold.info.id, 2, "one honest look before any paper trading").unwrap();
        let e = vault.seal("again", &hold.info.id, 1, "").unwrap_err();
        writeln!(out, "seal refused: {e}").unwrap();
    }

    let exp1 = registry
        .create_experiment(
            "sma-premium",
            "A simple moving-average crossover earns a risk premium",
            &["trend", "sma"],
            "",
        )
        .unwrap();
    let grid = ParamGrid::new()
        .add("fast", vec![2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0])
        .unwrap()
        .add("slow", vec![15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 80.0])
        .unwrap();
    let sweep1 = as_reference_engine(
        run_sweep(
            &research.bars,
            &factory,
            &cfg,
            &grid.all(),
            &objective_sharpe,
            SweepOptions { threads: 4 },
            1,
        )
        .unwrap(),
    );
    let ids1 = registry.record_sweep(&exp1, &research.info.id, strategy, &source, &cfg, &sweep1).unwrap();
    let best1 = sweep1.best_trial().unwrap().clone();
    writeln!(out, "exp1 {exp1} best {} {}", best1.params.canonical(), cfmt::fmt_g17(best1.metrics.sharpe))
        .unwrap();
    let v1 = validate_selection(&best1, &registry.trial_ledger().unwrap(), cfg.periods_per_year, None, 0.95);
    writeln!(out, "v1 {} {} {}", v1.n_trials, cfmt::fmt_g17(v1.psr_vs_zero), cfmt::fmt_g17(v1.dsr)).unwrap();

    let exp2 = registry
        .fork_experiment(&exp1, "sma-premium-v2", "Maybe a different grid works", &["trend"])
        .unwrap();
    let grid2 = ParamGrid::new()
        .add("fast", vec![1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 9.0, 11.0])
        .unwrap()
        .add("slow", vec![10.0, 14.0, 18.0, 22.0, 28.0, 35.0, 45.0, 70.0])
        .unwrap();
    let sweep2 = as_reference_engine(
        run_sweep(
            &research.bars,
            &factory,
            &cfg,
            &grid2.all(),
            &objective_sharpe,
            SweepOptions { threads: 4 },
            1,
        )
        .unwrap(),
    );
    registry.record_sweep(&exp2, &research.info.id, strategy, &source, &cfg, &sweep2).unwrap();
    let v2 = validate_selection(&best1, &registry.trial_ledger().unwrap(), cfg.periods_per_year, None, 0.95);
    writeln!(out, "v2 {} {} {}", v2.n_trials, cfmt::fmt_g17(v2.psr_vs_zero), cfmt::fmt_g17(v2.dsr)).unwrap();
    out.push_str("lineage");
    for id in registry.lineage(&exp2).unwrap() {
        write!(out, " {}", registry.experiment(&id).unwrap().name).unwrap();
    }
    out.push('\n');

    registry.add_note(&exp1, "first grid looked promising").unwrap();
    registry.add_note(&ids1[3], "inspected by hand").unwrap();

    let sc = SurvivalConfig {
        integer_params: ["fast".to_string(), "slow".to_string()].into_iter().collect(),
        ..SurvivalConfig::default()
    };
    let survival = run_survival_lab(&research.bars, &factory, &cfg, &best1.params, &sc).unwrap();
    let summary = Json::object().with("worst_verdict", Json::string(survival.worst.as_str()));
    let report_text = format_survival_report(&survival);
    let art = registry
        .record_artifact(&exp1, &ids1[0], "survival_report", report_text.as_bytes(), "(not saved)", summary)
        .unwrap();
    writeln!(out, "artifact {art} {}", survival.worst.as_str()).unwrap();

    {
        let mut vault = Vault::new(&mut registry, &store);
        let e = vault.checked_get(&hold.info.id).unwrap_err();
        writeln!(out, "refused: {e}").unwrap();
        writeln!(
            out,
            "checked_get research bars {}",
            vault.checked_get(&research.info.id).unwrap().bars.len()
        )
        .unwrap();
        for _ in 0..3 {
            match vault.evaluate("final-check", &factory, strategy, &source, &best1.params, &cfg) {
                Ok(h) => {
                    writeln!(
                        out,
                        "holdout {}/{} rem {} sharpe {} ret {} sr {} n {} psr {} dsr {} ems {} {}",
                        h.evaluation_no,
                        h.budget,
                        h.remaining,
                        cfmt::fmt_g17(h.metrics.sharpe),
                        cfmt::fmt_g17(h.metrics.total_return),
                        cfmt::fmt_g17(h.sharpe_per_period),
                        h.n_obs,
                        cfmt::fmt_g17(h.psr_vs_zero),
                        cfmt::fmt_g17(h.dsr),
                        cfmt::fmt_g17(h.expected_max_sharpe),
                        h.result_sha256
                    )
                    .unwrap();
                    for w in &h.warnings {
                        writeln!(out, "  warning: {w}").unwrap();
                    }
                }
                Err(e) => writeln!(out, "evaluate refused: {e}").unwrap(),
            }
        }
        let e = vault.evaluate("nope", &factory, strategy, &source, &best1.params, &cfg).unwrap_err();
        writeln!(out, "evaluate refused: {e}").unwrap();
        for st in vault.list().unwrap() {
            writeln!(
                out,
                "status {} {} budget {} started {} completed {} failed {} denied {} remaining {}",
                st.name,
                st.dataset_id,
                st.budget,
                st.started,
                st.completed,
                st.failed,
                st.denied,
                st.remaining()
            )
            .unwrap();
        }
    }

    let c = registry.compare(&ids1[0], &ids1[9]).unwrap();
    writeln!(
        out,
        "compare {} {} {} {} {} {} {} {}",
        b(c.same_dataset),
        b(c.same_strategy_source),
        b(c.same_params),
        b(c.same_config),
        b(c.same_engine),
        b(c.same_result),
        cfmt::fmt_g17(c.sharpe_delta),
        cfmt::fmt_g17(c.total_return_delta)
    )
    .unwrap();
    for d in &c.differences {
        writeln!(out, "  diff: {d}").unwrap();
    }
    let f = RunFilter { experiment_id: Some(exp1.clone()), min_sharpe: Some(0.0), ..RunFilter::default() };
    writeln!(
        out,
        "runs exp1 sharpe>=0: {} of {}; trials {}",
        registry.runs(&f).unwrap().len(),
        registry.runs(&RunFilter::default()).unwrap().len(),
        registry.trial_count(None).unwrap()
    )
    .unwrap();
    let vr = registry.verify();
    writeln!(out, "verify {} {} {}", b(vr.ok), vr.events, vr.last_hash).unwrap();
    let best_run_id = registry
        .runs(&f)
        .unwrap()
        .into_iter()
        .rfind(|r| r.result_sha256 == best1.run.result_sha256)
        .unwrap()
        .id;
    let rr = reproduce_run(&mut registry, &store, &best_run_id, &factory, &source).unwrap();
    writeln!(out, "reproduce {best_run_id} {} {}", b(rr.reproduced), rr.actual_result_sha256).unwrap();
    let engine_note = format!(
        "Recorded with engine 0.6.0; running {ENGINE_VERSION}. Results can legitimately differ across engine versions."
    );
    assert!(rr.notes.contains(&engine_note), "expected the engine-version note: {:?}", rr.notes);
    for n in rr.notes.iter().filter(|n| **n != engine_note) {
        writeln!(out, "  note: {n}").unwrap();
    }
    out
}

fn first_difference(a: &str, b: &str) -> String {
    a.lines().zip(b.lines()).enumerate().find(|(_, (x, y))| x != y).map_or_else(
        || "lengths differ".to_string(),
        |(n, (x, y))| format!("line {}\n  rust: {x}\n  c++ : {y}", n + 1),
    )
}

#[test]
fn registry_workflow_matches_cpp_reference_byte_for_byte() {
    let work = temp_dir("parity");
    let transcript = workflow(&work);
    let expected = std::fs::read_to_string(golden("registry.golden")).unwrap();
    assert!(transcript == expected, "transcript differs at {}", first_difference(&transcript, &expected));
    let log = std::fs::read_to_string(work.join("registry/registry.jsonl")).unwrap();
    let cpp_log = std::fs::read_to_string(golden("registry.jsonl")).unwrap();
    assert!(log == cpp_log, "registry.jsonl differs at {}", first_difference(&log, &cpp_log));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn rust_opens_verifies_and_extends_a_cpp_written_registry() {
    let work = temp_dir("cpp-log");
    std::fs::copy(golden("registry.jsonl"), work.join("registry.jsonl")).unwrap();
    let mut reg = Registry::open(&work).unwrap();
    let vr = reg.verify();
    assert!(vr.ok, "{}", vr.detail);
    assert_eq!(vr.events, 140);
    assert_eq!(reg.experiments().unwrap().len(), 2);
    assert_eq!(reg.trial_count(None).unwrap(), 128);
    let exp = reg.experiments().unwrap()[0].clone();
    assert_eq!(exp.notes, vec!["first grid looked promising".to_string()]);
    let run = &reg.runs(&RunFilter::default()).unwrap()[0];
    let cfg = config_from_json(&run.config).unwrap();
    assert_eq!(cfg.ledger.max_gross_leverage, None, "C++ records carry no leverage limit");
    assert_eq!(config_fingerprint(&cfg), run.config_sha256);
    reg.add_note(&exp.id, "extended by the Rust engine").unwrap();
    assert!(reg.verify().ok);
    let _ = std::fs::remove_dir_all(&work);
}
