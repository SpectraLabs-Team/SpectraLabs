//! Cross-market options Greeks and expiry settlement (M24-advanced Task 17, 18).
//!
//! Handles options capability verification across venues (IBKR: Yes, Kite: Yes, Binance USDⓈ-M: No),
//! options Greeks calculation (Delta, Gamma, Vega, Theta, Rho) via Black-Scholes-Merton,
//! and cash-settled vs physical delivery settlement at expiry.

use serde::{Deserialize, Serialize};
use spectra_core::OptionRight;
use spectra_engine::options::{OptionInputs, OptionValue};

use super::symbols::VenueId;

/// Options capabilities per execution venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueOptionsSupport {
    pub venue: VenueId,
    pub supports_options: bool,
    pub settlement_style: SettlementStyle,
    pub supports_greeks: bool,
    pub notes: &'static str,
}

/// Settlement style for expiring options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStyle {
    /// Cash-settled against index or reference price (CME index options, European style).
    CashSettled,
    /// Physical delivery / exercise into underlying shares or contracts (US equity options).
    PhysicalDelivery,
    /// Not supported on venue.
    Unsupported,
}

/// Returns the options capability specification for a given venue (Task 17).
#[must_use]
pub fn venue_options_support(venue: VenueId) -> VenueOptionsSupport {
    match venue {
        VenueId::InteractiveBrokers => VenueOptionsSupport {
            venue,
            supports_options: true,
            settlement_style: SettlementStyle::CashSettled, // Index / CME cash-settled; stock options physical
            supports_greeks: true,
            notes: "Full US & European equity and index options supported via Client Portal / TWS API.",
        },
        VenueId::ZerodhaKite => VenueOptionsSupport {
            venue,
            supports_options: true,
            settlement_style: SettlementStyle::CashSettled, // Nifty/BankNifty index options are cash-settled
            supports_greeks: true,
            notes: "NSE European index (NIFTY/BANKNIFTY) and stock options supported via Kite Connect.",
        },
        VenueId::BinanceUsdm => VenueOptionsSupport {
            venue,
            supports_options: false,
            settlement_style: SettlementStyle::Unsupported,
            supports_greeks: false,
            notes: "Binance USDⓈ-M only supports futures/perpetuals; Binance Options has a distinct API.",
        },
        VenueId::PaperSimulated => VenueOptionsSupport {
            venue,
            supports_options: true,
            settlement_style: SettlementStyle::CashSettled,
            supports_greeks: true,
            notes: "Simulated paper execution engine with Black-Scholes-Merton cash settlement.",
        },
    }
}

/// Computed Greeks and theoretical price for an option position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PositionGreeks {
    pub theoretical_price: f64,
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
    pub position_delta: f64,
    pub position_gamma: f64,
}

/// Computes the Black-Scholes-Merton Greeks and aggregate position exposure (Task 18).
pub fn compute_option_greeks(
    right: OptionRight,
    spot: f64,
    strike: f64,
    years_to_expiry: f64,
    rate: f64,
    dividend_yield: f64,
    volatility: f64,
    contracts: f64,
    multiplier: f64,
) -> Result<PositionGreeks, String> {
    let inputs = OptionInputs {
        right,
        spot,
        strike,
        years: years_to_expiry.max(0.0),
        rate,
        dividend_yield,
        volatility: volatility.max(0.0001),
    };

    let value: OptionValue = inputs.value().map_err(|e| format!("Greeks calculation error: {e}"))?;

    // Position delta = delta * contracts * multiplier
    let position_delta = value.delta * contracts * multiplier;
    let position_gamma = value.gamma * contracts * multiplier;

    Ok(PositionGreeks {
        theoretical_price: value.price,
        delta: value.delta,
        gamma: value.gamma,
        vega: value.vega,
        theta: value.theta,
        rho: value.rho,
        position_delta,
        position_gamma,
    })
}

/// Settlement result at option contract expiration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpirySettlement {
    pub symbol: String,
    pub is_in_the_money: bool,
    pub intrinsic_per_share: f64,
    pub total_cash_settlement: f64,
    pub delivery_required: bool,
}

/// Evaluates settlement payoff at option expiry.
#[must_use]
pub fn evaluate_expiry_settlement(
    symbol: &str,
    right: OptionRight,
    strike: f64,
    settlement_spot: f64,
    qty: f64,
    multiplier: f64,
    settlement_style: SettlementStyle,
) -> ExpirySettlement {
    let intrinsic = right.intrinsic(settlement_spot, strike);
    let is_itm = intrinsic > 0.0;
    let total_cash_settlement = if is_itm && settlement_style == SettlementStyle::CashSettled {
        qty * intrinsic * multiplier
    } else {
        0.0
    };

    let delivery_required = is_itm && settlement_style == SettlementStyle::PhysicalDelivery;

    ExpirySettlement {
        symbol: symbol.to_string(),
        is_in_the_money: is_itm,
        intrinsic_per_share: intrinsic,
        total_cash_settlement,
        delivery_required,
    }
}
