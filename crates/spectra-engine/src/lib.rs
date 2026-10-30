//! Spectra Labs event-driven backtester: transaction costs, order handling,
//! fills, metrics and run fingerprints. Ported from the C++ reference
//! (`quantforge` v0.6.0); see [`backtest`] for the timing contract and the
//! three deliberate fixes.

pub mod backtest;
pub mod bonds;
pub mod corporate;
pub mod costs;
pub mod dsl;
pub mod futures;
pub mod instruments;
pub mod metrics;
pub mod performance;
pub mod rebalance;

pub use backtest::{
    canonical_config, canonical_multi_config, config_fingerprint, run_backtest, run_backtest_controlled,
    run_backtest_with_actions, run_backtest_with_setup, run_multi_backtest, run_multi_backtest_with_actions,
    run_multi_backtest_with_setup, Allocation, BacktestConfig, BacktestResult, Context, ExecutionConfig,
    FinancingConfig, ImpactConfig, Order, OrderId, OrderSpec, OrderState, OrderType, Progress, RollEvent,
    RollKind, RunControl, RunError, RunLimits, RunRecord, Strategy, StrategyResult, TimeInForce,
    MAX_STRATEGIES,
};
pub use bonds::{BondRisk, BondTerms};
pub use corporate::{corporate_actions_sha256, CorporateAction, CorporateActionKind, MAX_SPLIT_SHARES};
pub use costs::{commission_fee, effective_slippage_bps, slipped_price, CommissionModel, CostModel};
pub use futures::{
    Chains, Continuous, RollAdjust, RollRule, DEFAULT_ROLL_BUSINESS_DAYS, MAX_ROLL_BUSINESS_DAYS,
};
pub use instruments::{BondQuote, FxRate, InstrumentSetup, FX_SCALE};
pub use metrics::{
    compute_drawdown_stats, compute_metrics, compute_metrics_from_equity, compute_tail_stats,
    nearest_rank_index, returns_from_curve, returns_from_equity, DrawdownStats, EquityPoint, Metrics,
    TailStats,
};
pub use rebalance::{RebalanceConfig, RebalanceRule, Rebalancer, WeightStrategy};
