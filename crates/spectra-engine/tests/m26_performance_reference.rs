//! M26 task 3f (part 1): reference-value tests for the return-series performance
//! ratios (`src/performance.rs`): CAGR, Sortino, Omega, drawdown episodes,
//! Calmar, Sterling, Burke and the sample standard deviation.
//!
//! The series is six semi-annual returns with three separate drawdowns (-10%,
//! -20%, -5%); the first two recover to the prior peak, so each is its own
//! episode. Expected values follow the table in `docs/models/engine-models.md`
//! and were checked at 40 digits (mpmath). Undefined cases return `None`.

use spectra_engine::performance::{
    burke, cagr, calmar, drawdown_episodes, omega, rolling_volatility, sortino, stdev, sterling,
};

const TOL: f64 = 1e-12;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want} (tol {TOL})");
}

/// Returns -10%, +1/9, -20%, +25%, -5%, +2%: the curve goes 1 -> 0.9 -> 1 -> 0.8 -> 1 -> 0.95 -> 0.969.
fn series() -> Vec<f64> {
    vec![-0.1, 1.0 / 9.0, -0.2, 0.25, -0.05, 0.02]
}

const PPY: f64 = 2.0;
const RF: f64 = 0.02;

#[test]
fn cagr_compounds_the_series_to_an_annual_rate() {
    // growth = 0.969; CAGR = 0.969^(ppy / n) - 1 = 0.969^(1/3) - 1.
    close(cagr(&series(), PPY).unwrap(), -0.010_441_988_952_055_842, "CAGR");
}

#[test]
fn drawdown_episodes_are_the_depth_of_each_separate_stretch() {
    let eps = drawdown_episodes(&series());
    assert_eq!(eps.len(), 3, "three episodes, got {eps:?}");
    close(eps[0], 0.1, "episode 1");
    close(eps[1], 0.2, "episode 2");
    close(eps[2], 0.05, "episode 3 (not recovered)");
}

#[test]
fn sortino_uses_the_downside_below_the_risk_free_target() {
    // target = rf / ppy = 0.01; downside deviation over the returns below it.
    close(sortino(&series(), PPY, RF).unwrap(), -0.068_205_534_893_872_369, "Sortino");
}

#[test]
fn omega_at_zero_is_gains_over_losses() {
    // gains = 1/9 + 0.25 + 0.02; losses = 0.1 + 0.2 + 0.05.
    close(omega(&series(), 0.0).unwrap(), 1.088_888_888_888_888_9, "Omega at 0");
}

#[test]
fn calmar_sterling_and_burke_divide_cagr_by_their_drawdown_measure() {
    let (g, eps) = (cagr(&series(), PPY).unwrap(), drawdown_episodes(&series()));
    // Max depth 0.2; mean depth 0.35/3; root-sum-square sqrt(0.0525).
    close(calmar(&series(), PPY).unwrap(), g / 0.2, "Calmar");
    close(sterling(&series(), PPY).unwrap(), g / (eps.iter().sum::<f64>() / 3.0), "Sterling");
    close(burke(&series(), PPY).unwrap(), g / eps.iter().map(|d| d * d).sum::<f64>().sqrt(), "Burke");
    close(calmar(&series(), PPY).unwrap(), -0.052_209_944_760_279_211, "Calmar value");
    close(sterling(&series(), PPY).unwrap(), -0.089_502_762_446_192_934, "Sterling value");
    close(burke(&series(), PPY).unwrap(), -0.045_572_575_979_703_334, "Burke value");
}

#[test]
fn sample_standard_deviation_uses_n_minus_one() {
    close(stdev(&series()).unwrap(), 0.159_740_015_112_004_85, "stdev");
}

#[test]
fn undefined_ratios_are_none_not_zero() {
    // No drawdown: Calmar, Sterling and Burke have no denominator.
    let rising = [0.01, 0.02, 0.03];
    assert!(calmar(&rising, PPY).is_none(), "Calmar with no drawdown");
    assert!(sterling(&rising, PPY).is_none(), "Sterling with no drawdown");
    assert!(burke(&rising, PPY).is_none(), "Burke with no drawdown");
    assert!(drawdown_episodes(&rising).is_empty(), "no episodes");

    // Every return above the 1% target: downside deviation is zero, so Sortino is undefined.
    assert!(sortino(&[0.05, 0.06, 0.07], PPY, RF).is_none(), "Sortino with no downside");

    // Nothing below the threshold: Omega has no denominator.
    assert!(omega(&[0.01, 0.02], 0.0).is_none(), "Omega with no losses");

    // One observation has no sample standard deviation.
    assert!(stdev(&[0.01]).is_none(), "stdev of one return");
}

#[test]
fn rolling_volatility_uses_expect_not_silent_default() {
    // A 3-element window over 5 returns produces 3 windows.
    // Returns: [-0.1, 0.2, -0.05, 0.1, -0.15]
    //
    // Window 0: [-0.1, 0.2, -0.05]
    //   mean = 0.05/3 ≈ 0.016667
    //   sum sq = (-0.1 - 0.016667)^2 + (0.2 - 0.016667)^2 + (-0.05 - 0.016667)^2
    //          = 0.013611 + 0.033611 + 0.004444
    //          = 0.051667
    //   stdev = sqrt(0.051667 / 2) ≈ 0.160728
    //   annualised (ppy=12) = 0.160728 * sqrt(12) ≈ 0.556776
    //
    // Window 1: [0.2, -0.05, 0.1]
    //   mean = 0.25/3 ≈ 0.083333
    //   sum sq = (0.2 - 0.083333)^2 + (-0.05 - 0.083333)^2 + (0.1 - 0.083333)^2
    //          = 0.013611 + 0.017778 + 0.000278
    //          = 0.031667
    //   stdev = sqrt(0.031667 / 2) ≈ 0.125831
    //   annualised (ppy=12) = 0.125831 * sqrt(12) ≈ 0.435890
    //
    // Window 2: [-0.05, 0.1, -0.15]
    //   mean = -0.1/3 ≈ -0.033333
    //   sum sq = (-0.05 + 0.033333)^2 + (0.1 + 0.033333)^2 + (-0.15 + 0.033333)^2
    //          = 0.000278 + 0.017778 + 0.013611
    //          = 0.031667
    //   stdev = sqrt(0.031667 / 2) ≈ 0.125831
    //   annualised (ppy=12) = 0.125831 * sqrt(12) ≈ 0.435890
    let r = [-0.1, 0.2, -0.05, 0.1, -0.15];
    let ppy = 12.0;
    let rv = rolling_volatility(&r, 3, ppy);
    assert_eq!(rv.len(), 3, "3-element window over 5 returns = 3 windows");

    // Check each window against hand-computed stdev * sqrt(ppy).
    let sd0 = (0.051_666_666_666_666_666_f64 / 2.0).sqrt();
    let sd1 = (0.031_666_666_666_666_666_f64 / 2.0).sqrt();
    close(rv[0], sd0 * ppy.sqrt(), "rolling_vol window 0");
    close(rv[1], sd1 * ppy.sqrt(), "rolling_vol window 1");
    close(rv[2], sd1 * ppy.sqrt(), "rolling_vol window 2");
}

#[test]
fn rolling_volatility_empty_when_window_too_large() {
    assert!(rolling_volatility(&[0.01, 0.02], 3, 252.0).is_empty());
    assert!(rolling_volatility(&[0.01], 2, 252.0).is_empty());
}
