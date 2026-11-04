//! M26 task 3e (part 2): reference-value tests for walk-forward folds and
//! efficiency (`src/walkforward.rs`).
//!
//! Folds are half-open index ranges over n sorted timestamps. For a rolling
//! fold starting at s: train [s, s + train), embargo skipped, test
//! [s + train + embargo, s + train + embargo + test); folds advance by `step`
//! (0 means `test`), and a fold is kept only when its test window ends by n.
//! Efficiency = mean out-of-sample / mean in-sample objective, 0 when
//! |mean in-sample| <= 1e-9. The expected folds were worked by hand from this
//! definition (the doc in `docs/models/research-modules.md`).
//!
//! Not covered here: the 1e-9 zero-denominator case is a documented defect (task 8: it
//! should be "not defined"), so only the ratio itself is tested.

use spectra_research::walkforward::{
    make_folds, walk_forward_efficiency, Fold, WalkForwardConfig, WalkForwardMode,
};

const TOL: f64 = 1e-12;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want} (tol {TOL})");
}

fn cfg(mode: WalkForwardMode, step: usize) -> WalkForwardConfig {
    WalkForwardConfig { train: 4, test: 2, step, embargo: 1, mode }
}

#[test]
fn rolling_folds_slide_by_the_step_and_keep_only_complete_windows() {
    // n = 12: train 4, embargo 1, test 2, step 2.
    // Fold 0: train [0,4), test [5,7). Fold 1: train [2,6), test [7,9). Fold 2: train [4,8), test [9,11).
    // The next fold would test [11,13), past n = 12, so it is dropped.
    let folds = make_folds(12, &cfg(WalkForwardMode::Rolling, 2)).unwrap();
    assert_eq!(
        folds,
        vec![
            Fold { train_begin: 0, train_end: 4, test_begin: 5, test_end: 7 },
            Fold { train_begin: 2, train_end: 6, test_begin: 7, test_end: 9 },
            Fold { train_begin: 4, train_end: 8, test_begin: 9, test_end: 11 },
        ]
    );
}

#[test]
fn anchored_folds_keep_the_training_window_starting_at_zero() {
    let folds = make_folds(12, &cfg(WalkForwardMode::Anchored, 2)).unwrap();
    assert_eq!(folds.len(), 3);
    assert!(folds.iter().all(|f| f.train_begin == 0), "every anchored fold trains from 0");
    assert_eq!(folds[2], Fold { train_begin: 0, train_end: 8, test_begin: 9, test_end: 11 });
}

#[test]
fn a_zero_step_advances_by_the_test_length() {
    // Step 0 means the test length (2), so the folds match step 2.
    let zero = make_folds(12, &cfg(WalkForwardMode::Rolling, 0)).unwrap();
    let two = make_folds(12, &cfg(WalkForwardMode::Rolling, 2)).unwrap();
    assert_eq!(zero, two);
}

#[test]
fn too_short_a_series_gives_no_folds() {
    // The first fold needs train 4 + embargo 1 + test 2 = 7 timestamps.
    assert!(make_folds(6, &cfg(WalkForwardMode::Rolling, 2)).is_err(), "cannot fit one fold");
    assert_eq!(make_folds(7, &cfg(WalkForwardMode::Rolling, 2)).unwrap().len(), 1);
}

#[test]
fn zero_train_or_test_windows_are_refused() {
    let no_train = WalkForwardConfig { train: 0, ..cfg(WalkForwardMode::Rolling, 2) };
    let no_test = WalkForwardConfig { test: 0, ..cfg(WalkForwardMode::Rolling, 2) };
    assert!(make_folds(12, &no_train).is_err(), "zero train window");
    assert!(make_folds(12, &no_test).is_err(), "zero test window");
    assert!(make_folds(0, &cfg(WalkForwardMode::Rolling, 2)).is_err(), "empty timestamp series");
}

#[test]
fn efficiency_is_the_out_of_sample_mean_over_the_in_sample_mean() {
    close(walk_forward_efficiency(0.02, 0.01).unwrap(), 0.5, "efficiency 0.5");
    close(walk_forward_efficiency(0.02, 0.03).unwrap(), 1.5, "efficiency above one");
    // A negative in-sample mean gives a negative ratio when the out-of-sample mean is positive.
    close(walk_forward_efficiency(-0.04, 0.01).unwrap(), -0.25, "efficiency with a negative in-sample mean");
}

#[test]
fn efficiency_undefined_or_nan_returns_error() {
    assert!(walk_forward_efficiency(0.0, 0.01).is_err(), "zero in-sample objective");
    assert!(walk_forward_efficiency(1e-10, 0.01).is_err(), "flat in-sample objective");
    assert!(walk_forward_efficiency(-1e-10, 0.01).is_err(), "flat negative in-sample objective");
    assert!(walk_forward_efficiency(f64::NAN, 0.01).is_err(), "NaN in-sample objective");
    assert!(walk_forward_efficiency(0.02, f64::NAN).is_err(), "NaN out-of-sample objective");
}
