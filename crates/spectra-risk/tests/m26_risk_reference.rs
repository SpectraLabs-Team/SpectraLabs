//! M26 task 3b: reference-value tests for the empirical VaR and CVaR
//! (`src/metrics.rs`) and the interpolated quantile (`src/engine.rs`).
//!
//! The method is type-7 interpolation: the quantile at confidence c of n sorted
//! losses sits at rank c (n - 1). Every expected value below is a closed form
//! worked by hand and checked at 40 digits (mpmath). Reference document:
//! `docs/models/risk-metrics.md`.

use spectra_risk::engine::quantile;
use spectra_risk::metrics::{conditional_var, quantile_sorted, value_at_risk};

const TOL: f64 = 1e-12;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want}");
}

/// Losses 1..=10 (already sorted; n = 10, rank = c * 9).
fn ten() -> Vec<f64> {
    (1..=10).map(f64::from).collect()
}

#[test]
fn var_interpolates_linearly_between_order_statistics() {
    let l = ten();
    close(value_at_risk(&l, 0.5).unwrap(), 5.5, "VaR 50%"); // rank 4.5 -> 5 and 6
    close(value_at_risk(&l, 0.95).unwrap(), 9.55, "VaR 95%"); // rank 8.55 -> 9 and 10
    close(value_at_risk(&l, 0.0).unwrap(), 1.0, "VaR 0%");
    close(value_at_risk(&l, 1.0).unwrap(), 10.0, "VaR 100%");
}

#[test]
fn var_does_not_depend_on_input_order() {
    let shuffled = [3.0, 0.5, 4.2, 1.1, 2.0, 0.9, 6.0];
    // Sorted: 0.5, 0.9, 1.1, 2.0, 3.0, 4.2, 6.0 (n = 7).
    close(value_at_risk(&shuffled, 0.9).unwrap(), 4.92, "VaR 90%, unsorted input");
    close(value_at_risk(&shuffled, 0.5).unwrap(), 2.0, "VaR 50%, exact order statistic");
}

#[test]
fn cvar_is_the_mean_of_the_losses_at_or_above_var() {
    let l = ten();
    close(conditional_var(&l, 0.5).unwrap(), 8.0, "CVaR 50%"); // mean of 6..=10
    close(conditional_var(&l, 0.8).unwrap(), 9.5, "CVaR 80%"); // VaR 8.2 -> mean of 9, 10
    close(conditional_var(&l, 0.95).unwrap(), 10.0, "CVaR 95%"); // VaR 9.55 -> only 10
}

#[test]
fn cvar_on_an_unsorted_sample() {
    let shuffled = [3.0, 0.5, 4.2, 1.1, 2.0, 0.9, 6.0];
    close(conditional_var(&shuffled, 0.9).unwrap(), 6.0, "CVaR 90%");
    // VaR 50% = 2.0; losses >= 2.0 are 2.0, 3.0, 4.2, 6.0: mean 3.8.
    close(conditional_var(&shuffled, 0.5).unwrap(), 3.8, "CVaR 50%");
}

#[test]
fn a_single_loss_is_its_own_var_and_cvar() {
    close(value_at_risk(&[4.25], 0.99).unwrap(), 4.25, "VaR of one loss");
    close(conditional_var(&[4.25], 0.99).unwrap(), 4.25, "CVaR of one loss");
}

#[test]
fn a_constant_sample_has_that_constant_as_var_and_cvar() {
    let flat = [5.0; 8];
    close(value_at_risk(&flat, 0.99).unwrap(), 5.0, "VaR of constant losses");
    close(conditional_var(&flat, 0.99).unwrap(), 5.0, "CVaR of constant losses");
}

#[test]
fn cvar_at_full_confidence_is_the_worst_loss() {
    close(conditional_var(&ten(), 1.0).unwrap(), 10.0, "CVaR 100%");
}

#[test]
fn empty_samples_are_refused() {
    assert!(value_at_risk(&[], 0.95).is_err(), "VaR of nothing");
    assert!(conditional_var(&[], 0.95).is_err(), "CVaR of nothing");
    assert!(quantile(&[], 0.5).is_err(), "quantile of nothing");
}

#[test]
fn the_engine_quantile_agrees_with_the_risk_var() {
    // engine::quantile sorts its input and uses the same type-7 rule as VaR.
    let shuffled = [3.0, 0.5, 4.2, 1.1, 2.0, 0.9, 6.0];
    close(quantile(&shuffled, 0.9).unwrap(), 4.92, "engine quantile 90%");
    close(quantile(&shuffled, 0.9).unwrap(), value_at_risk(&shuffled, 0.9).unwrap(), "engine vs VaR");
    let mut sorted = shuffled;
    sorted.sort_by(f64::total_cmp);
    close(quantile_sorted(&sorted, 0.9), 4.92, "quantile_sorted 90%");
}
