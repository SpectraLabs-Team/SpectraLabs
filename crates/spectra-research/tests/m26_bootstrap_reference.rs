//! M26 task 3e (part 3): checks for the circular block bootstrap of returns
//! (`src/trades.rs`, `block_bootstrap_returns`).
//!
//! The resample values depend on the seed, so they are not checked against an
//! independent number (the doc says seed-dependent output is pinned by the parity
//! golden, not by a value here). These tests check what must hold for every seed:
//!
//! - With one block covering the whole series, each resample is a rotation of
//!   the returns. The terminal return (a product) and the Sharpe ratio (mean and
//!   stdev) do not depend on the order, so every resample gives the same value,
//!   which is the closed form from the returns.
//! - Invalid inputs are flagged `valid = false` (the doc: "correctly flagged").
//! - The same seed gives the same result.

use spectra_research::stats::sharpe_per_period;
use spectra_research::trades::{block_bootstrap_returns, ReturnBlockBootstrap};

const TOL: f64 = 1e-12;
const PPY: f64 = 4.0;
const EQUITY: f64 = 1_000.0;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want} (tol {TOL})");
}

const R: [f64; 6] = [0.02, -0.01, 0.03, -0.02, 0.01, 0.04];

#[test]
fn a_block_covering_the_whole_series_gives_the_same_terminal_return_every_time() {
    // Growth = product(1 + r) = 1.02 * 0.99 * 1.03 * 0.98 * 1.01 * 1.04; terminal return = growth - 1.
    let growth: f64 = R.iter().map(|r| 1.0 + r).product();
    let out = block_bootstrap_returns(&R, EQUITY, PPY, R.len(), 40, 7).unwrap();
    assert!(out.valid);
    close(out.terminal_return.p05, growth - 1.0, "terminal 5th");
    close(out.terminal_return.p50, growth - 1.0, "terminal median");
    close(out.terminal_return.p95, growth - 1.0, "terminal 95th");
}

#[test]
fn a_block_covering_the_whole_series_gives_the_sharpe_of_the_returns_every_time() {
    // A rotation keeps the mean and stdev, so Sharpe = sharpe_per_period(R) * sqrt(ppy) for all resamples.
    let want = sharpe_per_period(&R).unwrap() * PPY.sqrt();
    let out = block_bootstrap_returns(&R, EQUITY, PPY, R.len(), 40, 7).unwrap();
    close(out.annualized_sharpe.p05, want, "Sharpe 5th");
    close(out.annualized_sharpe.p95, want, "Sharpe 95th");
}

#[test]
fn a_losing_product_is_a_loss_in_every_resample() {
    // Constant series has zero volatility, so Sharpe is undefined and errors.
    let losing = [-0.01; 4];
    assert!(block_bootstrap_returns(&losing, EQUITY, PPY, losing.len(), 25, 3).is_err());
    let losing_varying = [-0.01, -0.015, -0.005, -0.01];
    let out = block_bootstrap_returns(&losing_varying, EQUITY, PPY, losing_varying.len(), 25, 3).unwrap();
    assert_eq!(out.probability_of_loss, 1.0, "every resample loses");
}

#[test]
fn the_same_seed_gives_the_same_result() {
    let a: ReturnBlockBootstrap = block_bootstrap_returns(&R, EQUITY, PPY, 2, 100, 42).unwrap();
    let b: ReturnBlockBootstrap = block_bootstrap_returns(&R, EQUITY, PPY, 2, 100, 42).unwrap();
    assert_eq!(a, b, "a fixed seed is reproducible");
}

#[test]
fn invalid_inputs_are_flagged_not_silently_numbered() {
    let flagged = |returns: &[f64], equity: f64, block: usize, resamples: usize| {
        !block_bootstrap_returns(returns, equity, PPY, block, resamples, 1).unwrap().valid
    };
    assert!(flagged(&R[..1], EQUITY, 1, 10), "one return");
    assert!(flagged(&R, EQUITY, 0, 10), "zero block length");
    assert!(flagged(&R, EQUITY, R.len() + 1, 10), "block longer than the series");
    assert!(flagged(&R, EQUITY, 2, 0), "zero resamples");
    assert!(flagged(&R, 0.0, 2, 10), "zero initial equity");
    assert!(flagged(&[0.01, -1.0, 0.02], EQUITY, 1, 10), "a return of -100% or worse");
}
