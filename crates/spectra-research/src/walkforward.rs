//! Walk-forward validation.
//!
//! For each fold: choose parameters by sweeping over a TRAIN window, then
//! evaluate the winner, untouched, on the following TEST window. Stitching the
//! test windows gives an out-of-sample record in which every return was
//! produced by parameters chosen using only earlier data.
//!
//! Windows are measured in unique timestamps (not bars), so multi-instrument
//! data splits cleanly. An optional embargo leaves a gap between train and
//! test.
//!
//! Limitations: every test window starts cold (long look-backs lose their
//! warm-up), each fold starts with fresh capital, and overlapping folds are
//! not independent.

use spectra_core::{timestamp_offsets, validate_bars, Bar, Error, Result};
use spectra_engine::{compute_metrics_from_equity, returns_from_curve, BacktestConfig, Metrics, RunRecord};

use crate::sweep::{run_one, run_sweep, Objective, ParamSet, StrategyFactory, SweepOptions, TrialLedger};

/// Rolling or anchored train windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WalkForwardMode {
    /// The train window slides forward.
    #[default]
    Rolling,
    /// The train window always starts at the beginning.
    Anchored,
}

/// Window sizes, in unique timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkForwardConfig {
    /// Timestamps in the (initial) train window.
    pub train: usize,
    /// Timestamps in each test window.
    pub test: usize,
    /// How far each fold advances; 0 means `test`.
    pub step: usize,
    /// Timestamps skipped between train and test.
    pub embargo: usize,
    /// Rolling or anchored.
    pub mode: WalkForwardMode,
}

/// Half-open index ranges into the sorted list of unique timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fold {
    /// Train start.
    pub train_begin: usize,
    /// Train end (exclusive).
    pub train_end: usize,
    /// Test start.
    pub test_begin: usize,
    /// Test end (exclusive).
    pub test_end: usize,
}

/// The folds for `n` timestamps.
///
/// # Errors
/// If `train` or `test` is zero.
pub fn make_folds(n: usize, cfg: &WalkForwardConfig) -> Result<Vec<Fold>> {
    if cfg.train == 0 || cfg.test == 0 {
        return Err(Error::InvalidArgument("walk-forward: train and test must be > 0".into()));
    }
    let step = if cfg.step == 0 { cfg.test } else { cfg.step };
    let mut folds = Vec::new();
    let mut s = 0usize;
    loop {
        let train_end = s + cfg.train;
        let test_begin = train_end + cfg.embargo;
        let test_end = test_begin + cfg.test;
        if test_end > n {
            break;
        }
        folds.push(Fold {
            train_begin: if cfg.mode == WalkForwardMode::Anchored { 0 } else { s },
            train_end,
            test_begin,
            test_end,
        });
        s += step;
    }
    Ok(folds)
}

/// One fold's outcome.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FoldResult {
    /// The fold.
    pub fold: Fold,
    /// Parameters chosen on the train window.
    pub best_params: ParamSet,
    /// Their in-sample objective.
    pub in_sample_objective: f64,
    /// Their out-of-sample objective.
    pub out_of_sample_objective: f64,
    /// Out-of-sample metrics.
    pub oos_metrics: Metrics,
    /// Out-of-sample returns.
    pub oos_returns: Vec<f64>,
    /// Out-of-sample run record.
    pub oos_run: RunRecord,
    /// Trials run on the train window.
    pub trials: usize,
}

/// The whole walk-forward.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WalkForwardResult {
    /// Per-fold outcomes.
    pub folds: Vec<FoldResult>,
    /// All test windows concatenated in time order.
    pub oos_returns: Vec<f64>,
    /// Metrics of the compounded concatenated returns.
    pub oos_metrics: Metrics,
    /// Mean in-sample objective.
    pub mean_in_sample_objective: f64,
    /// Mean out-of-sample objective.
    pub mean_out_of_sample_objective: f64,
    /// Total trials.
    pub total_trials: usize,
}

/// Walk-forward efficiency: mean out-of-sample objective / mean in-sample objective. 0 when the
/// in-sample mean is effectively zero (`|mean_is| <= 1e-9`), where the ratio is meaningless.
pub fn walk_forward_efficiency(mean_in_sample: f64, mean_out_of_sample: f64) -> f64 {
    if mean_in_sample.abs() > 1e-9 {
        mean_out_of_sample / mean_in_sample
    } else {
        0.0
    }
}

impl WalkForwardResult {
    /// [`walk_forward_efficiency`] of this run.
    pub fn efficiency(&self) -> f64 {
        walk_forward_efficiency(self.mean_in_sample_objective, self.mean_out_of_sample_objective)
    }
}

/// Runs walk-forward validation. If `ledger` is given every sweep trial is
/// recorded in it (label "walk-forward fold k").
///
/// # Errors
/// Invalid bars, no parameter sets, or any backtest error.
#[allow(clippy::too_many_arguments)]
pub fn run_walk_forward(
    bars: &[Bar],
    factory: &StrategyFactory<'_>,
    config: &BacktestConfig,
    params: &[ParamSet],
    wf: &WalkForwardConfig,
    objective: &Objective<'_>,
    options: SweepOptions,
    mut ledger: Option<&mut TrialLedger>,
) -> Result<WalkForwardResult> {
    if let Some(err) = validate_bars(bars) {
        return Err(Error::InvalidArgument(format!("invalid bars: {err}")));
    }
    if params.is_empty() {
        return Err(Error::InvalidArgument("walk-forward: no parameter sets".into()));
    }
    let offsets = timestamp_offsets(bars);
    let n_ts = offsets.len() - 1;
    let folds = make_folds(n_ts, wf)?;
    let mut out = WalkForwardResult::default();
    let mut next_id = ledger.as_ref().map_or(1, |l| l.next_trial_id());

    for (k, f) in folds.iter().enumerate() {
        let train = &bars[offsets[f.train_begin]..offsets[f.train_end]];
        let test = &bars[offsets[f.test_begin]..offsets[f.test_end]];
        let sweep = run_sweep(train, factory, config, params, objective, options, next_id)?;
        next_id += sweep.trials.len() as u64;
        if let Some(l) = ledger.as_deref_mut() {
            l.add(&format!("walk-forward fold {k}"), &sweep);
        }
        let best = sweep.best_trial().expect("non-empty sweep");
        let oos = run_one(test, factory, config, &best.params)?;
        let fr = FoldResult {
            fold: *f,
            best_params: best.params.clone(),
            in_sample_objective: best.objective,
            out_of_sample_objective: objective(&oos),
            oos_metrics: oos.metrics,
            oos_returns: returns_from_curve(&oos.equity_curve),
            oos_run: oos.run.clone(),
            trials: sweep.trials.len(),
        };
        out.total_trials += fr.trials;
        out.oos_returns.extend_from_slice(&fr.oos_returns);
        out.mean_in_sample_objective += fr.in_sample_objective;
        out.mean_out_of_sample_objective += fr.out_of_sample_objective;
        out.folds.push(fr);
    }
    if !out.folds.is_empty() {
        let n = out.folds.len() as f64;
        out.mean_in_sample_objective /= n;
        out.mean_out_of_sample_objective /= n;
    }
    // Compound the stitched returns into an equity series for aggregate metrics.
    let mut equity = Vec::with_capacity(out.oos_returns.len() + 1);
    let mut e = 1.0;
    equity.push(e);
    for r in &out.oos_returns {
        e *= 1.0 + r;
        equity.push(e);
    }
    out.oos_metrics = compute_metrics_from_equity(&equity, config.periods_per_year, config.risk_free_rate);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds() {
        let cfg = WalkForwardConfig { train: 3, test: 2, ..WalkForwardConfig::default() };
        let f = make_folds(10, &cfg).unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(f[1], Fold { train_begin: 2, train_end: 5, test_begin: 5, test_end: 7 });
        let anchored = WalkForwardConfig { mode: WalkForwardMode::Anchored, embargo: 1, ..cfg };
        let g = make_folds(10, &anchored).unwrap();
        assert!(g.iter().all(|x| x.train_begin == 0 && x.test_begin == x.train_end + 1));
        assert!(make_folds(10, &WalkForwardConfig::default()).is_err());
    }
}
