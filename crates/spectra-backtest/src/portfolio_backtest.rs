//! Portfolio backtest (M15 Task 4): run a portfolio at target weights with rebalancing, and report
//! return, drawdown, turnover and the cost of each rebalance.
//!
//! This layer runs on the rebalancing simulator ([`crate::rebalance::simulate`]), so the rebalance
//! rules and costs are the ones Task 3 tests. Wiring the rebalances into the M3 engine's multi-strategy
//! run is a separate step and is not part of this module.

use crate::rebalance::Date;

use crate::portfolio::Holding;
use crate::rebalance::{
    simulate, RebalanceError, RebalanceEvent, RebalanceRule, RebalanceRun, TradeCostConfig,
};

/// Summary of a portfolio backtest.
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioBacktestSummary {
    /// Equity at the last close divided by the starting cash, minus one.
    pub total_return: f64,
    /// Largest peak-to-trough fall of the equity curve, as a positive fraction (0.2 for a 20% fall).
    pub max_drawdown: f64,
    /// Sum of each rebalance's turnover (traded notional as a fraction of equity before it).
    pub total_turnover: f64,
    /// Costs of all rebalances, in currency.
    pub total_cost: f64,
    /// Number of rebalances, including the initial one.
    pub rebalance_count: usize,
    /// Each rebalance's date and cost, in order.
    pub cost_per_rebalance: Vec<(Date, f64)>,
}

/// Summarize a finished rebalancing run against its starting cash.
///
/// # Panics
/// Never. A run with no equity points returns zero return and drawdown.
#[must_use]
pub fn summarize(run: &RebalanceRun, initial_cash: f64) -> PortfolioBacktestSummary {
    let last = run.equity.last().map_or(initial_cash, |(_, e)| *e);
    let total_return = last / initial_cash - 1.0;

    let mut peak = f64::NEG_INFINITY;
    let mut max_drawdown = 0.0_f64;
    for (_, equity) in &run.equity {
        peak = peak.max(*equity);
        if peak > 0.0 {
            max_drawdown = max_drawdown.max((peak - equity) / peak);
        }
    }

    PortfolioBacktestSummary {
        total_return,
        max_drawdown,
        total_turnover: run.total_turnover,
        total_cost: run.total_cost,
        rebalance_count: run.events.len(),
        cost_per_rebalance: run.events.iter().map(|e: &RebalanceEvent| (e.date, e.cost)).collect(),
    }
}

/// Run a portfolio backtest: simulate the rebalancing, then summarize it.
///
/// # Errors
/// As [`simulate`].
pub fn run_portfolio_backtest(
    symbols: &[String],
    targets: &[Holding],
    dates: &[Date],
    closes: &[Vec<f64>],
    initial_cash: f64,
    rule: RebalanceRule,
    cost: &TradeCostConfig,
) -> Result<(RebalanceRun, PortfolioBacktestSummary), RebalanceError> {
    let run = simulate(symbols, targets, dates, closes, initial_cash, rule, cost)?;
    let summary = summarize(&run, initial_cash);
    Ok((run, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rebalance::{Frequency, TradeCostConfig};

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::new(y, m, day)
    }

    fn zero_cost() -> TradeCostConfig {
        TradeCostConfig { name: "zero".into(), version: 1, slippage_bps: 0.0, min_fee: 0.0 }
    }

    fn equal_two() -> (Vec<String>, Vec<Holding>) {
        (
            vec!["A".to_string(), "B".to_string()],
            vec![Holding { symbol: "A".into(), weight: 0.5 }, Holding { symbol: "B".into(), weight: 0.5 }],
        )
    }

    #[test]
    fn equal_weight_zero_costs_one_rebalance_matches_hand_calculation() {
        // Equal weight, zero costs, one rebalance (the initial one; no calendar boundary in January).
        // Day 1: A=100, B=100 -> 1000 split into 5 units each.
        // Day 2: A=200, B=100 -> equity 1500. Return so far: +50%.
        // Day 3: A=100, B=100 -> equity 1000. Peak 1500, trough 1000: drawdown 1/3.
        // Final equity 1000, so total return 0%.
        // Turnover of the one rebalance: buying 1000 of stock on 1000 equity is 1.0.
        let (symbols, targets) = equal_two();
        let dates = [d(2026, 1, 2), d(2026, 1, 9), d(2026, 1, 16)];
        let closes = vec![vec![100.0, 100.0], vec![200.0, 100.0], vec![100.0, 100.0]];
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None };

        let (run, s) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, 1000.0, rule, &zero_cost()).unwrap();

        assert_eq!(s.rebalance_count, 1);
        assert!((s.total_return - 0.0).abs() < 1e-12, "return {}", s.total_return);
        assert!((s.max_drawdown - 1.0 / 3.0).abs() < 1e-12, "drawdown {}", s.max_drawdown);
        assert!((s.total_turnover - 1.0).abs() < 1e-12, "turnover {}", s.total_turnover);
        assert_eq!(s.total_cost, 0.0);
        assert_eq!(s.cost_per_rebalance, vec![(d(2026, 1, 2), 0.0)]);
        assert_eq!(run.events.len(), 1);
    }

    #[test]
    fn return_and_drawdown_on_a_rising_then_falling_curve() {
        // Equal weight with monthly rebalancing so the cost per event is visible; zero costs.
        // Start 1000; holdings split 500/500 at 100. Day 2 A=200 -> equity 1500. Day 3 A=100 -> 1000.
        // Final 1000: return 0%, drawdown 1/3 (from the 1500 peak).
        let (symbols, targets) = equal_two();
        let dates = [d(2026, 1, 2), d(2026, 1, 9), d(2026, 1, 16)];
        let closes = vec![vec![100.0, 100.0], vec![200.0, 100.0], vec![100.0, 100.0]];
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None };
        let (_, s) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, 1000.0, rule, &zero_cost()).unwrap();
        assert!((s.max_drawdown - 1.0 / 3.0).abs() < 1e-12);
        assert!((s.total_return).abs() < 1e-12);
    }

    #[test]
    fn costs_are_reported_per_rebalance() {
        let (symbols, targets) = equal_two();
        let dates = [d(2026, 1, 2), d(2026, 2, 2)];
        let closes = vec![vec![100.0, 100.0], vec![100.0, 100.0]];
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None };
        let cost =
            TradeCostConfig { name: "paper-default".into(), version: 1, slippage_bps: 5.0, min_fee: 1.0 };
        let (run, s) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, 10_000.0, rule, &cost).unwrap();
        assert_eq!(s.rebalance_count, 2);
        assert_eq!(s.cost_per_rebalance.len(), 2);
        let summed: f64 = s.cost_per_rebalance.iter().map(|(_, c)| c).sum();
        assert!((summed - s.total_cost).abs() < 1e-9);
        assert!((s.total_cost - run.total_cost).abs() < 1e-12);
        assert!(s.total_cost > 0.0);
        // Flat prices: the second rebalance trades nothing, so only the initial one costs anything.
        assert_eq!(run.events[1].trades.len(), 0);
    }
}
