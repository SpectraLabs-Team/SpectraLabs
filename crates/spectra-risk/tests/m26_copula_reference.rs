//! Property checks and silent-default characterisation for copula models in `spectra-risk`.
//!
//! Covers:
//! - Gaussian copula: standard normal margins after log-return transform, and sample correlation
//!   matching the input target within tolerance over a large N paths.
//! - Student-t copula: heavy-tailed margins generated via sqrt(nu / chi2) scaling; amplified
//!   extreme tail events and higher CVaR relative to Gaussian copula.
//! - Clayton copula: downside CVaR amplification under the current non-standard construction.
//! - Silent defaults / unvalidated fallback behaviour (documented findings for Task 7/8).

use std::collections::HashMap;

use spectra_risk::copula::{simulate_correlated, DependenceModel};
use spectra_risk::enforcement::{PortfolioSnapshot, PositionSnapshot};
use spectra_risk::portfolio::Portfolio;
use spectra_risk::returns::{correlation, mean, variance};
use spectra_risk::DEFAULT_VOLATILITY;

/// Reconstruct correlated standard normal variate x from asset loss:
///   terminal = initial * exp(drift_term + diffusion * x)
///   loss = initial - terminal => terminal / initial = 1 - loss / initial
///   x = (ln(1 - loss / initial) - drift_term) / diffusion
fn reconstruct_standard_variate(loss: f64, initial_value: f64, vol: f64, horizon_years: f64) -> f64 {
    let drift_term = -0.5 * vol * vol * horizon_years;
    let diffusion = vol * horizon_years.sqrt();
    let ret = (1.0 - loss / initial_value).ln();
    (ret - drift_term) / diffusion
}

#[test]
fn test_gaussian_copula_margins_and_target_correlation() {
    // Property check: Gaussian copula margins and correlation.
    // Over N = 50_000 paths:
    // - Reconstructed standard normal draws x_i have sample mean within [-0.025, 0.025]
    //   and sample variance within [0.96, 1.04] (standard error on mean ~ 1/sqrt(N) = 0.0045,
    //   standard error on variance ~ sqrt(2/N) = 0.0063; tolerance 0.04 is > 6 sigma).
    // - Sample correlation between x_0 and x_1 is within tolerance 0.020 of target rho = 0.60
    //   (standard error on correlation is (1 - rho^2)/sqrt(N) = 0.00286; tolerance is ~ 7 sigma).
    // - Margin distributions: empirical normal fractions match standard normal theory:
    //     P(x < 0) in [0.485, 0.515] (theory 0.500)
    //     P(|x| <= 1.0) in [0.665, 0.700] (theory 0.6827)
    //     P(|x| <= 2.0) in [0.940, 0.970] (theory 0.9545)
    //     P(|x| <= 3.0) in [0.990, 1.000] (theory 0.9973)
    let n_sims = 50_000;
    let seed = 42;
    let horizon = 1.0 / 252.0;
    let target_rho = 0.60;

    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.30);

    let corr_matrix = vec![vec![1.0, target_rho], vec![target_rho, 1.0]];

    let res = simulate_correlated(
        &p,
        Some(&corr_matrix),
        DependenceModel::GaussianCopula,
        n_sims,
        seed,
        horizon,
        0.99,
    )
    .expect("Gaussian copula simulation should succeed");

    let initial_a = 1_000.0;
    let initial_b = 1_000.0;
    let vol_a = 0.20;
    let vol_b = 0.30;

    let x_a: Vec<f64> = res.asset_losses[0]
        .iter()
        .map(|&loss| reconstruct_standard_variate(loss, initial_a, vol_a, horizon))
        .collect();
    let x_b: Vec<f64> = res.asset_losses[1]
        .iter()
        .map(|&loss| reconstruct_standard_variate(loss, initial_b, vol_b, horizon))
        .collect();

    // Check normal margins for Asset A
    let mean_a = mean(&x_a);
    let var_a = variance(&x_a);
    assert!(
        (-0.025..=0.025).contains(&mean_a),
        "Asset A sample mean {mean_a} should be near 0.0 within tolerance 0.025"
    );
    assert!(
        (0.96..=1.04).contains(&var_a),
        "Asset A sample variance {var_a} should be near 1.0 within tolerance 0.04"
    );

    // Check normal margins for Asset B
    let mean_b = mean(&x_b);
    let var_b = variance(&x_b);
    assert!(
        (-0.025..=0.025).contains(&mean_b),
        "Asset B sample mean {mean_b} should be near 0.0 within tolerance 0.025"
    );
    assert!(
        (0.96..=1.04).contains(&var_b),
        "Asset B sample variance {var_b} should be near 1.0 within tolerance 0.04"
    );

    // Empirical quantile / tail fractions for Asset A
    let n = x_a.len() as f64;
    let frac_neg = x_a.iter().filter(|&&v| v < 0.0).count() as f64 / n;
    let frac_1sigma = x_a.iter().filter(|&&v| v.abs() <= 1.0).count() as f64 / n;
    let frac_2sigma = x_a.iter().filter(|&&v| v.abs() <= 2.0).count() as f64 / n;
    let frac_3sigma = x_a.iter().filter(|&&v| v.abs() <= 3.0).count() as f64 / n;

    assert!((0.485..=0.515).contains(&frac_neg), "P(x < 0) was {frac_neg}, expected in [0.485, 0.515]");
    assert!(
        (0.665..=0.700).contains(&frac_1sigma),
        "P(|x| <= 1.0) was {frac_1sigma}, expected in [0.665, 0.700]"
    );
    assert!(
        (0.940..=0.970).contains(&frac_2sigma),
        "P(|x| <= 2.0) was {frac_2sigma}, expected in [0.940, 0.970]"
    );
    assert!(
        (0.990..=1.000).contains(&frac_3sigma),
        "P(|x| <= 3.0) was {frac_3sigma}, expected in [0.990, 1.000]"
    );

    // Sample correlation between reconstructed normal variates
    let sample_rho = correlation(&x_a, &x_b).expect("Correlation calculation should succeed");
    assert!(
        (sample_rho - target_rho).abs() < 0.020,
        "Sample correlation {sample_rho} should match target {target_rho} within tolerance 0.020"
    );
}

#[test]
fn test_student_t_copula_heavier_tails_than_gaussian() {
    // Property check: Student-t copula produces heavier tails than Gaussian copula
    // with the implemented scale = sqrt(nu / sum_sq) construction.
    // For nu = 4:
    // - Theoretical variance of t(4) is nu / (nu - 2) = 2.0 (vs 1.0 for Gaussian).
    // - Extreme tail count (|x| > 3.0) is markedly higher (> 5x Gaussian).
    // - Portfolio 99% CVaR under Student-t is strictly greater than under Gaussian copula.
    let n_sims = 50_000;
    let seed = 42;
    let horizon = 1.0 / 252.0;
    let target_rho = 0.50;

    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.20);

    let corr_matrix = vec![vec![1.0, target_rho], vec![target_rho, 1.0]];

    let res_gauss = simulate_correlated(
        &p,
        Some(&corr_matrix),
        DependenceModel::GaussianCopula,
        n_sims,
        seed,
        horizon,
        0.99,
    )
    .expect("Gaussian simulation failed");

    let res_t = simulate_correlated(
        &p,
        Some(&corr_matrix),
        DependenceModel::TCopula { degrees_of_freedom: 4 },
        n_sims,
        seed,
        horizon,
        0.99,
    )
    .expect("Student-t simulation failed");

    let initial_val = 1_000.0;
    let vol = 0.20;

    let x_gauss: Vec<f64> = res_gauss.asset_losses[0]
        .iter()
        .map(|&l| reconstruct_standard_variate(l, initial_val, vol, horizon))
        .collect();
    let x_t: Vec<f64> = res_t.asset_losses[0]
        .iter()
        .map(|&l| reconstruct_standard_variate(l, initial_val, vol, horizon))
        .collect();

    let var_gauss = variance(&x_gauss);
    let var_t = variance(&x_t);
    // Student-t(4) variance approaches 2.0; must be substantially higher than Gaussian variance (~1.0)
    assert!(
        var_t > 1.6 && var_gauss < 1.1,
        "Student-t variance ({var_t}) must exceed Gaussian variance ({var_gauss})"
    );

    // Extreme observations (|x| > 3.0): t(4) has ~3.98% tail vs normal's 0.27%
    let extreme_gauss = x_gauss.iter().filter(|&&v| v.abs() > 3.0).count();
    let extreme_t = x_t.iter().filter(|&&v| v.abs() > 3.0).count();
    assert!(
        extreme_t > 5 * extreme_gauss,
        "Student-t extreme tail count ({extreme_t}) must be > 5x Gaussian ({extreme_gauss})"
    );

    // CVaR under Student-t must exceed Gaussian CVaR at the same correlation
    let cvar_gauss: f64 = res_gauss.contributions.iter().map(|c| c.component_cvar).sum();
    let cvar_t: f64 = res_t.contributions.iter().map(|c| c.component_cvar).sum();
    assert!(
        cvar_t > cvar_gauss,
        "Student-t portfolio CVaR ({cvar_t}) must exceed Gaussian CVaR ({cvar_gauss})"
    );
}

#[test]
fn test_clayton_copula_amplifies_downside_cvar() {
    // finding for Task 7/8: Clayton construction is not a standard Clayton sampler; not verified against theory.
    // The implementation draws u_common = (|N| + 0.1).ln().abs() and scales only negative z.
    // Assert the current behaviour: CVaR under Clayton >= Gaussian at the same inputs.
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.20);

    let corr_matrix = vec![vec![1.0, 0.40], vec![0.40, 1.0]];

    let res_gauss = simulate_correlated(
        &p,
        Some(&corr_matrix),
        DependenceModel::GaussianCopula,
        10_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Gaussian simulation failed");

    let res_clayton = simulate_correlated(
        &p,
        Some(&corr_matrix),
        DependenceModel::ClaytonCopula { theta: 2.0 },
        10_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Clayton simulation failed");

    let cvar_gauss: f64 = res_gauss.contributions.iter().map(|c| c.component_cvar).sum();
    let cvar_clayton: f64 = res_clayton.contributions.iter().map(|c| c.component_cvar).sum();

    assert!(
        cvar_clayton >= cvar_gauss,
        "Clayton downside CVaR ({cvar_clayton}) must be >= Gaussian CVaR ({cvar_gauss})"
    );
}

#[test]
fn test_dof_floor_silent_default() {
    // finding for Task 7/8: degrees_of_freedom floor max(1) silently clamps dof 0 to 1 without warning or validation error.
    // In copula.rs line 173: let nu = degrees_of_freedom.max(1);
    // Verifies dof 0 produces identical simulation output to dof 1 under the same seed,
    // and that dof 0 and dof 1 are floored at 1 (distinct from dof 2).
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.20);

    let res_dof0 = simulate_correlated(
        &p,
        None,
        DependenceModel::TCopula { degrees_of_freedom: 0 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("TCopula dof 0 failed");

    let res_dof1 = simulate_correlated(
        &p,
        None,
        DependenceModel::TCopula { degrees_of_freedom: 1 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("TCopula dof 1 failed");

    let res_dof2 = simulate_correlated(
        &p,
        None,
        DependenceModel::TCopula { degrees_of_freedom: 2 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("TCopula dof 2 failed");

    assert_eq!(
        res_dof0.portfolio_losses, res_dof1.portfolio_losses,
        "dof 0 must behave identically to dof 1 due to max(1) clamp"
    );
    assert_ne!(res_dof0.portfolio_losses, res_dof2.portfolio_losses, "dof 0 must not floor to dof 2");
    assert_ne!(res_dof1.portfolio_losses, res_dof2.portfolio_losses, "dof 1 must not floor to dof 2");
}

#[test]
fn test_theta_floor_silent_default() {
    // finding for Task 7/8: theta floor max(0.1) silently clamps theta <= 0.0 to 0.1 without warning or validation error.
    // In copula.rs line 185: let th = theta.max(0.1);
    // Verifies theta 0.0 produces identical simulation output to theta 0.1 under the same seed,
    // and that theta 0.0 and 0.1 are floored at 0.1 (distinct from theta 0.5).
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.20);

    let res_th0 = simulate_correlated(
        &p,
        None,
        DependenceModel::ClaytonCopula { theta: 0.0 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Clayton theta 0 failed");

    let res_th0_1 = simulate_correlated(
        &p,
        None,
        DependenceModel::ClaytonCopula { theta: 0.1 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Clayton theta 0.1 failed");

    let res_th0_5 = simulate_correlated(
        &p,
        None,
        DependenceModel::ClaytonCopula { theta: 0.5 },
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Clayton theta 0.5 failed");

    assert_eq!(
        res_th0.portfolio_losses, res_th0_1.portfolio_losses,
        "theta 0.0 must behave identically to theta 0.1 due to max(0.1) clamp"
    );
    assert_ne!(res_th0.portfolio_losses, res_th0_5.portfolio_losses, "theta 0.0 must not floor to theta 0.5");
    assert_ne!(
        res_th0_1.portfolio_losses, res_th0_5.portfolio_losses,
        "theta 0.1 must not floor to theta 0.5"
    );
}

#[test]
fn test_identity_correlation_fallback_when_none() {
    // finding for Task 7/8: None correlation silently falls back to identity matrix (uncorrelated assets).
    // In copula.rs lines 124-130: when correlation is None, generates an n_assets x n_assets identity matrix.
    let mut p = Portfolio::new();
    p.add_asset_with_volatility("ASSET_A", 10.0, 100.0, 0.20)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.30);

    let identity = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
    let correlated = vec![vec![1.0, 0.80], vec![0.80, 1.0]];

    let res_none =
        simulate_correlated(&p, None, DependenceModel::GaussianCopula, 1_000, 42, 1.0 / 252.0, 0.99)
            .expect("Simulation with None correlation failed");

    let res_id = simulate_correlated(
        &p,
        Some(&identity),
        DependenceModel::GaussianCopula,
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Simulation with identity correlation failed");

    let res_corr = simulate_correlated(
        &p,
        Some(&correlated),
        DependenceModel::GaussianCopula,
        1_000,
        42,
        1.0 / 252.0,
        0.99,
    )
    .expect("Simulation with correlated matrix failed");

    assert_eq!(
        res_none.portfolio_losses, res_id.portfolio_losses,
        "Passing correlation: None must produce identical losses to explicit identity matrix"
    );
    assert_ne!(
        res_none.portfolio_losses, res_corr.portfolio_losses,
        "Uncorrelated fallback must produce distinct losses from correlated simulation"
    );
}

#[test]
fn test_default_volatility_fallback_when_nonpositive() {
    // finding for Task 7/8: asset volatility <= 0.0 silently falls back to DEFAULT_VOLATILITY (0.20).
    // In copula.rs line 145 and engine.rs line 158:
    //   let vol = if asset.volatility > 0.0 { asset.volatility } else { default_vol };
    // Portfolio::validate() requires volatility >= 0.0, so valid inputs with volatility 0.0
    // (such as default unpopulated assets via Portfolio::add_asset) silently use 0.20 instead of erroring.
    let mut p_zero = Portfolio::new();
    p_zero.add_asset("ASSET_A", 10.0, 100.0).add_asset_with_volatility("ASSET_B", 10.0, 100.0, 0.0);

    let mut p_explicit = Portfolio::new();
    p_explicit
        .add_asset_with_volatility("ASSET_A", 10.0, 100.0, DEFAULT_VOLATILITY)
        .add_asset_with_volatility("ASSET_B", 10.0, 100.0, DEFAULT_VOLATILITY);

    let res_zero =
        simulate_correlated(&p_zero, None, DependenceModel::GaussianCopula, 1_000, 42, 1.0 / 252.0, 0.99)
            .expect("Simulation with zero volatility failed");

    let res_explicit =
        simulate_correlated(&p_explicit, None, DependenceModel::GaussianCopula, 1_000, 42, 1.0 / 252.0, 0.99)
            .expect("Simulation with explicit DEFAULT_VOLATILITY failed");

    assert_eq!(
        res_zero.portfolio_losses, res_explicit.portfolio_losses,
        "Volatility 0.0 must silently fall back to DEFAULT_VOLATILITY (0.20)"
    );
    assert!(
        res_zero.portfolio_losses.iter().any(|&l| l.abs() > 1e-4),
        "Simulated losses with DEFAULT_VOLATILITY fallback must be non-zero"
    );
}

#[test]
fn test_parser_defaults_in_dependence_model_parse() {
    // finding for Task 7/8: DependenceModel::parse() hardcodes silent defaults:
    // "t_copula" -> dof 4, "clayton" -> theta 2.0.
    assert_eq!(
        DependenceModel::parse("t_copula").expect("parse t_copula"),
        DependenceModel::TCopula { degrees_of_freedom: 4 }
    );
    assert_eq!(
        DependenceModel::parse("student_t").expect("parse student_t"),
        DependenceModel::TCopula { degrees_of_freedom: 4 }
    );
    assert_eq!(
        DependenceModel::parse("t").expect("parse t"),
        DependenceModel::TCopula { degrees_of_freedom: 4 }
    );
    assert_eq!(
        DependenceModel::parse("clayton").expect("parse clayton"),
        DependenceModel::ClaytonCopula { theta: 2.0 }
    );
    assert_eq!(
        DependenceModel::parse("clayton_copula").expect("parse clayton_copula"),
        DependenceModel::ClaytonCopula { theta: 2.0 }
    );
    assert_eq!(DependenceModel::parse("gaussian").expect("parse gaussian"), DependenceModel::GaussianCopula);
    assert_eq!(DependenceModel::parse("aggregate").expect("parse aggregate"), DependenceModel::Aggregate);
}

#[test]
fn test_enforcement_var_pct_hardcodes_t_copula_dof_4() {
    // finding for Task 7/8: PortfolioSnapshot::var_pct hardcodes TCopula with degrees_of_freedom = 4
    // without allowing caller configuration (enforcement.rs line 143).
    let mut positions = HashMap::new();
    positions.insert("AAPL".to_string(), PositionSnapshot { qty: 100.0, price: 150.0, volatility: 0.20 });
    positions.insert("MSFT".to_string(), PositionSnapshot { qty: 50.0, price: 300.0, volatility: 0.20 });

    let snap = PortfolioSnapshot {
        cash: 50_000.0,
        equity: 100_000.0,
        peak_equity: 100_000.0,
        daily_start_equity: 100_000.0,
        positions,
    };

    let var_default = snap.var_pct();
    let var_t4 = snap.var_pct_with_model(DependenceModel::TCopula { degrees_of_freedom: 4 });
    let var_gauss = snap.var_pct_with_model(DependenceModel::GaussianCopula);

    assert!(
        (var_default - var_t4).abs() < 1e-12,
        "var_pct ({var_default}) must match var_pct_with_model with TCopula {{ degrees_of_freedom: 4 }} ({var_t4})"
    );
    assert_ne!(
        var_default, var_gauss,
        "var_pct under TCopula(4) must produce distinct VaR from Gaussian copula ({var_gauss})"
    );
}
