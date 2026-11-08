//! Long-only mean-variance weights (M4 mean-variance, Task 2).
//!
//! The weights maximize `w' mu - (lambda / 2) w' Sigma w` subject to `w_i >= 0`, `sum_i w_i = 1`
//! and `w_i <= cap` for every holding, where `mu` are expected returns, `Sigma` their covariance,
//! `lambda` the risk aversion and `cap` the largest weight one holding may take.
//!
//!  * [`mean_variance_weights`]: from `mu` and `Sigma` given as numbers.
//!  * [`mean_variance_weights_from_closes`]: from price history, with `mu` the shrunk mean and
//!    `Sigma` the shrunk covariance of [`crate::shrinkage`], both annualized.
//!
//! The risk aversion and the cap are inputs of each call ([`DEFAULT_RISK_AVERSION`] and
//! [`DEFAULT_WEIGHT_CAP`] are what a caller passes when the user gives none); nothing here stores
//! them. Everything here is pure: no I/O, no clock.

use crate::portfolio::{
    check_covariance, check_symbols, validate_weights, Holding, PortfolioError, DEFAULT_WEIGHT_TOLERANCE,
};
use crate::shrinkage::{shrunk_covariance_from_closes, shrunk_mean_from_closes};

/// The risk aversion `lambda` when a request gives none.
pub const DEFAULT_RISK_AVERSION: f64 = 1.0;

/// The weight cap when a request gives none: 1, i.e. no cap.
pub const DEFAULT_WEIGHT_CAP: f64 = 1.0;

/// Projected gradient iterations before [`mean_variance_weights`] gives up with
/// [`PortfolioError::MeanVarianceNotConverged`].
pub const MEAN_VARIANCE_MAX_ITERATIONS: usize = 100_000;

/// The solver has converged when a projected gradient step (of length `1 / L`, see
/// [`mean_variance_weights`]) moves no weight by more than this. The step is the gradient mapping
/// of the problem, which is zero exactly at the optimum.
pub const MEAN_VARIANCE_TOLERANCE: f64 = 1e-12;

/// Long-only mean-variance weights for expected returns and a covariance matrix.
///
/// Solved by accelerated projected gradient (FISTA, Beck and Teboulle, 2009) with the
/// gradient-based adaptive restart of O'Donoghue and Candes (2015). The objective's gradient is
/// Lipschitz with `L = lambda * max_i sum_j |Sigma_ij|` (a bound on `lambda` times the largest
/// eigenvalue), so each step moves `1 / L` along the gradient and projects exactly onto
/// `{0 <= w_i <= cap, sum_i w_i = 1}`. Starts from equal weights, which are always feasible, and
/// stops when [`MEAN_VARIANCE_TOLERANCE`] is met, or fails after [`MEAN_VARIANCE_MAX_ITERATIONS`].
/// For a positive definite `Sigma` the objective is strictly concave, so the optimum is unique,
/// and every step is deterministic.
///
/// # Errors
/// - [`PortfolioError::Empty`], [`PortfolioError::LengthMismatch`] (a covariance row count or an
///   expected return count that is not one per symbol) or [`PortfolioError::DuplicateSymbol`].
/// - [`PortfolioError::InvalidRiskAversion`] if `risk_aversion` is not finite and positive.
/// - [`PortfolioError::InvalidWeightCap`] if `cap` is not finite or is above 1, and
///   [`PortfolioError::InfeasibleWeightCap`] if it is below `1/N`.
/// - [`PortfolioError::InvalidExpectedReturn`] for an expected return that is not finite.
/// - [`PortfolioError::InvalidCovariance`] if the matrix is not square, finite and symmetric, and
///   [`PortfolioError::SingularCovariance`] if it is not positive definite.
/// - [`PortfolioError::MeanVarianceNotConverged`] if the tolerance is not met; no near answer is
///   returned.
pub fn mean_variance_weights(
    symbols: &[String],
    expected_returns: &[f64],
    covariance: &[Vec<f64>],
    risk_aversion: f64,
    cap: f64,
) -> Result<Vec<Holding>, PortfolioError> {
    check_symbols(symbols, covariance.len())?;
    check_symbols(symbols, expected_returns.len())?;
    if !(risk_aversion.is_finite() && risk_aversion > 0.0) {
        return Err(PortfolioError::InvalidRiskAversion(risk_aversion));
    }
    let n = symbols.len();
    if !cap.is_finite() || cap > 1.0 {
        return Err(PortfolioError::InvalidWeightCap(cap));
    }
    if cap < 1.0 / n as f64 {
        return Err(PortfolioError::InfeasibleWeightCap { cap, holdings: n });
    }
    for (symbol, &value) in symbols.iter().zip(expected_returns) {
        if !value.is_finite() {
            return Err(PortfolioError::InvalidExpectedReturn { symbol: symbol.clone(), value });
        }
    }
    check_covariance(symbols, covariance)?;
    let w =
        solve_mean_variance(expected_returns, covariance, risk_aversion, cap, MEAN_VARIANCE_MAX_ITERATIONS)?;
    let holdings: Vec<Holding> =
        symbols.iter().zip(w).map(|(s, weight)| Holding { symbol: s.clone(), weight }).collect();
    validate_weights(&holdings, DEFAULT_WEIGHT_TOLERANCE)?;
    Ok(holdings)
}

/// Mean-variance weights estimated from price history: `mu` from
/// [`shrunk_mean_from_closes`] and `Sigma` from [`shrunk_covariance_from_closes`] (both
/// annualized by `periods_per_year`), then [`mean_variance_weights`]. Same history rules as risk
/// parity: at least [`MIN_VOLATILITY_PRICES`](crate::portfolio::MIN_VOLATILITY_PRICES) closes per
/// holding, every series on the same dates.
///
/// # Errors
/// As [`shrunk_mean_from_closes`], [`shrunk_covariance_from_closes`] and
/// [`mean_variance_weights`].
pub fn mean_variance_weights_from_closes(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
    risk_aversion: f64,
    cap: f64,
) -> Result<Vec<Holding>, PortfolioError> {
    let mu = shrunk_mean_from_closes(symbols, closes, periods_per_year)?;
    let cov = shrunk_covariance_from_closes(symbols, closes, periods_per_year)?;
    mean_variance_weights(symbols, &mu.shrunk, &cov.shrunk, risk_aversion, cap)
}

/// FISTA with adaptive restart on `f(w) = (lambda / 2) w' Sigma w - w' mu` over the capped simplex
/// (see [`mean_variance_weights`]); the inputs are already checked.
fn solve_mean_variance(
    mu: &[f64],
    cov: &[Vec<f64>],
    risk_aversion: f64,
    cap: f64,
    max_iterations: usize,
) -> Result<Vec<f64>, PortfolioError> {
    let n = mu.len();
    let lipschitz =
        risk_aversion * cov.iter().map(|row| row.iter().map(|c| c.abs()).sum::<f64>()).fold(0.0, f64::max);
    let step_size = 1.0 / lipschitz;
    let mut x = vec![1.0 / n as f64; n];
    let mut y = x.clone();
    let mut momentum: f64 = 1.0;
    let mut step = f64::INFINITY;
    for _ in 0..max_iterations {
        let moved: Vec<f64> = cov
            .iter()
            .zip(mu)
            .zip(&y)
            .map(|((row, m), yi)| {
                let gradient = risk_aversion * row.iter().zip(&y).map(|(c, v)| c * v).sum::<f64>() - m;
                yi - step_size * gradient
            })
            .collect();
        let next = project_capped_simplex(&moved, cap);
        step = next.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        if step <= MEAN_VARIANCE_TOLERANCE {
            return Ok(next);
        }
        // Restart when the step points against the momentum: (y - next) . (next - x) > 0.
        let against: f64 = y.iter().zip(&next).zip(&x).map(|((yi, ni), xi)| (yi - ni) * (ni - xi)).sum();
        if against > 0.0 {
            momentum = 1.0;
            y.clone_from(&next);
        } else {
            let following = (1.0 + (1.0 + 4.0 * momentum * momentum).sqrt()) / 2.0;
            let beta = (momentum - 1.0) / following;
            y = next.iter().zip(&x).map(|(ni, xi)| ni + beta * (ni - xi)).collect();
            momentum = following;
        }
        x = next;
    }
    Err(PortfolioError::MeanVarianceNotConverged { iterations: max_iterations, step })
}

/// The Euclidean projection of `v` onto `{0 <= w_i <= cap, sum_i w_i = 1}`, for `cap >= 1/N`:
/// `w_i = clamp(v_i - tau, 0, cap)` with `tau` the root of `g(tau) = sum_i clamp(v_i - tau, 0, cap) = 1`.
/// `g` is piecewise linear and non-increasing, with breakpoints at `v_i - cap` and `v_i`, so `tau`
/// is found exactly between the two breakpoints that bracket 1.
fn project_capped_simplex(v: &[f64], cap: f64) -> Vec<f64> {
    let g = |tau: f64| v.iter().map(|x| (x - tau).clamp(0.0, cap)).sum::<f64>();
    let mut breakpoints: Vec<f64> = v.iter().flat_map(|x| [x - cap, *x]).collect();
    breakpoints.sort_by(f64::total_cmp);
    // At the smallest breakpoint every weight is at the cap: g = N * cap >= 1.
    let mut tau = breakpoints[0];
    let mut g_low = g(tau);
    if g_low > 1.0 {
        for &high in &breakpoints[1..] {
            let g_high = g(high);
            if g_high <= 1.0 {
                tau += (g_low - 1.0) / (g_low - g_high) * (high - tau);
                break;
            }
            tau = high;
            g_low = g_high;
        }
    }
    v.iter().map(|x| (x - tau).clamp(0.0, cap)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syms(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn weights(h: &[Holding]) -> Vec<f64> {
        h.iter().map(|x| x.weight).collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Closes from 100 following deterministic returns: a common factor and one idiosyncratic term
    /// per holding, a drift each, from a 64-bit LCG.
    fn correlated(drift: &[f64], days: usize) -> Vec<Vec<f64>> {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let mut series: Vec<Vec<f64>> = vec![vec![100.0]; drift.len()];
        for _ in 0..days {
            let f = next();
            for (k, d) in drift.iter().enumerate() {
                let r = d + 0.01 * f + 0.02 * next();
                let last = *series[k].last().unwrap();
                series[k].push(last * r.exp());
            }
        }
        series
    }

    /// Checks the optimality (KKT) conditions: with marginal utility `u_i = mu_i - lambda (Sigma w)_i`,
    /// every holding strictly inside `(0, cap)` has the same `u_i = nu`, holdings at 0 have
    /// `u_i <= nu` and holdings at the cap have `u_i >= nu`.
    fn assert_optimal(mu: &[f64], cov: &[Vec<f64>], lambda: f64, cap: f64, w: &[f64]) {
        let u: Vec<f64> = mu
            .iter()
            .zip(cov)
            .map(|(m, row)| m - lambda * row.iter().zip(w).map(|(c, v)| c * v).sum::<f64>())
            .collect();
        let inside = |wi: f64| wi > 1e-9 && wi < cap - 1e-9;
        let free: Vec<f64> = u.iter().zip(w).filter(|(_, wi)| inside(**wi)).map(|(ui, _)| *ui).collect();
        assert!(!free.is_empty(), "test case should leave a holding strictly inside: {w:?}");
        let nu = free[0];
        for (ui, wi) in u.iter().zip(w) {
            if inside(*wi) {
                assert!(close(*ui, nu, 1e-8), "free holdings differ: {u:?} {w:?}");
            } else if *wi <= 1e-9 {
                assert!(*ui <= nu + 1e-8, "a holding at 0 would add utility: {u:?} {w:?}");
            } else {
                assert!(*ui >= nu - 1e-8, "a capped holding would rather shrink: {u:?} {w:?}");
            }
        }
    }

    #[test]
    fn a_single_holding_gets_everything() {
        let h = mean_variance_weights(
            &syms(&["A"]),
            &[0.07],
            &[vec![0.04]],
            DEFAULT_RISK_AVERSION,
            DEFAULT_WEIGHT_CAP,
        )
        .unwrap();
        assert_eq!(weights(&h), vec![1.0]);
    }

    #[test]
    fn equal_means_uncorrelated_is_inverse_variance() {
        // Equal means: w' mu = 0.05 for every w on the simplex, so the optimum minimizes variance
        //   w_1^2 * 0.04 + w_2^2 * 0.01 with w_1 + w_2 = 1.
        // First order: 0.04 * w_1 = 0.01 * w_2, so w is proportional to 1 / variance:
        //   (1/0.04, 1/0.01) = (25, 100), divided by 125 = (0.2, 0.8).
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.01]];
        let mu = [0.05, 0.05];
        let w = weights(&mean_variance_weights(&syms(&["A", "B"]), &mu, &cov, 1.0, 1.0).unwrap());
        assert!(close(w[0], 0.2, 1e-10) && close(w[1], 0.8, 1e-10), "{w:?}");
    }

    #[test]
    fn a_higher_mean_gets_more_weight() {
        // Same variance 0.04, uncorrelated, lambda = 5, mu = (0.10, 0.05). Inside the simplex the
        // marginal utilities are equal: 0.10 - 5 * 0.04 * w_1 = 0.05 - 5 * 0.04 * w_2, so
        //   w_1 - w_2 = 0.05 / 0.2 = 0.25, and with w_1 + w_2 = 1: w = (0.625, 0.375).
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.04]];
        let w = weights(&mean_variance_weights(&syms(&["A", "B"]), &[0.10, 0.05], &cov, 5.0, 1.0).unwrap());
        assert!(w[0] > w[1]);
        assert!(close(w[0], 0.625, 1e-10) && close(w[1], 0.375, 1e-10), "{w:?}");
    }

    #[test]
    fn higher_risk_aversion_moves_toward_lower_variance() {
        // Same case with lambda = 1: w_1 - w_2 = 0.05 / 0.04 = 1.25 > 1, so the optimum is the
        // corner (1, 0).
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.04]];
        let w = weights(&mean_variance_weights(&syms(&["A", "B"]), &[0.10, 0.05], &cov, 1.0, 1.0).unwrap());
        assert!(close(w[0], 1.0, 1e-12) && close(w[1], 0.0, 1e-12), "{w:?}");
    }

    #[test]
    fn a_half_cap_on_two_holdings_splits_evenly() {
        // Uncapped this is the corner (1, 0) (w_1 - w_2 = 0.2 / 0.04 = 5 > 1). A cap of 0.5 = 1/N
        // leaves only (0.5, 0.5) feasible.
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.04]];
        let w = weights(&mean_variance_weights(&syms(&["A", "B"]), &[0.20, 0.0], &cov, 1.0, 0.5).unwrap());
        assert_eq!(w, vec![0.5, 0.5]);
    }

    #[test]
    fn a_cap_binds_and_the_rest_is_optimal() {
        // Five correlated holdings with spread drifts; a cap of 0.3 binds on the best of them.
        let symbols = syms(&["A", "B", "C", "D", "E"]);
        let series = correlated(&[0.002, 0.001, 0.0005, 0.0, -0.0005], 250);
        let mu = shrunk_mean_from_closes(&symbols, &series, 252.0).unwrap().shrunk;
        let cov = shrunk_covariance_from_closes(&symbols, &series, 252.0).unwrap().shrunk;
        for (lambda, cap) in [(DEFAULT_RISK_AVERSION, DEFAULT_WEIGHT_CAP), (5.0, 0.3), (20.0, 0.3)] {
            let w = weights(&mean_variance_weights(&symbols, &mu, &cov, lambda, cap).unwrap());
            assert!(close(w.iter().sum::<f64>(), 1.0, DEFAULT_WEIGHT_TOLERANCE), "{w:?}");
            assert!(w.iter().all(|x| (0.0..=cap).contains(x)), "{w:?}");
            if lambda > 1.0 {
                assert_optimal(&mu, &cov, lambda, cap, &w);
            }
        }
        let capped = weights(&mean_variance_weights(&symbols, &mu, &cov, 5.0, 0.3).unwrap());
        assert_eq!(capped[0], 0.3, "{capped:?}");
    }

    #[test]
    fn from_closes_is_deterministic() {
        let symbols = syms(&["A", "B", "C", "D"]);
        let series = correlated(&[0.001, 0.0005, 0.0, 0.0002], 120);
        let a = mean_variance_weights_from_closes(&symbols, &series, 252.0, 4.0, 0.6).unwrap();
        let b = mean_variance_weights_from_closes(&symbols, &series, 252.0, 4.0, 0.6).unwrap();
        let bits = |h: &[Holding]| h.iter().map(|x| x.weight.to_bits()).collect::<Vec<u64>>();
        assert_eq!(bits(&a), bits(&b));
        assert_eq!(a.iter().map(|h| h.symbol.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn refuses_a_cap_below_one_over_n() {
        let cov = vec![vec![0.04, 0.0, 0.0], vec![0.0, 0.04, 0.0], vec![0.0, 0.0, 0.04]];
        let mu = [0.05, 0.05, 0.05];
        let s = syms(&["A", "B", "C"]);
        for cap in [0.3, 0.0, -1.0] {
            assert_eq!(
                mean_variance_weights(&s, &mu, &cov, 1.0, cap),
                Err(PortfolioError::InfeasibleWeightCap { cap, holdings: 3 })
            );
        }
        let err = mean_variance_weights(&s, &mu, &cov, 1.0, 0.3).unwrap_err().to_string();
        assert!(err.contains("below 1/3"), "{err}");
        for cap in [f64::NAN, f64::INFINITY, 1.5] {
            assert!(matches!(
                mean_variance_weights(&s, &mu, &cov, 1.0, cap),
                Err(PortfolioError::InvalidWeightCap(_))
            ));
        }
        // Exactly 1/N is feasible: every holding at the cap.
        assert!(mean_variance_weights(&s, &mu, &cov, 1.0, 1.0 / 3.0).is_ok());
    }

    #[test]
    fn refuses_non_finite_inputs() {
        let s = syms(&["A", "B"]);
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.01]];
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                mean_variance_weights(&s, &[0.05, bad], &cov, 1.0, 1.0),
                Err(PortfolioError::InvalidExpectedReturn { ref symbol, .. }) if symbol == "B"
            ));
            let mut c = cov.clone();
            c[0][1] = bad;
            c[1][0] = bad;
            assert!(matches!(
                mean_variance_weights(&s, &[0.05, 0.05], &c, 1.0, 1.0),
                Err(PortfolioError::InvalidCovariance(_))
            ));
        }
        assert!(matches!(
            mean_variance_weights(&s, &[0.05], &cov, 1.0, 1.0),
            Err(PortfolioError::LengthMismatch { symbols: 2, values: 1 })
        ));
        assert_eq!(mean_variance_weights(&[], &[], &[], 1.0, 1.0), Err(PortfolioError::Empty));
    }

    #[test]
    fn refuses_a_covariance_that_is_not_symmetric_positive_definite() {
        let s = syms(&["A", "B"]);
        let mu = [0.05, 0.05];
        // Perfectly correlated: singular.
        assert_eq!(
            mean_variance_weights(&s, &mu, &[vec![0.04, 0.02], vec![0.02, 0.01]], 1.0, 1.0),
            Err(PortfolioError::SingularCovariance { symbol: "B".into() })
        );
        // Correlation above 1: indefinite.
        assert!(matches!(
            mean_variance_weights(&s, &mu, &[vec![0.04, 0.03], vec![0.03, 0.01]], 1.0, 1.0),
            Err(PortfolioError::SingularCovariance { .. })
        ));
        assert!(matches!(
            mean_variance_weights(&s, &mu, &[vec![0.04, 0.01], vec![0.0, 0.01]], 1.0, 1.0),
            Err(PortfolioError::InvalidCovariance(_))
        ));
        assert!(matches!(
            mean_variance_weights(&s, &mu, &[vec![0.04, 0.0], vec![0.0]], 1.0, 1.0),
            Err(PortfolioError::InvalidCovariance(_))
        ));
    }

    #[test]
    fn refuses_risk_aversion_that_is_not_positive() {
        let s = syms(&["A", "B"]);
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.01]];
        for lambda in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                mean_variance_weights(&s, &[0.05, 0.05], &cov, lambda, 1.0),
                Err(PortfolioError::InvalidRiskAversion(_))
            ));
        }
    }

    #[test]
    fn stops_with_an_error_instead_of_a_near_answer() {
        // The optimum is (0.625, 0.375); one step from equal weights does not reach it.
        let cov = vec![vec![0.04, 0.0], vec![0.0, 0.04]];
        let err = solve_mean_variance(&[0.10, 0.05], &cov, 5.0, 1.0, 1).unwrap_err();
        assert!(
            matches!(err, PortfolioError::MeanVarianceNotConverged { iterations: 1, step } if step > 0.0)
        );
    }

    #[test]
    fn projection_lands_on_the_capped_simplex() {
        // Uncapped: the classic simplex projection, (0.8, 0.2, 0) for v = (1, 0.4, -1).
        let w = project_capped_simplex(&[1.0, 0.4, -1.0], 1.0);
        assert!(close(w[0], 0.8, 1e-15) && close(w[1], 0.2, 1e-15) && w[2] == 0.0, "{w:?}");
        // Cap 0.5: the first holding is held at 0.5 and the rest goes to the second.
        let w = project_capped_simplex(&[1.0, 0.4, -1.0], 0.5);
        assert!(w[0] == 0.5 && close(w[1], 0.5, 1e-15) && w[2] == 0.0, "{w:?}");
        // A feasible point projects to itself.
        let w = project_capped_simplex(&[0.25, 0.25, 0.5], 0.5);
        assert_eq!(w, vec![0.25, 0.25, 0.5]);
    }
}
