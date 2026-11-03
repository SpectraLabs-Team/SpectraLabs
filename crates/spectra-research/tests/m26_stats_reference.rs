#![allow(clippy::unreadable_literal)] // the return series and reference values are copied verbatim

//! M26 task 5: reference-value tests for the Sharpe-family statistics in
//! `src/stats.rs` (PSR, expected maximum Sharpe, DSR, minimum track record
//! length, the normal quantile) and the trial variance in `sweep.rs`.
//!
//! The reference values were computed independently at 40 significant digits
//! (mpmath) from the same 30-point return series, using the formulas in
//! `docs/models/statistics.md` (Bailey & López de Prado 2012, 2014). The
//! series is stored here verbatim, so the test needs no generator.

use spectra_research::{
    deflated_sharpe, expected_max_sharpe, min_track_record_length, moments, normal_inv_cdf,
    probabilistic_sharpe, sharpe_per_period,
};

/// Absolute tolerance for values computed with 40 digits. The model agrees to
/// about 1e-12 here; this leaves room for platform differences.
const TOL: f64 = 1e-9;

const RETURNS: [f64; 30] = [
    0.008915, 0.012093, 0.000911, -0.005568, -0.007589, -0.004294, 0.00757, 0.013394, 0.004121, -0.00544,
    -0.0075, -0.006366, 0.005702, 0.013906, 0.004503, -0.002379, -0.006614, -0.00801, 0.003499, 0.011129,
    0.006867, 0.000911, -0.004962, -0.009056, -0.001324, 0.010126, 0.008564, 0.004209, -0.002636, -0.01188,
];

// Reference values (mpmath, 40 digits).
const REF_MEAN: f64 = 0.0010934;
const REF_STDEV: f64 = 0.007_592_303_601_174_929;
const REF_SKEW: f64 = 0.112_658_706_561_931_25;
const REF_KURT_RAW: f64 = 1.736_245_308_058_943_5;
const REF_SR: f64 = 0.144_014_261_999_585_14;
const REF_PSR_VS_ZERO: f64 = 0.782_421_336_559_927_93;
const REF_EMAX_V001_N10: f64 = 0.157_459_830_134_575;
const REF_DSR_V001_N10: f64 = 0.470_958_762_987_482_98;
const REF_MINTRL_095: f64 = 129.831_334_759_434_43;
const REF_PHI_INV_095: f64 = 1.644_853_626_951_472_7;
const REF_PHI_INV_0975: f64 = 1.959_963_984_540_054_2;

fn near(label: &str, got: f64, want: f64) {
    assert!((got - want).abs() < TOL, "{label}: got {got}, reference {want}, diff {}", got - want);
}

#[test]
fn moments_match_the_reference() {
    let m = moments(&RETURNS).unwrap();
    near("mean", m.mean, REF_MEAN);
    near("stdev", m.stdev, REF_STDEV);
    near("skew", m.skew, REF_SKEW);
    near("raw kurtosis", m.raw_kurtosis, REF_KURT_RAW);
}

#[test]
fn raw_kurtosis_and_engine_excess_kurtosis_agree_on_reference_series() {
    let m = moments(&RETURNS).unwrap();
    near("raw kurtosis", m.raw_kurtosis, REF_KURT_RAW);
    let engine_excess = spectra_engine::performance::excess_kurtosis(&RETURNS).unwrap();
    let excess_from_raw = m.raw_kurtosis - 3.0;
    assert!(
        (excess_from_raw - engine_excess).abs() < 1e-12,
        "excess mismatch: raw - 3 = {excess_from_raw}, engine = {engine_excess}"
    );
}

#[test]
fn per_period_sharpe_matches_the_reference() {
    near("sharpe", sharpe_per_period(&RETURNS).unwrap(), REF_SR);
}

#[test]
fn probabilistic_sharpe_matches_the_reference() {
    let m = moments(&RETURNS).unwrap();
    let sr = sharpe_per_period(&RETURNS).unwrap();
    near("PSR vs 0", probabilistic_sharpe(sr, 0.0, m.n, m.skew, m.raw_kurtosis).unwrap(), REF_PSR_VS_ZERO);
}

#[test]
fn expected_max_sharpe_matches_the_reference() {
    near("E[max SR], V=0.01, N=10", expected_max_sharpe(0.01, 10).unwrap(), REF_EMAX_V001_N10);
}

#[test]
fn deflated_sharpe_matches_the_reference() {
    near("DSR, V=0.01, N=10", deflated_sharpe(&RETURNS, 10, 0.01).unwrap(), REF_DSR_V001_N10);
}

#[test]
fn minimum_track_record_length_matches_the_reference() {
    let m = moments(&RETURNS).unwrap();
    let sr = sharpe_per_period(&RETURNS).unwrap();
    near(
        "MinTRL at 95%",
        min_track_record_length(sr, 0.0, m.skew, m.raw_kurtosis, 0.95).unwrap(),
        REF_MINTRL_095,
    );
}

#[test]
fn normal_quantiles_match_the_reference() {
    near("Phi^-1(0.95)", normal_inv_cdf(0.95).unwrap(), REF_PHI_INV_095);
    near("Phi^-1(0.975)", normal_inv_cdf(0.975).unwrap(), REF_PHI_INV_0975);
    assert!(
        normal_inv_cdf(0.0).unwrap() == f64::NEG_INFINITY && normal_inv_cdf(1.0).unwrap() == f64::INFINITY
    );
    assert!(normal_inv_cdf(1.5).is_err());
}

/// The degenerate cases must say so: an explicit Err, never a plausible-looking number or silent default.
#[test]
fn undefined_inputs_are_not_silently_numbers() {
    let m = moments(&RETURNS).unwrap();
    let sr = sharpe_per_period(&RETURNS).unwrap();
    assert!(probabilistic_sharpe(sr, 0.0, 1, m.skew, m.raw_kurtosis).is_err(), "one observation");
    // Variance term <= 0: huge negative skew with a positive Sharpe.
    assert!(probabilistic_sharpe(1.0, 0.0, 30, 10.0, 3.0).is_err(), "variance term not positive");
    assert_eq!(expected_max_sharpe(0.01, 1).unwrap(), 0.0, "one trial has no maximum to deflate");
    assert_eq!(expected_max_sharpe(0.0, 10).unwrap(), 0.0, "zero trial variance");
    assert!(expected_max_sharpe(0.01, 0).is_err(), "zero trials");
    assert!(expected_max_sharpe(-0.01, 10).is_err(), "negative trial variance");
    assert_eq!(
        min_track_record_length(0.1, 0.2, 0.0, 3.0, 0.95).unwrap(),
        f64::INFINITY,
        "SR below benchmark"
    );
    assert!(min_track_record_length(0.2, 0.1, 0.0, 3.0, 1.5).is_err(), "invalid confidence");
    assert!(sharpe_per_period(&[0.01; 10]).is_err(), "zero volatility");
    assert!(moments(&[]).is_err(), "empty moments input");
    assert!(moments(&[1.0]).is_err(), "single observation moments");
    assert!(moments(&[1.0, 1.0, 1.0]).is_err(), "zero variance moments");
    assert!(deflated_sharpe(&[0.01; 10], 10, 0.01).is_err(), "zero volatility DSR");
}

/// A trial variance of zero means "no spread between trials", so the expected maximum is zero
/// and the DSR reduces to the PSR against zero. This is the behaviour the callers need.
#[test]
fn zero_trial_variance_reduces_dsr_to_psr_against_zero() {
    let m = moments(&RETURNS).unwrap();
    let sr = sharpe_per_period(&RETURNS).unwrap();
    near(
        "DSR with V=0",
        deflated_sharpe(&RETURNS, 10, 0.0).unwrap(),
        probabilistic_sharpe(sr, 0.0, m.n, m.skew, m.raw_kurtosis).unwrap(),
    );
}

#[test]
fn expected_max_sharpe_grows_with_trials_and_variance() {
    assert!(expected_max_sharpe(0.01, 100).unwrap() > expected_max_sharpe(0.01, 10).unwrap());
    assert!(expected_max_sharpe(0.04, 10).unwrap() > expected_max_sharpe(0.01, 10).unwrap());
}
