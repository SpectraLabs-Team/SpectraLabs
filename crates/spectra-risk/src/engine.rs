//! The risk engine: validation -> backend selection -> batch planning ->
//! simulation -> optional adaptive early stop -> optional cross-check
//! against the CPU reference -> metric -> a reproducible report.

use std::time::Instant;

use spectra_core::{Error, Json, Result, ENGINE_VERSION};

use crate::copula::{simulate_correlated, DependenceModel, PositionRiskContribution};
use crate::evt::EvtTailModel;
use crate::metrics::{compute, quantile_sorted, Metric};
use crate::portfolio::Portfolio;
use crate::simulation::{
    execute, plan_batches, resolve_backend, run_reference, select_backend, Backend, SimulationTask,
};

/// Default volatility for assets that do not state one.
pub const DEFAULT_VOLATILITY: f64 = 0.20;

/// Batch size of adaptive runs (the convergence check runs between batches).
pub const ADAPTIVE_BATCH: u64 = 50_000;

/// Paths cross-checked against the CPU reference when `verify_against_cpu` is set.
pub const VERIFY_PATHS: u64 = 100_000;

/// Run configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskConfig {
    /// Quantile for VaR/CVaR, in (0, 1).
    pub confidence: f64,
    /// Paths (an upper bound when `adaptive`).
    pub simulations: u64,
    /// Seed.
    pub seed: u64,
    /// Backend request.
    pub backend: Backend,
    /// Stop early once the tail estimate is stable.
    pub adaptive: bool,
    /// Target relative standard error of the tail mean (adaptive runs).
    pub target_rel_std_err: f64,
    /// Re-run the first paths on the CPU reference and compare.
    pub verify_against_cpu: bool,
    /// Largest accepted relative difference in that comparison.
    pub verify_tolerance: f64,
    /// Horizon in years (1.0 = the C++ engine's implicit horizon; 1/252 for one trading day).
    pub horizon_years: f64,
    /// Worker threads for the parallel backend; 0 = all cores. Never changes results.
    pub threads: usize,
    /// Cross-asset dependence model (defaults to Aggregate for parity).
    pub dependence: DependenceModel,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            confidence: 0.99,
            simulations: 1_000_000,
            seed: 42,
            backend: Backend::Auto,
            adaptive: false,
            target_rel_std_err: 0.01,
            verify_against_cpu: false,
            verify_tolerance: 1e-6,
            horizon_years: 1.0,
            threads: 0,
            dependence: DependenceModel::Aggregate,
        }
    }
}

impl RiskConfig {
    /// Checks the configuration; messages match the C++ engine.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an out-of-range parameter.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::InvalidArgument(m.into()));
        if !(self.confidence > 0.0 && self.confidence < 1.0) {
            return bad("confidence must be in (0, 1).");
        }
        if self.simulations == 0 {
            return bad("simulations must be > 0.");
        }
        if self.adaptive && !(self.target_rel_std_err > 0.0) {
            return bad("target_rel_std_err must be > 0 when adaptive=true.");
        }
        if self.verify_against_cpu && !(self.verify_tolerance > 0.0) {
            return bad("verify_tolerance must be > 0.");
        }
        if !(self.horizon_years > 0.0 && self.horizon_years.is_finite()) {
            return bad("horizon_years must be > 0.");
        }
        Ok(())
    }
}

/// Outcome of a convergence check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Convergence {
    /// Whether the tail estimate is stable enough to stop.
    pub converged: bool,
    /// Standard error of the tail mean relative to the tail mean.
    pub relative_std_error: f64,
}

/// Whether the tail beyond the `confidence` quantile is estimated precisely
/// enough: the standard error of its mean (the CVaR estimate) relative to
/// that mean must be at most `target`. Needs 200 losses and 30 in the tail.
pub fn check_convergence(losses: &[f64], confidence: f64, target: f64) -> Convergence {
    let not_yet = Convergence { converged: false, relative_std_error: 1.0 };
    if losses.len() < 200 {
        return not_yet;
    }
    let mut sorted = losses.to_vec();
    sorted.sort_by(f64::total_cmp);
    let threshold = sorted[(confidence * (sorted.len() - 1) as f64).floor() as usize];
    let tail: Vec<f64> = sorted.iter().copied().filter(|l| *l >= threshold).collect();
    if tail.len() < 30 {
        return not_yet;
    }
    let mean = tail.iter().fold(0.0, |a, v| a + v) / tail.len() as f64;
    let var = tail.iter().fold(0.0, |a, v| a + (v - mean) * (v - mean)) / (tail.len() - 1) as f64;
    let se = (var / tail.len() as f64).sqrt();
    let rse = if mean == 0.0 { se } else { (se / mean).abs() };
    Convergence { converged: rse <= target, relative_std_error: rse }
}

/// The simulated losses of a run and how they were produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// One loss per simulated path.
    pub losses: Vec<f64>,
    /// What the caller asked for.
    pub requested_backend: Backend,
    /// What actually ran.
    pub backend_used: Backend,
    /// Paths simulated (fewer than requested if an adaptive run converged).
    pub simulations_run: u64,
    /// Batches run.
    pub batches: usize,
    /// Last convergence check (adaptive runs only).
    pub convergence: Option<Convergence>,
    /// The first batch's task (the one `verify_against_cpu` re-runs).
    pub first_task: SimulationTask,
    /// Wall time.
    pub total_ms: f64,
    /// Kernel time.
    pub compute_ms: f64,
}

/// Aggregate volatility of the model: the mean of the assets' volatilities.
pub fn aggregate_volatility(portfolio: &Portfolio) -> f64 {
    let a = portfolio.assets();
    if a.is_empty() {
        return 0.0;
    }
    let sum =
        a.iter().fold(0.0, |s, x| s + if x.volatility > 0.0 { x.volatility } else { DEFAULT_VOLATILITY });
    sum / a.len() as f64
}

/// Runs the simulation for `portfolio` under `config`.
///
/// # Errors
/// Validation errors, or [`Error::NumericalValidation`] when the CPU
/// cross-check fails.
pub fn simulate(portfolio: &Portfolio, config: &RiskConfig) -> Result<Simulation> {
    let wall = Instant::now();
    portfolio.validate()?;
    config.validate()?;
    let backend = resolve_backend(select_backend(config.backend, config.simulations));
    let batches = plan_batches(config.simulations, if config.adaptive { ADAPTIVE_BATCH } else { 0 });
    let initial_value = portfolio.total_value();
    let volatility = aggregate_volatility(portfolio);
    let task_for = |index: u64, paths: u64| SimulationTask {
        num_paths: paths,
        // Every batch gets its own sub-seed. (The C++ engine reused batch 0's
        // seed for every batch, so multi-batch runs repeated the same paths.)
        seed: crate::rng::seed_for_batch(config.seed, index),
        drift: 0.0,
        volatility,
        initial_value,
        horizon_years: config.horizon_years,
    };
    let first_task = task_for(0, batches[0]);
    let mut losses = Vec::with_capacity(usize::try_from(config.simulations).unwrap_or(0));
    let (mut compute_ms, mut run, mut done, mut convergence) = (0.0, 0u64, 0usize, None);
    for (i, &paths) in batches.iter().enumerate() {
        let r = execute(backend, &task_for(i as u64, paths), config.threads)?;
        losses.extend_from_slice(&r.terminal_losses);
        compute_ms += r.elapsed_ms;
        run += paths;
        done += 1;
        if config.adaptive {
            let c = check_convergence(&losses, config.confidence, config.target_rel_std_err);
            convergence = Some(c);
            if c.converged {
                break;
            }
        }
    }
    if config.verify_against_cpu && backend != Backend::Cpu {
        // Re-run the same task, same seed, on the trusted reference and
        // compare path by path. (The C++ check rebuilt a different task with
        // zero volatility and compared means, so it could not detect errors.)
        let n = first_task.num_paths.min(VERIFY_PATHS);
        let reference = run_reference(&SimulationTask { num_paths: n, ..first_task })?;
        let mut worst = 0.0f64;
        for (a, b) in losses.iter().zip(&reference.terminal_losses) {
            let scale = b.abs().max(1e-12);
            worst = worst.max((a - b).abs() / scale);
        }
        if !(worst <= config.verify_tolerance) {
            return Err(Error::NumericalValidation(format!(
                "Backend {} diverged from the CPU reference: relative difference {worst} exceeds verify_tolerance {}.",
                backend.as_str(),
                config.verify_tolerance
            )));
        }
    }
    Ok(Simulation {
        losses,
        requested_backend: config.backend,
        backend_used: backend,
        simulations_run: run,
        batches: done,
        convergence,
        first_task,
        total_ms: wall.elapsed().as_secs_f64() * 1e3,
        compute_ms,
    })
}

/// Git commit recorded at build time (set `SPECTRA_GIT_COMMIT` when building).
pub const GIT_COMMIT: &str = match option_env!("SPECTRA_GIT_COMMIT") {
    Some(c) => c,
    None => "unknown",
};

/// Every number the engine hands back, with what is needed to reproduce it.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskReport {
    /// Metric name.
    pub metric_name: String,
    /// Metric value.
    pub value: f64,
    /// Confidence.
    pub confidence: f64,
    /// Paths simulated.
    pub simulations_run: u64,
    /// Seed.
    pub seed: u64,
    /// Requested backend.
    pub requested_backend: Backend,
    /// Backend that ran.
    pub backend_used: Backend,
    /// Horizon in years.
    pub horizon_years: f64,
    /// Engine version.
    pub library_version: String,
    /// Build commit.
    pub git_commit: String,
    /// Wall time of the simulation.
    pub total_ms: f64,
    /// Kernel time.
    pub compute_ms: f64,
    /// Portfolio value.
    pub portfolio_value: f64,
    /// Model volatility.
    pub model_volatility: f64,
    /// Convergence of an adaptive run.
    pub convergence: Option<Convergence>,
    /// Dependence model used.
    pub dependence_model: DependenceModel,
    /// Per-position risk contributions (Euler component CVaR and component VaR).
    pub position_contributions: Vec<PositionRiskContribution>,
    /// Extreme Value Theory (EVT) VaR estimate, if computable.
    pub evt_var: Option<f64>,
    /// Extreme Value Theory (EVT) CVaR estimate, if computable.
    pub evt_cvar: Option<f64>,
}

fn num(v: f64) -> Json {
    Json::number(v).unwrap_or(Json::Null)
}

fn uint(v: u64) -> Json {
    i64::try_from(v).map_or_else(|_| Json::string(v.to_string()), Json::integer)
}

impl RiskReport {
    fn new(
        metric: &str,
        value: f64,
        losses: &[f64],
        sim_run: u64,
        req_backend: Backend,
        used_backend: Backend,
        total_ms: f64,
        compute_ms: f64,
        portfolio_val: f64,
        model_vol: f64,
        convergence: Option<Convergence>,
        dependence_model: DependenceModel,
        position_contributions: Vec<PositionRiskContribution>,
        config: &RiskConfig,
    ) -> Self {
        let (evt_var, evt_cvar) = if losses.len() >= 50 {
            EvtTailModel::fit(losses, 0.95)
                .map(|e| (Some(e.var(config.confidence)), Some(e.cvar(config.confidence))))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

        Self {
            metric_name: metric.into(),
            value,
            confidence: config.confidence,
            simulations_run: sim_run,
            seed: config.seed,
            requested_backend: req_backend,
            backend_used: used_backend,
            horizon_years: config.horizon_years,
            library_version: ENGINE_VERSION.into(),
            git_commit: GIT_COMMIT.into(),
            total_ms,
            compute_ms,
            portfolio_value: portfolio_val,
            model_volatility: model_vol,
            convergence,
            dependence_model,
            position_contributions,
            evt_var,
            evt_cvar,
        }
    }

    /// JSON with the C++ report's fields first (same names), then the
    /// additions. Numbers are printed exactly (%.17g), not to 6 digits.
    pub fn to_json_value(&self) -> Json {
        let model_str = match self.dependence_model {
            DependenceModel::Aggregate => "MonteCarlo/GBM aggregate position",
            DependenceModel::GaussianCopula => "MonteCarlo/Gaussian copula correlated paths",
            DependenceModel::TCopula { .. } => "MonteCarlo/t-copula correlated paths",
        };
        let mut j = Json::object()
            .with("metric", Json::string(self.metric_name.clone()))
            .with("value", num(self.value))
            .with("confidence", num(self.confidence))
            .with("simulations_run", uint(self.simulations_run))
            .with("seed", uint(self.seed))
            .with("backend", Json::string(self.backend_used.as_str()))
            .with("library_version", Json::string(self.library_version.clone()))
            .with("git_commit", Json::string(self.git_commit.clone()))
            .with("total_ms", num(self.total_ms))
            .with("compute_ms", num(self.compute_ms))
            .with("requested_backend", Json::string(self.requested_backend.as_str()))
            .with("horizon_years", num(self.horizon_years))
            .with("portfolio_value", num(self.portfolio_value))
            .with("model", Json::string(model_str))
            .with("model_volatility", num(self.model_volatility));
        if let Some(c) = self.convergence {
            j = j.with(
                "convergence",
                Json::object()
                    .with("converged", Json::boolean(c.converged))
                    .with("relative_std_error", num(c.relative_std_error)),
            );
        }
        if self.dependence_model != DependenceModel::Aggregate {
            j = j.with("dependence_model", Json::string(self.dependence_model.as_str()));
        }
        if !self.position_contributions.is_empty() {
            let contribs: Vec<Json> = self
                .position_contributions
                .iter()
                .map(|c| {
                    Json::object()
                        .with("symbol", Json::string(c.symbol.clone()))
                        .with("position_value", num(c.position_value))
                        .with("weight", num(c.weight))
                        .with("component_var", num(c.component_var))
                        .with("component_cvar", num(c.component_cvar))
                        .with("cvar_contribution_pct", num(c.cvar_contribution_pct))
                })
                .collect();
            j = j.with("position_contributions", Json::Array(contribs));
        }
        if let Some(v) = self.evt_var {
            j = j.with("evt_var", num(v));
        }
        if let Some(v) = self.evt_cvar {
            j = j.with("evt_cvar", num(v));
        }
        j
    }

    /// [`RiskReport::to_json_value`] as text.
    pub fn to_json(&self) -> String {
        self.to_json_value().dump()
    }
}

/// The plugin interface version this engine implements. Bump it when
/// [`RiskPlugin`] changes incompatibly; [`RiskEngine::register_plugin`]
/// refuses plugins written against another version.
pub const PLUGIN_API_VERSION: u32 = 1;

/// Common metadata contract for compiled Spectra extensions.
///
/// This trait is intentionally independent of a particular extension point.
/// Risk plugins receive a blanket implementation, while future data, storage,
/// or execution plugins can implement it directly.
pub trait Plugin: Send + Sync {
    /// Stable identifier used in configuration and logs.
    fn plugin_id(&self) -> &str;
    /// Extension version.
    fn plugin_version(&self) -> &str;
    /// Plugin API version supported by this implementation.
    fn plugin_api_version(&self) -> u32;
}

/// A user-supplied risk measure over simulated losses.
pub trait RiskPlugin: Send + Sync {
    /// Unique name.
    fn name(&self) -> &str;
    /// The plugin's own version, for listings and audit. Default `0.0.0`.
    #[allow(clippy::unnecessary_literal_bound)] // implementors may return a borrowed field
    fn version(&self) -> &str {
        "0.0.0"
    }
    /// The plugin interface this plugin was written against. Keep the default
    /// ([`PLUGIN_API_VERSION`] at build time) unless the plugin targets another
    /// engine release.
    fn api_version(&self) -> u32 {
        PLUGIN_API_VERSION
    }
    /// The measure.
    ///
    /// # Errors
    /// Whatever the plugin reports.
    fn compute(&self, losses: &[f64], confidence: f64) -> Result<f64>;
}

impl<T: RiskPlugin + ?Sized> Plugin for T {
    fn plugin_id(&self) -> &str {
        self.name()
    }

    fn plugin_version(&self) -> &str {
        self.version()
    }

    fn plugin_api_version(&self) -> u32 {
        self.api_version()
    }
}

/// A registered plugin, as [`RiskEngine::plugins`] lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Unique name (what `calculate_named` takes).
    pub name: String,
    /// The plugin's own version.
    pub version: String,
    /// The plugin interface it was written against.
    pub api_version: u32,
}

/// The engine: built-in metrics plus registered plugins.
#[derive(Default)]
pub struct RiskEngine {
    plugins: Vec<Box<dyn RiskPlugin>>,
}

impl std::fmt::Debug for RiskEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RiskEngine").field("plugins", &self.plugin_names()).finish()
    }
}

impl RiskEngine {
    /// An engine with no plugins.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a plugin.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if the name is taken (by a plugin or a
    /// built-in metric), and [`Error::Unsupported`] if the plugin was written
    /// against another [`PLUGIN_API_VERSION`].
    pub fn register_plugin(&mut self, plugin: Box<dyn RiskPlugin>) -> Result<()> {
        let name = plugin.name().to_string();
        if plugin.api_version() != PLUGIN_API_VERSION {
            return Err(Error::Unsupported(format!(
                "plugin '{name}' targets plugin API v{}, this engine implements v{PLUGIN_API_VERSION}",
                plugin.api_version()
            )));
        }
        if Metric::parse(&name).is_ok() || self.plugins.iter().any(|p| p.name() == name) {
            return Err(Error::InvalidArgument(format!("a metric named '{name}' is already registered")));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    /// Registered plugin names.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name().to_string()).collect()
    }

    /// Registered plugins with their versions.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| PluginInfo {
                name: p.name().to_string(),
                version: p.version().to_string(),
                api_version: p.api_version(),
            })
            .collect()
    }

    /// Simulates and computes one metric (built-in or plugin, by name).
    ///
    /// # Errors
    /// Unknown names, validation errors, unsupported metrics.
    pub fn calculate_named(
        &self,
        metric: &str,
        portfolio: &Portfolio,
        config: &RiskConfig,
    ) -> Result<RiskReport> {
        Ok(self.calculate_many(&[metric], portfolio, config)?.remove(0))
    }

    /// Simulates and computes a built-in metric.
    ///
    /// # Errors
    /// Validation errors or unsupported metrics.
    pub fn calculate(
        &self,
        metric: Metric,
        portfolio: &Portfolio,
        config: &RiskConfig,
    ) -> Result<RiskReport> {
        self.calculate_named(metric.name(), portfolio, config)
    }

    /// Simulates once and computes several metrics from the same losses.
    ///
    /// # Errors
    /// The first failing metric's error.
    pub fn calculate_many(
        &self,
        metrics: &[&str],
        portfolio: &Portfolio,
        config: &RiskConfig,
    ) -> Result<Vec<RiskReport>> {
        // Resolve every name before spending any compute.
        enum Which<'a> {
            Builtin(Metric),
            Plugin(&'a dyn RiskPlugin),
        }
        let resolved = metrics
            .iter()
            .map(|name| match self.plugins.iter().find(|p| p.name() == *name) {
                Some(p) => Ok(Which::Plugin(p.as_ref())),
                None => Metric::parse(name).map(Which::Builtin),
            })
            .collect::<Result<Vec<_>>>()?;
        for w in &resolved {
            if let Which::Builtin(m) = w {
                if !m.is_supported() {
                    return compute(*m, &[], 0.5, portfolio).map(|_| Vec::new());
                }
            }
        }
        let (
            losses,
            sim_run,
            req_backend,
            used_backend,
            total_ms,
            compute_ms,
            port_val,
            model_vol,
            convergence,
            contributions,
        ) = if config.dependence == DependenceModel::Aggregate {
            let sim = simulate(portfolio, config)?;
            (
                sim.losses,
                sim.simulations_run,
                sim.requested_backend,
                sim.backend_used,
                sim.total_ms,
                sim.compute_ms,
                sim.first_task.initial_value,
                sim.first_task.volatility,
                sim.convergence,
                Vec::new(),
            )
        } else {
            let corr = simulate_correlated(
                portfolio,
                None,
                config.dependence,
                config.simulations,
                config.seed,
                config.horizon_years,
                config.confidence,
            )?;
            (
                corr.portfolio_losses,
                corr.simulations_run,
                config.backend,
                Backend::Parallel,
                corr.elapsed_ms,
                corr.elapsed_ms,
                portfolio.total_value(),
                aggregate_volatility(portfolio),
                None,
                corr.contributions,
            )
        };

        resolved
            .iter()
            .map(|w| {
                let (name, value) = match w {
                    Which::Builtin(m) => {
                        (m.name().to_string(), compute(*m, &losses, config.confidence, portfolio)?)
                    }
                    Which::Plugin(p) => (p.name().to_string(), p.compute(&losses, config.confidence)?),
                };
                Ok(RiskReport::new(
                    &name,
                    value,
                    &losses,
                    sim_run,
                    req_backend,
                    used_backend,
                    total_ms,
                    compute_ms,
                    port_val,
                    model_vol,
                    convergence,
                    config.dependence,
                    contributions.clone(),
                    config,
                ))
            })
            .collect()
    }
}

/// A named price shock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scenario {
    /// Name.
    pub name: &'static str,
    /// Price multiplier (0.7 = -30%).
    pub shock_factor: f64,
    /// Description.
    pub description: &'static str,
}

/// The scenario library (including historical stress scenarios for Task 11).
pub const SCENARIOS: [Scenario; 6] = [
    Scenario { name: "market-crash", shock_factor: 0.70, description: "every price falls 30%" },
    Scenario { name: "mild-correction", shock_factor: 0.90, description: "every price falls 10%" },
    Scenario {
        name: "covid-crash-2020",
        shock_factor: 0.66,
        description: "March 2020 pandemic market shock (-34%)",
    },
    Scenario {
        name: "gfc-2008",
        shock_factor: 0.52,
        description: "2008 Global Financial Crisis equity shock (-48%)",
    },
    Scenario {
        name: "tech-bubble-2000",
        shock_factor: 0.51,
        description: "2000 Dot-com crash tech-heavy shock (-49%)",
    },
    Scenario {
        name: "black-monday-1987",
        shock_factor: 0.77,
        description: "October 1987 single-day market crash (-23%)",
    },
];

/// Looks up a scenario.
///
/// # Errors
/// [`Error::InvalidArgument`] for an unknown name.
pub fn scenario(name: &str) -> Result<Scenario> {
    SCENARIOS.iter().copied().find(|s| s.name == name).ok_or_else(|| {
        Error::InvalidArgument(format!(
            "Unknown scenario: {name} (known: market-crash, mild-correction, covid-crash-2020, gfc-2008, tech-bubble-2000, black-monday-1987)"
        ))
    })
}

/// Result of a stress test.
#[derive(Debug, Clone, PartialEq)]
pub struct StressResult {
    /// Scenario applied.
    pub scenario: Scenario,
    /// Value before the shock.
    pub value_before: f64,
    /// Value after the shock.
    pub value_after: f64,
    /// Immediate loss from the shock (before - after).
    pub scenario_loss: f64,
    /// VaR of the shocked portfolio.
    pub report: RiskReport,
}

impl StressResult {
    /// The report's JSON (what the C++ CLI printed) plus the scenario fields.
    pub fn to_json_value(&self) -> Json {
        self.report
            .to_json_value()
            .with("scenario", Json::string(self.scenario.name))
            .with("shock_factor", num(self.scenario.shock_factor))
            .with("value_before", num(self.value_before))
            .with("value_after", num(self.value_after))
            .with("scenario_loss", num(self.scenario_loss))
    }
}

/// Applies a price shock and computes the VaR of the shocked portfolio.
///
/// # Errors
/// Unknown scenario or simulation errors.
pub fn stress_test(
    engine: &RiskEngine,
    portfolio: &Portfolio,
    scenario_name: &str,
    config: &RiskConfig,
) -> Result<StressResult> {
    let s = scenario(scenario_name)?;
    let shocked = portfolio.shocked(s.shock_factor);
    let report = engine.calculate(Metric::VaR, &shocked, config)?;
    let (before, after) = (portfolio.total_value(), shocked.total_value());
    Ok(StressResult {
        scenario: s,
        value_before: before,
        value_after: after,
        scenario_loss: before - after,
        report,
    })
}

/// Benchmark limits (as in the C++ CLI).
pub const BENCHMARK_LIMITS: (u64, u64, usize, usize) = (1_000, 100_000_000, 30, 5);

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p * sorted.len() as f64).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

/// Times VaR runs on every backend; JSON in the C++ CLI's schema.
///
/// # Errors
/// [`Error::InvalidArgument`] outside 1,000-100,000,000 simulations, 1-30
/// repetitions or 0-5 warmups; simulation errors.
pub fn benchmark(
    portfolio: &Portfolio,
    simulations: u64,
    repetitions: usize,
    warmups: usize,
) -> Result<Json> {
    benchmark_with_progress(portfolio, simulations, repetitions, warmups, |_| {})
}

/// Benchmark like [`benchmark`], reporting completed warmup and measured runs.
/// The callback is invoked once after each backend repetition and can be used
/// by a job worker to persist coarse-grained progress.
///
/// # Errors
/// Returns the same validation or simulation error as [`benchmark`].
pub fn benchmark_with_progress(
    portfolio: &Portfolio,
    simulations: u64,
    repetitions: usize,
    warmups: usize,
    mut on_progress: impl FnMut(f32),
) -> Result<Json> {
    let (lo, hi, max_rep, max_warm) = BENCHMARK_LIMITS;
    if !(lo..=hi).contains(&simulations) || !(1..=max_rep).contains(&repetitions) || warmups > max_warm {
        return Err(Error::InvalidArgument(
            "Use 1,000–100,000,000 simulations, 1–30 measured repetitions, and 0–5 warmups.".into(),
        ));
    }
    let engine = RiskEngine::new();
    let mut results = Vec::new();
    let total_runs = 3 * (warmups + repetitions);
    let mut completed_runs = 0;
    for requested in [Backend::Cpu, Backend::Parallel, Backend::Cuda] {
        let config = RiskConfig { simulations, backend: requested, seed: 42, ..RiskConfig::default() };
        let mut actual = requested;
        for _ in 0..warmups {
            actual = engine.calculate(Metric::VaR, portfolio, &config)?.backend_used;
            completed_runs += 1;
            on_progress(completed_runs as f32 / total_runs as f32);
        }
        let (mut total, mut compute_t) = (Vec::new(), Vec::new());
        for _ in 0..repetitions {
            let r = engine.calculate(Metric::VaR, portfolio, &config)?;
            actual = r.backend_used;
            total.push(r.total_ms);
            compute_t.push(r.compute_ms);
            completed_runs += 1;
            on_progress(completed_runs as f32 / total_runs as f32);
        }
        let mut st = total.clone();
        st.sort_by(f64::total_cmp);
        let mut sc = compute_t;
        sc.sort_by(f64::total_cmp);
        let median = percentile(&st, 0.5);
        let mean = st.iter().sum::<f64>() / st.len() as f64;
        results.push(
            Json::object()
                .with("requested_backend", Json::string(requested.as_str()))
                .with("backend", Json::string(actual.as_str()))
                .with("status", Json::string(if actual == requested { "available" } else { "fallback" }))
                .with("simulations", uint(simulations))
                .with("repetitions", Json::integer(total.len() as i64))
                .with("warmups", Json::integer(warmups as i64))
                .with(
                    "latency_ms",
                    Json::object()
                        .with("min", num(st[0]))
                        .with("mean", num(mean))
                        .with("p50", num(median))
                        .with("p95", num(percentile(&st, 0.95)))
                        .with("p99", num(percentile(&st, 0.99)))
                        .with("max", num(st[st.len() - 1])),
                )
                .with(
                    "compute_ms",
                    Json::object()
                        .with("p50", num(percentile(&sc, 0.5)))
                        .with("p95", num(percentile(&sc, 0.95))),
                )
                .with(
                    "paths_per_second",
                    num(if median > 0.0 { simulations as f64 * 1000.0 / median } else { 0.0 }),
                )
                .with("samples_ms", Json::Array(total.iter().map(|v| num(*v)).collect())),
        );
    }
    Ok(Json::object()
        .with("simulations", uint(simulations))
        .with("requested_repetitions", Json::integer(repetitions as i64))
        .with("warmups", Json::integer(warmups as i64))
        .with("benchmarks", Json::Array(results)))
}

/// The interpolated quantile of unsorted values (exposed for callers that
/// already hold losses).
///
/// # Errors
/// [`Error::InvalidArgument`] for an empty slice.
pub fn quantile(values: &[f64], q: f64) -> Result<f64> {
    if values.is_empty() {
        return Err(Error::InvalidArgument("quantile of an empty slice".into()));
    }
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    Ok(quantile_sorted(&s, q))
}
