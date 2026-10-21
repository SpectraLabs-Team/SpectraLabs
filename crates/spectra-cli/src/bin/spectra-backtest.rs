//! spectra-backtest: run a backtest request document from the command line,
//! exactly as the backtest job will, and write the same outputs.
//!
//! ```text
//! spectra-backtest run REQUEST.json (--csv FILE | --store DIR --dataset ID) [--out DIR] [--points N]
//! spectra-backtest strategies     built-in strategies and their parameters, as JSON
//! spectra-backtest schema         JSON Schema of the configuration form
//! spectra-backtest presets        configuration presets, as JSON
//! ```
//!
//! `run` writes request.json, summary.json, equity.csv, orders.csv, fills.csv,
//! returns.csv and the downsampled equity and drawdown series to `--out`
//! (default `backtest-out`), then prints the run record. Its fingerprints are
//! the ones a job run of the same request and dataset must reproduce.
//!
//! Exit codes: 0 success, 1 run or data error, 2 usage error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use spectra_backtest::{
    artifacts, builtins_json, config_schema, presets_json, run_request_on, series_json, summary_json,
    BacktestRequest, SeriesKind, DEFAULT_SERIES_POINTS,
};
use spectra_core::{Json, Result};
use spectra_data::{load_csv_file, CsvOptions, DataStore};
use spectra_engine::{Progress, RunControl};

const USAGE: &str = "usage:
  spectra-backtest run REQUEST.json (--csv FILE | --store DIR --dataset ID) [--out DIR] [--points N]
  spectra-backtest strategies
  spectra-backtest schema
  spectra-backtest presets
";

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn print_json(r: Result<Json>) -> ExitCode {
    match r {
        Ok(j) => {
            println!("{}", j.dump());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

struct RunArgs {
    request: PathBuf,
    csv: Option<PathBuf>,
    store: Option<PathBuf>,
    dataset: Option<String>,
    out: PathBuf,
    points: usize,
}

fn parse_run(args: &[String]) -> Option<RunArgs> {
    let mut a = RunArgs {
        request: PathBuf::new(),
        csv: None,
        store: None,
        dataset: None,
        out: PathBuf::from("backtest-out"),
        points: DEFAULT_SERIES_POINTS,
    };
    let mut request = None;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args.get(i + 1).cloned();
        match flag {
            "--csv" => a.csv = Some(value?.into()),
            "--store" => a.store = Some(value?.into()),
            "--dataset" => a.dataset = Some(value?),
            "--out" => a.out = value?.into(),
            "--points" => a.points = value?.parse().ok()?,
            f if !f.starts_with("--") && request.is_none() => {
                request = Some(PathBuf::from(f));
                i += 1;
                continue;
            }
            _ => return None,
        }
        i += 2;
    }
    a.request = request?;
    let one_source = a.csv.is_some() != (a.store.is_some() && a.dataset.is_some());
    let half_store = a.store.is_some() != a.dataset.is_some();
    (one_source && !half_store).then_some(a)
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    std::fs::write(dir.join(name), bytes)?;
    Ok(())
}

fn run(a: &RunArgs) -> std::result::Result<(), String> {
    let text = std::fs::read_to_string(&a.request).map_err(|e| format!("{}: {e}", a.request.display()))?;
    let doc = Json::parse(&text).map_err(|e| format!("{}: {e}", a.request.display()))?;
    let request = BacktestRequest::from_json(&doc).map_err(|e| e.to_string())?;

    let dataset = match (&a.csv, &a.store, &a.dataset) {
        (Some(csv), _, _) => load_csv_file(csv, &CsvOptions::default()),
        (None, Some(store), Some(id)) => DataStore::new(store.clone()).and_then(|s| s.get(id)),
        _ => return Err("no dataset given".into()),
    }
    .map_err(|e| format!("dataset: {e}"))?;

    let mut last_tenth = 0;
    let mut report = |p: Progress| {
        let tenth = (p.fraction() * 10.0) as u32;
        if tenth > last_tenth {
            last_tenth = tenth;
            eprintln!("  {:>3}%  {} / {} bars", tenth * 10, p.bars_done, p.bars_total);
        }
    };
    let mut ctl = RunControl { progress: Some(&mut report), progress_every: 1000, ..RunControl::default() };
    let out = run_request_on(&dataset.bars, &dataset.symbols.symbols, &request, &mut ctl)
        .map_err(|e| e.to_string())?;

    std::fs::create_dir_all(&a.out).map_err(|e| format!("{}: {e}", a.out.display()))?;
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("request.json".into(), request.to_json().map_err(|e| e.to_string())?.dump().into_bytes()),
        ("summary.json".into(), summary_json(&out).map_err(|e| e.to_string())?.dump().into_bytes()),
    ];
    for kind in [SeriesKind::Equity, SeriesKind::Drawdown] {
        let s = series_json(&out.result.equity_curve, kind, a.points).map_err(|e| e.to_string())?;
        files.push((format!("{}_series.json", kind.as_str()), s.dump().into_bytes()));
    }
    for art in artifacts(&out.result) {
        files.push((art.name.to_string(), art.bytes));
    }
    for (name, bytes) in &files {
        write(&a.out, name, bytes).map_err(|e| format!("{name}: {e}"))?;
    }

    let r = &out.result;
    let m = &r.metrics;
    let name = if request.is_multi() { "multi-strategy run" } else { request.strategy.name };
    println!("{} on {} bars ({} instruments)", name, r.run.bar_count, dataset.symbols.symbols.len());
    if request.is_multi() {
        for (a, s) in request.strategies.iter().zip(&r.strategies) {
            println!(
                "  {:<16} weight {:.4}  return {:.2}%  params {}",
                a.strategy.name,
                a.weight,
                s.metrics.total_return * 100.0,
                a.params.canonical()
            );
        }
    } else {
        println!("  params           {}", request.params.canonical());
    }
    println!("  total return     {:.2}%", m.total_return * 100.0);
    println!("  sharpe           {:.2}", m.sharpe);
    println!("  max drawdown     {:.2}%", m.max_drawdown * 100.0);
    println!("  fills            {}", r.fill_count());
    println!("run record");
    println!("  engine           {}", r.run.engine_version);
    println!("  dataset          {}", r.run.dataset_sha256);
    println!("  config           {}", r.run.config_sha256);
    println!("  strategy         {}", request.strategy_sha256());
    println!("  result           {}", r.run.result_sha256);
    println!("wrote {} files to {}", files.len(), a.out.display());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("strategies") if args.len() == 1 => print_json(builtins_json()),
        Some("schema") if args.len() == 1 => print_json(config_schema()),
        Some("presets") if args.len() == 1 => print_json(presets_json()),
        Some("run") => {
            let Some(a) = parse_run(&args[1..]) else { return usage() };
            match run(&a) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => usage(),
    }
}
