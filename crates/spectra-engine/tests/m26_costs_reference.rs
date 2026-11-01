//! M26 task 3f (part 2, costs): reference-value tests for the commission and
//! slippage models (`src/costs.rs`) through the public API.
//!
//! Expected values are the closed forms in `docs/models/engine-models.md`
//! (commission = max(min fee, unit fee * qty + bps * notional); buys pay more,
//! sells receive less; effective bps = flat + half spread + volume term + impact),
//! worked by hand. Money, price and quantity are fixed point, so the comparisons
//! are exact.
//!
//! Zero bar volume is an error when volume slippage is active (task 6). Futures
//! roll and rebalance follow in part 3.

use spectra_core::{Bps, Money, Price, Qty, Side};
use spectra_engine::costs::{
    commission_fee, effective_slippage_bps, slipped_price, CommissionModel, CostModel,
};

fn m(v: f64) -> Money {
    Money::from_f64(v).unwrap()
}
fn p(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}
fn q(v: f64) -> Qty {
    Qty::from_f64(v).unwrap()
}
fn b(v: f64) -> Bps {
    Bps::from_f64(v).unwrap()
}

#[test]
fn commission_is_unit_fee_plus_bps_of_notional() {
    // 100 units at 0.01 = 1.00; 5 bps of 10,000 = 5.00; total 6.00.
    let c = CommissionModel { per_unit: m(0.01), bps_of_notional: b(5.0), min_fee: Money::ZERO };
    assert_eq!(commission_fee(&c, q(100.0), m(10_000.0)).unwrap(), m(6.0));
}

#[test]
fn commission_never_falls_below_the_minimum_fee() {
    // The formula gives 6.00; the minimum of 10.00 applies.
    let c = CommissionModel { per_unit: m(0.01), bps_of_notional: b(5.0), min_fee: m(10.0) };
    assert_eq!(commission_fee(&c, q(100.0), m(10_000.0)).unwrap(), m(10.0));
    // The formula above the minimum is kept: 500 units give 5.00 + 5.00 = 10.00 exactly.
    assert_eq!(commission_fee(&c, q(500.0), m(10_000.0)).unwrap(), m(10.0));
    assert_eq!(commission_fee(&c, q(1_000.0), m(10_000.0)).unwrap(), m(15.0));
}

#[test]
fn buys_pay_more_and_sells_receive_less() {
    // 10 bps on 100 is 0.10.
    assert_eq!(slipped_price(p(100.0), Side::Buy, b(10.0)).unwrap(), p(100.1));
    assert_eq!(slipped_price(p(100.0), Side::Sell, b(10.0)).unwrap(), p(99.9));
    // Zero slippage leaves the price unchanged on either side.
    assert_eq!(slipped_price(p(100.0), Side::Buy, b(0.0)).unwrap(), p(100.0));
    assert_eq!(slipped_price(p(100.0), Side::Sell, b(0.0)).unwrap(), p(100.0));
}

fn cost_model(slippage: f64, spread: f64, volume: f64) -> CostModel {
    CostModel {
        commission: CommissionModel::default(),
        slippage: b(slippage),
        spread: b(spread),
        volume_slippage: b(volume),
    }
}

#[test]
fn flat_slippage_and_half_the_spread_add_up() {
    // 2 bps flat + half of 10 bps = 7 bps.
    let bps = effective_slippage_bps(&cost_model(2.0, 10.0, 0.0), q(10.0), q(1_000.0), 0.0).unwrap();
    assert_eq!(bps, b(7.0));
}

#[test]
fn the_volume_term_scales_with_participation_and_is_capped_at_full() {
    let model = cost_model(2.0, 10.0, 10.0);
    // Participation 50 / 100 = 0.5, so the volume term is 5 bps: 2 + 5 + 5 = 12.
    assert_eq!(effective_slippage_bps(&model, q(50.0), q(100.0), 0.0).unwrap(), b(12.0));
    // Participation above 1 is capped at 1, so the term is 10 bps: 2 + 5 + 10 = 17.
    assert_eq!(effective_slippage_bps(&model, q(300.0), q(100.0), 0.0).unwrap(), b(17.0));
}

#[test]
fn extra_market_impact_is_added_on_top() {
    // 2 + 5 + 5 (volume at half participation) + 1.5 extra impact = 13.5 bps.
    let model = cost_model(2.0, 10.0, 10.0);
    assert_eq!(effective_slippage_bps(&model, q(50.0), q(100.0), 1.5).unwrap(), b(13.5));
}

#[test]
fn zero_bar_volume_returns_error_when_volume_slippage_is_configured() {
    let model = cost_model(2.0, 10.0, 10.0);
    let res = effective_slippage_bps(&model, q(50.0), q(0.0), 0.0);
    assert_eq!(res, Err(spectra_core::ArithmeticError::DivisionByZero));
}
