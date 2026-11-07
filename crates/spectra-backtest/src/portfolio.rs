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
//!
//! Risk parity and mean-variance are later methods (M15 decision 1) and are not here.
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
    if !(periods_per_year.is_finite() && periods_per_year > 0.0) {
        return Err(PortfolioError::InvalidVolatility {
            symbol: symbol.to_string(),
            value: periods_per_year,
        });
    }
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
    let returns: Vec<f64> = closes.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Ok(variance.sqrt() * periods_per_year.sqrt())
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
