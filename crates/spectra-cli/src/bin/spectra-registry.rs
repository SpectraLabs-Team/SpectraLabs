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
//!
//! # built with --features postgres; the database URL comes from --database-url or DATABASE_URL
//! spectra-registry --root DIR export --workspace UUID   (writes DIR/registry.jsonl, then verifies it)
//! spectra-registry --root DIR import --workspace UUID   (verifies DIR/registry.jsonl, then loads it
//!                                                        into an empty workspace)
//! ```
//!
//! Exit codes: 0 success, 1 error or failed verification, 2 usage.

use std::process::ExitCode;

use spectra_core::cfmt::fmt_g;
use spectra_data::DataStore;
use spectra_registry::{repair_incomplete_tail, system_clock_ns, Registry, RunFilter, Vault};

const USAGE: &str = "usage: spectra-registry --root DIR (experiments | runs [--experiment ID] [--strategy NAME] [--min-sharpe X] |
                                     show RUN_ID | trials | holdouts --data DIR | verify | repair |
                                     export --workspace UUID [--database-url URL] |
                                     import --workspace UUID [--database-url URL])
";

/// Where a Postgres transfer reads or writes.
struct Transfer {
    root: String,
    workspace: Option<String>,
    database_url: Option<String>,
}

#[cfg(feature = "postgres")]
fn transfer(command: &str, t: &Transfer) -> ExitCode {
    use spectra_registry::PgRegistry;

    let Some(workspace) = t.workspace.as_deref() else { return usage() };
    let Ok(workspace_id) = uuid::Uuid::parse_str(workspace) else {
        eprintln!("error: --workspace must be a UUID");
        return ExitCode::from(2);
    };
    let Some(url) = t.database_url.clone().or_else(|| std::env::var("DATABASE_URL").ok()) else {
        eprintln!("error: pass --database-url or set DATABASE_URL");
        return ExitCode::from(2);
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: could not start the async runtime: {e}");
            return ExitCode::from(1);
        }
    };
    let path = std::path::Path::new(&t.root).join("registry.jsonl");
    let result: Result<String, String> = runtime.block_on(async {
        let pool = spectra_db::connect(&spectra_db::PoolConfig::new(url)).await.map_err(|e| e.to_string())?;
        let registry = PgRegistry::new(pool, workspace_id);
        if command == "export" {
            if path.exists() {
                return Err(format!("{} already exists; export into an empty folder", path.display()));
            }
            let report = registry.verify().await.map_err(|e| e.to_string())?;
            if !report.ok {
                return Err(format!("the workspace chain is broken: {}", report.detail));
            }
            let text = registry.export_jsonl().await.map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&t.root).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
            // Re-open with the file registry: the same check the C++ engine applies.
            let file = Registry::open(&t.root).map_err(|e| format!("exported file does not verify: {e}"))?;
            let v = file.verify();
            if !v.ok {
                return Err(format!("exported file does not verify: {}", v.detail));
            }
            Ok(format!("EXPORTED: {} events to {}, head {}", v.events, path.display(), v.last_hash))
        } else {
            // Opening verifies the whole chain before anything touches the database.
            let file = Registry::open(&t.root).map_err(|e| e.to_string())?;
            let v = file.verify();
            if !v.ok {
                return Err(format!("{} does not verify: {}", path.display(), v.detail));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let n = registry.import_jsonl(&text).await.map_err(|e| e.to_string())?;
            let head = registry.verify().await.map_err(|e| e.to_string())?;
            Ok(format!("IMPORTED: {n} events into workspace {workspace_id}, head {}", head.last_hash))
        }
    });
    match result {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "postgres"))]
fn transfer(command: &str, t: &Transfer) -> ExitCode {
    let _ = (&t.root, &t.workspace, &t.database_url);
    eprintln!("error: `{command}` needs a build with `--features postgres`");
    ExitCode::from(2)
}

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

#[allow(clippy::too_many_lines)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut root, mut data_dir, mut command, mut positional) = (None, None, None::<String>, None::<String>);
    let (mut workspace, mut database_url) = (None, None);
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
            "--workspace" => workspace = value(),
            "--database-url" => database_url = value(),
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

    if command == "export" || command == "import" {
        return transfer(&command, &Transfer { root, workspace, database_url });
    }

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
