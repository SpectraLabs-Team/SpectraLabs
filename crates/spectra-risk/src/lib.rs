//! Spectra Labs portfolio risk engine (the Rust port of the C++ risk engine).
//!
//! * [`portfolio`]: positions, CSV loading, validation, concentration (HHI).
//! * [`simulation`]: the GBM terminal-loss kernel on a CPU reference backend
//!   and a deterministic multi-threaded backend (CUDA is not built).
//! * [`metrics`]: VaR, CVaR and distribution metrics of simulated losses.
//! * [`engine`]: configuration, the orchestration pipeline, reports, plugins,
//!   stress scenarios and the backend benchmark.
//! * [`rng`]: `mt19937_64` and libstdc++'s normal distribution, reproduced
//!   exactly so CPU results match the C++ engine bit for bit.
//!
//! ```
//! use spectra_risk::{Metric, Portfolio, RiskConfig, RiskEngine};
//! let mut p = Portfolio::new();
//! p.add_asset("AAPL", 1000.0, 190.0).add_asset("MSFT", 500.0, 420.0);
//! let cfg = RiskConfig { simulations: 20_000, horizon_years: 1.0 / 252.0, ..RiskConfig::default() };
//! let var = RiskEngine::new().calculate(Metric::VaR, &p, &cfg).unwrap();
//! assert!(var.value > 0.0);
//! ```

pub mod engine;
pub mod metrics;
pub mod portfolio;
pub mod rng;
pub mod simulation;

pub use engine::{
    aggregate_volatility, benchmark, benchmark_with_progress, check_convergence, quantile, scenario,
    simulate, stress_test, Convergence, Plugin, PluginInfo, RiskConfig, RiskEngine, RiskPlugin, RiskReport,
    Scenario, Simulation, StressResult, DEFAULT_VOLATILITY, PLUGIN_API_VERSION, SCENARIOS,
};
pub use metrics::{compute, Metric};
pub use portfolio::{Asset, Portfolio};
pub use simulation::{
    plan_batches, resolve_backend, run_parallel, run_reference, select_backend, Backend, SimulationResult,
    SimulationTask, PARALLEL_CHUNK,
};
