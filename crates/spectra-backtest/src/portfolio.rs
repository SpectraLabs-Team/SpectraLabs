//! Portfolio weights and allocation methods (M15, Tasks 1 and 2).
//!
//! A saved portfolio holds symbols with target weights. A save is refused unless
//! the weights are finite, non-negative, unique by symbol, and sum to 1 within
//! [`DEFAULT_WEIGHT_TOLERANCE`]. The allocation methods produce such weights:
//!
//!  * [`fixed_weights`]: the caller's weights, validated as they are.
//!  * [`equal_weights`]: 1/N for each of N symbols.
//!  * [`inverse_volatility_weights`]: proportional to 1 / volatility. The
//!    volatilities come from M7's inputs (e.g. `spectra_risk::volatility_from_bars`);
//!    this module takes them as numbers and does not load prices.
//!  * [`risk_parity_weights_from_closes`]: equal risk contributions, from the annualized
//!    covariance of the holdings' daily closes (the M15 follow-on to decision 1).
//!
//! Mean-variance is a later method (M15 decision 1) and is not here.
//! Everything in this module is pure: no I/O, no database, no clock.

use std::collections::HashSet;
use std::fmt;

/// Default tolerance for the weight sum: `|sum - 1| <= 1e-6`.
pub const DEFAULT_WEIGHT_TOLERANCE: f64 = 1e-6;

/// One symbol and its target weight (a fraction of the portfolio, not a percentage).
#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    pub symbol: String,
    pub weight: f64,
}

/// Why a portfolio or an allocation was refused. Each variant names the offending input.
#[derive(Debug, Clone, PartialEq)]
pub enum PortfolioError {
    /// No holdings were given.
    Empty,
    /// The same symbol appears more than once.
    DuplicateSymbol(String),
    /// A weight is NaN or infinite.
    NonFiniteWeight { symbol: String },
    /// A weight is negative. Short positions are not part of the first version.
    NegativeWeight { symbol: String, weight: f64 },
    /// The weights do not sum to 1 within the tolerance.
    WeightSum { sum: f64, tolerance: f64 },
    /// A volatility for inverse-volatility weights is not finite and strictly positive.
    InvalidVolatility { symbol: String, value: f64 },
    /// Symbols and volatilities have different lengths.
    LengthMismatch { symbols: usize, values: usize },
    /// The tolerance is not finite and non-negative.
    InvalidTolerance(f64),
    /// A price history is too short to estimate a volatility (M7 needs 60 bars).
    InsufficientHistory { symbol: String, need: usize, have: usize },
    /// A close is not finite and strictly positive.
    InvalidPrice { symbol: String, value: f64 },
    /// Risk parity needs every holding's closes on the same dates; this one has a different count.
    UnequalHistory { symbol: String, need: usize, have: usize },
    /// The covariance matrix is singular: this holding's returns are (almost exactly) a fixed mix
    /// of the holdings before it, or do not move at all, so equal risk contributions are not unique.
    SingularCovariance { symbol: String },
    /// A covariance matrix passed in is not square, symmetric and finite.
    InvalidCovariance(String),
    /// The risk parity solver did not reach [`RISK_PARITY_TOLERANCE`] within
    /// [`RISK_PARITY_MAX_SWEEPS`]; `residual` is how far the risk contributions still were from equal.
    NotConverged { sweeps: usize, residual: f64 },
}

impl fmt::Display for PortfolioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a portfolio needs at least one holding"),
            Self::DuplicateSymbol(s) => write!(f, "symbol {s} appears more than once"),
            Self::NonFiniteWeight { symbol } => write!(f, "weight for {symbol} is not a finite number"),
            Self::NegativeWeight { symbol, weight } => {
                write!(f, "weight for {symbol} is negative ({weight}); short positions are not supported")
            }
            Self::WeightSum { sum, tolerance } => {
                write!(f, "weights sum to {sum}, which is not 1 within {tolerance}")
            }
            Self::InvalidVolatility { symbol, value } => {
                write!(f, "volatility for {symbol} must be finite and positive, got {value}")
            }
            Self::LengthMismatch { symbols, values } => {
                write!(f, "{symbols} symbols but {values} volatilities")
            }
            Self::InvalidTolerance(t) => write!(f, "tolerance must be finite and non-negative, got {t}"),
            Self::InsufficientHistory { symbol, need, have } => {
                write!(f, "need {need} prices for {symbol} to estimate volatility, have {have}")
            }
            Self::InvalidPrice { symbol, value } => {
                write!(f, "price {value} for {symbol} is not finite and positive")
            }
            Self::UnequalHistory { symbol, need, have } => {
                write!(f, "risk parity needs closes on the same dates: {symbol} has {have}, expected {need}")
            }
            Self::SingularCovariance { symbol } => write!(
                f,
                "the covariance of the holdings' returns is singular: {symbol} moves as a fixed mix of the \
                 other holdings (or not at all), so risk parity weights are not defined"
            ),
            Self::InvalidCovariance(why) => write!(f, "invalid covariance matrix: {why}"),
            Self::NotConverged { sweeps, residual } => write!(
                f,
                "risk parity did not converge in {sweeps} sweeps (risk contributions still differ by {residual:e})"
            ),
        }
    }
}

impl std::error::Error for PortfolioError {}

/// Check a set of holdings for a saved portfolio.
///
/// # Errors
/// - [`PortfolioError::Empty`] if there are no holdings.
/// - [`PortfolioError::DuplicateSymbol`] if a symbol repeats.
/// - [`PortfolioError::NonFiniteWeight`] or [`PortfolioError::NegativeWeight`] for a bad weight.
/// - [`PortfolioError::WeightSum`] if the weights do not sum to 1 within `tolerance`.
/// - [`PortfolioError::InvalidTolerance`] if `tolerance` is not finite and non-negative.
pub fn validate_weights(holdings: &[Holding], tolerance: f64) -> Result<(), PortfolioError> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(PortfolioError::InvalidTolerance(tolerance));
    }
    if holdings.is_empty() {
        return Err(PortfolioError::Empty);
    }
    let mut seen = HashSet::with_capacity(holdings.len());
    let mut sum = 0.0;
    for h in holdings {
        if !seen.insert(h.symbol.as_str()) {
            return Err(PortfolioError::DuplicateSymbol(h.symbol.clone()));
        }
        if !h.weight.is_finite() {
            return Err(PortfolioError::NonFiniteWeight { symbol: h.symbol.clone() });
        }
        if h.weight < 0.0 {
            return Err(PortfolioError::NegativeWeight { symbol: h.symbol.clone(), weight: h.weight });
        }
        sum += h.weight;
    }
    if (sum - 1.0).abs() > tolerance {
        return Err(PortfolioError::WeightSum { sum, tolerance });
    }
    Ok(())
}

/// Fixed weights: the caller's target weights, validated with [`DEFAULT_WEIGHT_TOLERANCE`].
///
/// # Errors
/// As [`validate_weights`].
pub fn fixed_weights(holdings: &[Holding]) -> Result<Vec<Holding>, PortfolioError> {
    validate_weights(holdings, DEFAULT_WEIGHT_TOLERANCE)?;
    Ok(holdings.to_vec())
}

/// Equal weight: `1 / N` for each of the N symbols, in the order given.
///
/// # Errors
/// - [`PortfolioError::Empty`] if `symbols` is empty.
/// - [`PortfolioError::DuplicateSymbol`] if a symbol repeats.
pub fn equal_weights(symbols: &[String]) -> Result<Vec<Holding>, PortfolioError> {
    if symbols.is_empty() {
        return Err(PortfolioError::Empty);
    }
    let mut seen = HashSet::with_capacity(symbols.len());
    for s in symbols {
        if !seen.insert(s.as_str()) {
            return Err(PortfolioError::DuplicateSymbol(s.clone()));
        }
    }
    let w = 1.0 / symbols.len() as f64;
    Ok(symbols.iter().map(|s| Holding { symbol: s.clone(), weight: w }).collect())
}

/// Inverse volatility: `w_i = (1 / vol_i) / sum_j (1 / vol_j)`.
///
/// `symbols[i]` pairs with `volatilities[i]`. Volatilities are annualized
/// (or all on the same basis); only their ratios matter.
///
/// # Errors
/// - [`PortfolioError::Empty`] if there are no symbols.
/// - [`PortfolioError::LengthMismatch`] if the two slices differ in length.
/// - [`PortfolioError::DuplicateSymbol`] if a symbol repeats.
/// - [`PortfolioError::InvalidVolatility`] if any volatility is not finite and strictly positive.
pub fn inverse_volatility_weights(
    symbols: &[String],
    volatilities: &[f64],
) -> Result<Vec<Holding>, PortfolioError> {
    if symbols.is_empty() {
        return Err(PortfolioError::Empty);
    }
    if symbols.len() != volatilities.len() {
        return Err(PortfolioError::LengthMismatch { symbols: symbols.len(), values: volatilities.len() });
    }
    let mut seen = HashSet::with_capacity(symbols.len());
    for s in symbols {
        if !seen.insert(s.as_str()) {
            return Err(PortfolioError::DuplicateSymbol(s.clone()));
        }
    }
    let mut inv = Vec::with_capacity(symbols.len());
    for (s, &v) in symbols.iter().zip(volatilities) {
        if !v.is_finite() || v <= 0.0 {
            return Err(PortfolioError::InvalidVolatility { symbol: s.clone(), value: v });
        }
        inv.push(1.0 / v);
    }
    let total: f64 = inv.iter().sum();
    Ok(symbols.iter().zip(inv).map(|(s, x)| Holding { symbol: s.clone(), weight: x / total }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syms(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn h(symbol: &str, weight: f64) -> Holding {
        Holding { symbol: symbol.to_string(), weight }
    }

    #[test]
    fn four_equal_assets_get_25_percent_each() {
        let w = equal_weights(&syms(&["A", "B", "C", "D"])).unwrap();
        assert_eq!(w.len(), 4);
        for x in &w {
            assert!((x.weight - 0.25).abs() < 1e-15, "{} got {}", x.symbol, x.weight);
        }
    }

    #[test]
    fn inverse_volatility_matches_hand_calculation() {
        // vols 0.10, 0.20, 0.40 -> raw 10, 5, 2.5 -> sum 17.5
        let w = inverse_volatility_weights(&syms(&["A", "B", "C"]), &[0.10, 0.20, 0.40]).unwrap();
        let expected = [10.0 / 17.5, 5.0 / 17.5, 2.5 / 17.5];
        for (x, e) in w.iter().zip(expected) {
            assert!((x.weight - e).abs() < 1e-12, "{}: {} vs {}", x.symbol, x.weight, e);
        }
        let sum: f64 = w.iter().map(|x| x.weight).sum();
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn inverse_volatility_is_proportional_to_inverse_vol() {
        let w = inverse_volatility_weights(&syms(&["A", "B"]), &[0.2, 0.5]).unwrap();
        // w_A / w_B must equal vol_B / vol_A = 2.5
        assert!((w[0].weight / w[1].weight - 2.5).abs() < 1e-12);
    }

    #[test]
    fn inverse_volatility_is_scale_invariant() {
        let a = inverse_volatility_weights(&syms(&["A", "B"]), &[0.2, 0.5]).unwrap();
        let b = inverse_volatility_weights(&syms(&["A", "B"]), &[2.0, 5.0]).unwrap();
        for (x, y) in a.iter().zip(&b) {
            assert!((x.weight - y.weight).abs() < 1e-12);
        }
    }

    #[test]
    fn inverse_volatility_rejects_zero_negative_and_nan_vol() {
        for bad in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            let err = inverse_volatility_weights(&syms(&["A", "B"]), &[0.2, bad]).unwrap_err();
            assert!(matches!(err, PortfolioError::InvalidVolatility { ref symbol, .. } if symbol == "B"));
        }
    }

    #[test]
    fn inverse_volatility_rejects_length_mismatch() {
        let err = inverse_volatility_weights(&syms(&["A", "B"]), &[0.2]).unwrap_err();
        assert_eq!(err, PortfolioError::LengthMismatch { symbols: 2, values: 1 });
    }

    #[test]
    fn fixed_weights_accept_a_valid_sum() {
        let ok = vec![h("A", 0.6), h("B", 0.4)];
        assert_eq!(fixed_weights(&ok).unwrap(), ok);
    }

    #[test]
    fn weight_sum_outside_tolerance_is_refused() {
        let err = validate_weights(&[h("A", 0.5), h("B", 0.4)], DEFAULT_WEIGHT_TOLERANCE).unwrap_err();
        assert!(matches!(err, PortfolioError::WeightSum { .. }));
    }

    #[test]
    fn weight_sum_within_tolerance_is_accepted() {
        validate_weights(&[h("A", 0.5), h("B", 0.5 + 1e-8)], DEFAULT_WEIGHT_TOLERANCE).unwrap();
    }

    #[test]
    fn empty_duplicate_negative_and_nonfinite_are_refused() {
        assert_eq!(validate_weights(&[], 1e-6), Err(PortfolioError::Empty));
        assert_eq!(
            validate_weights(&[h("A", 0.5), h("A", 0.5)], 1e-6),
            Err(PortfolioError::DuplicateSymbol("A".into()))
        );
        assert!(matches!(
            validate_weights(&[h("A", 1.5), h("B", -0.5)], 1e-6),
            Err(PortfolioError::NegativeWeight { .. })
        ));
        assert!(matches!(
            validate_weights(&[h("A", f64::NAN)], 1e-6),
            Err(PortfolioError::NonFiniteWeight { .. })
        ));
    }

    #[test]
    fn invalid_tolerance_is_refused() {
        assert!(matches!(
            validate_weights(&[h("A", 1.0)], f64::NAN),
            Err(PortfolioError::InvalidTolerance(_))
        ));
        assert!(matches!(validate_weights(&[h("A", 1.0)], -1.0), Err(PortfolioError::InvalidTolerance(_))));
    }

    #[test]
    fn equal_weights_refuse_empty_and_duplicates() {
        assert_eq!(equal_weights(&[]), Err(PortfolioError::Empty));
        assert_eq!(equal_weights(&syms(&["A", "A"])), Err(PortfolioError::DuplicateSymbol("A".into())));
    }
}

/// Bars needed to estimate a volatility: the same minimum M7 uses (`MIN_VOLATILITY_BARS`).
pub const MIN_VOLATILITY_PRICES: usize = 60;

/// Annualized volatility from a close series: the sample standard deviation (N - 1) of log returns,
/// scaled by `sqrt(periods_per_year)`. The same method as M7's `volatility_from_bars`.
///
/// # Errors
/// - [`PortfolioError::InsufficientHistory`] with fewer than [`MIN_VOLATILITY_PRICES`] closes.
/// - [`PortfolioError::InvalidPrice`] if a close is not finite and strictly positive.
/// - [`PortfolioError::InvalidVolatility`] if `periods_per_year` is not finite and positive.
pub fn annualized_volatility(
    symbol: &str,
    closes: &[f64],
    periods_per_year: f64,
) -> Result<f64, PortfolioError> {
    check_periods(symbol, periods_per_year)?;
    let returns = log_returns(symbol, closes)?;
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Ok(variance.sqrt() * periods_per_year.sqrt())
}

fn check_periods(symbol: &str, periods_per_year: f64) -> Result<(), PortfolioError> {
    if periods_per_year.is_finite() && periods_per_year > 0.0 {
        Ok(())
    } else {
        Err(PortfolioError::InvalidVolatility { symbol: symbol.to_string(), value: periods_per_year })
    }
}

/// The log returns of a close series, after the history checks every price-based method shares:
/// at least [`MIN_VOLATILITY_PRICES`] closes, each finite and strictly positive.
fn log_returns(symbol: &str, closes: &[f64]) -> Result<Vec<f64>, PortfolioError> {
    if closes.len() < MIN_VOLATILITY_PRICES {
        return Err(PortfolioError::InsufficientHistory {
            symbol: symbol.to_string(),
            need: MIN_VOLATILITY_PRICES,
            have: closes.len(),
        });
    }
    if let Some(&bad) = closes.iter().find(|p| !(p.is_finite() && **p > 0.0)) {
        return Err(PortfolioError::InvalidPrice { symbol: symbol.to_string(), value: bad });
    }
    Ok(closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect())
}

/// Inverse-volatility weights estimated from price history: each symbol's volatility comes from
/// [`annualized_volatility`], then the weights are as [`inverse_volatility_weights`].
///
/// `closes[i]` is the close series of `symbols[i]`, oldest first.
///
/// # Errors
/// As [`annualized_volatility`] and [`inverse_volatility_weights`].
pub fn inverse_volatility_weights_from_closes(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<Vec<Holding>, PortfolioError> {
    if symbols.len() != closes.len() {
        return Err(PortfolioError::LengthMismatch { symbols: symbols.len(), values: closes.len() });
    }
    let mut vols = Vec::with_capacity(symbols.len());
    for (symbol, series) in symbols.iter().zip(closes) {
        vols.push(annualized_volatility(symbol, series, periods_per_year)?);
    }
    inverse_volatility_weights(symbols, &vols)
}

#[cfg(test)]
mod volatility_tests {
    use super::*;

    /// Closes whose log returns alternate +a and -a, starting from 100: mean 0, and the sample
    /// variance of N returns is N * a^2 / (N - 1).
    fn alternating(a: f64, returns: usize) -> Vec<f64> {
        let mut p = 100.0;
        let mut out = vec![p];
        for k in 0..returns {
            let r = if k % 2 == 0 { a } else { -a };
            p *= r.exp();
            out.push(p);
        }
        out
    }

    #[test]
    fn volatility_matches_the_closed_form_on_alternating_returns() {
        let n = 60;
        let a = 0.01;
        let vol = annualized_volatility("A", &alternating(a, n), 252.0).unwrap();
        let expected = a * (n as f64 / (n as f64 - 1.0)).sqrt() * 252.0_f64.sqrt();
        assert!((vol - expected).abs() < 1e-12, "{vol} vs {expected}");
    }

    #[test]
    fn inverse_volatility_from_prices_is_proportional_to_inverse_vol() {
        // Asset B moves twice as much as A, so its weight is half of A's.
        let symbols = vec!["A".to_string(), "B".to_string()];
        let closes = vec![alternating(0.01, 60), alternating(0.02, 60)];
        let w = inverse_volatility_weights_from_closes(&symbols, &closes, 252.0).unwrap();
        assert!((w[0].weight / w[1].weight - 2.0).abs() < 1e-12);
        let sum: f64 = w.iter().map(|h| h.weight).sum();
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn short_history_is_refused_with_the_symbol_named() {
        let err = annualized_volatility("AAA", &alternating(0.01, 30), 252.0).unwrap_err();
        assert_eq!(err, PortfolioError::InsufficientHistory { symbol: "AAA".into(), need: 60, have: 31 });
    }

    #[test]
    fn bad_prices_and_bad_periods_are_refused() {
        let mut closes = alternating(0.01, 60);
        closes[10] = 0.0;
        assert!(matches!(
            annualized_volatility("A", &closes, 252.0),
            Err(PortfolioError::InvalidPrice { .. })
        ));
        assert!(matches!(
            annualized_volatility("A", &alternating(0.01, 60), 0.0),
            Err(PortfolioError::InvalidVolatility { .. })
        ));
    }
}

/// Sweeps of the cyclical coordinate method before [`risk_parity_weights`] gives up with
/// [`PortfolioError::NotConverged`]. One sweep updates every holding once.
pub const RISK_PARITY_MAX_SWEEPS: usize = 10_000;

/// Risk parity has converged when every holding's share of the total risk is within this of
/// `1/N`, relatively: `max_i |N * RC_i / sum_j RC_j - 1| <= RISK_PARITY_TOLERANCE`, where
/// `RC_i = w_i * (Sigma w)_i`.
pub const RISK_PARITY_TOLERANCE: f64 = 1e-10;

/// A covariance matrix is singular when a Cholesky pivot falls to this fraction of its diagonal
/// entry or below, i.e. when a holding's returns are explained by the holdings before it with
/// R^2 >= 1 - 1e-10.
pub const SINGULAR_PIVOT_TOLERANCE: f64 = 1e-10;

/// The annualized covariance matrix of the holdings' daily log returns: the sample covariance
/// (N - 1), times `periods_per_year`. Its diagonal is the square of [`annualized_volatility`], with
/// the same history checks.
///
/// `closes[i]` is the close series of `symbols[i]`, oldest first, and every series must be on the
/// same dates (the same length). The caller passes only closes known on the date the weights are
/// for; nothing here looks at dates.
///
/// # Errors
/// - [`PortfolioError::Empty`], [`PortfolioError::LengthMismatch`] (a symbol without a close
///   series) or [`PortfolioError::DuplicateSymbol`].
/// - As [`annualized_volatility`]: [`PortfolioError::InsufficientHistory`],
///   [`PortfolioError::InvalidPrice`], [`PortfolioError::InvalidVolatility`] for a bad
///   `periods_per_year`.
/// - [`PortfolioError::UnequalHistory`] if the series differ in length.
pub fn annualized_covariance(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<Vec<Vec<f64>>, PortfolioError> {
    let mut returns = aligned_log_returns(symbols, closes, periods_per_year)?;
    for r in &mut returns {
        let mean = r.iter().sum::<f64>() / r.len() as f64;
        for x in r.iter_mut() {
            *x -= mean;
        }
    }
    let scale = periods_per_year / (returns[0].len() as f64 - 1.0);
    let n = returns.len();
    let mut cov = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let c = returns[i].iter().zip(&returns[j]).map(|(a, b)| a * b).sum::<f64>() * scale;
            cov[i][j] = c;
            cov[j][i] = c;
        }
    }
    Ok(cov)
}

/// The daily log returns of every holding, after the history checks shared by the price-based
/// estimates (risk parity here, the shrunk mean and covariance in [`crate::shrinkage`]): at least one
/// unique symbol with one close series each, a valid `periods_per_year`, at least
/// [`MIN_VOLATILITY_PRICES`] finite positive closes per holding, and every series the same length.
///
/// # Errors
/// As [`annualized_covariance`].
pub(crate) fn aligned_log_returns(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<Vec<Vec<f64>>, PortfolioError> {
    check_symbols(symbols, closes.len())?;
    let mut returns: Vec<Vec<f64>> = Vec::with_capacity(symbols.len());
    for (symbol, series) in symbols.iter().zip(closes) {
        check_periods(symbol, periods_per_year)?;
        let r = log_returns(symbol, series)?;
        if series.len() != closes[0].len() {
            return Err(PortfolioError::UnequalHistory {
                symbol: symbol.clone(),
                need: closes[0].len(),
                have: series.len(),
            });
        }
        returns.push(r);
    }
    Ok(returns)
}

/// Risk parity (equal risk contribution) weights for a covariance matrix: `w_i >= 0`,
/// `sum w_i = 1`, and `w_i * (Sigma w)_i` the same for every holding.
///
/// Solved with the cyclical coordinate method (Griveau-Billion, Richard and Roncalli, 2013):
/// minimize `y' Sigma y / 2 - (1/N) sum_i ln y_i` over `y > 0`, one coordinate at a time, each by
/// its closed-form root `y_i = (-c_i + sqrt(c_i^2 + 4 Sigma_ii / N)) / (2 Sigma_ii)` with
/// `c_i = sum_{j != i} Sigma_ij y_j`. The minimum has `y_i (Sigma y)_i = 1/N` for all `i`, so
/// `w = y / sum(y)` has equal risk contributions; for a positive definite `Sigma` it is unique.
/// Starts from inverse volatility, sweeps in symbol order (so the result is deterministic), and
/// stops when [`RISK_PARITY_TOLERANCE`] is met, or fails after [`RISK_PARITY_MAX_SWEEPS`].
///
/// # Errors
/// - [`PortfolioError::Empty`], [`PortfolioError::LengthMismatch`] or
///   [`PortfolioError::DuplicateSymbol`].
/// - [`PortfolioError::InvalidCovariance`] if the matrix is not square, finite and symmetric.
/// - [`PortfolioError::SingularCovariance`] if it is not positive definite (see
///   [`SINGULAR_PIVOT_TOLERANCE`]), naming the first holding that makes it so.
/// - [`PortfolioError::NotConverged`] if the tolerance is not met; no near answer is returned.
pub fn risk_parity_weights(
    symbols: &[String],
    covariance: &[Vec<f64>],
) -> Result<Vec<Holding>, PortfolioError> {
    check_symbols(symbols, covariance.len())?;
    check_covariance(symbols, covariance)?;
    let w = solve_risk_parity(covariance, RISK_PARITY_MAX_SWEEPS)?;
    Ok(symbols.iter().zip(w).map(|(s, weight)| Holding { symbol: s.clone(), weight }).collect())
}

/// Risk parity weights estimated from price history: [`annualized_covariance`] of the closes,
/// then [`risk_parity_weights`]. Same history rules as [`inverse_volatility_weights_from_closes`]
/// (at least [`MIN_VOLATILITY_PRICES`] closes each), and the series must be on the same dates.
///
/// # Errors
/// As [`annualized_covariance`] and [`risk_parity_weights`].
pub fn risk_parity_weights_from_closes(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<Vec<Holding>, PortfolioError> {
    let cov = annualized_covariance(symbols, closes, periods_per_year)?;
    risk_parity_weights(symbols, &cov)
}

/// At least one symbol, `values` inputs for them (one per symbol), and no symbol twice.
fn check_symbols(symbols: &[String], values: usize) -> Result<(), PortfolioError> {
    if symbols.is_empty() {
        return Err(PortfolioError::Empty);
    }
    if symbols.len() != values {
        return Err(PortfolioError::LengthMismatch { symbols: symbols.len(), values });
    }
    let mut seen = HashSet::with_capacity(symbols.len());
    for s in symbols {
        if !seen.insert(s.as_str()) {
            return Err(PortfolioError::DuplicateSymbol(s.clone()));
        }
    }
    Ok(())
}

/// Square, finite and symmetric, then positive definite: a Cholesky factorization whose every
/// pivot stays above [`SINGULAR_PIVOT_TOLERANCE`] times its diagonal entry.
fn check_covariance(symbols: &[String], cov: &[Vec<f64>]) -> Result<(), PortfolioError> {
    let n = cov.len();
    for (i, row) in cov.iter().enumerate() {
        if row.len() != n {
            return Err(PortfolioError::InvalidCovariance(format!(
                "row {i} has {} entries, expected {n}",
                row.len()
            )));
        }
        for (j, &x) in row.iter().enumerate() {
            if !x.is_finite() {
                return Err(PortfolioError::InvalidCovariance(format!("entry ({i}, {j}) is not finite")));
            }
            let mirror = cov[j][i];
            if (x - mirror).abs() > 1e-12 * (x.abs() + mirror.abs()) {
                return Err(PortfolioError::InvalidCovariance(format!(
                    "entries ({i}, {j}) and ({j}, {i}) differ"
                )));
            }
        }
    }
    let mut l = vec![vec![0.0; n]; n];
    for k in 0..n {
        let pivot = cov[k][k] - l[k][..k].iter().map(|x| x * x).sum::<f64>();
        if !(pivot > SINGULAR_PIVOT_TOLERANCE * cov[k][k]) {
            return Err(PortfolioError::SingularCovariance { symbol: symbols[k].clone() });
        }
        let lkk = pivot.sqrt();
        l[k][k] = lkk;
        for i in k + 1..n {
            let dot = l[i][..k].iter().zip(&l[k][..k]).map(|(a, b)| a * b).sum::<f64>();
            l[i][k] = (cov[i][k] - dot) / lkk;
        }
    }
    Ok(())
}

/// How far the risk contributions of `w` are from equal: `max_i |N * RC_i / sum_j RC_j - 1|`.
fn risk_parity_residual(cov: &[Vec<f64>], w: &[f64]) -> f64 {
    let rc: Vec<f64> = cov
        .iter()
        .zip(w)
        .map(|(row, wi)| wi * row.iter().zip(w).map(|(c, wj)| c * wj).sum::<f64>())
        .collect();
    let total: f64 = rc.iter().sum();
    let n = w.len() as f64;
    if !(total.is_finite() && total > 0.0) {
        return f64::INFINITY;
    }
    rc.iter().map(|r| (n * r / total - 1.0).abs()).fold(0.0, f64::max)
}

/// The cyclical coordinate method for a positive definite `cov` (see [`risk_parity_weights`]).
fn solve_risk_parity(cov: &[Vec<f64>], max_sweeps: usize) -> Result<Vec<f64>, PortfolioError> {
    let n = cov.len();
    let budget = 1.0 / n as f64;
    let mut y: Vec<f64> = cov.iter().enumerate().map(|(i, row)| 1.0 / row[i].sqrt()).collect();
    let mut residual = f64::INFINITY;
    for _ in 0..max_sweeps {
        for i in 0..n {
            let c: f64 =
                cov[i].iter().zip(&y).enumerate().filter(|(j, _)| *j != i).map(|(_, (s, v))| s * v).sum();
            let root = (c * c + 4.0 * cov[i][i] * budget).sqrt();
            // The same root in two forms, the one without cancellation for the sign of c.
            y[i] = if c >= 0.0 { 2.0 * budget / (c + root) } else { (root - c) / (2.0 * cov[i][i]) };
        }
        let total: f64 = y.iter().sum();
        let w: Vec<f64> = y.iter().map(|v| v / total).collect();
        residual = risk_parity_residual(cov, &w);
        if residual <= RISK_PARITY_TOLERANCE {
            return Ok(w);
        }
    }
    Err(PortfolioError::NotConverged { sweeps: max_sweeps, residual })
}

#[cfg(test)]
mod risk_parity_tests {
    use super::*;

    fn syms(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    /// Closes from 100 following the given log returns.
    fn closes(returns: impl IntoIterator<Item = f64>) -> Vec<f64> {
        let mut p = 100.0;
        let mut out = vec![p];
        for r in returns {
            p *= f64::exp(r);
            out.push(p);
        }
        out
    }

    /// Risk contributions `w_i * (Sigma w)_i`.
    fn contributions(cov: &[Vec<f64>], w: &[f64]) -> Vec<f64> {
        cov.iter().zip(w).map(|(row, wi)| wi * row.iter().zip(w).map(|(c, wj)| c * wj).sum::<f64>()).collect()
    }

    fn weights(h: &[Holding]) -> Vec<f64> {
        h.iter().map(|x| x.weight).collect()
    }

    /// Deterministic returns: a common factor and one idiosyncratic term per asset, from a 64-bit LCG.
    fn correlated(loadings: &[(f64, f64)], days: usize) -> Vec<Vec<f64>> {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let mut rets = vec![Vec::with_capacity(days); loadings.len()];
        for _ in 0..days {
            let f = next();
            for (k, (beta, idio)) in loadings.iter().enumerate() {
                rets[k].push(beta * f + idio * next());
            }
        }
        rets.into_iter().map(closes).collect()
    }

    #[test]
    fn two_uncorrelated_assets_10_and_20_percent() {
        // Uncorrelated, so RC_i = w_i^2 * sigma_i^2. Equal RC: w_1 * 0.10 = w_2 * 0.20, so w is
        // proportional to 1 / sigma (not 1 / variance): w = (2/3, 1/3).
        let cov = vec![vec![0.01, 0.0], vec![0.0, 0.04]];
        let w = weights(&risk_parity_weights(&syms(&["A", "B"]), &cov).unwrap());
        assert!((w[0] - 2.0 / 3.0).abs() < 1e-9 && (w[1] - 1.0 / 3.0).abs() < 1e-9, "{w:?}");
        let rc = contributions(&cov, &w);
        assert!((rc[0] / rc[1] - 1.0).abs() < 1e-9, "{rc:?}");
        // Weights proportional to 1 / variance (0.8, 0.2) do not equalize risk: 0.0064 vs 0.0016.
        let rc = contributions(&cov, &[0.8, 0.2]);
        assert!((rc[0] - 0.0064).abs() < 1e-15 && (rc[1] - 0.0016).abs() < 1e-15);
    }

    #[test]
    fn two_uncorrelated_assets_from_closes() {
        // A's daily log returns alternate +a, -a; B's run +b, +b, -b, -b. Over 60 returns both
        // means are 0 and the cross products cancel in every block of 4, so the sample covariance
        // is exactly diagonal; with b = 2a the volatilities are in the ratio 10% : 20%.
        let a = 0.01 / 252.0_f64.sqrt();
        let ra = (0..60).map(|k| if k % 2 == 0 { a } else { -a });
        let rb = (0..60).map(|k| if k % 4 < 2 { 2.0 * a } else { -2.0 * a });
        let series = vec![closes(ra), closes(rb)];
        let cov = annualized_covariance(&syms(&["A", "B"]), &series, 252.0).unwrap();
        assert!(cov[0][1].abs() < 1e-15, "{cov:?}");
        let w = weights(&risk_parity_weights_from_closes(&syms(&["A", "B"]), &series, 252.0).unwrap());
        assert!((w[0] - 2.0 / 3.0).abs() < 1e-9 && (w[1] - 1.0 / 3.0).abs() < 1e-9, "{w:?}");
    }

    #[test]
    fn three_assets_match_a_hand_calculation() {
        // A and B: volatility 20%, correlation 0.5 (covariance 0.02). C: volatility 10%,
        // uncorrelated with both. By symmetry w_A = w_B = w, and
        //   RC_A = w * (0.04 w + 0.02 w) = 0.06 w^2,   RC_C = 0.01 w_C^2.
        // Equal: w_C = sqrt(6) w. Sum: 2w + sqrt(6) w = 1, so
        //   w = 1 / (2 + sqrt 6) = 0.224744871...,  w_C = sqrt 6 / (2 + sqrt 6) = 0.550510257...
        let cov = vec![vec![0.04, 0.02, 0.0], vec![0.02, 0.04, 0.0], vec![0.0, 0.0, 0.01]];
        let w = weights(&risk_parity_weights(&syms(&["A", "B", "C"]), &cov).unwrap());
        let s6 = 6.0_f64.sqrt();
        let expected = [1.0 / (2.0 + s6), 1.0 / (2.0 + s6), s6 / (2.0 + s6)];
        for (x, e) in w.iter().zip(expected) {
            assert!((x - e).abs() < 1e-9, "{w:?} vs {expected:?}");
        }
        assert!((expected[0] - 0.224_744_871).abs() < 1e-9 && (expected[2] - 0.550_510_257).abs() < 1e-9);
    }

    #[test]
    fn correlated_closes_give_equal_contributions_and_the_same_weights_every_time() {
        let symbols = syms(&["SPY", "QQQ", "TLT", "GLD"]);
        let series = correlated(&[(0.02, 0.005), (0.025, 0.008), (-0.006, 0.01), (0.003, 0.012)], 250);
        let a = risk_parity_weights_from_closes(&symbols, &series, 252.0).unwrap();
        let b = risk_parity_weights_from_closes(&symbols, &series, 252.0).unwrap();
        assert_eq!(a, b, "deterministic, bit for bit");
        let w = weights(&a);
        assert!(w.iter().all(|x| *x > 0.0));
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let cov = annualized_covariance(&symbols, &series, 252.0).unwrap();
        let rc = contributions(&cov, &w);
        let mean = rc.iter().sum::<f64>() / 4.0;
        assert!(rc.iter().all(|r| (r / mean - 1.0).abs() <= RISK_PARITY_TOLERANCE), "{rc:?}");
    }

    #[test]
    fn a_single_holding_gets_everything() {
        let w = risk_parity_weights(&syms(&["A"]), &[vec![0.09]]).unwrap();
        assert_eq!(w, vec![Holding { symbol: "A".into(), weight: 1.0 }]);
    }

    #[test]
    fn too_few_closes_and_bad_prices_are_refused_like_inverse_volatility() {
        let symbols = syms(&["A", "B"]);
        let ok = correlated(&[(0.01, 0.01), (0.01, 0.02)], 60);
        let mut short = ok.clone();
        short[1].truncate(59);
        assert_eq!(
            risk_parity_weights_from_closes(&symbols, &short, 252.0),
            Err(PortfolioError::InsufficientHistory { symbol: "B".into(), need: 60, have: 59 })
        );
        for bad in [0.0, -5.0, f64::NAN] {
            let mut prices = ok.clone();
            prices[0][7] = bad;
            assert!(matches!(
                risk_parity_weights_from_closes(&symbols, &prices, 252.0),
                Err(PortfolioError::InvalidPrice { ref symbol, .. }) if symbol == "A"
            ));
        }
        assert!(matches!(
            risk_parity_weights_from_closes(&symbols, &ok, 0.0),
            Err(PortfolioError::InvalidVolatility { .. })
        ));
        let mut uneven = ok;
        uneven[1].push(101.0);
        assert_eq!(
            risk_parity_weights_from_closes(&symbols, &uneven, 252.0),
            Err(PortfolioError::UnequalHistory { symbol: "B".into(), need: 61, have: 62 })
        );
    }

    #[test]
    fn a_missing_symbol_is_refused() {
        // Three symbols, closes for two of them.
        let series = correlated(&[(0.01, 0.01), (0.01, 0.02)], 60);
        assert_eq!(
            risk_parity_weights_from_closes(&syms(&["A", "B", "C"]), &series, 252.0),
            Err(PortfolioError::LengthMismatch { symbols: 3, values: 2 })
        );
        assert_eq!(risk_parity_weights_from_closes(&[], &[], 252.0), Err(PortfolioError::Empty));
        assert_eq!(
            risk_parity_weights_from_closes(&syms(&["A", "A"]), &series, 252.0),
            Err(PortfolioError::DuplicateSymbol("A".into()))
        );
    }

    #[test]
    fn a_singular_covariance_is_refused() {
        let base = correlated(&[(0.01, 0.01), (0.01, 0.02)], 60);
        // B is A at twice the price: identical log returns.
        let twice: Vec<f64> = base[0].iter().map(|p| 2.0 * p).collect();
        assert_eq!(
            risk_parity_weights_from_closes(&syms(&["A", "B"]), &[base[0].clone(), twice], 252.0),
            Err(PortfolioError::SingularCovariance { symbol: "B".into() })
        );
        // C does not move: zero variance.
        assert_eq!(
            risk_parity_weights_from_closes(&syms(&["A", "C"]), &[base[0].clone(), vec![50.0; 61]], 252.0),
            Err(PortfolioError::SingularCovariance { symbol: "C".into() })
        );
        // Perfectly correlated in the matrix itself.
        assert_eq!(
            risk_parity_weights(&syms(&["A", "B"]), &[vec![0.04, 0.02], vec![0.02, 0.01]]),
            Err(PortfolioError::SingularCovariance { symbol: "B".into() })
        );
    }

    #[test]
    fn a_malformed_covariance_is_refused() {
        let s = syms(&["A", "B"]);
        assert!(matches!(
            risk_parity_weights(&s, &[vec![0.04, 0.01], vec![0.0, 0.01]]),
            Err(PortfolioError::InvalidCovariance(_))
        ));
        assert!(matches!(
            risk_parity_weights(&s, &[vec![0.04], vec![0.0, 0.01]]),
            Err(PortfolioError::InvalidCovariance(_))
        ));
        assert!(matches!(
            risk_parity_weights(&s, &[vec![f64::NAN, 0.0], vec![0.0, 0.01]]),
            Err(PortfolioError::InvalidCovariance(_))
        ));
    }

    #[test]
    fn running_out_of_sweeps_is_an_error_not_a_near_answer() {
        let cov = vec![vec![0.04, 0.03, 0.0], vec![0.03, 0.09, -0.01], vec![0.0, -0.01, 0.01]];
        let err = solve_risk_parity(&cov, 1).unwrap_err();
        assert!(
            matches!(err, PortfolioError::NotConverged { sweeps: 1, residual } if residual > RISK_PARITY_TOLERANCE),
            "{err:?}"
        );
        // With the real cap it converges.
        assert!(solve_risk_parity(&cov, RISK_PARITY_MAX_SWEEPS).is_ok());
    }
}
