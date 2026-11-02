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

fn synthetic_3state_returns() -> Vec<f64> {
    let mut rng_state = 123_456_789_u64;
    let mut next_u01 = || -> f64 {
        rng_state = rng_state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (rng_state >> 33) as f64 / 2_147_483_648.0
    };
    let mut transitions_series = Vec::with_capacity(300);
    let mut cur_state = 0;
    for _ in 0..300 {
        let u = next_u01();
        cur_state = match cur_state {
            0 => {
                if u < 0.7 {
                    0
                } else if u < 0.9 {
                    1
                } else {
                    2
                }
            }
            1 => {
                if u < 0.15 {
                    0
                } else if u < 0.85 {
                    1
                } else {
                    2
                }
            }
            _ => {
                if u < 0.1 {
                    0
                } else if u < 0.3 {
                    1
                } else {
                    2
                }
            }
        };
        let noise = (next_u01() - 0.5) * 0.001;
        let val = match cur_state {
            0 => 0.001 + noise,
            1 => -0.0005 + noise * 3.0,
            _ => -0.005 + noise * 10.0,
        };
        transitions_series.push(val);
    }
    transitions_series
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
    let returns = synthetic_3state_returns();
    let model = Regime3State::fit(&returns)?;
    // transition rows sum to 1 within 1e-12 and all normalised entries within [0.01, 0.98]
    for row in &model.transitions {
        let row_sum: f64 = row.iter().copied().sum();
        assert!((row_sum - 1.0).abs() < 1e-12);
        for &p in row {
            assert!((0.01..=0.98).contains(&p), "transition entry out of bounds: {p}");
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
    let returns = synthetic_3state_returns();
    let model = Regime3State::fit(&returns)?;
    // Values from a clean run (tolerance 1e-9)
    assert!((model.state_0.mu_daily - 0.000_445_552_853_191_599).abs() < TOL);
    assert!((model.state_0.sigma_daily - 0.000_882_121_307_649_567).abs() < TOL);
    assert!((model.state_1.mu_daily + 0.006_576_719_172_874_858).abs() < TOL);
    assert!((model.state_1.sigma_daily - 0.002_181_632_255_174_091).abs() < TOL);
    assert!((model.state_2.mu_daily + 0.003_399_730_772_805_249).abs() < TOL);
    assert!((model.state_2.sigma_daily - 0.002_766_703_876_852_641).abs() < TOL);
    // transition matrix entries
    assert!((model.transitions[0][0] - 0.905_871_661_402_667_6).abs() < TOL);
    assert!((model.transitions[0][1] - 0.018_372_360_743_176_74).abs() < TOL);
    assert!((model.transitions[0][2] - 0.075_755_977_854_155_57).abs() < TOL);
    assert!((model.transitions[1][0] - 0.238_035_193_023_473_04).abs() < TOL);
    assert!((model.transitions[1][1] - 0.606_859_618_987_887).abs() < TOL);
    assert!((model.transitions[1][2] - 0.155_105_187_988_639_92).abs() < TOL);
    assert!((model.transitions[2][0] - 0.057_167_355_963_997_49).abs() < TOL);
    assert!((model.transitions[2][1] - 0.452_618_945_405_069_75).abs() < TOL);
    assert!((model.transitions[2][2] - 0.490_213_698_630_932_84).abs() < TOL);
    // initial probabilities
    assert!((model.initial_probs[0] - 0.0).abs() < TOL);
    assert!((model.initial_probs[1] - 0.000_000_000_000_784_171).abs() < TOL);
    assert!((model.initial_probs[2] - 0.999_999_999_999_215_8).abs() < TOL);
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

#[test]
fn two_state_fit_drives_transition_to_clamp_returns_error() {
    // Two blocks of 50 constant observations with large separation drive transition to 0.999 clamp
    let mut series = vec![0.001; 50];
    series.extend(vec![0.05; 50]);
    let err = RegimeModel::fit(&series).unwrap_err();
    assert_eq!(err, Error::Data("regime transition probability hit clamp 0.999; fit is degenerate".into()));

    // 95 observations of 0.001 and 5 rare outliers drive transition to 0.001 clamp
    let mut series_outlier = vec![0.001; 95];
    series_outlier.extend(vec![0.1; 5]);
    let err_outlier = RegimeModel::fit(&series_outlier).unwrap_err();
    assert_eq!(
        err_outlier,
        Error::Data("regime transition probability hit clamp 0.001; fit is degenerate".into())
    );
}

#[test]
fn three_state_fit_synthetic_returns_hits_clamp() {
    // synthetic_returns() has only 1 harmonic component, driving 3-state transition [0][2] to clamp 0.01
    let returns = synthetic_returns();
    let err = Regime3State::fit(&returns).unwrap_err();
    assert_eq!(err, Error::Data("regime transition probability hit clamp 0.01; fit is degenerate".into()));
}
