//! spectra-registry: inspect (and repair) an experiment registry.
//!
//! ```text
//! spectra-registry --root DIR experiments
//! spectra-registry --root DIR runs [--experiment ID] [--strategy NAME] [--min-sharpe X]
//! spectra-registry --root DIR show RUN_ID
//! spectra-registry --root DIR trials
//! spectra-registry --root DIR holdouts --data DATA_DIR
//! spectra-registry --root DIR verify
//! spectra-registry --root DIR repair      (new: removes a crashed writer's incomplete last line)
//! ```
//!
//! Exit codes: 0 success, 1 error or failed verification, 2 usage.

use std::process::ExitCode;

use spectra_core::cfmt::fmt_g;
use spectra_data::DataStore;
use spectra_registry::{repair_incomplete_tail, system_clock_ns, Registry, RunFilter, Vault};

const USAGE: &str = "usage: spectra-registry --root DIR (experiments | runs [--experiment ID] [--strategy NAME] [--min-sharpe X] |
                                     show RUN_ID | trials | holdouts --data DIR | verify | repair)
";

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

#[allow(clippy::too_many_lines)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut root, mut data_dir, mut command, mut positional) = (None, None, None::<String>, None::<String>);
    let mut filter = RunFilter::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].clone();
        let mut value = || {
            i += 1;
            args.get(i).cloned()
        };
        match a.as_str() {
            "--root" => root = value(),
            "--data" => data_dir = value(),
            "--experiment" => filter.experiment_id = value(),
            "--strategy" => filter.strategy = value(),
            "--min-sharpe" => match value().and_then(|v| v.parse().ok()) {
                Some(x) => filter.min_sharpe = Some(x),
                None => return usage(),
            },
            _ if command.is_none() => command = Some(a),
            _ if positional.is_none() => positional = Some(a),
            _ => return usage(),
        }
        i += 1;
    }
    let (Some(root), Some(command)) = (root, command) else { return usage() };

    if command == "repair" {
        return match repair_incomplete_tail(&root, system_clock_ns()) {
            Ok(r) if r.bytes_removed == 0 => {
                println!("OK: nothing to repair ({} events)", r.events_kept);
                ExitCode::SUCCESS
            }
            Ok(r) => {
                println!(
                    "REPAIRED: removed {} bytes (sha256 {}) after {} complete events; logged as registry.repaired",
                    r.bytes_removed, r.removed_sha256, r.events_kept
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        };
    }

    // Opening verifies the whole chain; `verify` reports a broken log as its result.
    let mut reg = match Registry::open(&root) {
        Ok(r) => r,
        Err(e) if command == "verify" => {
            println!("BROKEN: {e}");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    let result = (|| -> spectra_core::Result<ExitCode> {
        match command.as_str() {
            "experiments" => {
                for x in reg.experiments()? {
                    let runs = reg
                        .runs(&RunFilter { experiment_id: Some(x.id.clone()), ..RunFilter::default() })?
                        .len();
                    let parent = if x.parent_id.is_empty() {
                        String::new()
                    } else {
                        format!("  parent={}", x.parent_id)
                    };
                    println!("{}  {:<24} runs={runs}{parent}\n    {}", x.id, x.name, x.hypothesis);
                }
            }
            "runs" => {
                for r in reg.runs(&filter)? {
                    println!(
                        "{}  {:<14} {:<10} {:<28} Sharpe {:6.2}  ret {:7.2}%",
                        r.id,
                        r.kind,
                        r.strategy,
                        r.params.canonical(),
                        r.metrics.sharpe,
                        r.metrics.total_return * 100.0
                    );
                }
            }
            "show" => {
                let Some(id) = &positional else { return Ok(usage()) };
                let r = reg.run(id)?;
                println!(
                    "run        {} ({})\nexperiment {}\nstrategy   {}  source {}\nparams     {}\ndataset    {}",
                    r.id,
                    r.kind,
                    r.experiment_id,
                    r.strategy,
                    r.strategy_sha256,
                    r.params.canonical(),
                    r.dataset_id
                );
                println!(
                    "engine     {}  seed {}\nconfig     {}\nresult     {}\nSharpe {:.3}  return {:.3}%  max drawdown {:.3}%\nconfig json: {}",
                    r.engine_version,
                    r.seed,
                    r.config_sha256,
                    r.result_sha256,
                    r.metrics.sharpe,
                    r.metrics.total_return * 100.0,
                    r.metrics.max_drawdown * 100.0,
                    r.config.dump()
                );
                for n in &r.notes {
                    println!("note: {n}");
                }
            }
            "trials" => {
                let l = reg.trial_ledger()?;
                println!(
                    "{} trials recorded; Sharpe variance across trials {}",
                    l.count(),
                    fmt_g(l.sharpe_variance(), 6)
                );
            }
            "holdouts" => {
                let Some(dir) = &data_dir else { return Ok(usage()) };
                let store = DataStore::new(dir)?;
                let mut vault = Vault::new(&mut reg, &store);
                for h in vault.list()? {
                    println!(
                        "{}  dataset {}...  budget {}  used {}  completed {}  failed {}  denied {}  remaining {}",
                        h.name,
                        h.dataset_id.chars().take(16).collect::<String>(),
                        h.budget,
                        h.started,
                        h.completed,
                        h.failed,
                        h.denied,
                        h.remaining()
                    );
                }
            }
            "verify" => {
                let v = reg.verify();
                let detail = if v.detail.is_empty() { String::new() } else { format!("\n  {}", v.detail) };
                println!(
                    "{}: {} events, head {}{detail}",
                    if v.ok { "OK" } else { "BROKEN" },
                    v.events,
                    v.last_hash
                );
                return Ok(if v.ok { ExitCode::SUCCESS } else { ExitCode::from(1) });
            }
            _ => return Ok(usage()),
        }
        Ok(ExitCode::SUCCESS)
    })();
    match result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
