//! C++ <-> Rust parity for the risk engine: the same scenarios as
//! `parity/cpp/risk_golden.cpp` must print exactly what the C++ risk engine
//! printed (`parity/golden/risk/risk.golden`): the Monte Carlo kernel path by
//! path, every distribution metric, full CPU-backend engine runs, the seed
//! mixer, batch planner, backend selector, convergence check, validation
//! messages and CSV loading. Only one renaming is applied: the C++ selector's
//! "OpenMP" is the Rust "Parallel" backend.

// Bit-exact parity relies on glibc's libm (exp, log, pow): the golden files were produced on
// Linux x86-64 with glibc, and other C libraries may round transcendental functions differently.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::fmt::Write as _;
use std::path::PathBuf;

use spectra_core::cfmt::fmt_g17;
use spectra_risk::rng::seed_for_batch;
use spectra_risk::*;

fn metrics_line(out: &mut String, label: &str, v: &[f64], c: f64) {
    let p = Portfolio::new();
    let m = |metric| fmt_g17(compute(metric, v, c, &p).unwrap());
    write!(
        out,
        "{label} c={} var {} cvar {} vol {} dd {} semi {}",
        fmt_g17(c),
        m(Metric::VaR),
        m(Metric::CVaR),
        m(Metric::Volatility),
        m(Metric::DownsideDeviation),
        m(Metric::SemiDeviation)
    )
    .unwrap();
    if v.len() >= 3 {
        write!(out, " skew {}", m(Metric::Skewness)).unwrap();
    }
    if v.len() >= 4 {
        write!(out, " kurt {}", m(Metric::Kurtosis)).unwrap();
    }
    out.push('\n');
}

fn engine_line(out: &mut String, label: &str, metric: Metric, p: &Portfolio, cfg: &RiskConfig) {
    match RiskEngine::new().calculate(metric, p, cfg) {
        Ok(r) => writeln!(
            out,
            "engine {label} {} value {} sims {} seed {} backend {} conf {}",
            r.metric_name,
            fmt_g17(r.value),
            r.simulations_run,
            r.seed,
            r.backend_used.as_str(),
            fmt_g17(r.confidence)
        )
        .unwrap(),
        Err(e) => writeln!(out, "engine {label} error: {e}").unwrap(),
    }
}

fn cpp_name(b: Backend) -> &'static str {
    if b == Backend::Parallel {
        "OpenMP"
    } else {
        b.as_str()
    }
}

#[allow(clippy::too_many_lines)]
fn transcript() -> String {
    let mut out = String::new();
    for base in [0u64, 42, 7, u64::MAX] {
        write!(out, "seed {base}:").unwrap();
        for b in 0..4 {
            write!(out, " {}", seed_for_batch(base, b)).unwrap();
        }
        out.push('\n');
    }

    let task = |seed, n, drift, vol, init| SimulationTask {
        num_paths: n,
        seed,
        drift,
        volatility: vol,
        initial_value: init,
        horizon_years: 1.0,
    };
    for t in [
        task(42, 9, 0.0, 0.20, 1_000_000.0),
        task(0, 5, 0.05, 0.35, 12_345.678),
        task(7, 4, -0.1, 0.0, 100.0),
        task(123_456_789, 6, 0.0, 1.5, -5_000.0),
        task(1, 3, 0.0, 0.2, 0.0),
    ] {
        write!(out, "kernel seed {}:", t.seed).unwrap();
        for l in run_reference(&t).unwrap().terminal_losses {
            write!(out, " {}", fmt_g17(l)).unwrap();
        }
        out.push('\n');
    }
    let big = run_reference(&task(99, 200_001, 0.0, 0.25, 2_500_000.0)).unwrap().terminal_losses;
    out.push_str("big:");
    for l in big.iter().step_by(9_973) {
        write!(out, " {}", fmt_g17(*l)).unwrap();
    }
    writeln!(out, " last {}", fmt_g17(*big.last().unwrap())).unwrap();

    let ramp: Vec<f64> = (0..100).map(f64::from).collect();
    let small = [3.5, -1.25, 7.0, 0.0, 2.0];
    let ties = [1.0, 1.0, 1.0, 2.0, 2.0, 5.0];
    for c in [0.5, 0.9, 0.95, 0.99, 0.999, 0.01] {
        metrics_line(&mut out, "ramp", &ramp, c);
        metrics_line(&mut out, "small", &small, c);
        metrics_line(&mut out, "ties", &ties, c);
        metrics_line(&mut out, "big", &big, c);
    }
    metrics_line(&mut out, "one", &[4.0], 0.99);
    metrics_line(&mut out, "flat", &[2.0; 4], 0.99);
    let p0 = Portfolio::new();
    writeln!(out, "empty var: {}", compute(Metric::VaR, &[], 0.99, &p0).unwrap_err()).unwrap();
    writeln!(out, "short skew: {}", compute(Metric::Skewness, &[1.0, 2.0], 0.99, &p0).unwrap_err()).unwrap();

    for target in [0.001, 0.01, 0.05] {
        for n in [150usize, 5_000, 200_001] {
            let c = check_convergence(&big[..n], 0.99, target);
            writeln!(
                out,
                "convergence n={n} target={} converged {} rse {}",
                fmt_g17(target),
                u8::from(c.converged),
                fmt_g17(c.relative_std_error)
            )
            .unwrap();
        }
    }
    let c = check_convergence(&[3.0; 500], 0.95, 0.01);
    writeln!(
        out,
        "convergence flat converged {} rse {}",
        u8::from(c.converged),
        fmt_g17(c.relative_std_error)
    )
    .unwrap();

    for (total, cap) in [(10u64, 0u64), (10, 3), (9, 3), (5, 10), (2_000_001, 2_000_000)] {
        write!(out, "plan {total} {cap}:").unwrap();
        for b in plan_batches(total, cap) {
            write!(out, " {b}").unwrap();
        }
        out.push('\n');
    }
    for n in [1u64, 49_999, 50_000, 999_999, 1_000_000] {
        writeln!(
            out,
            "select {n} auto {} cpu {}",
            cpp_name(select_backend(Backend::Auto, n)),
            cpp_name(select_backend(Backend::Cpu, n))
        )
        .unwrap();
    }

    let mut p = Portfolio::new();
    p.add_asset("AAPL", 1000.0, 190.0)
        .add_asset("MSFT", 500.0, 420.0)
        .add_asset("GOOGL", 300.0, 175.0)
        .add_asset("NVDA", 200.0, 130.0);
    let mut single = Portfolio::new();
    single.add_asset("SYN", 1.0, 100.0);
    let mut cfg = RiskConfig { backend: Backend::Cpu, simulations: 20_000, ..RiskConfig::default() };
    for m in [
        Metric::VaR,
        Metric::CVaR,
        Metric::Volatility,
        Metric::DownsideDeviation,
        Metric::Skewness,
        Metric::Kurtosis,
    ] {
        engine_line(&mut out, "p99", m, &p, &cfg);
    }
    cfg.confidence = 0.95;
    cfg.seed = 7;
    cfg.simulations = 33_333;
    engine_line(&mut out, "p95", Metric::VaR, &p, &cfg);
    engine_line(&mut out, "p95", Metric::CVaR, &p, &cfg);
    cfg.simulations = 1;
    engine_line(&mut out, "one", Metric::VaR, &single, &cfg);
    cfg.simulations = 49_999;
    cfg.backend = Backend::Auto;
    engine_line(&mut out, "auto", Metric::VaR, &single, &cfg);

    let mut bad = RiskConfig { backend: Backend::Cpu, ..RiskConfig::default() };
    engine_line(&mut out, "empty", Metric::VaR, &Portfolio::new(), &bad);
    bad.confidence = 1.5;
    engine_line(&mut out, "conf", Metric::VaR, &single, &bad);
    bad.confidence = 0.99;
    bad.simulations = 0;
    engine_line(&mut out, "sims", Metric::VaR, &single, &bad);
    bad.simulations = 10;
    bad.adaptive = true;
    bad.target_rel_std_err = 0.0;
    engine_line(&mut out, "adaptive", Metric::VaR, &single, &bad);
    bad.adaptive = false;
    bad.verify_against_cpu = true;
    bad.verify_tolerance = -1.0;
    engine_line(&mut out, "verify", Metric::VaR, &single, &bad);
    let mut neg = Portfolio::new();
    neg.add_asset("X", 1.0, -2.0);
    engine_line(&mut out, "negprice", Metric::VaR, &neg, &RiskConfig::default());
    let mut nosym = Portfolio::new();
    nosym.add_asset("", 1.0, 2.0);
    engine_line(&mut out, "nosym", Metric::VaR, &nosym, &RiskConfig::default());

    for (name, text) in [
        ("ok.csv", "symbol,quantity,price\nAAPL,1000,190.00\nMSFT,500,420.00\n\nGOOGL,300,175.00\n"),
        ("short.csv", "symbol,quantity,price\nAAPL,-10,190.5\nBND,2000,72.25\n"),
        ("malformed.csv", "symbol,quantity,price\nAAPL,1000\n"),
        ("text.csv", "symbol,quantity,price\nAAPL,lots,190\n"),
    ] {
        match Portfolio::from_csv_str(text) {
            Ok(q) => {
                writeln!(out, "csv {name}: {} assets value {}", q.len(), fmt_g17(q.total_value())).unwrap();
            }
            Err(e) => writeln!(out, "csv {name} error: {e}").unwrap(),
        }
    }
    assert!(Portfolio::from_csv_file("/nonexistent/spectra/missing.csv").is_err());
    out.push_str("csv missing.csv error\n");
    out
}

#[test]
fn risk_engine_matches_cpp_reference() {
    let out = transcript();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden/risk/risk.golden");
    let golden = std::fs::read_to_string(path).unwrap();
    if out != golden {
        let (n, (a, g)) = out
            .lines()
            .zip(golden.lines())
            .enumerate()
            .find(|(_, (a, g))| a != g)
            .unwrap_or((0, ("<length differs>", "")));
        panic!("risk parity: first difference at line {}\n  rust: {a}\n  c++ : {g}", n + 1);
    }
}
