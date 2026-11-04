//! M26 task 3g: reference-value tests for the covariance, correlation and
//! Cholesky helpers (`src/returns.rs`), concentration (`src/portfolio.rs`), the
//! uniform stress shock and limit utilization (`src/enforcement.rs`).
//!
//! Expected values are closed forms worked by hand and checked at 40 digits
//! (mpmath). Reference document: `docs/models/risk-controls.md`.
//!
//! Not covered here, on purpose: the zero-limit utilization case and the
//! Cholesky jitter. The doc marks both as current defects (tasks 7 and 8), so a
//! test would lock in the wrong behaviour.

use spectra_risk::enforcement::utilization;
use spectra_risk::portfolio::Portfolio;
use spectra_risk::returns::{cholesky, correlation, correlation_matrix, covariance};

const TOL: f64 = 1e-12;

fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (tol {tol})");
}

const X: [f64; 5] = [1.0, 2.0, 3.0, 4.0, 5.0];
const Y: [f64; 5] = [2.0, 4.0, 5.0, 4.0, 5.0];

#[test]
fn covariance_uses_the_sample_denominator() {
    // Deviations of x: -2, -1, 0, 1, 2; of y: -2, 0, 1, 0, 1. Products sum to 6; 6 / (n - 1) = 1.5.
    close(covariance(&X, &Y).unwrap(), 1.5, TOL, "covariance");
}

#[test]
fn correlation_matches_the_closed_form() {
    // cov / (s_x s_y) = 1.5 / sqrt(2.5 * 1.5) = sqrt(0.6).
    close(correlation(&X, &Y).unwrap(), 0.774_596_669_241_483_4, TOL, "correlation");
}

#[test]
fn a_series_correlates_perfectly_with_itself_and_its_negation() {
    close(correlation(&X, &X).unwrap(), 1.0, TOL, "self-correlation");
    let neg: Vec<f64> = X.iter().map(|v| -v).collect();
    close(correlation(&X, &neg).unwrap(), -1.0, TOL, "correlation with negation");
}

#[test]
fn correlation_matrix_is_symmetric_with_unit_diagonal() {
    let m = correlation_matrix(&[X.to_vec(), Y.to_vec()]).unwrap();
    close(m[0][0], 1.0, TOL, "diagonal 0");
    close(m[1][1], 1.0, TOL, "diagonal 1");
    close(m[0][1], 0.774_596_669_241_483_4, TOL, "off-diagonal");
    close(m[1][0], m[0][1], 0.0, "symmetry");
}

#[test]
fn covariance_and_correlation_refuse_bad_input() {
    assert!(covariance(&X, &Y[..4]).is_err(), "unequal lengths");
    assert!(covariance(&[1.0], &[2.0]).is_err(), "one observation");
    assert!(correlation(&X, &[3.0; 5]).is_err(), "zero-variance series");
    assert!(correlation_matrix(&[X.to_vec(), Y[..4].to_vec()]).is_err(), "matrix with unequal series");
}

#[test]
fn cholesky_factor_reproduces_the_matrix() {
    // A = [[4, 2], [2, 3]] = L Lᵀ with L = [[2, 0], [1, sqrt(2)]].
    let l = cholesky(&[vec![4.0, 2.0], vec![2.0, 3.0]]).unwrap();
    close(l[0][0], 2.0, TOL, "L00");
    close(l[1][0], 1.0, TOL, "L10");
    close(l[1][1], std::f64::consts::SQRT_2, TOL, "L11");
    close(l[1][0] * l[1][0] + l[1][1] * l[1][1], 3.0, TOL, "L11^2 + L10^2 = A22");
}

#[test]
fn cholesky_refuses_a_matrix_that_is_not_positive_definite() {
    assert!(cholesky(&[vec![1.0, 2.0], vec![2.0, 1.0]]).is_err(), "indefinite");
    assert!(cholesky(&[vec![1.0, 0.0, 0.0], vec![0.0, 1.0]]).is_err(), "not square");
}

#[test]
fn hhi_of_equal_positions_is_one_over_n() {
    let mut p = Portfolio::new();
    p.add_asset("A", 1.0, 100.0)
        .add_asset("B", 1.0, 100.0)
        .add_asset("C", 1.0, 100.0)
        .add_asset("D", 1.0, 100.0);
    close(p.hhi().unwrap(), 0.25, TOL, "HHI of four equal positions");
}

#[test]
fn hhi_of_one_position_is_one() {
    let mut p = Portfolio::new();
    p.add_asset("A", 10.0, 50.0);
    close(p.hhi().unwrap(), 1.0, TOL, "HHI of one position");
}

#[test]
fn hhi_is_undefined_without_exposure() {
    assert!(Portfolio::new().hhi().is_err(), "HHI of an empty portfolio");
}

#[test]
fn a_uniform_shock_scales_the_portfolio_value_exactly() {
    let mut p = Portfolio::new();
    p.add_asset("AAPL", 1000.0, 190.0).add_asset("MSFT", 500.0, 420.0).add_asset("GOOGL", 300.0, 175.0);
    // 452,500 before; every price times 0.70 gives 316,750.
    close(p.total_value(), 452_500.0, 1e-6, "value before the shock");
    close(p.shocked(0.70).total_value(), 316_750.0, 1e-6, "value after a 30% shock");
}

#[test]
fn utilization_is_value_over_limit_for_a_set_limit() {
    close(utilization(50.0, 100.0).unwrap(), 0.5, TOL, "half used");
    close(utilization(150.0, 100.0).unwrap(), 1.5, TOL, "over the limit");
}
