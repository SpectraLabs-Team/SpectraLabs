//! M26 task 3d: reference-value tests for the GARCH family's forecast recursions
//! (`src/garch.rs`) and the inverse normal CDF used by the regime model
//! (`src/regime.rs`).
//!
//! These tests use fixed, hand-chosen parameters, so they test the model's
//! equations and not the quasi-maximum-likelihood fit. The expected values were
//! worked from the equations in `docs/models/volatility-regime-copula.md` and
//! checked at 40 digits (mpmath). The fit is covered later (task 3d, part 2).

use spectra_risk::garch::{Egarch11, Garch11, GjrGarch11};
use spectra_risk::regime::normal_quantile;

const TOL: f64 = 1e-13;

fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (tol {tol})");
}

/// omega = sigma_L^2 (1 - alpha - beta) with sigma_L^2 = 2e-4: 2e-4 * 0.05 = 1e-5.
fn garch() -> Garch11 {
    Garch11 {
        omega: 1e-5,
        alpha: 0.1,
        beta: 0.85,
        unconditional_variance: 2e-4,
        mu: 0.0,
        log_likelihood: 0.0,
        persistence: 0.95,
    }
}

#[test]
fn garch_one_step_forecast_is_the_recursion() {
    // h_1 = omega + alpha * r^2 + beta * h_last = 1e-5 + 0.1 * 4e-4 + 0.85 * 3e-4
    let h = garch().forecast_variance(0.02, Some(3e-4), 1);
    close(h, 3.05e-4, TOL, "GARCH one-step variance");
}

#[test]
fn garch_without_a_last_variance_uses_the_unconditional_one() {
    // h_1 = 1e-5 + 0.1 * 4e-4 + 0.85 * 2e-4
    let h = garch().forecast_variance(0.02, None, 1);
    close(h, 2.2e-4, TOL, "GARCH one-step variance, no last variance");
}

#[test]
fn garch_multi_step_forecast_decays_to_the_unconditional_variance() {
    // h_k = UV + persistence^(k-1) (h_1 - UV), with persistence 0.95.
    let m = garch();
    close(m.forecast_variance(0.02, Some(3e-4), 2), 2.9975e-4, TOL, "GARCH k=2");
    close(m.forecast_variance(0.02, Some(3e-4), 3), 2.947_625e-4, TOL, "GARCH k=3");
    close(m.forecast_variance(0.02, Some(3e-4), 10), 2.661_761_880_210_84e-4, TOL, "GARCH k=10");
    // Far ahead the forecast is the unconditional variance.
    close(m.forecast_variance(0.02, Some(3e-4), 2000), 2e-4, 1e-12, "GARCH k=2000");
}

#[test]
fn garch_annualized_volatility_uses_252_trading_days() {
    // sqrt(3.05e-4 * 252)
    close(
        garch().forecast_volatility_annualized(0.02, Some(3e-4)),
        0.277_236_361_251_550_1,
        1e-12,
        "annualized",
    );
    close(
        garch().unconditional_volatility_annualized(),
        (2e-4_f64 * 252.0).sqrt(),
        1e-12,
        "unconditional annualized",
    );
}

/// GJR with alpha 0.05, gamma 0.1: negative shocks use alpha + gamma.
fn gjr() -> GjrGarch11 {
    GjrGarch11 {
        omega: 1e-5,
        alpha: 0.05,
        gamma: 0.1,
        beta: 0.85,
        unconditional_variance: 2e-4,
        mu: 0.0,
        log_likelihood: 0.0,
        persistence: 0.95,
    }
}

#[test]
fn gjr_applies_the_leverage_term_only_after_negative_returns() {
    let m = gjr();
    // Negative return -0.02: (alpha + gamma) * 4e-4 + omega + beta * 3e-4 = 3.25e-4.
    close(m.forecast_variance(-0.02, Some(3e-4), 1), 3.25e-4, TOL, "GJR after a negative return");
    // Positive return +0.02: alpha * 4e-4 + omega + beta * 3e-4 = 2.85e-4.
    close(m.forecast_variance(0.02, Some(3e-4), 1), 2.85e-4, TOL, "GJR after a positive return");
}

/// EGARCH with omega -0.1, alpha 0.2, gamma -0.05, beta 0.95.
fn egarch() -> Egarch11 {
    Egarch11 {
        omega: -0.1,
        alpha: 0.2,
        gamma: -0.05,
        beta: 0.95,
        unconditional_variance: 2e-4,
        mu: 0.0,
        log_likelihood: 0.0,
        persistence: 0.95,
    }
}

#[test]
fn egarch_one_step_forecast_is_the_log_variance_recursion() {
    // z = 0.02 / sqrt(3e-4); log h_1 = omega + alpha (|z| - sqrt(2/pi)) + gamma z + beta ln h_last.
    let h = egarch().forecast_variance(0.02, Some(3e-4), 1);
    close(h, 4.128_139_666_021_002_5e-4, 1e-13, "EGARCH one-step variance");
    close(
        egarch().forecast_volatility_annualized(0.02, Some(3e-4)),
        0.322_535_454_770_059_8,
        1e-12,
        "EGARCH annualized volatility",
    );
}

#[test]
fn egarch_multi_step_forecast_decays_in_log_variance() {
    // log h_k = ln UV + beta^(k-1) (log h_1 - ln UV), persistence = beta.
    let m = egarch();
    close(m.forecast_variance(0.02, Some(3e-4), 2), 3.981_238_203_764_815_7e-4, 1e-13, "EGARCH k=2");
    close(m.forecast_variance(0.02, Some(3e-4), 3), 3.846_526_727_991_086_9e-4, 1e-13, "EGARCH k=3");
}

#[test]
fn inverse_normal_cdf_matches_the_standard_quantiles() {
    close(normal_quantile(0.5), 0.0, 1e-9, "median");
    close(normal_quantile(0.975), 1.959_963_984_540_054, 1e-8, "97.5%");
    close(normal_quantile(0.999), 3.090_232_306_167_813, 1e-8, "99.9%");
    close(normal_quantile(0.025), -1.959_963_984_540_054, 1e-8, "2.5%");
}
