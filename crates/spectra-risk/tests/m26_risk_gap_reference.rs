//! Reference tests for M26 task 3: uncovered spectra-risk models.
//!
//! Covers:
//! - Risk distribution metrics (volatility, downside deviation, skewness, excess kurtosis)
//! - Monte Carlo convergence criteria (check_convergence)
//! - Deterministic Monte Carlo GBM path (run_reference with sigma = 0)
//! - Stress testing and scenario shock factors (stress_test, scenario)
//! - Pre-trade risk enforcement and kill-switch handling (PreTradeChecker::evaluate)

#![allow(clippy::unreadable_literal)]

use std::collections::HashMap;

use spectra_risk::enforcement::{
    OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, PreTradeChecker, PreTradeVerdict,
    RiskLimits,
};
use spectra_risk::metrics::{downside_deviation, excess_kurtosis, skewness, volatility};
use spectra_risk::{
    check_convergence, run_reference, scenario, stress_test, Portfolio, RiskConfig, RiskEngine,
    SimulationTask,
};

const TOL_METRICS: f64 = 1e-12;
const TOL_MC: f64 = 1e-10;
const TOL_STRESS: f64 = 1e-12;

// ---------------------------------------------------------------------------
// A. Metrics (metrics.rs)
// ---------------------------------------------------------------------------

#[test]
fn test_risk_metrics_reference_fixture() {
    let losses = [-4.0, -2.0, 2.0, 4.0];

    // Volatility: sample stdev with divisor n - 1
    // ss = 16 + 4 + 4 + 16 = 40; divisor = 3; sqrt(40/3) = 3.6514837167011072...
    let vol = volatility(&losses).unwrap();
    let expected_vol = (40.0 / 3.0_f64).sqrt();
    assert!((vol - expected_vol).abs() < TOL_METRICS, "volatility: expected {expected_vol}, got {vol}");
    assert!((vol - 3.6514837167011072).abs() < TOL_METRICS);

    // Downside deviation: RMS of losses above mean (mean = 0.0)
    // Elements above mean: [2.0, 4.0]; ss = 4 + 16 = 20; divisor = count = 2; sqrt(20/2) = sqrt(10)
    let dd = downside_deviation(&losses).unwrap();
    let expected_dd = (10.0_f64).sqrt();
    assert!((dd - expected_dd).abs() < TOL_METRICS, "downside deviation: expected {expected_dd}, got {dd}");
    assert!((dd - 3.1622776601683795).abs() < TOL_METRICS);

    // Skewness: symmetric losses around 0.0 -> skewness = 0.0
    let skew = skewness(&losses).unwrap();
    assert!((skew - 0.0).abs() < TOL_METRICS, "skewness: expected 0.0, got {skew}");

    // Excess kurtosis: documented convention in docs/models/risk-metrics.md
    // Sample standardiser s = sqrt(40/3), outer divisor n = 4.
    // Sum of standardized 4th powers = sum((l_i / s)^4) = 544 * 9 / 1600 = 3.06.
    // Outer division by n: 3.06 / 4 = 0.765 = 136 * 9 / 1600.
    // Excess kurtosis = 0.765 - 3.0 = -2.235 (exact).
    // Note: Population moments would yield -1.64; -2.235 is the documented divergence.
    let kurt = excess_kurtosis(&losses).unwrap();
    let expected_kurt = 0.765 - 3.0;
    assert!(
        (kurt - expected_kurt).abs() < TOL_METRICS,
        "excess kurtosis: expected {expected_kurt}, got {kurt}"
    );
    assert!((kurt - (-2.235)).abs() < TOL_METRICS);

    // Errors: sample count guards
    assert!(excess_kurtosis(&[1.0, 2.0, 3.0]).is_err(), "excess_kurtosis of 3 samples must be Err");
    assert!(skewness(&[1.0, 2.0]).is_err(), "skewness of 2 samples must be Err");
    assert!(volatility(&[]).is_err(), "volatility of empty slice must be Err");

    // Constant series: skewness and excess_kurtosis return 0.0 when stdev is 0
    let constant = [5.0, 5.0, 5.0, 5.0];
    assert_eq!(skewness(&constant).unwrap(), 0.0);
    assert_eq!(excess_kurtosis(&constant).unwrap(), 0.0);
}

// ---------------------------------------------------------------------------
// B. Convergence (engine.rs, check_convergence)
// ---------------------------------------------------------------------------

#[test]
fn test_convergence_reference_fixture() {
    // Fewer than 200 losses: converged == false, relative_std_error == 1.0
    let short_losses = vec![10.0; 199];
    let res_short = check_convergence(&short_losses, 0.90, 0.01);
    assert!(!res_short.converged);
    assert_eq!(res_short.relative_std_error, 1.0);

    // 250 identical losses, confidence 0.90, target 0.01:
    // Tail length = 250 >= 30, tail mean = 10.0, variance = 0.0, standard error = 0.0, relative error = 0.0.
    // Converged == true, relative_std_error == 0.0 exactly.
    let identical_losses = vec![10.0; 250];
    let res_identical = check_convergence(&identical_losses, 0.90, 0.01);
    assert!(res_identical.converged);
    assert_eq!(res_identical.relative_std_error, 0.0);
}

// ---------------------------------------------------------------------------
// C. Monte Carlo deterministic path (simulation.rs, run_reference)
// ---------------------------------------------------------------------------

#[test]
fn test_monte_carlo_deterministic_path() {
    // S0 = 100_000, mu = 0.05, sigma = 0, T = 1.0.
    // In simulation.rs fill():
    //   drift_term = (task.drift - 0.5 * sigma^2) * T = 0.05 * 1.0 = 0.05
    //   diffusion = sigma * sqrt(T) = 0.0
    //   terminal = initial_value * exp(drift_term + diffusion * z) = 100_000 * exp(0.05)
    //   loss = initial_value - terminal = 100_000 * (1 - exp(0.05))
    // With mu > 0, terminal > initial_value, so loss is negative (portfolio gained value).
    let task = SimulationTask {
        num_paths: 10,
        seed: 42,
        drift: 0.05,
        volatility: 0.0,
        initial_value: 100_000.0,
        horizon_years: 1.0,
    };
    let sim = run_reference(&task).unwrap();
    assert_eq!(sim.terminal_losses.len(), 10);

    let expected_loss = 100_000.0 * (1.0 - (0.05_f64).exp());
    // -5127.10963760241...
    for loss in &sim.terminal_losses {
        assert!((loss - expected_loss).abs() < TOL_MC, "MC loss: expected {expected_loss}, got {loss}");
        assert!((loss - (-5127.10963760241)).abs() < 1e-9);
    }
}

// ---------------------------------------------------------------------------
// D. Stress testing (engine.rs, stress_test, scenario)
// ---------------------------------------------------------------------------

#[test]
fn test_stress_testing_reference_fixture() {
    let mut portfolio = Portfolio::new();
    portfolio.add_asset("AAPL", 100.0, 150.00).add_asset("MSFT", 50.0, 300.00);

    let engine = RiskEngine::new();
    let cfg = RiskConfig { simulations: 1_000, ..RiskConfig::default() };

    let res = stress_test(&engine, &portfolio, "market-crash", &cfg).unwrap();
    assert!((res.value_before - 30_000.00).abs() < TOL_STRESS);
    assert!((res.value_after - 21_000.00).abs() < TOL_STRESS);
    assert!((res.scenario_loss - 9_000.00).abs() < TOL_STRESS);

    // Scenario lookup checks
    let crash = scenario("market-crash").unwrap();
    assert_eq!(crash.shock_factor, 0.70);

    assert!(scenario("no-such-name").is_err());
}

// ---------------------------------------------------------------------------
// E. Pre-trade risk enforcement (enforcement.rs, PreTradeChecker::evaluate)
// ---------------------------------------------------------------------------

#[test]
fn test_pre_trade_enforcement_kill_switch_fixture() {
    // Portfolio constructor: AAPL +50 shares @ 100.00
    // Constructor code:
    // let mut positions = HashMap::new();
    // positions.insert("AAPL".to_string(), PositionSnapshot { qty: 50.0, price: 100.0 });
    // let portfolio = PortfolioSnapshot { positions, ..PortfolioSnapshot::default() };
    let mut positions = HashMap::new();
    positions.insert("AAPL".to_string(), PositionSnapshot { qty: 50.0, price: 100.0 });
    let portfolio = PortfolioSnapshot { positions, ..PortfolioSnapshot::default() };

    let limits = RiskLimits::default();

    // 1. Buy 10 AAPL: opening order while kill switch active -> Rejected, approved 0.0, breach "kill_switch_active"
    let buy_order =
        OrderCandidate { symbol: "AAPL".to_string(), side: OrderSide::Buy, qty: 10.0, price: 100.0 };
    let res_buy = PreTradeChecker::evaluate(&limits, &portfolio, &buy_order, true).unwrap();
    assert_eq!(res_buy.verdict, PreTradeVerdict::Rejected);
    assert_eq!(res_buy.approved_qty, 0.0);
    assert!(res_buy.breaches.iter().any(|b| b.code == "kill_switch_active"));

    // 2. Sell 20 AAPL: closing order <= 50 shares -> Approved, approved 20.0
    let sell_close =
        OrderCandidate { symbol: "AAPL".to_string(), side: OrderSide::Sell, qty: 20.0, price: 100.0 };
    let res_close = PreTradeChecker::evaluate(&limits, &portfolio, &sell_close, true).unwrap();
    assert_eq!(res_close.verdict, PreTradeVerdict::Approved);
    assert_eq!(res_close.approved_qty, 20.0);
    assert!(res_close.breaches.is_empty());

    // 3. Sell 80 AAPL with size_reduction_enabled = true: exceeds 50 shares -> Reduced, approved 50.0, breach "kill_switch_closing_only"
    let limits_reduced = RiskLimits { size_reduction_enabled: true, ..RiskLimits::default() };
    let sell_oversize =
        OrderCandidate { symbol: "AAPL".to_string(), side: OrderSide::Sell, qty: 80.0, price: 100.0 };
    let res_reduced = PreTradeChecker::evaluate(&limits_reduced, &portfolio, &sell_oversize, true).unwrap();
    assert_eq!(res_reduced.verdict, PreTradeVerdict::Reduced);
    assert_eq!(res_reduced.approved_qty, 50.0);
    assert!(res_reduced.breaches.iter().any(|b| b.code == "kill_switch_closing_only"));
}
