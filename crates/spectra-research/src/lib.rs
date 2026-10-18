//! Spectra Labs research validation: parameter sweeps with a trial ledger,
//! walk-forward validation, PSR/DSR, PBO, trade analysis, benchmark-relative
//! statistics and the Strategy Survival Lab. Ported from the C++ reference
//! (`quantforge` v0.6.0); reports are text-identical.

pub mod benchmark;
pub mod pbo;
pub mod stats;
pub mod survival;
pub mod sweep;
pub mod trades;
pub mod validation;
pub mod walkforward;

pub use benchmark::{aligned_returns, benchmark_returns, compute_benchmark_metrics, BenchmarkMetrics};
pub use pbo::{cscv_pbo, PboResult};
pub use stats::{
    deflated_sharpe, expected_max_sharpe, min_track_record_length, moments, normal_cdf, normal_inv_cdf,
    probabilistic_sharpe, sharpe_per_period, Moments,
};
pub use survival::{format_survival_report, run_survival_lab, SurvivalConfig, SurvivalReport, Verdict};
pub use sweep::{
    objective_sharpe, objective_total_return, run_one, run_sweep, Objective, ParamGrid, ParamSet,
    StrategyFactory, SweepOptions, SweepResult, Trial, TrialLedger, TrialSummary,
};
pub use trades::{compute_trade_stats, extract_trades, resample_trades, Trade, TradeResampling, TradeStats};
pub use validation::{format_report, validate_selection, ValidationReport};
pub use walkforward::{
    make_folds, run_walk_forward, Fold, FoldResult, WalkForwardConfig, WalkForwardMode, WalkForwardResult,
};
