//! M26 task 3e (part 1): reference-value tests for the strategy-versus-benchmark
//! metrics (`src/benchmark.rs`): beta, alpha, correlation, tracking error,
//! information ratio and up/down capture.
//!
//! Series: five periods of benchmark and strategy returns, annualized with 4
//! periods a year. Expected values are the definitions in
//! `docs/models/research-modules.md` (sample variances, tracking error as the
//! sample stdev of the active return times sqrt(ppy)), worked independently at
//! 40 digits (mpmath).
//!
//! Not covered here: invalid-input zeros. Returning zeros without an error is a
//! documented defect (task 8), so a test would lock in the wrong behaviour; the
//! `valid` flag is checked instead.

use spectra_research::benchmark::compute_benchmark_metrics;

const TOL: f64 = 1e-12;
const PPY: f64 = 4.0;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want} (tol {TOL})");
}

const B: [f64; 5] = [0.02, -0.01, 0.03, -0.02, 0.01];
const S: [f64; 5] = [0.03, -0.02, 0.02, -0.01, 0.02];

#[test]
fn beta_and_correlation_match_the_sample_definitions() {
    let m = compute_benchmark_metrics(&S, &B, PPY).unwrap();
    assert!(m.valid);
    close(m.beta, 0.906_976_744_186_046_5, "beta");
    close(m.correlation, 0.867_523_903_935_251_7, "correlation");
}

#[test]
fn alpha_is_the_simple_annualized_intercept() {
    // (mean strategy - beta * mean benchmark) * ppy.
    let m = compute_benchmark_metrics(&S, &B, PPY).unwrap();
    close(m.alpha_annualized, 0.010_232_558_139_534_884, "alpha");
}

#[test]
fn tracking_error_and_information_ratio_use_the_active_return() {
    // Tracking error = sample stdev(s - b) * sqrt(ppy); IR = mean(s - b) / TE_per_period * sqrt(ppy).
    let m = compute_benchmark_metrics(&S, &B, PPY).unwrap();
    close(m.tracking_error, 0.021_908_902_300_206_644, "tracking error");
    close(m.information_ratio, 0.365_148_371_670_110_74, "information ratio");
}

#[test]
fn up_and_down_capture_compare_average_returns_in_each_benchmark_regime() {
    let m = compute_benchmark_metrics(&S, &B, PPY).unwrap();
    // Up periods (benchmark > 0: indices 0, 2, 4): mean strategy 0.02333 / mean benchmark 0.02.
    // Down periods (benchmark < 0: indices 1, 3): mean strategy -0.015 / mean benchmark -0.015.
    close(m.up_capture, 1.166_666_666_666_666_7, "up capture");
    close(m.down_capture, 1.0, "down capture");
}

#[test]
fn a_strategy_identical_to_the_benchmark_has_no_tracking_error() {
    let m = compute_benchmark_metrics(&B, &B, PPY).unwrap();
    close(m.beta, 1.0, "beta against itself");
    close(m.correlation, 1.0, "correlation with itself");
    close(m.tracking_error, 0.0, "tracking error against itself");
    assert_eq!(m.information_ratio, 0.0, "IR with no active return");
}

#[test]
fn unequal_series_are_refused() {
    assert!(compute_benchmark_metrics(&S, &B[..4], PPY).is_err(), "length mismatch");
}

#[test]
fn a_constant_benchmark_is_flagged_invalid() {
    // Zero benchmark variance: beta is undefined, and the valid flag says so.
    let flat = [0.01; 5];
    let m = compute_benchmark_metrics(&S, &flat, PPY).unwrap();
    assert!(!m.valid, "a constant benchmark must not be reported as valid");
}
