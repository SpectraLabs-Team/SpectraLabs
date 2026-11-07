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
