//! Pre-trade risk enforcement and portfolio limit evaluation.
//!
//! Standard-library only: zero external dependencies.

use std::collections::HashMap;

/// Configurable limits for risk enforcement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskLimits {
    /// Maximum allowable gross exposure in base currency (sum of absolute position values).
    pub max_gross_exposure: f64,
    /// Maximum allowable net exposure in base currency (longs - shorts).
    pub max_net_exposure: f64,
    /// Maximum allowable single position size in base currency.
    pub max_position_size: f64,
    /// Maximum portfolio concentration in a single asset as a fraction [0.0, 1.0].
    pub max_concentration_pct: f64,
    /// Maximum gross leverage (gross exposure / equity).
    pub max_leverage: f64,
    /// Maximum portfolio drawdown from peak equity as a fraction [0.0, 1.0].
    pub max_drawdown_pct: f64,
    /// Maximum loss allowed in a single trading day in base currency.
    pub max_daily_loss: f64,
    /// Maximum Value-at-Risk as a fraction [0.0, 1.0].
    pub max_var_pct: f64,
    /// If true, orders that breach limits are reduced to the maximum permissible size rather than rejected.
    pub size_reduction_enabled: bool,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_gross_exposure: 1_000_000.0,
            max_net_exposure: 500_000.0,
            max_position_size: 100_000.0,
            max_concentration_pct: 0.25,
            max_leverage: 3.0,
            max_drawdown_pct: 0.15,
            max_daily_loss: 25_000.0,
            max_var_pct: 0.05,
            size_reduction_enabled: false,
        }
    }
}

/// Order direction for candidate orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// A candidate order submitted for pre-trade risk evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderCandidate {
    pub symbol: String,
    pub side: OrderSide,
    pub qty: f64,
    pub price: f64,
}

impl OrderCandidate {
    /// Nominal value of the candidate order (qty * price).
    pub fn notional(&self) -> f64 {
        self.qty * self.price
    }
}

/// A held position in the current portfolio.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionSnapshot {
    /// Signed quantity: positive for long, negative for short.
    pub qty: f64,
    /// Current marked price.
    pub price: f64,
}

impl PositionSnapshot {
    /// Marked market value of the position (qty * price).
    pub fn value(&self) -> f64 {
        self.qty * self.price
    }
}

/// A snapshot of the current portfolio state before evaluating an order.
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioSnapshot {
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
    pub positions: HashMap<String, PositionSnapshot>,
}

impl Default for PortfolioSnapshot {
    fn default() -> Self {
        Self {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        }
    }
}

impl PortfolioSnapshot {
    /// Calculate current gross exposure (sum of absolute position values).
    pub fn gross_exposure(&self) -> f64 {
        self.positions.values().map(|p| p.value().abs()).sum()
    }

    /// Calculate current net exposure (sum of signed position values).
    pub fn net_exposure(&self) -> f64 {
        self.positions.values().map(PositionSnapshot::value).sum()
    }

    /// Current leverage: gross exposure / equity.
    pub fn leverage(&self) -> f64 {
        if self.equity <= 0.0 {
            f64::INFINITY
        } else {
            self.gross_exposure() / self.equity
        }
    }

    /// Current drawdown: (peak_equity - equity) / peak_equity.
    pub fn drawdown_pct(&self) -> f64 {
        if self.peak_equity <= 0.0 {
            0.0
        } else {
            ((self.peak_equity - self.equity) / self.peak_equity).max(0.0)
        }
    }

    /// Current daily loss: daily_start_equity - equity.
    pub fn daily_loss(&self) -> f64 {
        (self.daily_start_equity - self.equity).max(0.0)
    }

    /// Parametric 1-day 99% VaR approximation based on portfolio leverage.
    pub fn var_pct(&self) -> f64 {
        if self.equity <= 0.0 {
            0.0
        } else {
            self.leverage() * 0.0165 * 2.33
        }
    }
}

/// Details of a specific limit violation.
#[derive(Debug, Clone, PartialEq)]
pub struct RiskBreach {
    pub code: String,
    pub limit: f64,
    pub projected: f64,
    pub message: String,
}

/// The outcome of evaluating an order against pre-trade risk controls.
#[derive(Debug, Clone, PartialEq)]
pub struct PreTradeResult {
    pub verdict: PreTradeVerdict,
    pub requested_qty: f64,
    pub approved_qty: f64,
    pub breaches: Vec<RiskBreach>,
    pub projected_metrics: ProjectedMetrics,
}

/// Pre-trade order verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreTradeVerdict {
    Approved,
    Reduced,
    Rejected,
}

/// Projected portfolio risk metrics after hypothetical order execution.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectedMetrics {
    pub gross_exposure: f64,
    pub net_exposure: f64,
    pub leverage: f64,
    pub position_size: f64,
    pub concentration_pct: f64,
    pub drawdown_pct: f64,
    pub daily_loss: f64,
    pub var_pct: f64,
}

/// Evaluates pre-trade risk checks on candidate orders.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreTradeChecker;

impl PreTradeChecker {
    /// Create a fail-closed rejected verdict when limits or portfolio state cannot be loaded.
    pub fn fail_closed(order: &OrderCandidate, reason: &str) -> PreTradeResult {
        PreTradeResult {
            verdict: PreTradeVerdict::Rejected,
            requested_qty: order.qty,
            approved_qty: 0.0,
            breaches: vec![RiskBreach {
                code: "system_fail_closed".into(),
                limit: 0.0,
                projected: 0.0,
                message: reason.to_string(),
            }],
            projected_metrics: ProjectedMetrics::default(),
        }
    }

    /// Perform pre-trade evaluation of `order` against `limits` given current `portfolio` and kill-switch status.
    pub fn evaluate(
        limits: &RiskLimits,
        portfolio: &PortfolioSnapshot,
        order: &OrderCandidate,
        kill_switch_active: bool,
    ) -> PreTradeResult {
        let mut breaches = Vec::new();
        let current_pos_qty = portfolio.positions.get(&order.symbol).map_or(0.0, |p| p.qty);

        // 1. Emergency Kill Switch check (Task 5: block opening orders, allow closing/flattening)
        if kill_switch_active {
            let is_closing = match order.side {
                OrderSide::Buy => current_pos_qty < 0.0,
                OrderSide::Sell => current_pos_qty > 0.0,
            };

            if !is_closing {
                breaches.push(RiskBreach {
                    code: "kill_switch_active".into(),
                    limit: 0.0,
                    projected: 1.0,
                    message: "Emergency kill switch is currently active; opening orders are blocked.".into(),
                });
                return PreTradeResult {
                    verdict: PreTradeVerdict::Rejected,
                    requested_qty: order.qty,
                    approved_qty: 0.0,
                    breaches,
                    projected_metrics: ProjectedMetrics {
                        gross_exposure: portfolio.gross_exposure(),
                        net_exposure: portfolio.net_exposure(),
                        leverage: portfolio.leverage(),
                        position_size: (current_pos_qty * order.price).abs(),
                        concentration_pct: 0.0,
                        drawdown_pct: portfolio.drawdown_pct(),
                        daily_loss: portfolio.daily_loss(),
                        var_pct: portfolio.var_pct(),
                    },
                };
            }

            // Order is closing: check if quantity exceeds existing position (which would flip/open new risk)
            let max_close_qty = current_pos_qty.abs();
            if order.qty > max_close_qty {
                if limits.size_reduction_enabled && max_close_qty > 0.0 {
                    let approved_qty = max_close_qty;
                    let mut projected_gross = 0.0;
                    for (sym, pos) in &portfolio.positions {
                        if sym != &order.symbol {
                            projected_gross += pos.value().abs();
                        }
                    }
                    let projected_leverage = if portfolio.equity <= 0.0 {
                        f64::INFINITY
                    } else {
                        projected_gross / portfolio.equity
                    };

                    return PreTradeResult {
                        verdict: PreTradeVerdict::Reduced,
                        requested_qty: order.qty,
                        approved_qty,
                        breaches: vec![RiskBreach {
                            code: "kill_switch_closing_only".into(),
                            limit: max_close_qty,
                            projected: order.qty,
                            message: format!(
                                "Kill switch active: order reduced from {:.4} to {:.4} to only flatten existing position.",
                                order.qty, max_close_qty
                            ),
                        }],
                        projected_metrics: ProjectedMetrics {
                            gross_exposure: projected_gross,
                            net_exposure: portfolio.net_exposure(),
                            leverage: projected_leverage,
                            position_size: 0.0,
                            concentration_pct: 0.0,
                            drawdown_pct: portfolio.drawdown_pct(),
                            daily_loss: portfolio.daily_loss(),
                            var_pct: 0.0,
                        },
                    };
                }

                breaches.push(RiskBreach {
                    code: "kill_switch_active".into(),
                    limit: max_close_qty,
                    projected: order.qty,
                    message: format!(
                        "Emergency kill switch active: order quantity {:.4} exceeds existing position {:.4}.",
                        order.qty, max_close_qty
                    ),
                });
                return PreTradeResult {
                    verdict: PreTradeVerdict::Rejected,
                    requested_qty: order.qty,
                    approved_qty: 0.0,
                    breaches,
                    projected_metrics: ProjectedMetrics {
                        gross_exposure: portfolio.gross_exposure(),
                        net_exposure: portfolio.net_exposure(),
                        leverage: portfolio.leverage(),
                        position_size: (current_pos_qty * order.price).abs(),
                        concentration_pct: 0.0,
                        drawdown_pct: portfolio.drawdown_pct(),
                        daily_loss: portfolio.daily_loss(),
                        var_pct: portfolio.var_pct(),
                    },
                };
            }

            // Order is fully within existing position: approved to flatten/reduce
            let signed_delta = match order.side {
                OrderSide::Buy => order.qty,
                OrderSide::Sell => -order.qty,
            };
            let projected_pos_qty = current_pos_qty + signed_delta;
            let projected_pos_val = (projected_pos_qty * order.price).abs();
            let mut projected_gross = 0.0;
            for (sym, pos) in &portfolio.positions {
                if sym == &order.symbol {
                    projected_gross += projected_pos_val;
                } else {
                    projected_gross += pos.value().abs();
                }
            }
            let projected_leverage =
                if portfolio.equity <= 0.0 { f64::INFINITY } else { projected_gross / portfolio.equity };

            return PreTradeResult {
                verdict: PreTradeVerdict::Approved,
                requested_qty: order.qty,
                approved_qty: order.qty,
                breaches: Vec::new(),
                projected_metrics: ProjectedMetrics {
                    gross_exposure: projected_gross,
                    net_exposure: portfolio.net_exposure(),
                    leverage: projected_leverage,
                    position_size: projected_pos_val,
                    concentration_pct: if portfolio.equity <= 0.0 {
                        0.0
                    } else {
                        projected_pos_val / portfolio.equity
                    },
                    drawdown_pct: portfolio.drawdown_pct(),
                    daily_loss: portfolio.daily_loss(),
                    var_pct: portfolio.var_pct(),
                },
            };
        }

        // 2. Drawdown & Daily Loss limits (Task 2: stored equity history)
        let current_dd = portfolio.drawdown_pct();
        if current_dd > limits.max_drawdown_pct {
            breaches.push(RiskBreach {
                code: "drawdown_exceeded".into(),
                limit: limits.max_drawdown_pct,
                projected: current_dd,
                message: format!(
                    "Current drawdown {:.2}% exceeds limit {:.2}%.",
                    current_dd * 100.0,
                    limits.max_drawdown_pct * 100.0
                ),
            });
        }

        let current_loss = portfolio.daily_loss();
        if current_loss > limits.max_daily_loss {
            breaches.push(RiskBreach {
                code: "daily_loss_exceeded".into(),
                limit: limits.max_daily_loss,
                projected: current_loss,
                message: format!(
                    "Daily loss ${:.2} exceeds limit ${:.2}.",
                    current_loss, limits.max_daily_loss
                ),
            });
        }

        if !breaches.is_empty() {
            return PreTradeResult {
                verdict: PreTradeVerdict::Rejected,
                requested_qty: order.qty,
                approved_qty: 0.0,
                breaches,
                projected_metrics: ProjectedMetrics {
                    gross_exposure: portfolio.gross_exposure(),
                    net_exposure: portfolio.net_exposure(),
                    leverage: portfolio.leverage(),
                    position_size: portfolio.positions.get(&order.symbol).map_or(0.0, |p| p.value().abs()),
                    concentration_pct: 0.0,
                    drawdown_pct: current_dd,
                    daily_loss: current_loss,
                    var_pct: portfolio.var_pct(),
                },
            };
        }

        // 3. Project new position for target symbol
        let signed_delta = match order.side {
            OrderSide::Buy => order.qty,
            OrderSide::Sell => -order.qty,
        };
        let projected_pos_qty = current_pos_qty + signed_delta;
        let projected_pos_val = (projected_pos_qty * order.price).abs();

        // 4. Calculate projected portfolio exposure
        let mut projected_gross = 0.0;
        let mut projected_net = 0.0;
        let mut found = false;

        for (sym, p) in &portfolio.positions {
            if sym == &order.symbol {
                found = true;
                projected_gross += projected_pos_val;
                projected_net += projected_pos_qty * order.price;
            } else {
                projected_gross += p.value().abs();
                projected_net += p.value();
            }
        }
        if !found {
            projected_gross += projected_pos_val;
            projected_net += projected_pos_qty * order.price;
        }

        let projected_leverage =
            if portfolio.equity <= 0.0 { f64::INFINITY } else { projected_gross / portfolio.equity };

        let projected_concentration =
            if portfolio.equity <= 0.0 { 0.0 } else { projected_pos_val / portfolio.equity };

        let projected_var = if portfolio.equity <= 0.0 { 0.0 } else { projected_leverage * 0.0165 * 2.33 };

        let projected_metrics = ProjectedMetrics {
            gross_exposure: projected_gross,
            net_exposure: projected_net,
            leverage: projected_leverage,
            position_size: projected_pos_val,
            concentration_pct: projected_concentration,
            drawdown_pct: current_dd,
            daily_loss: current_loss,
            var_pct: projected_var,
        };

        // Check exposure, size, leverage, and VaR limits (Task 9)
        if projected_pos_val > limits.max_position_size {
            breaches.push(RiskBreach {
                code: "position_size_exceeded".into(),
                limit: limits.max_position_size,
                projected: projected_pos_val,
                message: format!(
                    "Position size ${:.2} would exceed limit ${:.2}.",
                    projected_pos_val, limits.max_position_size
                ),
            });
        }

        if projected_gross > limits.max_gross_exposure {
            breaches.push(RiskBreach {
                code: "gross_exposure_exceeded".into(),
                limit: limits.max_gross_exposure,
                projected: projected_gross,
                message: format!(
                    "Gross exposure ${:.2} would exceed limit ${:.2}.",
                    projected_gross, limits.max_gross_exposure
                ),
            });
        }

        if projected_net.abs() > limits.max_net_exposure {
            breaches.push(RiskBreach {
                code: "net_exposure_exceeded".into(),
                limit: limits.max_net_exposure,
                projected: projected_net.abs(),
                message: format!(
                    "Net exposure ${:.2} would exceed limit ${:.2}.",
                    projected_net.abs(),
                    limits.max_net_exposure
                ),
            });
        }

        if projected_leverage > limits.max_leverage {
            breaches.push(RiskBreach {
                code: "leverage_exceeded".into(),
                limit: limits.max_leverage,
                projected: projected_leverage,
                message: format!(
                    "Leverage {:.2}x would exceed limit {:.2}x.",
                    projected_leverage, limits.max_leverage
                ),
            });
        }

        if projected_concentration > limits.max_concentration_pct {
            breaches.push(RiskBreach {
                code: "concentration_exceeded".into(),
                limit: limits.max_concentration_pct,
                projected: projected_concentration,
                message: format!(
                    "Concentration {:.2}% would exceed limit {:.2}%.",
                    projected_concentration * 100.0,
                    limits.max_concentration_pct * 100.0
                ),
            });
        }

        if limits.max_var_pct > 0.0 && projected_var > limits.max_var_pct {
            breaches.push(RiskBreach {
                code: "var_limit_exceeded".into(),
                limit: limits.max_var_pct,
                projected: projected_var,
                message: format!(
                    "Projected VaR {:.2}% would exceed limit {:.2}%.",
                    projected_var * 100.0,
                    limits.max_var_pct * 100.0
                ),
            });
        }

        // If no breaches, approved in full!
        if breaches.is_empty() {
            return PreTradeResult {
                verdict: PreTradeVerdict::Approved,
                requested_qty: order.qty,
                approved_qty: order.qty,
                breaches,
                projected_metrics,
            };
        }

        // 5. Size reduction policy
        if limits.size_reduction_enabled {
            let max_qty_for_pos =
                ((limits.max_position_size - (current_pos_qty * order.price).abs()) / order.price).max(0.0);
            let other_gross = projected_gross - projected_pos_val;
            let max_qty_for_gross = ((limits.max_gross_exposure - other_gross) / order.price).max(0.0);
            let max_qty_for_lev =
                (((limits.max_leverage * portfolio.equity) - other_gross) / order.price).max(0.0);

            let permissible_qty = max_qty_for_pos.min(max_qty_for_gross).min(max_qty_for_lev);

            if permissible_qty > 0.0 && permissible_qty < order.qty {
                return PreTradeResult {
                    verdict: PreTradeVerdict::Reduced,
                    requested_qty: order.qty,
                    approved_qty: (permissible_qty * 100.0).floor() / 100.0,
                    breaches,
                    projected_metrics,
                };
            }
        }

        PreTradeResult {
            verdict: PreTradeVerdict::Rejected,
            requested_qty: order.qty,
            approved_qty: 0.0,
            breaches,
            projected_metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approved_when_within_limits() {
        let limits = RiskLimits::default();
        let portfolio = PortfolioSnapshot::default();
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 100.0, price: 150.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false);
        assert_eq!(res.verdict, PreTradeVerdict::Approved);
        assert_eq!(res.approved_qty, 100.0);
        assert!(res.breaches.is_empty());
    }

    #[test]
    fn test_kill_switch_blocks_opening_orders() {
        let limits = RiskLimits::default();
        let portfolio = PortfolioSnapshot::default();
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true);
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res.approved_qty, 0.0);
        assert_eq!(res.breaches[0].code, "kill_switch_active");
    }

    #[test]
    fn test_kill_switch_allows_closing_orders_to_flatten() {
        let limits = RiskLimits::default();
        let mut portfolio = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 65_000.0,
            peak_equity: 65_000.0,
            daily_start_equity: 65_000.0,
            positions: HashMap::new(),
        };
        portfolio.positions.insert("AAPL".into(), PositionSnapshot { qty: 100.0, price: 150.0 });

        // Sell 100 on existing long 100 is flattening -> Approved even under kill switch!
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Sell, qty: 100.0, price: 150.0 };
        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true);
        assert_eq!(res.verdict, PreTradeVerdict::Approved);
        assert_eq!(res.approved_qty, 100.0);
        assert!(res.breaches.is_empty());

        // But Buy 10 on existing long 100 is opening more -> Rejected!
        let buy_order =
            OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let buy_res = PreTradeChecker::evaluate(&limits, &portfolio, &buy_order, true);
        assert_eq!(buy_res.verdict, PreTradeVerdict::Rejected);
        assert_eq!(buy_res.breaches[0].code, "kill_switch_active");
    }

    #[test]
    fn test_kill_switch_reduces_to_flatten_when_size_reduction_enabled() {
        let limits = RiskLimits { size_reduction_enabled: true, ..RiskLimits::default() };
        let mut portfolio = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 65_000.0,
            peak_equity: 65_000.0,
            daily_start_equity: 65_000.0,
            positions: HashMap::new(),
        };
        portfolio.positions.insert("AAPL".into(), PositionSnapshot { qty: 50.0, price: 150.0 });

        // Sell 100 when holding 50 with size reduction -> Reduced to 50
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Sell, qty: 100.0, price: 150.0 };
        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true);
        assert_eq!(res.verdict, PreTradeVerdict::Reduced);
        assert_eq!(res.approved_qty, 50.0);
        assert_eq!(res.breaches[0].code, "kill_switch_closing_only");
    }

    #[test]
    fn test_size_reduction_when_exceeding_position_limit() {
        let limits =
            RiskLimits { max_position_size: 15_000.0, size_reduction_enabled: true, ..RiskLimits::default() };
        let portfolio = PortfolioSnapshot::default();
        let order = OrderCandidate {
            symbol: "TSLA".into(),
            side: OrderSide::Buy,
            qty: 100.0,
            price: 200.0, // 20,000 exceeds 15,000
        };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false);
        assert_eq!(res.verdict, PreTradeVerdict::Reduced);
        assert_eq!(res.approved_qty, 75.0); // 15,000 / 200 = 75
        assert_eq!(res.breaches[0].code, "position_size_exceeded");
    }

    #[test]
    fn test_rejected_when_drawdown_exceeded() {
        let limits = RiskLimits { max_drawdown_pct: 0.10, ..RiskLimits::default() };
        let portfolio = PortfolioSnapshot {
            cash: 80_000.0,
            equity: 80_000.0,
            peak_equity: 100_000.0, // 20% drawdown > 10%
            daily_start_equity: 85_000.0,
            positions: HashMap::new(),
        };
        let order = OrderCandidate { symbol: "MSFT".into(), side: OrderSide::Buy, qty: 10.0, price: 300.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false);
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res.breaches[0].code, "drawdown_exceeded");
    }

    #[test]
    fn test_property_limit_arithmetic_invariants() {
        let limits = RiskLimits::default();
        let portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };

        // Property: any order that does not exceed position, concentration, gross, net, or leverage limits is Approved
        for price in [10.0, 50.0, 100.0, 250.0] {
            let max_qty_pos = limits.max_position_size / price;
            let max_qty_conc = (portfolio.equity * limits.max_concentration_pct) / price;
            let max_safe_qty = max_qty_pos.min(max_qty_conc);
            let safe_qty = max_safe_qty * 0.8;
            let order = OrderCandidate { symbol: "TEST".into(), side: OrderSide::Buy, qty: safe_qty, price };
            let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false);
            assert_eq!(res.verdict, PreTradeVerdict::Approved, "Safe order must be approved");
            assert_eq!(res.approved_qty, safe_qty);

            // Property: any order strictly greater than max_position_size with reduction disabled is Rejected
            let over_qty = max_qty_pos * 1.5;
            let over_order =
                OrderCandidate { symbol: "TEST".into(), side: OrderSide::Buy, qty: over_qty, price };
            let over_res = PreTradeChecker::evaluate(&limits, &portfolio, &over_order, false);
            assert_eq!(over_res.verdict, PreTradeVerdict::Rejected, "Over-limit order must be rejected");
        }
    }

    #[test]
    fn test_rejected_when_var_limit_exceeded() {
        // Tight max_var_pct limit of 0.001 (0.1%)
        let limits = RiskLimits {
            max_var_pct: 0.001,
            max_position_size: 1_000_000.0,
            max_gross_exposure: 1_000_000.0,
            ..RiskLimits::default()
        };
        let portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };

        // Buying $50,000 of stock generates ~0.6% 1-day 99% VaR, which breaches the 0.1% limit
        let order = OrderCandidate {
            symbol: "AAPL".into(),
            side: OrderSide::Buy,
            qty: 250.0,
            price: 200.0, // $50,000
        };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false);
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert!(res.breaches.iter().any(|b| b.code == "var_limit_exceeded"));
    }
}
