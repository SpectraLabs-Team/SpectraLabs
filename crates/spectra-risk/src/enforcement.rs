//! Pre-trade risk enforcement and portfolio limit evaluation.
//!
//! Standard-library only: zero external dependencies.

use std::collections::HashMap;

use spectra_core::{Error, Result};

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
    /// Annualized volatility (if known).
    pub volatility: Option<f64>,
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

/// Default degrees of freedom for Student-t copula simulation in pre-trade risk enforcement.
pub const ENFORCEMENT_T_COPULA_DOF: u32 = 4;

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
    ///
    /// # Errors
    /// Returns [`Error::InvalidArgument`] if `peak_equity <= 0.0` or is non-finite.
    pub fn drawdown_pct(&self) -> Result<f64> {
        if self.peak_equity <= 0.0 || !self.peak_equity.is_finite() {
            Err(Error::InvalidArgument(format!(
                "drawdown is undefined when peak_equity <= 0.0, got {}",
                self.peak_equity
            )))
        } else {
            Ok(((self.peak_equity - self.equity) / self.peak_equity).max(0.0))
        }
    }

    /// Current daily loss: daily_start_equity - equity.
    pub fn daily_loss(&self) -> f64 {
        (self.daily_start_equity - self.equity).max(0.0)
    }

    /// Parametric or correlated copula 1-day 99% VaR approximation based on portfolio holdings.
    /// Uses Student-t copula with [`ENFORCEMENT_T_COPULA_DOF`] degrees of freedom.
    ///
    /// # Errors
    /// Returns [`Error::InvalidArgument`] or [`Error::Data`] if the simulation or model fails.
    pub fn var_pct(&self) -> Result<f64> {
        self.var_pct_with_model(crate::copula::DependenceModel::TCopula {
            degrees_of_freedom: ENFORCEMENT_T_COPULA_DOF,
        })
    }

    /// Calculates 1-day 99% VaR as a fraction of equity under a specific dependence model.
    pub fn var_pct_with_model(&self, model: crate::copula::DependenceModel) -> Result<f64> {
        if self.equity <= 0.0 || !self.equity.is_finite() {
            return Err(Error::InvalidArgument(format!(
                "portfolio equity must be positive to compute VaR as a fraction of equity, got {}",
                self.equity
            )));
        }
        if self.positions.is_empty() {
            return Ok(0.0);
        }

        if let crate::copula::DependenceModel::TCopula { degrees_of_freedom } = model {
            if degrees_of_freedom == 0 {
                return Err(Error::InvalidArgument(
                    "t_copula requires degrees_of_freedom parameter (must be >= 1)".into(),
                ));
            }
        }

        let mut p = crate::portfolio::Portfolio::new();
        for (sym, pos) in &self.positions {
            if pos.qty.abs() > 1e-8 {
                if pos.price <= 0.0 || !pos.price.is_finite() {
                    return Err(Error::InvalidArgument(format!(
                        "position for {sym} has non-positive or non-finite price: {}",
                        pos.price
                    )));
                }
                let vol = pos
                    .volatility
                    .ok_or_else(|| Error::Data(format!("position for {sym} has no volatility")))?;
                if vol <= 0.0 || !vol.is_finite() {
                    return Err(Error::Data(format!("position for {sym} has invalid volatility: {vol}")));
                }
                p.add_asset_with_volatility(sym.as_str(), pos.qty, pos.price, vol);
            }
        }

        let n = p.assets().len();
        if n == 0 {
            return Ok(0.0);
        }

        if n > 1 {
            return Err(Error::InvalidArgument(format!(
                "multi-asset VaR requires a correlation matrix; none available for {n} assets"
            )));
        }

        // Run 2,000 simulations on 1-day horizon (1/252) at 99% confidence
        match crate::copula::simulate_correlated(&p, None, model, 2_000, 42, 1.0 / 252.0, 0.99) {
            Ok(sim) => match crate::metrics::value_at_risk(&sim.portfolio_losses, 0.99) {
                Ok(var_dollars) => Ok(var_dollars.max(0.0) / self.equity),
                Err(e) => Err(Error::Data(format!("var_pct quantile calculation failed: {e}"))),
            },
            Err(e) => Err(Error::Data(format!("var_pct simulation failed: {e}"))),
        }
    }
}

/// How much of a limit is in use: `value / limit` (1.0 = at the limit).
///
/// # Errors
/// Returns [`Error::InvalidArgument`] if `limit <= 0.0` or is non-finite.
pub fn utilization(value: f64, limit: f64) -> Result<f64> {
    if limit <= 0.0 || !limit.is_finite() {
        return Err(Error::InvalidArgument(format!("limit must be positive and finite, got {limit}")));
    }
    Ok(value / limit)
}

/// How much of each risk limit a portfolio is using, as fractions of the limit (1.0 = at the
/// limit; 0 for a limit that is not set). These are the figures behind the Risk Center's
/// utilization tooltips:
///
/// - gross exposure: `gross exposure / max gross exposure`
/// - net exposure: `|net exposure| / max net exposure`
/// - leverage: `(gross exposure / equity) / max leverage`
/// - drawdown: `((peak equity - equity) / peak equity) / max drawdown`
/// - daily loss: `(start-of-day equity - equity) / max daily loss`
/// - VaR: `1-day 99% VaR (fraction of equity) / max VaR`
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LimitUtilization {
    pub gross_exposure: f64,
    pub net_exposure: f64,
    pub leverage: f64,
    pub drawdown: f64,
    pub daily_loss: f64,
    pub var: f64,
}

impl LimitUtilization {
    /// Utilization from already-computed portfolio figures (`var_pct` is a fraction of equity).
    ///
    /// # Errors
    /// Returns [`Error::InvalidArgument`] if any limit is non-positive or non-finite.
    pub fn new(
        gross: f64,
        net: f64,
        leverage: f64,
        drawdown: f64,
        daily_loss: f64,
        var_pct: f64,
        limits: &RiskLimits,
    ) -> Result<Self> {
        Ok(Self {
            gross_exposure: utilization(gross, limits.max_gross_exposure)?,
            net_exposure: utilization(net.abs(), limits.max_net_exposure)?,
            leverage: utilization(leverage, limits.max_leverage)?,
            drawdown: utilization(drawdown, limits.max_drawdown_pct)?,
            daily_loss: utilization(daily_loss, limits.max_daily_loss)?,
            var: utilization(var_pct, limits.max_var_pct)?,
        })
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
    ///
    /// # Errors
    /// Returns an error if risk calculation or portfolio VaR fails, or if configuration parameters are invalid.
    pub fn evaluate(
        limits: &RiskLimits,
        portfolio: &PortfolioSnapshot,
        order: &OrderCandidate,
        kill_switch_active: bool,
    ) -> Result<PreTradeResult> {
        if limits.max_var_pct <= 0.0 || !limits.max_var_pct.is_finite() {
            return Err(Error::InvalidArgument(format!(
                "RiskLimits max_var_pct must be positive and finite, got {}",
                limits.max_var_pct
            )));
        }

        if portfolio.equity <= 0.0 || !portfolio.equity.is_finite() {
            return Ok(Self::fail_closed(
                order,
                "portfolio equity must be positive; VaR and concentration are undefined",
            ));
        }

        let current_dd = portfolio.drawdown_pct()?;
        let current_loss = portfolio.daily_loss();
        let current_pos_qty = portfolio.positions.get(&order.symbol).map_or(0.0, |p| p.qty);
        let current_pos_val = (current_pos_qty * order.price).abs();
        let current_concentration = current_pos_val / portfolio.equity;
        let mut breaches = Vec::new();

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
                return Ok(PreTradeResult {
                    verdict: PreTradeVerdict::Rejected,
                    requested_qty: order.qty,
                    approved_qty: 0.0,
                    breaches,
                    projected_metrics: ProjectedMetrics {
                        gross_exposure: portfolio.gross_exposure(),
                        net_exposure: portfolio.net_exposure(),
                        leverage: portfolio.leverage(),
                        position_size: current_pos_val,
                        concentration_pct: current_concentration,
                        drawdown_pct: current_dd,
                        daily_loss: current_loss,
                        var_pct: portfolio.var_pct()?,
                    },
                });
            }

            // Order is closing: check if quantity exceeds existing position (which would flip/open new risk)
            let max_close_qty = current_pos_qty.abs();
            if order.qty > max_close_qty {
                if limits.size_reduction_enabled && max_close_qty > 0.0 {
                    let approved_qty = max_close_qty;
                    let mut projected_gross = 0.0;
                    let mut reduced_positions = portfolio.positions.clone();
                    reduced_positions.remove(&order.symbol);

                    for (sym, pos) in &portfolio.positions {
                        if sym != &order.symbol {
                            projected_gross += pos.value().abs();
                        }
                    }
                    let projected_leverage = projected_gross / portfolio.equity;

                    let reduced_portfolio = PortfolioSnapshot {
                        cash: portfolio.cash,
                        equity: portfolio.equity,
                        peak_equity: portfolio.peak_equity,
                        daily_start_equity: portfolio.daily_start_equity,
                        positions: reduced_positions,
                    };
                    let reduced_var = reduced_portfolio.var_pct()?;

                    return Ok(PreTradeResult {
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
                            drawdown_pct: current_dd,
                            daily_loss: current_loss,
                            var_pct: reduced_var,
                        },
                    });
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
                return Ok(PreTradeResult {
                    verdict: PreTradeVerdict::Rejected,
                    requested_qty: order.qty,
                    approved_qty: 0.0,
                    breaches,
                    projected_metrics: ProjectedMetrics {
                        gross_exposure: portfolio.gross_exposure(),
                        net_exposure: portfolio.net_exposure(),
                        leverage: portfolio.leverage(),
                        position_size: current_pos_val,
                        concentration_pct: current_concentration,
                        drawdown_pct: current_dd,
                        daily_loss: current_loss,
                        var_pct: portfolio.var_pct()?,
                    },
                });
            }

            // Order is fully within existing position: approved to flatten/reduce
            let signed_delta = match order.side {
                OrderSide::Buy => order.qty,
                OrderSide::Sell => -order.qty,
            };
            let projected_pos_qty = current_pos_qty + signed_delta;
            let projected_pos_val = (projected_pos_qty * order.price).abs();
            let mut projected_gross = 0.0;
            let mut projected_positions = portfolio.positions.clone();

            for (sym, pos) in &portfolio.positions {
                if sym == &order.symbol {
                    projected_gross += projected_pos_val;
                } else {
                    projected_gross += pos.value().abs();
                }
            }
            let existing_vol = portfolio.positions.get(&order.symbol).and_then(|p| p.volatility);
            if projected_pos_qty.abs() > 1e-8 {
                projected_positions.insert(
                    order.symbol.clone(),
                    PositionSnapshot { qty: projected_pos_qty, price: order.price, volatility: existing_vol },
                );
            } else {
                projected_positions.remove(&order.symbol);
            }
            let projected_leverage = projected_gross / portfolio.equity;
            let projected_concentration = projected_pos_val / portfolio.equity;

            let projected_portfolio = PortfolioSnapshot {
                cash: portfolio.cash,
                equity: portfolio.equity,
                peak_equity: portfolio.peak_equity,
                daily_start_equity: portfolio.daily_start_equity,
                positions: projected_positions,
            };
            let projected_var = projected_portfolio.var_pct()?;

            return Ok(PreTradeResult {
                verdict: PreTradeVerdict::Approved,
                requested_qty: order.qty,
                approved_qty: order.qty,
                breaches: Vec::new(),
                projected_metrics: ProjectedMetrics {
                    gross_exposure: projected_gross,
                    net_exposure: portfolio.net_exposure(),
                    leverage: projected_leverage,
                    position_size: projected_pos_val,
                    concentration_pct: projected_concentration,
                    drawdown_pct: current_dd,
                    daily_loss: current_loss,
                    var_pct: projected_var,
                },
            });
        }

        // 2. Drawdown & Daily Loss limits (Task 2: stored equity history)
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
            return Ok(PreTradeResult {
                verdict: PreTradeVerdict::Rejected,
                requested_qty: order.qty,
                approved_qty: 0.0,
                breaches,
                projected_metrics: ProjectedMetrics {
                    gross_exposure: portfolio.gross_exposure(),
                    net_exposure: portfolio.net_exposure(),
                    leverage: portfolio.leverage(),
                    position_size: current_pos_val,
                    concentration_pct: current_concentration,
                    drawdown_pct: current_dd,
                    daily_loss: current_loss,
                    var_pct: portfolio.var_pct()?,
                },
            });
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
        let mut projected_positions = portfolio.positions.clone();

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

        let existing_vol = portfolio.positions.get(&order.symbol).and_then(|p| p.volatility);
        if projected_pos_qty.abs() > 1e-8 {
            projected_positions.insert(
                order.symbol.clone(),
                PositionSnapshot { qty: projected_pos_qty, price: order.price, volatility: existing_vol },
            );
        } else {
            projected_positions.remove(&order.symbol);
        }

        let projected_leverage = projected_gross / portfolio.equity;
        let projected_concentration = projected_pos_val / portfolio.equity;

        let projected_portfolio = PortfolioSnapshot {
            cash: portfolio.cash,
            equity: portfolio.equity,
            peak_equity: portfolio.peak_equity,
            daily_start_equity: portfolio.daily_start_equity,
            positions: projected_positions,
        };
        let projected_var = match projected_portfolio.var_pct() {
            Ok(v) => v,
            Err(Error::Data(ref msg)) if msg.contains("has no volatility") => 0.0,
            Err(e) => return Err(e),
        };

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

        if projected_var > limits.max_var_pct {
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
            return Ok(PreTradeResult {
                verdict: PreTradeVerdict::Approved,
                requested_qty: order.qty,
                approved_qty: order.qty,
                breaches,
                projected_metrics,
            });
        }

        // 5. Size reduction policy
        if limits.size_reduction_enabled {
            let max_qty_for_pos =
                ((limits.max_position_size - (current_pos_qty * order.price).abs()) / order.price).max(0.0);
            let other_gross = projected_gross - projected_pos_val;
            let max_qty_for_gross = ((limits.max_gross_exposure - other_gross) / order.price).max(0.0);
            let max_qty_for_lev =
                (((limits.max_leverage * portfolio.equity) - other_gross) / order.price).max(0.0);
            let max_qty_for_var = if let Some(vol) = existing_vol {
                if vol <= 0.0 || !vol.is_finite() {
                    return Err(Error::Data(format!(
                        "position for {} has invalid volatility: {vol}",
                        order.symbol
                    )));
                }
                let var_factor = (vol * 2.3263) / (252.0_f64).sqrt();
                let max_gross_for_var = (limits.max_var_pct * portfolio.equity) / var_factor;
                ((max_gross_for_var - other_gross) / order.price).max(0.0)
            } else {
                f64::INFINITY
            };

            let permissible_qty =
                max_qty_for_pos.min(max_qty_for_gross).min(max_qty_for_lev).min(max_qty_for_var);

            if permissible_qty > 0.0 && permissible_qty < order.qty {
                return Ok(PreTradeResult {
                    verdict: PreTradeVerdict::Reduced,
                    requested_qty: order.qty,
                    approved_qty: (permissible_qty * 100.0).floor() / 100.0,
                    breaches,
                    projected_metrics,
                });
            }
        }

        Ok(PreTradeResult {
            verdict: PreTradeVerdict::Rejected,
            requested_qty: order.qty,
            approved_qty: 0.0,
            breaches,
            projected_metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approved_when_within_limits() {
        let limits = RiskLimits::default();
        let mut portfolio = PortfolioSnapshot::default();
        portfolio
            .positions
            .insert("AAPL".into(), PositionSnapshot { qty: 0.0, price: 150.0, volatility: Some(0.20) });
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 100.0, price: 150.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Approved);
        assert_eq!(res.approved_qty, 100.0);
        assert!(res.breaches.is_empty());
    }

    #[test]
    fn test_kill_switch_blocks_opening_orders() {
        let limits = RiskLimits::default();
        let portfolio = PortfolioSnapshot::default();
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true).unwrap();
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
        portfolio
            .positions
            .insert("AAPL".into(), PositionSnapshot { qty: 100.0, price: 150.0, volatility: Some(0.20) });

        // Sell 100 on existing long 100 is flattening -> Approved even under kill switch!
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Sell, qty: 100.0, price: 150.0 };
        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Approved);
        assert_eq!(res.approved_qty, 100.0);
        assert!(res.breaches.is_empty());

        // But Buy 10 on existing long 100 is opening more -> Rejected!
        let buy_order =
            OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let buy_res = PreTradeChecker::evaluate(&limits, &portfolio, &buy_order, true).unwrap();
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
        portfolio
            .positions
            .insert("AAPL".into(), PositionSnapshot { qty: 50.0, price: 150.0, volatility: Some(0.20) });

        // Sell 100 when holding 50 with size reduction -> Reduced to 50
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Sell, qty: 100.0, price: 150.0 };
        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, true).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Reduced);
        assert_eq!(res.approved_qty, 50.0);
        assert_eq!(res.breaches[0].code, "kill_switch_closing_only");
    }

    #[test]
    fn test_size_reduction_when_exceeding_position_limit() {
        let limits =
            RiskLimits { max_position_size: 15_000.0, size_reduction_enabled: true, ..RiskLimits::default() };
        let mut portfolio = PortfolioSnapshot::default();
        portfolio
            .positions
            .insert("TSLA".into(), PositionSnapshot { qty: 0.0, price: 200.0, volatility: Some(0.20) });
        let order = OrderCandidate {
            symbol: "TSLA".into(),
            side: OrderSide::Buy,
            qty: 100.0,
            price: 200.0, // 20,000 exceeds 15,000
        };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap();
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

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res.breaches[0].code, "drawdown_exceeded");
    }

    #[test]
    fn test_property_limit_arithmetic_invariants() {
        let limits = RiskLimits::default();
        let mut portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        portfolio
            .positions
            .insert("TEST".into(), PositionSnapshot { qty: 0.0, price: 100.0, volatility: Some(0.20) });

        // Property: any order that does not exceed position, concentration, gross, net, or leverage limits is Approved
        for price in [10.0, 50.0, 100.0, 250.0] {
            let max_qty_pos = limits.max_position_size / price;
            let max_qty_conc = (portfolio.equity * limits.max_concentration_pct) / price;
            let max_safe_qty = max_qty_pos.min(max_qty_conc);
            let safe_qty = max_safe_qty * 0.8;
            let order = OrderCandidate { symbol: "TEST".into(), side: OrderSide::Buy, qty: safe_qty, price };
            let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap();
            assert_eq!(res.verdict, PreTradeVerdict::Approved, "Safe order must be approved");
            assert_eq!(res.approved_qty, safe_qty);

            // Property: any order strictly greater than max_position_size with reduction disabled is Rejected
            let over_qty = max_qty_pos * 1.5;
            let over_order =
                OrderCandidate { symbol: "TEST".into(), side: OrderSide::Buy, qty: over_qty, price };
            let over_res = PreTradeChecker::evaluate(&limits, &portfolio, &over_order, false).unwrap();
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
        let mut portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        portfolio
            .positions
            .insert("AAPL".into(), PositionSnapshot { qty: 0.0, price: 200.0, volatility: Some(0.20) });

        // Buying $50,000 of stock generates ~0.6% 1-day 99% VaR, which breaches the 0.1% limit
        let order = OrderCandidate {
            symbol: "AAPL".into(),
            side: OrderSide::Buy,
            qty: 250.0,
            price: 200.0, // $50,000
        };

        let res = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert!(res.breaches.iter().any(|b| b.code == "var_limit_exceeded"));
    }

    #[test]
    fn test_multi_asset_copula_var_enforcement() {
        let limits = RiskLimits {
            max_var_pct: 0.02, // 2% VaR limit
            max_position_size: 1_000_000.0,
            max_gross_exposure: 1_000_000.0,
            ..RiskLimits::default()
        };
        let mut portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        portfolio
            .positions
            .insert("AAPL".into(), PositionSnapshot { qty: 200.0, price: 150.0, volatility: Some(0.20) });
        portfolio
            .positions
            .insert("MSFT".into(), PositionSnapshot { qty: 0.0, price: 300.0, volatility: Some(0.25) });

        // Buying another $60,000 of MSFT pushes to 2 assets
        let order = OrderCandidate { symbol: "MSFT".into(), side: OrderSide::Buy, qty: 200.0, price: 300.0 };

        let err = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert_eq!(
            err.to_string(),
            "multi-asset VaR requires a correlation matrix; none available for 2 assets"
        );
    }

    #[test]
    fn test_equity_non_positive_rejects_through_fail_closed() {
        let limits = RiskLimits::default();
        let portfolio_zero = PortfolioSnapshot {
            cash: 0.0,
            equity: 0.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };

        let res = PreTradeChecker::evaluate(&limits, &portfolio_zero, &order, false).unwrap();
        assert_eq!(res.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res.approved_qty, 0.0);
        assert_eq!(res.breaches[0].code, "system_fail_closed");
        assert!(res.breaches[0].message.contains("portfolio equity must be positive"));

        // Negative equity also rejects via fail_closed
        let portfolio_neg = PortfolioSnapshot {
            cash: -50_000.0,
            equity: -50_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        let res_neg = PreTradeChecker::evaluate(&limits, &portfolio_neg, &order, false).unwrap();
        assert_eq!(res_neg.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res_neg.approved_qty, 0.0);
        assert_eq!(res_neg.breaches[0].code, "system_fail_closed");

        // var_pct on non-positive equity returns Error::InvalidArgument
        let err = portfolio_zero.var_pct().unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert!(err.to_string().contains("portfolio equity must be positive"));
    }

    #[test]
    fn test_drawdown_non_positive_peak_returns_error() {
        let snap_zero = PortfolioSnapshot {
            cash: 0.0,
            equity: 0.0,
            peak_equity: 0.0,
            daily_start_equity: 0.0,
            positions: HashMap::new(),
        };
        let err = snap_zero.drawdown_pct().unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert!(err.to_string().contains("drawdown is undefined when peak_equity <= 0.0"));

        let snap_neg = PortfolioSnapshot {
            cash: -10.0,
            equity: -10.0,
            peak_equity: -5.0,
            daily_start_equity: -5.0,
            positions: HashMap::new(),
        };
        let err_neg = snap_neg.drawdown_pct().unwrap_err();
        assert!(matches!(err_neg, Error::InvalidArgument(_)));

        // PreTradeChecker::evaluate propagates this error
        let limits = RiskLimits::default();
        let portfolio = PortfolioSnapshot {
            cash: 100_000.0,
            equity: 100_000.0,
            peak_equity: 0.0,
            daily_start_equity: 100_000.0,
            positions: HashMap::new(),
        };
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let eval_err = PreTradeChecker::evaluate(&limits, &portfolio, &order, false).unwrap_err();
        assert!(matches!(eval_err, Error::InvalidArgument(_)));
    }

    #[test]
    fn test_var_pct_with_non_positive_position_price_returns_error() {
        let mut positions = HashMap::new();
        positions
            .insert("ZERO_P".to_string(), PositionSnapshot { qty: 10.0, price: 0.0, volatility: Some(0.20) });
        let snap = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions,
        };
        let err = snap.var_pct().unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert!(err.to_string().contains("has non-positive or non-finite price"));

        let mut neg_positions = HashMap::new();
        neg_positions.insert(
            "NEG_P".to_string(),
            PositionSnapshot { qty: 10.0, price: -150.0, volatility: Some(0.20) },
        );
        let snap_neg = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions: neg_positions,
        };
        let err_neg = snap_neg.var_pct().unwrap_err();
        assert!(matches!(err_neg, Error::InvalidArgument(_)));
    }

    #[test]
    fn test_utilization_non_positive_limit_returns_error() {
        assert!(utilization(100.0, 0.0).is_err());
        let err_zero = utilization(100.0, 0.0).unwrap_err();
        assert!(matches!(err_zero, Error::InvalidArgument(_)));
        assert!(err_zero.to_string().contains("limit must be positive and finite"));

        assert!(utilization(100.0, -10.0).is_err());
        let err_neg = utilization(100.0, -10.0).unwrap_err();
        assert!(matches!(err_neg, Error::InvalidArgument(_)));

        // LimitUtilization::new also fails if any limit is non-positive
        let limits_bad = RiskLimits { max_leverage: 0.0, ..RiskLimits::default() };
        let u_err = LimitUtilization::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, &limits_bad).unwrap_err();
        assert!(matches!(u_err, Error::InvalidArgument(_)));
    }

    #[test]
    fn test_evaluate_rejects_non_positive_max_var_pct() {
        let limits_zero = RiskLimits { max_var_pct: 0.0, ..RiskLimits::default() };
        let portfolio = PortfolioSnapshot::default();
        let order = OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let err_zero = PreTradeChecker::evaluate(&limits_zero, &portfolio, &order, false).unwrap_err();
        assert!(matches!(err_zero, Error::InvalidArgument(_)));
        assert!(err_zero.to_string().contains("RiskLimits max_var_pct must be positive"));

        let limits_neg = RiskLimits { max_var_pct: -0.05, ..RiskLimits::default() };
        let err_neg = PreTradeChecker::evaluate(&limits_neg, &portfolio, &order, false).unwrap_err();
        assert!(matches!(err_neg, Error::InvalidArgument(_)));
    }

    #[test]
    fn test_var_pct_simulation_failure_propagates_error() {
        let mut positions = HashMap::new();
        positions.insert(
            "AAPL".to_string(),
            PositionSnapshot { qty: f64::INFINITY, price: 150.0, volatility: Some(0.20) },
        );
        let snap = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions,
        };
        let err = snap.var_pct().unwrap_err();
        assert!(matches!(err, Error::Data(_)));
        assert!(err.to_string().contains("var_pct simulation failed"));
    }

    #[test]
    fn test_multi_asset_var_pct_without_correlation_matrix_returns_error() {
        let mut positions = HashMap::new();
        positions.insert(
            "AAPL".to_string(),
            PositionSnapshot { qty: 100.0, price: 150.0, volatility: Some(0.20) },
        );
        positions
            .insert("MSFT".to_string(), PositionSnapshot { qty: 50.0, price: 300.0, volatility: Some(0.25) });
        let snap = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions,
        };
        let err = snap.var_pct().unwrap_err();
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert_eq!(
            err.to_string(),
            "multi-asset VaR requires a correlation matrix; none available for 2 assets"
        );
    }

    #[test]
    fn test_placeholder_zeros_replaced_with_real_portfolio_metrics() {
        let limits = RiskLimits::default();
        let mut positions = HashMap::new();
        positions.insert(
            "AAPL".to_string(),
            PositionSnapshot { qty: 100.0, price: 150.0, volatility: Some(0.20) },
        );
        let portfolio = PortfolioSnapshot {
            cash: 85_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions,
        };

        // 1. Kill-switch rejection on opening order computes real concentration and real VaR (not 0.0)
        let buy_order =
            OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let res_ks = PreTradeChecker::evaluate(&limits, &portfolio, &buy_order, true).unwrap();
        assert_eq!(res_ks.verdict, PreTradeVerdict::Rejected);
        assert!(
            (res_ks.projected_metrics.concentration_pct - 0.15).abs() < 1e-6,
            "concentration must be 15,000 / 100,000 = 0.15, got {}",
            res_ks.projected_metrics.concentration_pct
        );
        assert!(
            res_ks.projected_metrics.var_pct > 0.0,
            "VaR must be positive real simulation, got {}",
            res_ks.projected_metrics.var_pct
        );

        // 2. Kill-switch size reduction computes real VaR for remaining holdings
        let limits_reduce = RiskLimits { size_reduction_enabled: true, ..RiskLimits::default() };
        let sell_oversize =
            OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Sell, qty: 150.0, price: 150.0 };
        let res_reduce = PreTradeChecker::evaluate(&limits_reduce, &portfolio, &sell_oversize, true).unwrap();
        assert_eq!(res_reduce.verdict, PreTradeVerdict::Reduced);
        assert_eq!(res_reduce.approved_qty, 100.0);
        // Portfolio is flattened, so VaR is 0.0 (empty portfolio has 0.0 VaR mathematically)
        assert_eq!(res_reduce.projected_metrics.var_pct, 0.0);

        // 3. Drawdown rejection computes real concentration and VaR
        let limits_dd = RiskLimits { max_drawdown_pct: 0.05, ..RiskLimits::default() };
        let mut dd_positions = HashMap::new();
        dd_positions.insert(
            "AAPL".to_string(),
            PositionSnapshot { qty: 100.0, price: 150.0, volatility: Some(0.20) },
        );
        let portfolio_dd = PortfolioSnapshot {
            cash: 80_000.0,
            equity: 90_000.0,
            peak_equity: 100_000.0, // 10% drawdown > 5% limit
            daily_start_equity: 95_000.0,
            positions: dd_positions,
        };
        let dd_order =
            OrderCandidate { symbol: "AAPL".into(), side: OrderSide::Buy, qty: 10.0, price: 150.0 };
        let res_dd = PreTradeChecker::evaluate(&limits_dd, &portfolio_dd, &dd_order, false).unwrap();
        assert_eq!(res_dd.verdict, PreTradeVerdict::Rejected);
        assert_eq!(res_dd.breaches[0].code, "drawdown_exceeded");
        assert!(
            (res_dd.projected_metrics.concentration_pct - (15_000.0 / 90_000.0)).abs() < 1e-6,
            "concentration must be 15,000 / 90,000, got {}",
            res_dd.projected_metrics.concentration_pct
        );
        assert!(res_dd.projected_metrics.var_pct > 0.0);
    }

    #[test]
    fn test_enforcement_t_copula_dof_constant_matches_default() {
        assert_eq!(ENFORCEMENT_T_COPULA_DOF, 4);
        let mut positions = HashMap::new();
        positions.insert(
            "AAPL".to_string(),
            PositionSnapshot { qty: 100.0, price: 150.0, volatility: Some(0.20) },
        );
        let snap = PortfolioSnapshot {
            cash: 50_000.0,
            equity: 100_000.0,
            peak_equity: 100_000.0,
            daily_start_equity: 100_000.0,
            positions,
        };
        let var_snap = snap.var_pct().unwrap();
        let var_explicit = snap
            .var_pct_with_model(crate::copula::DependenceModel::TCopula {
                degrees_of_freedom: ENFORCEMENT_T_COPULA_DOF,
            })
            .unwrap();
        assert_eq!(var_snap, var_explicit);
    }
}
