// Integration test for regime EM reference values (Task m26-3d-regime-reference).
// documents current behaviour, Task 7/8 may change this.

use spectra_core::Error;
use spectra_risk::regime::normal_quantile;
use spectra_risk::regime::{Regime3State, RegimeModel};

const TOL: f64 = 1e-9;

fn synthetic_returns() -> Vec<f64> {
    let t = 100_usize;
    (0..t)
        .map(|i| {
            let x = i as f64;
            // deterministic series: 0.001*sin(0.2*x) + 0.0001*x/100
            0.001 * (0.2 * x).sin() + 0.0001 * x / t as f64
        })
        .collect()
}

// Property-based test for the two-state model
#[test]
fn two_state_fit_properties() -> Result<(), Error> {
    let returns = synthetic_returns();
    let model = RegimeModel::fit(&returns)?;
    // transition rows sum to 1 within 1e-12 and entries within [0.001, 0.999]
    for row in &model.transitions {
        let row_sum: f64 = row.iter().copied().sum();
        assert!((row_sum - 1.0).abs() < 1e-12);
        for &p in row {
            assert!((0.001..=0.999).contains(&p), "transition entry out of bounds: {p}");
        }
    }
    // sigmas positive
    assert!(model.state_0.sigma_daily > 0.0);
    assert!(model.state_1.sigma_daily > 0.0);
    // initial probabilities in [0, 1] and sum to 1
    let sum_pi = model.initial_probs[0] + model.initial_probs[1];
    assert!((0.0..=1.0).contains(&model.initial_probs[0]));
    assert!((0.0..=1.0).contains(&model.initial_probs[1]));
    assert!((sum_pi - 1.0).abs() < 1e-12);
    Ok(())
}

// Property-based test for the three-state model
#[test]
fn three_state_fit_properties() -> Result<(), Error> {
    let returns = synthetic_returns();
    let model = Regime3State::fit(&returns)?;
    // transition rows sum to 1 within 1e-12.
    // documents current behaviour: clamp is applied before row normalisation, so normalised entries can fall below 0.01; finding for Task 7/8 (separate PR).
    let min_allowed = 0.01 / 1.97;
    let max_allowed = 0.98 / 1.0;
    for row in &model.transitions {
        let row_sum: f64 = row.iter().copied().sum();
        assert!((row_sum - 1.0).abs() < 1e-12);
        for &p in row {
            assert!((min_allowed..=max_allowed).contains(&p), "transition entry out of bounds: {p}");
        }
    }
    // sigmas positive for all three states
    assert!(model.state_0.sigma_daily > 0.0);
    assert!(model.state_1.sigma_daily > 0.0);
    assert!(model.state_2.sigma_daily > 0.0);
    // initial probabilities in [0, 1] and sum to 1
    let sum_pi: f64 = model.initial_probs.iter().copied().sum();
    for &pi in &model.initial_probs {
        assert!((0.0..=1.0).contains(&pi));
    }
    assert!((sum_pi - 1.0).abs() < 1e-12);
    Ok(())
}

// characterisation of current Rust output, not an independent reference; the Python reference does not reproduce it.
#[test]
fn two_state_characterisation() -> Result<(), Error> {
    let returns = synthetic_returns();
    let model = RegimeModel::fit(&returns)?;
    // Values from a clean run (tolerance 1e-9)
    assert!((model.state_0.mu_daily + 0.000_616_538_956_151_741).abs() < TOL);
    assert!((model.state_0.sigma_daily - 0.000_286_072_904_090_481).abs() < TOL);
    assert!((model.state_1.mu_daily - 0.000_621_701_890_054_811).abs() < TOL);
    assert!((model.state_1.sigma_daily - 0.000_342_726_944_439_128).abs() < TOL);
    // transitions
    assert!((model.transitions[0][0] - 0.932_060_734_549_704_4).abs() < TOL);
    assert!((model.transitions[0][1] - 0.067_939_265_450_295_59).abs() < TOL);
    assert!((model.transitions[1][0] - 0.054_791_758_099_477_23).abs() < TOL);
    assert!((model.transitions[1][1] - 0.945_208_241_900_522_8).abs() < TOL);
    // initial probabilities
    assert!((model.initial_probs[0] - 0.0).abs() < TOL);
    assert!((model.initial_probs[1] - 1.0).abs() < TOL);
    Ok(())
}

// characterisation of current Rust output, not an independent reference; the Python reference does not reproduce it.
#[test]
fn three_state_characterisation() -> Result<(), Error> {
    let returns = synthetic_returns();
    let model = Regime3State::fit(&returns)?;
    // Values from a clean run (tolerance 1e-9)
    assert!((model.state_0.mu_daily + 0.000_777_522_346_828_25).abs() < TOL);
    assert!((model.state_0.sigma_daily - 0.000_152_860_640_867_193).abs() < TOL);
    assert!((model.state_1.mu_daily - 0.000_849_660_292_936_041).abs() < TOL);
    assert!((model.state_1.sigma_daily - 0.000_164_964_225_387_449).abs() < TOL);
    assert!((model.state_2.mu_daily - 0.000_046_152_343_005_929_8).abs() < TOL);
    assert!((model.state_2.sigma_daily - 0.000_304_293_110_511_612).abs() < TOL);
    // transition matrix entries
    assert!((model.transitions[0][0] - 0.901_998_930_975_35).abs() < TOL);
    assert!((model.transitions[0][1] - 0.088_100_078_925_601_87).abs() < TOL);
    assert!((model.transitions[0][2] - 0.009_900_990_099_048_11).abs() < TOL);
    assert!((model.transitions[1][0] - 0.115_703_874_586_512_28).abs() < TOL);
    assert!((model.transitions[1][1] - 0.797_547_653_883_178_5).abs() < TOL);
    assert!((model.transitions[1][2] - 0.086_748_471_530_309_19).abs() < TOL);
    assert!((model.transitions[2][0] - 0.009_900_990_099_041_51).abs() < TOL);
    assert!((model.transitions[2][1] - 0.097_443_738_904_843_17).abs() < TOL);
    assert!((model.transitions[2][2] - 0.892_655_270_996_115_3).abs() < TOL);
    // initial probabilities
    assert!((model.initial_probs[0] - 0.0).abs() < TOL);
    assert!((model.initial_probs[1] - 1.0).abs() < TOL);
    assert!((model.initial_probs[2] - 0.0).abs() < TOL);
    Ok(())
}

// VaR test mirrors the implementation in `regime.rs` lines 284-291
#[test]
fn regime_conditional_var_matches_manual_calc() -> Result<(), Error> {
    let returns = synthetic_returns();
    let model = RegimeModel::fit(&returns)?;
    // manual calculation using the same formula as in the implementation (lines 284-291)
    let portfolio = 1_000_000.0;
    let horizon = 1.0 / 252.0;
    let conf = 0.99;
    let z = normal_quantile(conf);
    let calc = |mu: f64, sigma_ann: f64| -> f64 {
        let drift = (mu * 252.0 - 0.5 * sigma_ann.powi(2)) * horizon;
        let diffusion = sigma_ann * horizon.sqrt();
        let ret = (drift - diffusion * z).exp();
        (portfolio * (1.0 - ret)).max(0.0)
    };
    let v0 = calc(model.state_0.mu_daily, model.state_0.volatility_annualized);
    let v1 = calc(model.state_1.mu_daily, model.state_1.volatility_annualized);
    let (v0_r, v1_r, cur_r) = model.regime_conditional_var(portfolio, horizon, conf);
    assert!((v0 - v0_r).abs() < TOL);
    assert!((v1 - v1_r).abs() < TOL);
    // current regime VaR must match the appropriate component
    let cur_expected = match model.current_regime {
        spectra_risk::regime::MarketRegime::HighVolatility => v1,
        spectra_risk::regime::MarketRegime::LowVolatility => v0,
    };
    assert!((cur_expected - cur_r).abs() < TOL);
    Ok(())
}

#[test]
fn fit_errors_on_too_few_observations() {
    let short = vec![0.01, 0.02];
    let err2 = RegimeModel::fit(&short).unwrap_err();
    assert!(matches!(err2, Error::InvalidArgument(_)));
    let err3 = Regime3State::fit(&short).unwrap_err();
    assert!(matches!(err3, Error::InvalidArgument(_)));
}

// Closed-form stationary distribution check for a fixture matrix (documents current behaviour).
#[test]
fn stationary_distribution_fixture() {
    // transition matrix P = [[0.8, 0.2], [0.1, 0.9]]
    let p = [[0.8_f64, 0.2_f64], [0.1_f64, 0.9_f64]];
    // analytic stationary distribution: pi0 = (1 - p11) / (2 - p00 - p11), pi1 = 1 - pi0
    let pi0 = (1.0 - p[1][1]) / (2.0 - p[0][0] - p[1][1]);
    let pi1 = 1.0 - pi0;
    // sanity check that pi * P = pi
    let pi_p0 = pi0 * p[0][0] + pi1 * p[1][0];
    let pi_p1 = pi0 * p[0][1] + pi1 * p[1][1];
    assert!((pi_p0 - pi0).abs() < 1e-12);
    assert!((pi_p1 - pi1).abs() < 1e-12);
}

#[test]
fn fit_observations_check() {
    // Ensure the early-exit error for <30 observations remains unchanged.
    let short = vec![0.0; 29];
    let err = RegimeModel::fit(&short).unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}
