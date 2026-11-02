//! Reference-value tests for research gaps: CSCV PBO, selection validation report,
//! and trade statistics (M26 Task 3 gap closure).

use spectra_core::Money;
use spectra_research::pbo::cscv_pbo;
use spectra_research::sweep::{Trial, TrialLedger, TrialSummary};
use spectra_research::trades::{compute_trade_stats, Trade};
use spectra_research::validation::validate_selection;

const TOL: f64 = 1e-12;

fn close(actual: f64, expected: f64, tol: f64, label: &str) {
    let diff = (actual - expected).abs();
    assert!(diff <= tol, "{label}: expected {expected}, got {actual} (diff {diff} exceeds tol {tol})");
}

// ---------------------------------------------------------------------------
// A. CSCV PBO (pbo.rs, cscv_pbo(returns, n_blocks))
// ---------------------------------------------------------------------------

#[test]
fn pbo_zero_for_dominant_strategy() {
    // N=2 strategies, n_blocks=4, 2 observations per block (T=8). Non-constant returns.
    // Strategy 0 consistently outperforms Strategy 1 across all blocks.
    let strat0 = vec![0.03, 0.05, 0.03, 0.05, 0.03, 0.05, 0.03, 0.05];
    let strat1 = vec![-0.03, -0.05, -0.03, -0.05, -0.03, -0.05, -0.03, -0.05];
    let res = cscv_pbo(&[strat0, strat1], 4).expect("cscv_pbo should succeed");

    assert_eq!(res.combinations, 6);
    // Overfit condition: logit <= 0.0. For dominant strategy, overfit count is 0.
    let overfit_count = (res.pbo * res.combinations as f64).round() as usize;
    assert_eq!(overfit_count, 0, "dominant strategy must have 0 overfit combinations");
    assert_eq!(res.pbo, 0.0, "dominant strategy PBO must be exactly 0.0");
}

#[test]
fn pbo_one_for_anti_dominant_strategy() {
    // Strategy 0 performs well on blocks 0 & 1, but poorly on blocks 2 & 3.
    // Strategy 1 is its exact negation.
    let strat0 = vec![0.03, 0.05, 0.03, 0.05, -0.03, -0.05, -0.03, -0.05];
    let strat1: Vec<f64> = strat0.iter().map(|&x| -x).collect();
    let res = cscv_pbo(&[strat0, strat1], 4).expect("cscv_pbo should succeed");

    assert_eq!(res.combinations, 6);
    // Overfit condition: logit <= 0.0. For anti-dominant strategy, all combinations overfit.
    let overfit_count = (res.pbo * res.combinations as f64).round() as usize;
    assert_eq!(overfit_count, 6, "anti-dominant strategy must have 6 overfit combinations");
    assert_eq!(res.pbo, 1.0, "anti-dominant strategy PBO must be exactly 1.0");
}

// ---------------------------------------------------------------------------
// B. Validation report (validation.rs, validate_selection)
// ---------------------------------------------------------------------------

fn make_validation_fixture() -> (Trial, TrialLedger) {
    // 20 returns: nineteen 0.0 and one 10.0
    let mut returns = vec![0.0; 19];
    returns.push(10.0);

    let trial = Trial { returns: returns.clone(), ..Trial::default() };

    let mut ledger = TrialLedger::default();
    ledger.add_summary(TrialSummary { id: 1, n_obs: returns.len(), ..TrialSummary::default() });

    (trial, ledger)
}

#[test]
fn test_moments_and_kurtosis() {
    let (trial, ledger) = make_validation_fixture();
    let rep = validate_selection(&trial, &ledger, 252.0, None, 0.95);

    // raw_kurtosis = m4 / (m2^2) = (6517/16) / (361/16) = 6517.0 / 361.0
    close(rep.raw_kurtosis, 6517.0 / 361.0, 1e-10, "raw_kurtosis equals 6517/361");

    // Skewness: m3 / (m2^1.5) = (6840/160) / (19/4)^1.5 = 42.75 / (4.75 * sqrt(4.75)) = 18.0 / sqrt(19.0)
    let expected_skew = 18.0 / 19.0f64.sqrt();
    close(rep.skew, expected_skew, 1e-10, "skew equals 18 / sqrt(19)");

    // validation.rs:57:
    // let sharpe_pp = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
    // Here mean = 0.5, and sample stdev = sqrt(95.0 / 19.0) = sqrt(5.0).
    let sample_stdev = 5.0f64.sqrt();
    let expected_sharpe_pp = 0.5 / sample_stdev;
    close(rep.sharpe_per_period, expected_sharpe_pp, 1e-10, "sharpe_per_period = 0.5 / sample stdev");
}

#[test]
fn test_warning_strings() {
    let (trial, ledger) = make_validation_fixture();
    let rep = validate_selection(&trial, &ledger, 252.0, None, 0.95);

    let warnings = &rep.warnings;

    assert!(
        warnings.iter().any(|w| w
            .contains("Short sample (20 observations): Sharpe, skew and kurtosis estimates are unreliable.")),
        "Expected short sample warning in: {warnings:?}"
    );

    assert!(
        warnings.iter().any(|w| w.contains("Only one trial recorded: no multiple-testing adjustment was applied, so DSR equals PSR. If other variants were tried, add them to the ledger.")),
        "Expected single trial warning in: {warnings:?}"
    );

    assert!(
        warnings.iter().any(|w| w.contains(
            "Heavy tails (kurtosis 18.052632): PSR/DSR confidence intervals are likely too narrow."
        )),
        "Expected heavy tails warning in: {warnings:?}"
    );
}

#[test]
fn test_no_pbo_warning_when_pbo_absent() {
    let (trial, ledger) = make_validation_fixture();
    let rep = validate_selection(&trial, &ledger, 252.0, None, 0.95);

    assert!(
        !rep.warnings.iter().any(|w| w.starts_with("PBO")),
        "No PBO warning should be emitted when pbo is None; found: {:?}",
        rep.warnings
    );
}

// ---------------------------------------------------------------------------
// C. Trade statistics (trades.rs, compute_trade_stats)
// ---------------------------------------------------------------------------

fn make_trade(net: f64, holding_ns: i64) -> Trade {
    Trade { open_ts: 0, close_ts: holding_ns, net: Money::from_f64(net).unwrap(), ..Trade::default() }
}

#[test]
fn test_trade_stats_fixture() {
    let trades = vec![
        make_trade(300.0, 1000),
        make_trade(100.0, 2000),
        make_trade(-100.0, 3000),
        make_trade(-100.0, 4000),
    ];

    let st = compute_trade_stats(&trades);

    assert_eq!(st.n, 4);
    assert_eq!(st.wins, 2);
    assert_eq!(st.losses, 2);
    close(st.win_rate, 0.5, TOL, "win_rate");
    close(st.total_net, 200.0, TOL, "total_net");
    close(st.expectancy, 50.0, TOL, "expectancy");
    close(st.avg_win, 200.0, TOL, "avg_win");
    close(st.avg_loss, 100.0, TOL, "avg_loss");
    close(st.payoff_ratio, 2.0, TOL, "payoff_ratio");
    close(st.profit_factor, 2.0, TOL, "profit_factor");
    close(st.best, 300.0, TOL, "best");
    close(st.worst, -100.0, TOL, "worst");
    close(st.top_trade_share, 0.75, TOL, "top_trade_share");
    close(st.avg_holding_ns, 2500.0, TOL, "avg_holding_ns");

    // Zero-pnl (break-even) trade must not be counted as a win:
    let scratch = compute_trade_stats(&[make_trade(0.0, 500)]);
    assert_eq!(scratch.wins, 0, "break-even trade must not be counted as a win");
}
