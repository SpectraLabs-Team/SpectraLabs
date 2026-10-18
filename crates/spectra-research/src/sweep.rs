//! Parameter sweeps and trial tracking.
//!
//! A sweep runs one backtest per parameter set (in parallel if asked) and
//! records every trial. The [`TrialLedger`] counts EVERY trial ever run, so
//! the Deflated Sharpe Ratio uses the true number of attempts rather than a
//! number the user remembers.
//!
//! Determinism: results are stored by parameter-set index, so output (and
//! every run fingerprint) is identical for any thread count.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use spectra_core::cfmt::fmt_g17;
use spectra_core::{Bar, Error, Result, Rng};
use spectra_engine::{
    returns_from_curve, run_backtest, BacktestConfig, BacktestResult, Metrics, RunRecord, Strategy,
};

use crate::stats::sharpe_per_period;

/// An ordered set of named numeric parameters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParamSet {
    /// (name, value) pairs in order.
    pub values: Vec<(String, f64)>,
}

impl ParamSet {
    /// Builds a set from pairs.
    pub fn from_pairs<S: Into<String>>(pairs: impl IntoIterator<Item = (S, f64)>) -> Self {
        Self { values: pairs.into_iter().map(|(k, v)| (k.into(), v)).collect() }
    }

    /// The value of `name`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for unknown names.
    pub fn get(&self, name: &str) -> Result<f64> {
        self.values
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| *v)
            .ok_or_else(|| Error::InvalidArgument(format!("unknown parameter: {name}")))
    }

    /// The value of `name` rounded to the nearest integer (half away from zero).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for unknown names.
    pub fn get_int(&self, name: &str) -> Result<i64> {
        Ok(self.get(name)?.round() as i64)
    }

    /// Stable text form, e.g. "fast=5;slow=30", used in hashes and reports.
    pub fn canonical(&self) -> String {
        self.values.iter().map(|(k, v)| format!("{k}={}", fmt_g17(*v))).collect::<Vec<_>>().join(";")
    }
}

/// Cartesian grid of parameter values. The last axis added varies fastest.
#[derive(Debug, Clone, Default)]
pub struct ParamGrid {
    axes: Vec<(String, Vec<f64>)>,
}

impl ParamGrid {
    /// An empty grid (one point with no parameters).
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an axis.
    ///
    /// # Errors
    /// Empty value lists or duplicate names.
    pub fn add(mut self, name: impl Into<String>, values: impl Into<Vec<f64>>) -> Result<Self> {
        let name = name.into();
        let values = values.into();
        if values.is_empty() {
            return Err(Error::InvalidArgument(format!("ParamGrid: axis '{name}' has no values")));
        }
        if self.axes.iter().any(|(n, _)| *n == name) {
            return Err(Error::InvalidArgument(format!("ParamGrid: duplicate axis '{name}'")));
        }
        self.axes.push((name, values));
        Ok(self)
    }

    /// Number of grid points (1 for a grid with no axes).
    pub fn size(&self) -> usize {
        self.axes.iter().map(|(_, v)| v.len()).product()
    }

    /// The point at `index`, or `None` if out of range.
    pub fn at(&self, mut index: usize) -> Option<ParamSet> {
        if index >= self.size() {
            return None;
        }
        let mut values = vec![(String::new(), 0.0); self.axes.len()];
        for (i, (name, axis)) in self.axes.iter().enumerate().rev() {
            values[i] = (name.clone(), axis[index % axis.len()]);
            index /= axis.len();
        }
        Some(ParamSet { values })
    }

    /// Every point in enumeration order.
    pub fn all(&self) -> Vec<ParamSet> {
        (0..self.size()).filter_map(|i| self.at(i)).collect()
    }

    /// Random search: `count` distinct points drawn without replacement,
    /// returned in grid order. The whole grid if `count >= size()`.
    pub fn sample(&self, count: usize, seed: u64) -> Vec<ParamSet> {
        let n = self.size();
        if count >= n {
            return self.all();
        }
        // Partial Fisher-Yates over a virtual array 0..n-1 with a sparse swap map.
        let mut rng = Rng::new(seed);
        let mut swapped: HashMap<usize, usize> = HashMap::new();
        let mut picked = Vec::with_capacity(count);
        for i in 0..count {
            let j = i + (rng.next_u64() % (n - i) as u64) as usize;
            let vi = *swapped.get(&i).unwrap_or(&i);
            let vj = *swapped.get(&j).unwrap_or(&j);
            swapped.insert(j, vi);
            picked.push(vj);
        }
        picked.sort_unstable();
        picked.into_iter().filter_map(|i| self.at(i)).collect()
    }
}

/// Builds a strategy for a parameter set. Must be callable from several
/// threads when a sweep runs in parallel; returning an error means the
/// strategy rejects those parameters.
pub type StrategyFactory<'a> = dyn Fn(&ParamSet) -> Result<Box<dyn Strategy>> + Sync + 'a;

/// Scores a backtest; higher is better.
pub type Objective<'a> = dyn Fn(&BacktestResult) -> f64 + Sync + 'a;

/// Annualized Sharpe from the equity curve; -inf when metrics are not valid.
pub fn objective_sharpe(r: &BacktestResult) -> f64 {
    if r.metrics.valid {
        r.metrics.sharpe
    } else {
        f64::NEG_INFINITY
    }
}

/// Total return; -inf when metrics are not valid.
pub fn objective_total_return(r: &BacktestResult) -> f64 {
    if r.metrics.valid {
        r.metrics.total_return
    } else {
        f64::NEG_INFINITY
    }
}

/// One backtest of one parameter set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Trial {
    /// Trial id.
    pub id: u64,
    /// Parameters.
    pub params: ParamSet,
    /// Run record.
    pub run: RunRecord,
    /// Headline metrics.
    pub metrics: Metrics,
    /// Objective value.
    pub objective: f64,
    /// Per-period simple returns of the equity curve.
    pub returns: Vec<f64>,
}

/// Sweep options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepOptions {
    /// Worker threads (1 = run inline).
    pub threads: usize,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self { threads: 1 }
    }
}

/// All trials of a sweep.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SweepResult {
    /// Trials in parameter order.
    pub trials: Vec<Trial>,
    /// Index of the highest objective (NaN counts as -inf; ties -> lowest).
    pub best: usize,
}

impl SweepResult {
    /// The best trial, if any.
    pub fn best_trial(&self) -> Option<&Trial> {
        self.trials.get(self.best)
    }
}

/// Runs `factory(params)` and backtests it.
///
/// # Errors
/// The factory's or the backtest's error.
pub fn run_one(
    bars: &[Bar],
    factory: &StrategyFactory<'_>,
    config: &BacktestConfig,
    params: &ParamSet,
) -> Result<BacktestResult> {
    let mut strategy = factory(params)?;
    run_backtest(bars, strategy.as_mut(), config)
}

/// Runs one backtest per parameter set. Trial ids are `first_trial_id`,
/// `first_trial_id + 1`, ...
///
/// # Errors
/// The error of the lowest-index parameter set that failed (deterministic,
/// whatever the thread count).
pub fn run_sweep(
    bars: &[Bar],
    factory: &StrategyFactory<'_>,
    config: &BacktestConfig,
    params: &[ParamSet],
    objective: &Objective<'_>,
    options: SweepOptions,
    first_trial_id: u64,
) -> Result<SweepResult> {
    let mut result = SweepResult::default();
    if params.is_empty() {
        return Ok(result);
    }
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Result<Trial>>>> = Mutex::new(vec![None; params.len()]);

    let worker = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        if i >= params.len() {
            return;
        }
        let outcome = run_one(bars, factory, config, &params[i]).map(|r| Trial {
            id: first_trial_id + i as u64,
            params: params[i].clone(),
            run: r.run.clone(),
            metrics: r.metrics,
            objective: objective(&r),
            returns: returns_from_curve(&r.equity_curve),
        });
        let failed = outcome.is_err();
        slots.lock().expect("sweep slots")[i] = Some(outcome);
        if failed {
            next.store(params.len(), Ordering::Relaxed); // stop handing out work
            return;
        }
    };

    let n_threads = options.threads.clamp(1, params.len());
    if n_threads == 1 {
        worker();
    } else {
        std::thread::scope(|s| {
            for _ in 0..n_threads {
                s.spawn(worker);
            }
        });
    }

    let slots = slots.into_inner().expect("sweep slots");
    for slot in slots {
        match slot {
            Some(Ok(t)) => result.trials.push(t),
            Some(Err(e)) => return Err(e),
            None => {} // after an error, later sets may not have run
        }
    }
    if result.trials.len() != params.len() {
        return Err(Error::Internal("sweep stopped without reporting an error".into()));
    }
    let score = |v: f64| if v.is_nan() { f64::NEG_INFINITY } else { v };
    for i in 1..result.trials.len() {
        if score(result.trials[i].objective) > score(result.trials[result.best].objective) {
            result.best = i;
        }
    }
    Ok(result)
}

/// One recorded trial.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrialSummary {
    /// Trial id.
    pub id: u64,
    /// Experiment the trial belongs to.
    pub label: String,
    /// Canonical parameters.
    pub params: String,
    /// Result fingerprint.
    pub result_sha256: String,
    /// Per-period Sharpe.
    pub sharpe_per_period: f64,
    /// Return observations.
    pub n_obs: usize,
    /// Objective.
    pub objective: f64,
}

/// Append-only record of every trial run in a project.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrialLedger {
    trials: Vec<TrialSummary>,
}

impl TrialLedger {
    /// Records every trial of a sweep.
    pub fn add(&mut self, label: &str, sweep: &SweepResult) {
        for t in &sweep.trials {
            self.trials.push(TrialSummary {
                id: t.id,
                label: label.into(),
                params: t.params.canonical(),
                result_sha256: t.run.result_sha256.clone(),
                sharpe_per_period: sharpe_per_period(&t.returns),
                n_obs: t.returns.len(),
                objective: t.objective,
            });
        }
    }
    /// Records one summary.
    pub fn add_summary(&mut self, summary: TrialSummary) {
        self.trials.push(summary);
    }
    /// Number of trials.
    pub fn count(&self) -> usize {
        self.trials.len()
    }
    /// Id for the next trial.
    pub fn next_trial_id(&self) -> u64 {
        self.trials.len() as u64 + 1
    }
    /// All trials.
    pub fn trials(&self) -> &[TrialSummary] {
        &self.trials
    }
    /// Sample variance of per-period Sharpe across all trials (0 if fewer than 2).
    pub fn sharpe_variance(&self) -> f64 {
        if self.trials.len() < 2 {
            return 0.0;
        }
        let n = self.trials.len() as f64;
        let mut mean = 0.0;
        for t in &self.trials {
            mean += t.sharpe_per_period;
        }
        mean /= n;
        let mut ss = 0.0;
        for t in &self.trials {
            ss += (t.sharpe_per_period - mean) * (t.sharpe_per_period - mean);
        }
        ss / (n - 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_enumeration_and_sampling() {
        let g = ParamGrid::new()
            .add("fast", vec![1.0, 2.0])
            .unwrap()
            .add("slow", vec![10.0, 20.0, 30.0])
            .unwrap();
        assert_eq!(g.size(), 6);
        assert_eq!(g.at(0).unwrap().canonical(), "fast=1;slow=10");
        assert_eq!(g.at(1).unwrap().canonical(), "fast=1;slow=20");
        assert_eq!(g.at(5).unwrap().canonical(), "fast=2;slow=30");
        assert!(g.at(6).is_none());
        let s = g.sample(4, 9);
        assert_eq!(s.len(), 4);
        assert_eq!(g.sample(10, 9).len(), 6);
        assert!(ParamGrid::new().add("a", vec![]).is_err());
        assert!(ParamGrid::new().add("a", vec![1.0]).unwrap().add("a", vec![2.0]).is_err());
        assert_eq!(ParamGrid::new().size(), 1);
    }

    #[test]
    fn params_accessors() {
        let p = ParamSet::from_pairs([("fast", 2.5), ("x", 0.1)]);
        assert_eq!(p.get_int("fast").unwrap(), 3);
        assert!(p.get("nope").is_err());
        assert_eq!(p.canonical(), "fast=2.5;x=0.10000000000000001");
    }
}
