//! M26 task 3a: reference-value tests for the bond, swap and curve models
//! (`src/bonds.rs`, `src/curves.rs`). Reference document: `docs/models/engine-models.md`.
//!
//! Reference values were computed independently at 40 significant digits
//! (mpmath) from the closed forms: flat-curve discount factors, the simple
//! deposit and par-swap bootstrap, the textbook bond price and yield formulas,
//! and the swap PV = N (1 - DF(T) - K * annuity). Risk figures (DV01) are
//! closed-form derivatives, checked against the model's ±1bp difference.

use spectra_core::DayCount;
use spectra_engine::bonds::{days_from_civil, BondTerms};
use spectra_engine::curves::{pv_flows, z_spread, Curve, CurveQuote, Swap};

const DAY_NS: i64 = 86_400_000_000_000;

fn ns(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d) * DAY_NS
}

fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (tol {tol})");
}

// ---- curves ----------------------------------------------------------------

#[test]
fn flat_curve_discount_factors_are_exponential() {
    let curve = Curve::flat(0.05, 3.0).unwrap();
    // Log-linear interpolation between integer nodes of a flat curve is exact.
    close(curve.df(0.5), 0.975_309_912_028_332_6, 1e-12, "df(0.5)");
    close(curve.df(1.5), 0.927_743_486_328_552_9, 1e-12, "df(1.5)");
    close(curve.df(2.0), 0.904_837_418_035_959_5, 1e-12, "df(2)");
    close(curve.zero_rate(1.5), 0.05, 1e-12, "zero rate at 1.5y");
}

#[test]
fn deposit_quote_bootstraps_to_simple_discount_factor() {
    // DF = 1 / (1 + rate * years): 1 / (1 + 0.04 * 0.25).
    let curve = Curve::bootstrap(&[CurveQuote::Deposit { years: 0.25, rate: 0.04 }]).unwrap();
    close(curve.df(0.25), 0.990_099_009_900_990_1, 1e-12, "deposit DF");
}

#[test]
fn one_year_par_swap_bootstraps_to_its_closed_form() {
    // A one-year annual par swap at 3%: 0.03 * DF(1) = 1 - DF(1), so DF(1) = 1 / 1.03.
    let curve = Curve::bootstrap(&[CurveQuote::Swap { years: 1.0, rate: 0.03, frequency: 1 }]).unwrap();
    close(curve.df(1.0), 0.970_873_786_407_766_99, 1e-12, "par swap DF(1)");
}

#[test]
fn curve_refuses_bad_points() {
    assert!(Curve::from_discount_factors(&[(1.0, 0.9), (0.5, 0.95)]).is_err(), "unsorted times");
    assert!(Curve::from_discount_factors(&[(1.0, -0.1)]).is_err(), "negative factor");
}

// ---- swaps -----------------------------------------------------------------

fn two_year_swap(pay_fixed: bool) -> Swap {
    Swap { notional: 1_000_000.0, fixed_rate: 0.04, years: 2.0, frequency: 1, pay_fixed }
}

#[test]
fn payer_swap_value_matches_the_closed_form() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let v = two_year_swap(true).value(&curve).unwrap();
    // PV = N * (1 - DF(2) - 0.04 * (DF(1) + DF(2))) = 20919.908262573484
    close(v.pv, 20_919.908_262_573_484, 1e-6, "payer PV");
    close(v.annuity, 1.856_066_842_536_673_6, 1e-12, "annuity");
    close(v.par_rate, 0.051_271_096_376_024_04, 1e-12, "par rate");
}

#[test]
fn receiver_swap_is_the_opposite_of_the_payer() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let payer = two_year_swap(true).value(&curve).unwrap();
    let receiver = two_year_swap(false).value(&curve).unwrap();
    close(receiver.pv, -payer.pv, 1e-9, "receiver PV");
}

#[test]
fn swap_dv01_matches_the_closed_form_derivative() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let v = two_year_swap(true).value(&curve).unwrap();
    // dPV/ds = N (T DF(T) + K sum(tau t DF(t))) for a parallel shift s; DV01 = dPV/ds * 1bp.
    // The model's central difference over ±1bp differs from the derivative by ~2e-6.
    close(v.dv01, 192.011_100_649_482_45, 1e-4, "payer DV01");
}

#[test]
fn a_swap_at_its_par_rate_is_worth_zero() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let par = two_year_swap(true).value(&curve).unwrap().par_rate;
    let at_par = Swap { fixed_rate: par, ..two_year_swap(true) }.value(&curve).unwrap();
    close(at_par.pv, 0.0, 1e-6, "PV at par rate");
}

#[test]
fn swap_refuses_bad_terms() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let bad_freq = Swap { frequency: 3, ..two_year_swap(true) };
    assert!(bad_freq.value(&curve).is_err(), "frequency 3");
    let no_maturity = Swap { years: 0.0, ..two_year_swap(true) };
    assert!(no_maturity.value(&curve).is_err(), "zero maturity");
}

// ---- spreads ---------------------------------------------------------------

#[test]
fn present_value_of_flows_matches_the_closed_form() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let flows = [(1.0, 5.0), (2.0, 105.0)];
    // 5 exp(-0.05) + 105 exp(-0.10)
    close(pv_flows(&flows, &curve, 0.0), 99.764_076_016_279_32, 1e-9, "PV at zero spread");
}

#[test]
fn z_spread_recovers_the_spread_used_to_price() {
    let curve = Curve::flat(0.05, 2.0).unwrap();
    let flows = [(1.0, 5.0), (2.0, 105.0)];
    // Priced at a 100bp spread: 97.835 468 523 222 78.
    let price = pv_flows(&flows, &curve, 0.01);
    close(price, 97.835_468_523_222_78, 1e-9, "price at 100bp");
    close(z_spread(&flows, &curve, price).unwrap(), 0.01, 1e-10, "z-spread");
}

// ---- bonds -----------------------------------------------------------------

/// 6% semi-annual bond, 30/360, maturing 2031-01-01.
fn bond() -> BondTerms {
    BondTerms { maturity_ns: ns(2031, 1, 1), coupon_rate: 0.06, frequency: 2, day_count: DayCount::Thirty360 }
}

#[test]
fn a_bond_on_a_coupon_date_prices_at_par_at_its_coupon_yield() {
    let val = ns(2029, 7, 1);
    close(bond().price_from_yield(0.06, val), 100.0, 1e-9, "par price");
}

#[test]
fn bond_price_at_eight_percent_matches_the_closed_form() {
    // Three periods of 3, 3 and 103 discounted at 4% a period.
    let val = ns(2029, 7, 1);
    close(bond().price_from_yield(0.08, val), 97.224_908_966_772_87, 1e-9, "price at 8%");
}

#[test]
fn yield_round_trips_through_price() {
    let val = ns(2029, 7, 1);
    let y = bond().yield_from_price(97.224_908_966_772_87, val).unwrap();
    close(y, 0.08, 1e-9, "yield from price");
}

#[test]
fn accrued_interest_is_thirty_360_on_the_coupon() {
    // 90 of 30/360 days into a six-month period: 90/360 * 6 = 1.5 per 100 face.
    let day = ns(2029, 10, 1);
    close(bond().accrued(day), 1.5, 1e-12, "accrued");
    close(bond().dirty(97.0, day), 98.5, 1e-12, "dirty from clean");
    close(bond().clean(98.5, day), 97.0, 1e-12, "clean from dirty");
}

#[test]
fn bond_risk_matches_the_closed_form() {
    let val = ns(2029, 7, 1);
    let risk = bond().risk(0.08, val);
    // Macaulay duration in years: (1*3/1.04 + 2*3/1.04^2 + 3*103/1.04^3) / P / 2.
    close(risk.macaulay, 1.456_066_302_868_930_4, 1e-9, "Macaulay duration");
    close(risk.modified, risk.macaulay / 1.04, 1e-12, "modified duration");
    close(risk.dv01, 0.013_612_107_090_963_2, 1e-7, "DV01");
}
