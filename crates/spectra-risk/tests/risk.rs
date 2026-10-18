//! Behavioural tests for the risk engine: the C++ unit tests, plus the
//! fixes made in the port (deterministic parallel backend, distinct batch
//! seeds, a working CPU cross-check, horizons, honest unsupported metrics).

use spectra_core::{Error, Result};
use spectra_risk::*;

fn sample() -> Portfolio {
    let mut p = Portfolio::new();
    p.add_asset("AAPL", 1000.0, 190.0).add_asset("MSFT", 500.0, 420.0).add_asset("GOOGL", 300.0, 175.0);
    p
}

#[test]
fn portfolio_value_and_validation() {
    let mut p = Portfolio::new();
    p.add_asset("AAPL", 10.0, 150.0).add_asset("MSFT", 5.0, 300.0);
    assert_eq!(p.len(), 2);
    assert_eq!(p.total_value(), 10.0 * 150.0 + 5.0 * 300.0);
    let e = RiskEngine::new();
    assert!(matches!(
        e.calculate(Metric::VaR, &Portfolio::new(), &RiskConfig::default()),
        Err(Error::InvalidArgument(_))
    ));
    let bad = RiskConfig { confidence: 1.5, ..RiskConfig::default() };
    assert!(matches!(e.calculate(Metric::VaR, &p, &bad), Err(Error::InvalidArgument(_))));
    let nan = RiskConfig { confidence: f64::NAN, ..RiskConfig::default() };
    assert!(e.calculate(Metric::VaR, &p, &nan).is_err(), "NaN confidence must be rejected");
    let mut inf = Portfolio::new();
    inf.add_asset("X", f64::INFINITY, 1.0);
    assert!(inf.validate().is_err());
}

#[test]
fn var_metric_on_a_known_sequence() {
    let losses: Vec<f64> = (0..100).map(f64::from).collect();
    let p = Portfolio::new();
    let var99 = compute(Metric::VaR, &losses, 0.99, &p).unwrap();
    assert!((var99 - 98.01).abs() < 1e-9);
    assert!((compute(Metric::VaR, &losses, 0.5, &p).unwrap() - 49.5).abs() < 1e-9);
    assert!(compute(Metric::CVaR, &losses, 0.99, &p).unwrap() >= var99);
}

#[test]
fn parallel_backend_is_deterministic_for_any_thread_count() {
    let task = SimulationTask {
        num_paths: 3 * PARALLEL_CHUNK as u64 + 17,
        seed: 11,
        drift: 0.0,
        volatility: 0.3,
        initial_value: 1e6,
        horizon_years: 1.0,
    };
    let one = run_parallel(&task, 1).unwrap().terminal_losses;
    for threads in [2, 3, 8, 0] {
        assert_eq!(run_parallel(&task, threads).unwrap().terminal_losses, one, "threads={threads}");
    }
    // Chunk 0 is the reference stream, so the first chunk equals the CPU backend exactly.
    let cpu =
        run_reference(&SimulationTask { num_paths: PARALLEL_CHUNK as u64, ..task }).unwrap().terminal_losses;
    assert_eq!(&one[..PARALLEL_CHUNK], &cpu[..]);
    // Later chunks are different streams, not repeats.
    assert_ne!(&one[PARALLEL_CHUNK..2 * PARALLEL_CHUNK], &one[..PARALLEL_CHUNK]);
}

#[test]
fn cpu_and_parallel_var_agree_statistically() {
    let mut p = Portfolio::new();
    p.add_asset("SYN", 1.0, 100.0);
    let e = RiskEngine::new();
    let cpu = RiskConfig { backend: Backend::Cpu, simulations: 2_000_000, seed: 7, ..RiskConfig::default() };
    let par = RiskConfig { backend: Backend::Parallel, ..cpu };
    let a = e.calculate(Metric::VaR, &p, &cpu).unwrap();
    let b = e.calculate(Metric::VaR, &p, &par).unwrap();
    assert_eq!(b.backend_used, Backend::Parallel);
    assert!((a.value - b.value).abs() / a.value.abs() < 0.02, "{} vs {}", a.value, b.value);
}

#[test]
fn cuda_requests_fall_back_to_the_parallel_backend() {
    let cfg = RiskConfig { backend: Backend::Cuda, simulations: 10_000, ..RiskConfig::default() };
    let r = RiskEngine::new().calculate(Metric::VaR, &sample(), &cfg).unwrap();
    assert_eq!(r.requested_backend, Backend::Cuda);
    assert_eq!(r.backend_used, Backend::Parallel);
    let auto = RiskConfig { backend: Backend::Auto, simulations: 2_000_000, ..RiskConfig::default() };
    assert_eq!(resolve_backend(select_backend(auto.backend, auto.simulations)), Backend::Parallel);
}

#[test]
fn verify_against_cpu_passes_for_a_correct_backend() {
    let cfg = RiskConfig {
        backend: Backend::Parallel,
        simulations: 300_000,
        verify_against_cpu: true,
        ..RiskConfig::default()
    };
    RiskEngine::new().calculate(Metric::VaR, &sample(), &cfg).unwrap();
}

#[test]
fn adaptive_runs_stop_early_and_never_repeat_paths() {
    let cfg = RiskConfig {
        backend: Backend::Cpu,
        simulations: 2_000_000,
        adaptive: true,
        target_rel_std_err: 0.0015,
        ..RiskConfig::default()
    };
    let sim = simulate(&sample(), &cfg).unwrap();
    assert!(sim.simulations_run < 2_000_000, "ran {}", sim.simulations_run);
    assert!(sim.convergence.unwrap().converged);
    assert_eq!(sim.losses.len() as u64, sim.simulations_run);
    assert!(sim.batches >= 2);
    assert_ne!(&sim.losses[..1000], &sim.losses[50_000..51_000], "each batch has its own seed");
    // Batch 0 is exactly the non-adaptive run's first 50k paths.
    let plain = simulate(&sample(), &RiskConfig { adaptive: false, simulations: 50_000, ..cfg }).unwrap();
    assert_eq!(&sim.losses[..50_000], &plain.losses[..]);
}

#[test]
fn horizon_scales_risk() {
    let year = RiskConfig { simulations: 100_000, backend: Backend::Parallel, ..RiskConfig::default() };
    let day = RiskConfig { horizon_years: 1.0 / 252.0, ..year };
    let e = RiskEngine::new();
    let y = e.calculate(Metric::VaR, &sample(), &year).unwrap().value;
    let d = e.calculate(Metric::VaR, &sample(), &day).unwrap().value;
    let ratio = y / d;
    assert!(ratio > 12.0 && ratio < 20.0, "1y/1d VaR ratio {ratio}");
    assert!(RiskConfig { horizon_years: 0.0, ..year }.validate().is_err());
}

#[test]
fn volatility_column_is_read_and_used() {
    let p =
        Portfolio::from_csv_str("symbol,quantity,price,volatility\r\nA,10,100,0.40\r\nB,5,50,\r\n").unwrap();
    assert_eq!(p.assets()[0].volatility, 0.40);
    assert_eq!(p.assets()[1].volatility, 0.0);
    assert!((aggregate_volatility(&p) - 0.30).abs() < 1e-15);
    assert!(Portfolio::from_csv_str("s,q,p,v\nA,1,1,-0.2\n").is_err());
    assert!(Portfolio::from_csv_str("s,q,p\nA,1,12abc\n").is_err(), "partial numbers are rejected");
}

#[test]
fn hhi_comes_from_the_weights() {
    let mut p = Portfolio::new();
    p.add_asset("A", 1.0, 50.0).add_asset("B", -1.0, 50.0);
    assert!((p.hhi().unwrap() - 0.5).abs() < 1e-15);
    let r = RiskEngine::new()
        .calculate(Metric::Hhi, &sample(), &RiskConfig { simulations: 1_000, ..RiskConfig::default() })
        .unwrap();
    let w = sample().gross_weights();
    assert!((r.value - w.iter().map(|x| x * x).sum::<f64>()).abs() < 1e-15);
}

#[test]
fn placeholder_metrics_are_refused_without_simulating() {
    for m in Metric::all().filter(|m| !m.is_supported()) {
        let err = RiskEngine::new().calculate(m, &sample(), &RiskConfig::default()).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)), "{m:?}: {err}");
    }
    assert_eq!(Metric::parse("max_drawdown").unwrap(), Metric::MaxDrawdown);
    assert_eq!(Metric::parse("var").unwrap(), Metric::VaR);
    assert!(Metric::parse("nonsense").is_err());
}

#[test]
fn several_metrics_share_one_simulation() {
    let cfg = RiskConfig { simulations: 50_000, ..RiskConfig::default() };
    let r = RiskEngine::new().calculate_many(&["var", "cvar", "volatility"], &sample(), &cfg).unwrap();
    assert_eq!(r.len(), 3);
    assert!(r[1].value >= r[0].value);
    let single = RiskEngine::new().calculate(Metric::CVaR, &sample(), &cfg).unwrap();
    assert_eq!(single.value, r[1].value);
}

struct WorstLoss;
impl RiskPlugin for WorstLoss {
    fn name(&self) -> &'static str {
        "WorstLoss"
    }
    fn compute(&self, losses: &[f64], _: f64) -> Result<f64> {
        Ok(losses.iter().copied().fold(f64::NEG_INFINITY, f64::max))
    }
}

#[test]
fn plugins_are_registered_and_computed() {
    let mut e = RiskEngine::new();
    e.register_plugin(Box::new(WorstLoss)).unwrap();
    assert!(e.register_plugin(Box::new(WorstLoss)).is_err());
    let cfg = RiskConfig { simulations: 10_000, ..RiskConfig::default() };
    let worst = e.calculate_named("WorstLoss", &sample(), &cfg).unwrap();
    let var = e.calculate(Metric::VaR, &sample(), &cfg).unwrap();
    assert!(worst.value >= var.value);
    assert_eq!(e.plugin_names(), vec!["WorstLoss".to_string()]);
}

#[test]
fn stress_reports_the_shock_and_the_shocked_var() {
    let e = RiskEngine::new();
    let cfg = RiskConfig { simulations: 20_000, ..RiskConfig::default() };
    let s = stress_test(&e, &sample(), "market-crash", &cfg).unwrap();
    assert!((s.scenario_loss - 0.3 * sample().total_value()).abs() < 1e-6);
    let base = e.calculate(Metric::VaR, &sample(), &cfg).unwrap();
    assert!((s.report.value / base.value - 0.7).abs() < 1e-9, "VaR scales with value");
    assert!(stress_test(&e, &sample(), "alien-invasion", &cfg).is_err());
    let j = s.to_json_value();
    assert_eq!(j.at("scenario").unwrap().as_str().unwrap(), "market-crash");
    assert_eq!(j.at("metric").unwrap().as_str().unwrap(), "VaR");
}

#[test]
fn benchmark_uses_the_dashboard_schema() {
    let j = benchmark(&sample(), 1_000, 2, 0).unwrap();
    let rows = j.at("benchmarks").unwrap().items().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].at("requested_backend").unwrap().as_str().unwrap(), "CUDA");
    assert_eq!(rows[2].at("status").unwrap().as_str().unwrap(), "fallback");
    assert_eq!(rows[0].at("samples_ms").unwrap().items().unwrap().len(), 2);
    assert!(rows[1].at("latency_ms").unwrap().at("p95").is_ok());
    assert!(benchmark(&sample(), 10, 2, 0).is_err());
}

#[test]
fn report_json_keeps_the_cpp_fields_with_full_precision() {
    let cfg = RiskConfig { simulations: 5_000, ..RiskConfig::default() };
    let r = RiskEngine::new().calculate(Metric::VaR, &sample(), &cfg).unwrap();
    let j = spectra_core::Json::parse(&r.to_json()).unwrap();
    for k in [
        "metric",
        "value",
        "confidence",
        "simulations_run",
        "seed",
        "backend",
        "library_version",
        "git_commit",
        "total_ms",
        "compute_ms",
    ] {
        assert!(j.find(k).is_some(), "missing {k}");
    }
    assert_eq!(j.at("value").unwrap().as_double().unwrap(), r.value, "round-trips exactly");
}
