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

/// Commission, slippage, bid-ask spread and volume-based slippage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostModel {
    /// Commission per fill.
    pub commission: CommissionModel,
    /// Adverse price move applied to market orders, in basis points.
    pub slippage: Bps,
    /// Bid-ask spread in basis points (half of this spread is paid as crossing cost on market/stop fills).
    pub spread: Bps,
    /// Volume-based slippage in basis points, scaled by bar volume participation (qty / bar volume).
    pub volume_slippage: Bps,
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

/// Calculates the total slippage and spread basis points for a market or stop fill.
///
/// Combines flat slippage, half the bid-ask spread, participation-based volume slippage
/// (`volume_slippage * min(1.0, qty / bar_volume)`), and any additional market impact basis points.
///
/// # Errors
/// Arithmetic overflow.
pub fn effective_slippage_bps(
    model: &CostModel,
    qty: Qty,
    bar_volume: Qty,
    extra_impact_bps: f64,
) -> Result<Bps, ArithmeticError> {
    let mut bps = model.slippage;
    if model.spread.raw() > 0 {
        let half = Bps::from_raw(model.spread.raw() / 2);
        bps = bps.checked_add(half)?;
    }
    if model.volume_slippage.raw() > 0 && bar_volume.raw() > 0 {
        let part = (qty.to_f64() / bar_volume.to_f64()).min(1.0);
        let vol_bps = model.volume_slippage.to_f64() * part;
        bps = bps.checked_add(Bps::from_f64(vol_bps)?)?;
    }
    if extra_impact_bps > 0.0 {
        bps = bps.checked_add(Bps::from_f64(extra_impact_bps)?)?;
    }
    Ok(bps)
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

    #[test]
    fn effective_slippage_incorporates_spread_and_volume_participation() {
        let model = CostModel {
            commission: CommissionModel::default(),
            slippage: bps(2.0),
            spread: bps(4.0),           // half-spread = 2 bps
            volume_slippage: bps(10.0), // at 50% participation = 5 bps
        };
        // 50% participation: 2 (flat) + 2 (spread) + 5 (vol) = 9 bps
        let eff = effective_slippage_bps(&model, q(50.0), q(100.0), 0.0).unwrap();
        assert_eq!(eff, bps(9.0));

        // With extra 3 bps impact: 9 + 3 = 12 bps
        let with_impact = effective_slippage_bps(&model, q(50.0), q(100.0), 3.0).unwrap();
        assert_eq!(with_impact, bps(12.0));

        // When volume is zero, volume slippage is skipped safely
        let zero_vol = effective_slippage_bps(&model, q(50.0), q(0.0), 0.0).unwrap();
        assert_eq!(zero_vol, bps(4.0));
    }
}
