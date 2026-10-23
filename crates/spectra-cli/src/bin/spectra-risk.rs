//! spectra-risk: portfolio risk from the command line. Prints one JSON line.
//!
//! ```text
//! spectra-risk risk <portfolio.csv> [--metric var|cvar|volatility|downsidedeviation|semideviation|
//!                   skewness|kurtosis|hhi] [--confidence 0.99] [--simulations 1000000]
//!                   [--backend auto|cpu|parallel|cuda] [--seed 42] [--adaptive] [--verify]
//!                   [--horizon-days N | --horizon-years X] [--threads N]
//! spectra-risk benchmark <portfolio.csv> [--simulations 100000] [--repetitions 5] [--warmups 1]
//! spectra-risk stress <portfolio.csv> [--scenario market-crash|mild-correction] [run options]
//! spectra-risk metrics
//! ```
//!
//! CSV columns: `symbol,quantity,price[,volatility]`. The default horizon is
//! one year (as in the C++ engine); `--horizon-days 1` gives a one-trading-day
//! VaR (1/252 year). `openmp` is accepted as an alias of `parallel`.
//!
//! Exit codes: 0 success, 1 error, 2 usage.

use std::process::ExitCode;

use spectra_risk::{benchmark, stress_test, Backend, Metric, Portfolio, RiskConfig, RiskEngine, SCENARIOS};

const USAGE: &str = "usage:
  spectra-risk risk <portfolio.csv> [--metric NAME] [--confidence 0.99] [--simulations 1000000]
                    [--backend auto|cpu|parallel|cuda] [--seed 42] [--adaptive] [--verify]
                    [--horizon-days N | --horizon-years X] [--threads N]
  spectra-risk benchmark <portfolio.csv> [--simulations 100000] [--repetitions 5] [--warmups 1]
  spectra-risk stress <portfolio.csv> [--scenario market-crash|mild-correction] [run options]
  spectra-risk metrics
";

fn usage(msg: &str) -> ExitCode {
    if !msg.is_empty() {
        eprintln!("{msg}");
    }
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn parse<T: std::str::FromStr>(flag: &str, v: Option<&String>) -> Result<T, String> {
    v.ok_or_else(|| format!("Missing value for {flag}"))?
        .parse()
        .map_err(|_| format!("Invalid value for {flag}"))
}

struct Options {
    config: RiskConfig,
    metric: String,
    scenario: String,
    repetitions: usize,
    warmups: usize,
    simulations_given: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        config: RiskConfig::default(),
        metric: "var".into(),
        scenario: "market-crash".into(),
        repetitions: 5,
        warmups: 1,
        simulations_given: false,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let next = args.get(i + 1);
        let mut takes = true;
        match flag {
            "--metric" => o.metric = parse(flag, next)?,
            "--confidence" => o.config.confidence = parse(flag, next)?,
            "--simulations" => {
                o.config.simulations = parse(flag, next)?;
                o.simulations_given = true;
            }
            "--backend" => {
                o.config.backend =
                    Backend::parse(&parse::<String>(flag, next)?).map_err(|e| e.to_string())?;
            }
            "--seed" => o.config.seed = parse(flag, next)?,
            "--horizon-years" => o.config.horizon_years = parse(flag, next)?,
            "--horizon-days" => o.config.horizon_years = parse::<f64>(flag, next)? / 252.0,
            "--threads" => o.config.threads = parse(flag, next)?,
            "--dependence" | "--copula" => {
                o.config.dependence = spectra_risk::DependenceModel::parse(&parse::<String>(flag, next)?)
                    .map_err(|e| e.to_string())?;
            }
            "--scenario" => o.scenario = parse(flag, next)?,
            "--repetitions" => o.repetitions = parse(flag, next)?,
            "--warmups" => o.warmups = parse(flag, next)?,
            "--adaptive" => {
                o.config.adaptive = true;
                takes = false;
            }
            "--verify" => {
                o.config.verify_against_cpu = true;
                takes = false;
            }
            _ => return Err(format!("Unknown argument: {flag}")),
        }
        i += if takes { 2 } else { 1 };
    }
    Ok(o)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else { return usage("") };
    if command == "metrics" {
        for m in Metric::all() {
            println!(
                "{:<22} {}",
                m.name(),
                if m.is_supported() { "supported" } else { "recognized, not computable by this model" }
            );
        }
        for s in SCENARIOS {
            println!("scenario {:<14} x{} ({})", s.name, s.shock_factor, s.description);
        }
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
        return usage("missing portfolio CSV");
    };
    let opts = match parse_options(&args[2..]) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    let run = || -> spectra_core::Result<String> {
        let portfolio = Portfolio::from_csv_file(path)?;
        let engine = RiskEngine::new();
        match command.as_str() {
            "risk" => Ok(engine.calculate_named(&opts.metric, &portfolio, &opts.config)?.to_json()),
            "stress" => {
                Ok(stress_test(&engine, &portfolio, &opts.scenario, &opts.config)?.to_json_value().dump())
            }
            "benchmark" => {
                let sims = if opts.simulations_given { opts.config.simulations } else { 100_000 };
                Ok(benchmark(&portfolio, sims, opts.repetitions, opts.warmups)?.dump())
            }
            other => Err(spectra_core::Error::InvalidArgument(format!("Unknown command: {other}"))),
        }
    };
    match run() {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("spectra-risk error: {e}");
            ExitCode::from(1)
        }
    }
}
