//! Monte Carlo simulation: the GBM terminal-loss kernel and its backends.
//!
//! Model (unchanged from the C++ engine): the portfolio is one aggregate
//! position worth `V` (the sum of position values) with volatility `sigma`
//! equal to the simple average of the assets' volatilities (20% for assets
//! without one) and zero drift. Each path draws `z ~ N(0, 1)` and
//!
//! `loss = V - V * exp((mu - sigma^2 / 2) * T + sigma * sqrt(T) * z)`
//!
//! over a horizon of `T` years (default 1, as the C++ engine implicitly
//! used). This ignores correlations and per-asset paths: it treats every
//! asset as perfectly correlated, which overstates diversification-free risk
//! and cannot attribute risk to positions. It is a defensible first model,
//! not a production market-risk model.
//!
//! Backends:
//! * [`Backend::Cpu`]: the single-threaded reference kernel; bit-identical
//!   to the C++ CPU backend for the same seed.
//! * [`Backend::Parallel`]: replaces the C++ OpenMP backend. Paths are split
//!   into fixed chunks of [`PARALLEL_CHUNK`] paths; chunk 0 uses the task
//!   seed (so it reproduces the reference exactly) and chunk `k` uses
//!   `seed_for_batch(seed, k)`. Results depend only on the seed and the
//!   path count, never on the thread count (the OpenMP backend's results
//!   changed with the number of threads).
//! * [`Backend::Cuda`] is recognized but not built in this port; requests
//!   for it fall back to `Parallel` and are reported as a fallback.

use std::time::Instant;

use spectra_core::{Error, Result};

use crate::rng::{seed_for_batch, Mt19937_64, PolarNormal};

/// Paths per deterministic chunk of the parallel backend.
pub const PARALLEL_CHUNK: usize = 131_072;

/// Execution backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Backend {
    /// Let the engine choose from the workload size.
    #[default]
    Auto,
    /// Single-threaded reference.
    Cpu,
    /// Multi-threaded, deterministic (replaces OpenMP).
    Parallel,
    /// GPU; not available in this build (falls back to `Parallel`).
    Cuda,
}

impl Backend {
    /// Display name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Cpu => "CPU",
            Self::Parallel => "Parallel",
            Self::Cuda => "CUDA",
        }
    }

    /// Parses "auto", "cpu", "parallel" (alias "openmp") or "cuda".
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for anything else.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "parallel" | "openmp" => Ok(Self::Parallel),
            "cuda" | "gpu" => Ok(Self::Cuda),
            _ => {
                Err(Error::InvalidArgument(format!("Unknown backend: {s} (use auto, cpu, parallel or cuda)")))
            }
        }
    }

    /// Whether this build can run the backend.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Cpu | Self::Parallel)
    }
}

/// The static heuristic of the C++ selector: CPU below 50k paths, the
/// parallel backend below 1M, CUDA above. An explicit choice always wins.
pub fn select_backend(requested: Backend, simulations: u64) -> Backend {
    if requested != Backend::Auto {
        return requested;
    }
    if simulations < 50_000 {
        Backend::Cpu
    } else if simulations < 1_000_000 {
        Backend::Parallel
    } else {
        Backend::Cuda
    }
}

/// Resolves a desired backend to one this build can run: CUDA -> Parallel ->
/// CPU. CPU is always available.
pub fn resolve_backend(desired: Backend) -> Backend {
    match desired {
        Backend::Cpu => Backend::Cpu,
        Backend::Auto | Backend::Parallel | Backend::Cuda => Backend::Parallel,
    }
}

/// Numeric parameters for one batch of paths. Knows nothing about portfolios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulationTask {
    /// Paths to simulate.
    pub num_paths: u64,
    /// Seed of this batch.
    pub seed: u64,
    /// Annual drift.
    pub drift: f64,
    /// Annual volatility.
    pub volatility: f64,
    /// Position value at the start.
    pub initial_value: f64,
    /// Horizon in years.
    pub horizon_years: f64,
}

impl SimulationTask {
    /// Validates simulation task inputs for NaN or non-finite values.
    pub fn validate(&self) -> Result<()> {
        if self.drift.is_nan() {
            return Err(Error::InvalidArgument("Monte Carlo drift is NaN".into()));
        }
        if self.volatility.is_nan() {
            return Err(Error::InvalidArgument("Monte Carlo volatility is NaN".into()));
        }
        if self.initial_value.is_nan() {
            return Err(Error::InvalidArgument("Monte Carlo initial_value is NaN".into()));
        }
        if self.horizon_years.is_nan() {
            return Err(Error::InvalidArgument("Monte Carlo horizon_years is NaN".into()));
        }
        if self.volatility < 0.0 || !self.volatility.is_finite() {
            return Err(Error::InvalidArgument(
                "Monte Carlo volatility must be non-negative and finite".into(),
            ));
        }
        if self.initial_value <= 0.0 || !self.initial_value.is_finite() {
            return Err(Error::InvalidArgument(
                "Monte Carlo initial_value must be positive and finite".into(),
            ));
        }
        if self.horizon_years <= 0.0 || !self.horizon_years.is_finite() {
            return Err(Error::InvalidArgument(
                "Monte Carlo horizon_years must be positive and finite".into(),
            ));
        }
        Ok(())
    }
}

/// Losses of one batch.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SimulationResult {
    /// One loss per path (positive = the portfolio lost value).
    pub terminal_losses: Vec<f64>,
    /// Number of paths that produced NaN and were dropped.
    pub dropped_nan_paths: u64,
    /// Kernel time.
    pub elapsed_ms: f64,
}

fn coefficients(task: &SimulationTask) -> (f64, f64) {
    let t = task.horizon_years;
    let drift_term = (task.drift - 0.5 * task.volatility * task.volatility) * t;
    let diffusion = task.volatility * t.sqrt();
    (drift_term, diffusion)
}

fn fill(out: &mut [f64], seed: u64, task: &SimulationTask, drift_term: f64, diffusion: f64) -> u64 {
    let mut rng = Mt19937_64::new(seed);
    let mut normal = PolarNormal::new();
    let mut dropped = 0u64;
    for slot in out {
        let z = normal.sample(&mut rng);
        let terminal = task.initial_value * (drift_term + diffusion * z).exp();
        let loss = task.initial_value - terminal;
        if loss.is_nan() {
            dropped += 1;
            *slot = f64::NAN;
        } else {
            *slot = loss;
        }
    }
    dropped
}

fn path_count(task: &SimulationTask) -> Result<usize> {
    usize::try_from(task.num_paths).map_err(|_| Error::InvalidArgument("too many simulation paths".into()))
}

/// The single-threaded reference kernel.
///
/// # Errors
/// If the path count does not fit in memory addressing.
pub fn run_reference(task: &SimulationTask) -> Result<SimulationResult> {
    task.validate()?;
    let start = Instant::now();
    let mut losses = vec![0.0; path_count(task)?];
    let (d, s) = coefficients(task);
    let dropped = fill(&mut losses, task.seed, task, d, s);
    if dropped > 0 {
        losses.retain(|l| !l.is_nan());
        if losses.is_empty() {
            return Err(Error::NumericalValidation("All Monte Carlo paths produced NaN".into()));
        }
    }
    Ok(SimulationResult {
        terminal_losses: losses,
        dropped_nan_paths: dropped,
        elapsed_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

/// The seed of chunk `k` of the parallel backend.
pub const fn chunk_seed(task_seed: u64, chunk: u64) -> u64 {
    if chunk == 0 {
        task_seed
    } else {
        seed_for_batch(task_seed, chunk)
    }
}

/// The deterministic multi-threaded kernel. `threads` of 0 means "all cores".
///
/// # Errors
/// If the path count does not fit in memory addressing.
pub fn run_parallel(task: &SimulationTask, threads: usize) -> Result<SimulationResult> {
    task.validate()?;
    let start = Instant::now();
    let mut losses = vec![0.0; path_count(task)?];
    let (d, s) = coefficients(task);
    let threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    } else {
        threads
    };
    let mut chunks: Vec<(u64, &mut [f64])> =
        losses.chunks_mut(PARALLEL_CHUNK).enumerate().map(|(i, c)| (i as u64, c)).collect();
    let workers = threads.min(chunks.len()).max(1);
    let mut dropped_total = 0u64;
    if workers == 1 {
        for (k, c) in &mut chunks {
            dropped_total += fill(c, chunk_seed(task.seed, *k), task, d, s);
        }
    } else {
        // Deal chunks round-robin; each chunk's content depends only on its index.
        let mut lanes: Vec<Vec<(u64, &mut [f64])>> = (0..workers).map(|_| Vec::new()).collect();
        for (i, c) in chunks.into_iter().enumerate() {
            lanes[i % workers].push(c);
        }
        let dropped_counts: Vec<u64> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(lanes.len());
            for lane in lanes {
                handles.push(scope.spawn(move || {
                    let mut lane_dropped = 0u64;
                    for (k, c) in lane {
                        lane_dropped += fill(c, chunk_seed(task.seed, k), task, d, s);
                    }
                    lane_dropped
                }));
            }
            handles.into_iter().map(|h| h.join().unwrap_or(0)).collect()
        });
        dropped_total = dropped_counts.into_iter().sum();
    }
    if dropped_total > 0 {
        losses.retain(|l| !l.is_nan());
        if losses.is_empty() {
            return Err(Error::NumericalValidation("All Monte Carlo paths produced NaN".into()));
        }
    }
    Ok(SimulationResult {
        terminal_losses: losses,
        dropped_nan_paths: dropped_total,
        elapsed_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

/// Runs a task on a concrete backend (`Cpu` or `Parallel`).
///
/// # Errors
/// [`Error::Unsupported`] for `Auto` or `Cuda` (resolve first).
pub fn execute(backend: Backend, task: &SimulationTask, threads: usize) -> Result<SimulationResult> {
    match backend {
        Backend::Cpu => run_reference(task),
        Backend::Parallel => run_parallel(task, threads),
        other => {
            Err(Error::Unsupported(format!("backend {} is not available in this build", other.as_str())))
        }
    }
}

/// Splits `total` into batches of at most `max_batch` (0 = one batch).
pub fn plan_batches(total: u64, max_batch: u64) -> Vec<u64> {
    if max_batch == 0 || max_batch >= total {
        return vec![total];
    }
    let mut out = Vec::with_capacity(total.div_ceil(max_batch) as usize);
    let mut remaining = total;
    while remaining > 0 {
        let b = remaining.min(max_batch);
        out.push(b);
        remaining -= b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_task_rejects_nan_inputs() {
        let base_task = SimulationTask {
            num_paths: 100,
            seed: 42,
            drift: 0.0,
            volatility: 0.20,
            initial_value: 100_000.0,
            horizon_years: 1.0,
        };

        let mut nan_vol = base_task;
        nan_vol.volatility = f64::NAN;
        assert!(nan_vol.validate().is_err());
        assert!(run_reference(&nan_vol).is_err());
        assert!(run_parallel(&nan_vol, 2).is_err());

        let mut nan_val = base_task;
        nan_val.initial_value = f64::NAN;
        assert!(nan_val.validate().is_err());
        assert!(run_reference(&nan_val).is_err());

        let mut nan_drift = base_task;
        nan_drift.drift = f64::NAN;
        assert!(nan_drift.validate().is_err());

        let mut nan_horizon = base_task;
        nan_horizon.horizon_years = f64::NAN;
        assert!(nan_horizon.validate().is_err());
    }

    #[test]
    fn test_simulation_runs_finite_paths() {
        let task = SimulationTask {
            num_paths: 500,
            seed: 1234,
            drift: 0.05,
            volatility: 0.15,
            initial_value: 50_000.0,
            horizon_years: 0.5,
        };

        let res_ref = run_reference(&task).unwrap();
        assert_eq!(res_ref.terminal_losses.len(), 500);
        assert_eq!(res_ref.dropped_nan_paths, 0);

        let res_par = run_parallel(&task, 2).unwrap();
        assert_eq!(res_par.terminal_losses.len(), 500);
        assert_eq!(res_par.dropped_nan_paths, 0);
    }
}
