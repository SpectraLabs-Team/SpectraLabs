//! Multi-venue broker execution adapters and shared foundation (M24-advanced).
//!
//! Provides the unified `BrokerAdapter` trait for futures and options orders,
//! venue symbol mapping, paper-execution reconciliation, pre-trade margin/leverage checks,
//! and live/paper execution mode gating.

#![allow(
    clippy::missing_errors_doc,
    clippy::unreadable_literal,
    clippy::implicit_hasher,
    clippy::manual_let_else,
    clippy::redundant_closure_for_method_calls,
    clippy::if_same_then_else,
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::too_many_arguments
)]

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use spectra_risk::enforcement::{OrderCandidate, PortfolioSnapshot, PositionSnapshot, RiskLimits};

pub mod binance;
pub mod ibkr;
pub mod kite;
pub mod options;
pub mod symbols;

#[cfg(test)]
pub mod tests;

pub use binance::{compute_funding_payment, BinanceUsdmAdapter, FundingPayment};
pub use ibkr::{check_futures_roll_due, InteractiveBrokersAdapter};
pub use kite::{compute_indian_fo_taxes, IndianTaxBreakdown, ZerodhaKiteAdapter};
pub use options::{
    compute_option_greeks, evaluate_expiry_settlement, venue_options_support, PositionGreeks, SettlementStyle,
};
pub use symbols::{map_venue_symbol_to_instrument, SymbolMapError, VenueId};

/// Order direction for venue orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl From<spectra_risk::enforcement::OrderSide> for OrderSide {
    fn from(s: spectra_risk::enforcement::OrderSide) -> Self {
        match s {
            spectra_risk::enforcement::OrderSide::Buy => Self::Buy,
            spectra_risk::enforcement::OrderSide::Sell => Self::Sell,
        }
    }
}

impl From<OrderSide> for spectra_risk::enforcement::OrderSide {
    fn from(s: OrderSide) -> Self {
        match s {
            OrderSide::Buy => Self::Buy,
            OrderSide::Sell => Self::Sell,
        }
    }
}

/// Execution mode for trading (Task 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Paper,
    Live,
}

impl ExecutionMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "paper" | "sandbox" | "simulated" => Some(Self::Paper),
            "live" | "production" | "real" => Some(Self::Live),
            _ => None,
        }
    }

    /// Reads execution mode from environment variable `SPECTRA_EXECUTION_MODE`.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var("SPECTRA_EXECUTION_MODE").ok().and_then(|v| Self::parse(&v)).unwrap_or(Self::Paper)
    }
}

/// Gating check: fails closed if live trading is attempted in paper mode (Task 5).
pub fn verify_execution_mode_allowed(
    configured_mode: ExecutionMode,
    is_live_order: bool,
) -> Result<(), BrokerError> {
    if is_live_order && configured_mode == ExecutionMode::Paper {
        return Err(BrokerError::LiveTradingDisabled(
            "Live venue order rejected: system is configured in paper execution mode. Set SPECTRA_EXECUTION_MODE=live to enable."
                .to_string(),
        ));
    }
    Ok(())
}

/// Order request payload to a venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueOrderRequest {
    pub symbol: String,
    pub side: OrderSide,
    pub qty: f64,
    pub order_type: String,
    pub price: Option<f64>,
    pub client_order_id: Option<String>,
    pub conid: Option<i64>,
    pub sec_type: Option<String>,
}

/// Response returned by a venue upon order submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueOrderResponse {
    pub venue_order_id: String,
    pub client_order_id: String,
    pub status: String,
    pub filled_qty: f64,
    pub avg_fill_price: f64,
    pub raw_response: serde_json::Value,
}

/// Response returned upon order cancellation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderResponse {
    pub venue_order_id: String,
    pub status: String,
}

/// Position held at a venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenuePosition {
    pub symbol: String,
    pub qty: f64,
    pub entry_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
    pub initial_margin: f64,
    pub maintenance_margin: f64,
}

/// Account collateral and margin summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueAccountSummary {
    pub equity: f64,
    pub available_cash: f64,
    pub initial_margin: f64,
    pub maintenance_margin: f64,
    pub unrealized_pnl: f64,
    pub currency: String,
}

/// Margin requirement for an order or position (Task 4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarginRequirement {
    pub initial_margin: f64,
    pub maintenance_margin: f64,
    pub margin_rate: f64,
}

/// Errors occurring in broker adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerError {
    Authentication(String),
    Network(String),
    OrderRejected(String),
    MalformedResponse(String),
    LiveTradingDisabled(String),
    MarginExceeded(String),
    LeverageExceeded(String),
    PositionLimitExceeded(String),
    SymbolMapping(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authentication(e) => write!(f, "broker authentication failure: {e}"),
            Self::Network(e) => write!(f, "broker network error: {e}"),
            Self::OrderRejected(e) => write!(f, "order rejected by broker: {e}"),
            Self::MalformedResponse(e) => write!(f, "malformed broker response: {e}"),
            Self::LiveTradingDisabled(e) => write!(f, "live execution disabled: {e}"),
            Self::MarginExceeded(e) => write!(f, "margin limit exceeded: {e}"),
            Self::LeverageExceeded(e) => write!(f, "leverage limit exceeded: {e}"),
            Self::PositionLimitExceeded(e) => write!(f, "position limit exceeded: {e}"),
            Self::SymbolMapping(e) => write!(f, "symbol mapping error: {e}"),
        }
    }
}

impl std::error::Error for BrokerError {}

/// Unified BrokerAdapter trait for futures and options orders (Task 1).
pub trait BrokerAdapter: Send + Sync {
    /// Identifier of the venue.
    fn venue_id(&self) -> VenueId;

    /// Submits a futures or options order.
    fn submit_order(
        &self,
        req: &VenueOrderRequest,
    ) -> impl std::future::Future<Output = Result<VenueOrderResponse, BrokerError>> + Send;

    /// Cancels an open order.
    fn cancel_order(
        &self,
        venue_order_id: &str,
    ) -> impl std::future::Future<Output = Result<CancelOrderResponse, BrokerError>> + Send;

    /// Fetches all active positions from the venue.
    fn get_positions(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<VenuePosition>, BrokerError>> + Send;

    /// Fetches account balances, margins, and equity summary.
    fn get_account_summary(
        &self,
    ) -> impl std::future::Future<Output = Result<VenueAccountSummary, BrokerError>> + Send;

    /// Calculates margin requirement for an order candidate.
    fn get_margin_requirements(
        &self,
        symbol: &str,
        qty: f64,
        side: OrderSide,
    ) -> impl std::future::Future<Output = Result<MarginRequirement, BrokerError>> + Send;
}

/// Pre-trade margin, leverage, and position checks for derivatives (Task 4).
pub fn evaluate_derivatives_risk(
    limits: &RiskLimits,
    portfolio: &PortfolioSnapshot,
    candidate: &OrderCandidate,
    margin_req: &MarginRequirement,
) -> Result<(), BrokerError> {
    // 1. Initial Margin Check: Free available cash must cover the initial margin requirement
    if margin_req.initial_margin > portfolio.cash {
        return Err(BrokerError::MarginExceeded(format!(
            "Insufficient margin: required initial margin {:.2} exceeds available cash {:.2}",
            margin_req.initial_margin, portfolio.cash
        )));
    }

    // 2. Gross Exposure and Leverage Check
    let mut projected_gross = 0.0;
    for (sym, pos) in &portfolio.positions {
        if sym == &candidate.symbol {
            let delta = match candidate.side {
                spectra_risk::enforcement::OrderSide::Buy => candidate.qty,
                spectra_risk::enforcement::OrderSide::Sell => -candidate.qty,
            };
            projected_gross += (pos.qty + delta).abs() * pos.price;
        } else {
            projected_gross += pos.value().abs();
        }
    }
    if !portfolio.positions.contains_key(&candidate.symbol) {
        projected_gross += candidate.notional().abs();
    }

    let projected_leverage =
        if portfolio.equity > 0.0 { projected_gross / portfolio.equity } else { f64::INFINITY };

    if projected_leverage > limits.max_leverage {
        return Err(BrokerError::LeverageExceeded(format!(
            "Projected gross leverage {:.2} exceeds limit {:.2}",
            projected_leverage, limits.max_leverage
        )));
    }

    // 3. Position Size Limit Check
    let current_pos_val = portfolio.positions.get(&candidate.symbol).map_or(0.0, |p| p.value().abs());
    let projected_pos_val = current_pos_val + candidate.notional().abs();
    if projected_pos_val > limits.max_position_size {
        return Err(BrokerError::PositionLimitExceeded(format!(
            "Projected position size {:.2} for {} exceeds limit {:.2}",
            projected_pos_val, candidate.symbol, limits.max_position_size
        )));
    }

    Ok(())
}

/// Reconciliation report between local paper execution state and venue (Task 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionReconciliationReport {
    pub matched_positions: usize,
    pub discrepancies: Vec<PositionDiscrepancy>,
    pub local_total_equity: f64,
    pub venue_total_equity: f64,
    pub equity_diff: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionDiscrepancy {
    pub symbol: String,
    pub local_qty: f64,
    pub venue_qty: f64,
    pub qty_diff: f64,
    pub local_price: f64,
    pub venue_price: f64,
}

/// Reconciles local positions against external venue positions (Task 3).
#[must_use]
pub fn reconcile_positions_and_fills(
    local_positions: &HashMap<String, PositionSnapshot>,
    venue_positions: &[VenuePosition],
    local_equity: f64,
    venue_equity: f64,
) -> PositionReconciliationReport {
    let mut discrepancies = Vec::new();
    let mut matched = 0;

    let venue_map: HashMap<String, &VenuePosition> =
        venue_positions.iter().map(|vp| (vp.symbol.clone(), vp)).collect();

    for (sym, local_pos) in local_positions {
        if let Some(venue_pos) = venue_map.get(sym) {
            let qty_diff = (local_pos.qty - venue_pos.qty).abs();
            if qty_diff > 1e-5 {
                discrepancies.push(PositionDiscrepancy {
                    symbol: sym.clone(),
                    local_qty: local_pos.qty,
                    venue_qty: venue_pos.qty,
                    qty_diff,
                    local_price: local_pos.price,
                    venue_price: venue_pos.mark_price,
                });
            } else {
                matched += 1;
            }
        } else if local_pos.qty.abs() > 1e-5 {
            discrepancies.push(PositionDiscrepancy {
                symbol: sym.clone(),
                local_qty: local_pos.qty,
                venue_qty: 0.0,
                qty_diff: local_pos.qty.abs(),
                local_price: local_pos.price,
                venue_price: 0.0,
            });
        }
    }

    for vp in venue_positions {
        if !local_positions.contains_key(&vp.symbol) && vp.qty.abs() > 1e-5 {
            discrepancies.push(PositionDiscrepancy {
                symbol: vp.symbol.clone(),
                local_qty: 0.0,
                venue_qty: vp.qty,
                qty_diff: vp.qty.abs(),
                local_price: 0.0,
                venue_price: vp.mark_price,
            });
        }
    }

    PositionReconciliationReport {
        matched_positions: matched,
        discrepancies,
        local_total_equity: local_equity,
        venue_total_equity: venue_equity,
        equity_diff: (local_equity - venue_equity).abs(),
    }
}
