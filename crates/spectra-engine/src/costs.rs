//! Transaction cost models: commission and slippage.

use spectra_core::fixed::{div_round_half_away, narrow};
use spectra_core::{notional, ArithmeticError, Bps, Money, Price, Qty, Side, SCALE};

const BPS_DIVISOR: i128 = SCALE as i128 * 10_000; // raw bps -> fraction

/// Commission charged on each fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommissionModel {
    /// Fee per unit of quantity traded.
    pub per_unit: Money,
    /// Fee as basis points of notional.
    pub bps_of_notional: Bps,
    /// Floor applied to the total fee of a fill.
    pub min_fee: Money,
}

/// Commission plus slippage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostModel {
    /// Commission per fill.
    pub commission: CommissionModel,
    /// Adverse price move applied to market orders, in basis points.
    pub slippage: Bps,
}

/// `max(min_fee, per_unit * qty + notional * bps / 10,000)`, rounded half away from zero.
///
/// # Errors
/// Overflow.
pub fn commission_fee(
    model: &CommissionModel,
    qty: Qty,
    notional_value: Money,
) -> Result<Money, ArithmeticError> {
    let unit_fee = notional(model.per_unit.cast::<spectra_core::fixed::PriceUnit>(), qty)?;
    let bps_raw = i128::from(notional_value.raw()) * i128::from(model.bps_of_notional.raw());
    let bps_fee = Money::from_raw(narrow(div_round_half_away(bps_raw, BPS_DIVISOR))?);
    Ok(model.min_fee.max(unit_fee.checked_add(bps_fee)?))
}

/// Buys pay more, sells receive less. The result is always at least one raw unit.
///
/// # Errors
/// Overflow.
pub fn slipped_price(px: Price, side: Side, slippage: Bps) -> Result<Price, ArithmeticError> {
    let delta_raw = i128::from(px.raw()) * i128::from(slippage.raw());
    let delta = Price::from_raw(narrow(div_round_half_away(delta_raw, BPS_DIVISOR))?);
    let moved = if side == Side::Buy { px.checked_add(delta)? } else { px.checked_sub(delta)? };
    Ok(moved.max(Price::from_raw(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f64) -> Money {
        Money::from_f64(v).unwrap()
    }
    fn p(v: f64) -> Price {
        Price::from_f64(v).unwrap()
    }
    fn q(v: f64) -> Qty {
        Qty::from_f64(v).unwrap()
    }
    fn bps(v: f64) -> Bps {
        Bps::from_f64(v).unwrap()
    }

    #[test]
    fn slippage_moves_price_against_the_trader() {
        assert_eq!(slipped_price(p(100.0), Side::Buy, bps(10.0)).unwrap(), p(100.1));
        assert_eq!(slipped_price(p(100.0), Side::Sell, bps(10.0)).unwrap(), p(99.9));
        assert_eq!(slipped_price(p(100.0), Side::Buy, bps(0.0)).unwrap(), p(100.0));
        assert_eq!(slipped_price(p(100.0), Side::Sell, bps(0.5)).unwrap(), p(99.995));
    }

    #[test]
    fn slippage_never_produces_non_positive_price() {
        assert_eq!(slipped_price(p(100.0), Side::Sell, bps(20_000.0)).unwrap().raw(), 1);
    }

    #[test]
    fn commission_combines_unit_and_bps_fees_and_respects_the_floor() {
        let mut c = CommissionModel { per_unit: m(0.01), bps_of_notional: bps(5.0), min_fee: Money::ZERO };
        assert_eq!(commission_fee(&c, q(100.0), m(10_000.0)).unwrap(), m(6.0));
        c.min_fee = m(2.0);
        assert_eq!(commission_fee(&c, q(1.0), m(100.0)).unwrap(), m(2.0));
        assert_eq!(commission_fee(&c, q(100.0), m(10_000.0)).unwrap(), m(6.0));
        assert!(commission_fee(&CommissionModel::default(), q(100.0), m(10_000.0)).unwrap().is_zero());
    }
}
