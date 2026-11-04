//! M26 task 3c: reference-value tests for the peaks-over-threshold GPD tail
//! model (`src/evt.rs`), using the probability-weighted-moment fit
//! (Hosking and Wallis) documented in `docs/models/risk-metrics.md`.
//!
//! Each dataset has 81 losses at exactly 1.0, then 20 tail losses 1 + y_j. With
//! the 0.8 threshold quantile of 101 losses (rank 80), the threshold is exactly
//! 1.0 and the 20 exceedances are y_j. The fit, KS statistic, VaR and CVaR
//! below were worked from the formulas in the doc and checked at 40 digits
//! (mpmath). The first dataset has a shape of -0.2969 and the second 1/21, so
//! neither hits the [-0.5, 0.9] clamp.

use spectra_risk::evt::EvtTailModel;

const TOL: f64 = 1e-9;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want}");
}

/// 81 losses at 1.0, then 20 tail losses `1 + f(j)` for j = 1..=20.
fn dataset(f: impl Fn(f64) -> f64) -> Vec<f64> {
    let mut losses = vec![1.0; 81];
    losses.extend((1..=20).map(|j| 1.0 + f(f64::from(j))));
    losses
}

/// Shape -0.2969: exceedances j^1.5 / 10.
fn heavy_tail_losses() -> Vec<f64> {
    dataset(|j| j.powf(1.5) / 10.0)
}

/// Shape 1/21: exceedances j^2 / 100.
fn light_tail_losses() -> Vec<f64> {
    dataset(|j| j * j / 100.0)
}

#[test]
fn fit_recovers_the_pwm_shape_scale_and_threshold() {
    let m = EvtTailModel::fit(&heavy_tail_losses(), 0.8).unwrap();
    close(m.threshold, 1.0, "threshold");
    assert_eq!(m.total_observations, 101);
    assert_eq!(m.exceedances_count, 20);
    close(m.xi, -0.296_850_990_823_572_8, "shape");
    close(m.beta, 4.933_199_446_606_695, "scale");
}

#[test]
fn goodness_of_fit_statistic_matches_the_reference() {
    let m = EvtTailModel::fit(&heavy_tail_losses(), 0.8).unwrap();
    close(m.ks_statistic, 0.074_063_535_718_418_41, "KS statistic");
    assert!(m.passes_goodness_of_fit, "KS 0.074 is below the 0.304 critical value");
}

#[test]
fn var_matches_the_gpd_tail_formula() {
    // VaR_p = u + (beta / xi) * ((N / Nu) (1 - p))^(-xi) - beta / xi
    let m = EvtTailModel::fit(&heavy_tail_losses(), 0.8).unwrap();
    close(m.var(0.95).unwrap(), 6.573_814_510_660_889, "VaR 95%");
    close(m.var(0.99).unwrap(), 10.768_904_334_183_269, "VaR 99%");
}

#[test]
fn cvar_matches_the_gpd_tail_formula() {
    // CVaR_p = (VaR_p + beta - xi u) / (1 - xi), valid for xi < 1.
    let m = EvtTailModel::fit(&heavy_tail_losses(), 0.8).unwrap();
    close(m.cvar(0.95).unwrap(), 9.101_943_886_857_073, "CVaR 95%");
    close(m.cvar(0.99).unwrap(), 12.336_771_830_241_891, "CVaR 99%");
}

#[test]
fn a_light_tail_fit_matches_the_reference() {
    let m = EvtTailModel::fit(&light_tail_losses(), 0.8).unwrap();
    close(m.xi, 1.0 / 21.0, "shape");
    close(m.beta, 41.0 / 30.0, "scale");
    close(m.ks_statistic, 0.089_809_359_345_619_35, "KS statistic");
    close(m.var(0.99).unwrap(), 5.384_907_620_559_730, "VaR 99%");
    close(m.cvar(0.99).unwrap(), 7.039_153_001_587_717, "CVaR 99%");
}

#[test]
fn the_fit_does_not_depend_on_input_order() {
    let mut reversed = heavy_tail_losses();
    reversed.reverse();
    let a = EvtTailModel::fit(&heavy_tail_losses(), 0.8).unwrap();
    let b = EvtTailModel::fit(&reversed, 0.8).unwrap();
    close(a.xi, b.xi, "shape, reversed input");
    close(a.beta, b.beta, "scale, reversed input");
}

#[test]
fn too_few_observations_or_exceedances_are_refused() {
    let short = vec![1.0; 49];
    assert!(EvtTailModel::fit(&short, 0.8).is_err(), "49 losses");

    // 55 losses at 1.0 and 5 above: quantile 0.9 sits at 1.0, leaving 5 exceedances (< 8).
    let mut few = vec![1.0; 55];
    few.extend([1.5, 2.0, 2.5, 3.0, 3.5]);
    assert!(EvtTailModel::fit(&few, 0.9).is_err(), "5 exceedances");
}

#[test]
fn threshold_quantile_outside_the_range_is_refused() {
    let losses = heavy_tail_losses();
    assert!(EvtTailModel::fit(&losses, 0.69).is_err(), "below 0.70");
    assert!(EvtTailModel::fit(&losses, 0.999).is_err(), "at 0.999");
}

#[test]
fn evt_fit_shape_outside_range_returns_error() {
    let mut losses = vec![1.0; 81];
    losses.extend((1..=20).map(|j| 1.0 + f64::from(j).exp()));
    let err = EvtTailModel::fit(&losses, 0.8).unwrap_err();
    assert_eq!(
        err,
        spectra_core::Error::Data(
            "EVT shape 0.9347415179545022 outside the supported range [-0.5, 0.9]".into()
        )
    );
}

#[test]
fn evt_cvar_undefined_for_shape_ge_1() {
    let model = EvtTailModel {
        threshold: 1.0,
        xi: 1.0,
        beta: 2.0,
        total_observations: 100,
        exceedances_count: 20,
        ks_statistic: 0.05,
        passes_goodness_of_fit: true,
    };
    assert_eq!(model.cvar(0.99), Err(spectra_core::Error::Data("EVT CVaR undefined for shape >= 1".into())));

    let model_heavy = EvtTailModel {
        threshold: 1.0,
        xi: 1.5,
        beta: 2.0,
        total_observations: 100,
        exceedances_count: 20,
        ks_statistic: 0.05,
        passes_goodness_of_fit: true,
    };
    assert_eq!(
        model_heavy.cvar(0.99),
        Err(spectra_core::Error::Data("EVT CVaR undefined for shape >= 1".into()))
    );
}
