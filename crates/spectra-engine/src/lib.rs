//! Spectra Labs event-driven backtester: transaction costs, order handling,
//! fills, metrics and run fingerprints. Ported from the C++ reference
//! (`quantforge` v0.6.0); see [`backtest`] for the timing contract and the
//! three deliberate fixes.

pub mod backtest;
pub mod costs;
pub mod metrics;
pub mod performance;
pub mod rebalance;

pub use backtest::{
    canonical_config, canonical_multi_config, config_fingerprint, run_backtest, run_backtest_controlled,
    run_multi_backtest, Allocation, BacktestConfig, BacktestResult, Context, ExecutionConfig,
    FinancingConfig, ImpactConfig, Order, OrderId, OrderSpec, OrderState, OrderType, Progress, RunControl,
    RunError, RunLimits, RunRecord, Strategy, StrategyResult, TimeInForce, MAX_STRATEGIES,
};
pub use costs::{commission_fee, slipped_price, CommissionModel, CostModel};
pub use metrics::{
    compute_drawdown_stats, compute_metrics, compute_metrics_from_equity, compute_tail_stats,
    nearest_rank_index, returns_from_curve, returns_from_equity, DrawdownStats, EquityPoint, Metrics,
    TailStats,
};
pub use rebalance::{RebalanceConfig, RebalanceRule, Rebalancer, WeightStrategy};
