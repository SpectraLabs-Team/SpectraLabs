//! Portfolios & Baskets Rebalancing Engine (M40).
//!
//! Tracks manual and paper holdings, validates target allocation baskets
//! (exact 100% sum, duplicate rejection), and computes cost-aware rebalancing
//! trade orders to align portfolios to target models within tolerance.

use std::collections::{HashMap, HashSet};

/// Errors in basket construction and portfolio rebalancing.
#[derive(Debug, Clone, PartialEq)]
pub enum BasketError {
    DuplicateSymbol(String),
    InvalidWeightSum { sum: f64, expected: f64 },
    NonPositiveWeight(String, f64),
    EmptyBasket,
}

impl std::fmt::Display for BasketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateSymbol(s) => write!(f, "basket contains duplicate symbol: {}", s),
            Self::InvalidWeightSum { sum, expected } => {
                write!(f, "basket weights sum to {:.4}, expected {:.4}", sum, expected)
            }
            Self::NonPositiveWeight(s, w) => {
                write!(f, "symbol {} has non-positive weight: {:.4}", s, w)
            }
            Self::EmptyBasket => write!(f, "basket cannot be empty"),
        }
    }
}

impl std::error::Error for BasketError {}

/// An instrument in a target allocation basket.
#[derive(Debug, Clone, PartialEq)]
pub struct BasketItem {
    pub symbol: String,
    pub target_weight: f64,
}

/// A named basket of instruments with target weights.
#[derive(Debug, Clone, PartialEq)]
pub struct Basket {
    pub id: String,
    pub name: String,
    pub items: Vec<BasketItem>,
}

impl Basket {
    pub fn new(id: impl Into<String>, name: impl Into<String>, items: Vec<BasketItem>) -> Self {
        Self { id: id.into(), name: name.into(), items }
    }

    /// Validates basket constraints: non-empty, no duplicates, weights sum to 1.0 (100%).
    pub fn validate(&self) -> Result<(), BasketError> {
        if self.items.is_empty() {
            return Err(BasketError::EmptyBasket);
        }

        let mut seen = HashSet::new();
        let mut total_weight = 0.0;

        for item in &self.items {
            let sym = item.symbol.to_uppercase();
            if !seen.insert(sym) {
                return Err(BasketError::DuplicateSymbol(item.symbol.clone()));
            }
            if item.target_weight <= 0.0 {
                return Err(BasketError::NonPositiveWeight(item.symbol.clone(), item.target_weight));
            }
            total_weight += item.target_weight;
        }

        if (total_weight - 1.0).abs() > 1e-4 {
            return Err(BasketError::InvalidWeightSum { sum: total_weight, expected: 1.0 });
        }

        Ok(())
    }
}

/// A position holding in a portfolio.
#[derive(Debug, Clone, PartialEq)]
pub struct HoldingPosition {
    pub symbol: String,
    pub quantity: f64,
    pub cost_basis_per_share: f64,
    pub current_price: f64,
    pub is_paper: bool,
}

impl HoldingPosition {
    pub fn market_value(&self) -> f64 {
        self.quantity * self.current_price
    }

    pub fn cost_basis_total(&self) -> f64 {
        self.quantity * self.cost_basis_per_share
    }

    pub fn unrealized_pnl(&self) -> f64 {
        self.market_value() - self.cost_basis_total()
    }
}

/// Summary valuation of a portfolio.
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioValuation {
    pub cash: f64,
    pub equity_value: f64,
    pub total_portfolio_value: f64,
    pub total_cost_basis: f64,
    pub total_unrealized_pnl: f64,
}

pub fn evaluate_portfolio(cash: f64, holdings: &[HoldingPosition]) -> PortfolioValuation {
    let mut equity = 0.0;
    let mut cost = 0.0;
    for h in holdings {
        equity += h.market_value();
        cost += h.cost_basis_total();
    }
    PortfolioValuation {
        cash,
        equity_value: equity,
        total_portfolio_value: cash + equity,
        total_cost_basis: cost,
        total_unrealized_pnl: equity - cost,
    }
}

/// Side of rebalance order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Proposed rebalancing trade order.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceTrade {
    pub symbol: String,
    pub side: OrderSide,
    pub shares: f64,
    pub price: f64,
    pub trade_notional: f64,
    pub estimated_cost: f64,
    pub target_weight: f64,
    pub current_weight: f64,
}

/// Transaction cost settings for rebalancing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostConfig {
    pub fixed_fee_per_order: f64,
    pub bps_slippage_and_comm: f64, // e.g. 5 bps = 0.0005
}

impl Default for CostConfig {
    fn default() -> Self {
        Self { fixed_fee_per_order: 1.0, bps_slippage_and_comm: 0.0005 }
    }
}

/// Rebalancing plan output.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalancePlan {
    pub total_value_before: f64,
    pub estimated_total_costs: f64,
    pub trades: Vec<RebalanceTrade>,
}

/// Computes trade orders required to rebalance current portfolio to target basket weights.
pub fn compute_rebalance_trades(
    cash: f64,
    holdings: &[HoldingPosition],
    basket: &Basket,
    prices: &HashMap<String, f64>,
    cost_cfg: CostConfig,
    drift_tolerance: f64,
) -> Result<RebalancePlan, BasketError> {
    basket.validate()?;

    let val = evaluate_portfolio(cash, holdings);
    let total_val = val.total_portfolio_value;
    if total_val <= 0.0 {
        return Ok(RebalancePlan {
            total_value_before: total_val,
            estimated_total_costs: 0.0,
            trades: Vec::new(),
        });
    }

    let mut holding_map = HashMap::new();
    for h in holdings {
        holding_map.insert(h.symbol.to_uppercase(), h.clone());
    }

    let mut trades = Vec::new();
    let mut total_cost = 0.0;

    for item in &basket.items {
        let sym = item.symbol.to_uppercase();
        let price = match prices.get(&sym).copied() {
            Some(p) => p,
            None => 1.0,
        };
        let curr_qty = match holding_map.get(&sym) {
            Some(h) => h.quantity,
            None => 0.0,
        };
        let curr_val = curr_qty * price;
        let curr_weight = curr_val / total_val;
        let target_weight = item.target_weight;

        let weight_diff = target_weight - curr_weight;
        if weight_diff.abs() > drift_tolerance {
            let target_val = total_val * target_weight;
            let val_diff = target_val - curr_val;
            let shares = (val_diff.abs() / price).round();

            if shares > 0.0 {
                let notional = shares * price;
                let cost = cost_cfg.fixed_fee_per_order + notional * cost_cfg.bps_slippage_and_comm;
                total_cost += cost;

                trades.push(RebalanceTrade {
                    symbol: item.symbol.clone(),
                    side: if val_diff > 0.0 { OrderSide::Buy } else { OrderSide::Sell },
                    shares,
                    price,
                    trade_notional: notional,
                    estimated_cost: cost,
                    target_weight,
                    current_weight: curr_weight,
                });
            }
        }
    }

    Ok(RebalancePlan { total_value_before: total_val, estimated_total_costs: total_cost, trades })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basket_validation() {
        // Valid 50/50 basket
        let valid = Basket::new(
            "b1",
            "Tech 50/50",
            vec![
                BasketItem { symbol: "AAPL".into(), target_weight: 0.50 },
                BasketItem { symbol: "MSFT".into(), target_weight: 0.50 },
            ],
        );
        assert!(valid.validate().is_ok());

        // Duplicate symbol
        let dup = Basket::new(
            "b2",
            "Dup",
            vec![
                BasketItem { symbol: "AAPL".into(), target_weight: 0.50 },
                BasketItem { symbol: "AAPL".into(), target_weight: 0.50 },
            ],
        );
        assert!(matches!(dup.validate(), Err(BasketError::DuplicateSymbol(_))));

        // Weights sum to 90% (not 100%)
        let bad_sum = Basket::new(
            "b3",
            "Bad Sum",
            vec![
                BasketItem { symbol: "AAPL".into(), target_weight: 0.40 },
                BasketItem { symbol: "MSFT".into(), target_weight: 0.50 },
            ],
        );
        assert!(matches!(bad_sum.validate(), Err(BasketError::InvalidWeightSum { .. })));
    }

    #[test]
    fn test_rebalance_trades_computation() {
        // Portfolio with $10,000 cash and no stock holdings
        let holdings = vec![];
        let basket = Basket::new(
            "b_target",
            "Target 60/40",
            vec![
                BasketItem { symbol: "AAPL".into(), target_weight: 0.60 },
                BasketItem { symbol: "MSFT".into(), target_weight: 0.40 },
            ],
        );

        let mut prices = HashMap::new();
        prices.insert("AAPL".into(), 100.0);
        prices.insert("MSFT".into(), 200.0);

        let plan =
            compute_rebalance_trades(10_000.0, &holdings, &basket, &prices, CostConfig::default(), 0.01)
                .unwrap();

        assert_eq!(plan.trades.len(), 2);
        // AAPL target: $6,000 / $100 = 60 shares Buy
        let aapl = plan.trades.iter().find(|t| t.symbol == "AAPL").unwrap();
        assert_eq!(aapl.side, OrderSide::Buy);
        assert_eq!(aapl.shares, 60.0);
        assert!(aapl.estimated_cost > 0.0);

        // MSFT target: $4,000 / $200 = 20 shares Buy
        let msft = plan.trades.iter().find(|t| t.symbol == "MSFT").unwrap();
        assert_eq!(msft.side, OrderSide::Buy);
        assert_eq!(msft.shares, 20.0);
    }
}
