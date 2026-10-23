//! Per-asset return modeling, covariance, correlation, and Cholesky factorization.
//!
//! Standard-library only: zero external dependencies.

use spectra_core::{Error, Result};

/// Trading days in a standard year for annualizing daily returns.
pub const TRADING_DAYS_PER_YEAR: f64 = 252.0;

/// Summary statistics for an individual asset's return history.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetReturnStats {
    /// Asset ticker or identifier.
    pub symbol: String,
    /// Sample daily mean return.
    pub mean_daily: f64,
    /// Sample daily volatility (standard deviation).
    pub volatility_daily: f64,
    /// Annualized mean return (`mean_daily * 252`).
    pub mean_annualized: f64,
    /// Annualized volatility (`volatility_daily * sqrt(252)`).
    pub volatility_annualized: f64,
    /// Number of observations used.
    pub count: usize,
}

/// Calculate arithmetic mean of a slice.
pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Calculate sample variance with Bessel's correction (`N - 1`).
pub fn variance(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    let ss = values.iter().fold(0.0, |acc, &x| {
        let diff = x - m;
        acc + diff * diff
    });
    ss / (values.len() - 1) as f64
}

/// Calculate sample standard deviation (volatility).
pub fn std_dev(values: &[f64]) -> f64 {
    variance(values).sqrt()
}

/// Calculate sample covariance between two series of equal length.
///
/// # Errors
/// [`Error::InvalidArgument`] if lengths differ or length < 2.
pub fn covariance(x: &[f64], y: &[f64]) -> Result<f64> {
    if x.len() != y.len() {
        return Err(Error::InvalidArgument(format!(
            "Series lengths must match for covariance: {} vs {}",
            x.len(),
            y.len()
        )));
    }
    if x.len() < 2 {
        return Err(Error::InvalidArgument("Need at least 2 observations for covariance".into()));
    }
    let mx = mean(x);
    let my = mean(y);
    let n = x.len();
    let mut sum = 0.0;
    for i in 0..n {
        sum += (x[i] - mx) * (y[i] - my);
    }
    Ok(sum / (n - 1) as f64)
}

/// Calculate Pearson correlation coefficient between two series.
///
/// # Errors
/// [`Error::InvalidArgument`] if lengths differ, length < 2, or either series has zero variance.
pub fn correlation(x: &[f64], y: &[f64]) -> Result<f64> {
    let cov = covariance(x, y)?;
    let sx = std_dev(x);
    let sy = std_dev(y);
    if sx <= 1e-12 || sy <= 1e-12 {
        return Err(Error::InvalidArgument("Zero or negligible variance in series for correlation".into()));
    }
    let rho = cov / (sx * sy);
    // Clamp to [-1.0, 1.0] to guard against floating-point inaccuracy
    Ok(rho.clamp(-1.0, 1.0))
}

/// Fit return statistics for a single asset.
///
/// # Errors
/// [`Error::InvalidArgument`] if returns has fewer than 2 elements.
pub fn fit_asset_returns(symbol: &str, returns: &[f64]) -> Result<AssetReturnStats> {
    if returns.len() < 2 {
        return Err(Error::InvalidArgument(format!(
            "Asset '{symbol}' must have at least 2 return observations"
        )));
    }
    let mean_d = mean(returns);
    let vol_d = std_dev(returns);
    Ok(AssetReturnStats {
        symbol: symbol.to_string(),
        mean_daily: mean_d,
        volatility_daily: vol_d,
        mean_annualized: mean_d * TRADING_DAYS_PER_YEAR,
        volatility_annualized: vol_d * TRADING_DAYS_PER_YEAR.sqrt(),
        count: returns.len(),
    })
}

/// Compute the full $N \times N$ correlation matrix from $N$ series of equal length.
///
/// # Errors
/// [`Error::InvalidArgument`] if series lengths differ or series are empty.
pub fn correlation_matrix(asset_returns: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = asset_returns.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let len = asset_returns[0].len();
    for (i, r) in asset_returns.iter().enumerate() {
        if r.len() != len {
            return Err(Error::InvalidArgument(format!(
                "Asset return series length mismatch: series 0 has {len}, series {i} has {}",
                r.len()
            )));
        }
    }

    let mut corr = vec![vec![0.0; n]; n];
    for i in 0..n {
        corr[i][i] = 1.0;
        for j in 0..i {
            let r = correlation(&asset_returns[i], &asset_returns[j])?;
            corr[i][j] = r;
            corr[j][i] = r;
        }
    }
    Ok(corr)
}

/// Cholesky factorization of a symmetric positive-definite matrix $A = L L^T$.
/// Returns lower-triangular matrix $L$.
///
/// Applies small diagonal jitter (`1e-8`) if the matrix is positive semi-definite but nearly singular.
///
/// # Errors
/// [`Error::InvalidArgument`] if matrix is not square or not positive semi-definite.
pub fn cholesky(matrix: &[Vec<f64>]) -> Result<Vec<Vec<f64>>> {
    let n = matrix.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    for (i, row) in matrix.iter().enumerate() {
        if row.len() != n {
            return Err(Error::InvalidArgument(format!(
                "Matrix must be square: row {i} has {} cols, expected {n}",
                row.len()
            )));
        }
    }

    let mut l = vec![vec![0.0; n]; n];
    let jitter = 1e-8;

    for i in 0..n {
        for j in 0..=i {
            let mut sum = 0.0;
            for k in 0..j {
                sum += l[i][k] * l[j][k];
            }

            if i == j {
                let mut val = matrix[i][i] - sum;
                if val <= 0.0 {
                    val += jitter;
                }
                if val <= 0.0 {
                    return Err(Error::InvalidArgument(format!(
                        "Matrix is not positive definite at index {i}"
                    )));
                }
                l[i][j] = val.sqrt();
            } else {
                if l[j][j].abs() <= 1e-12 {
                    return Err(Error::InvalidArgument(format!(
                        "Singular diagonal during Cholesky at index {j}"
                    )));
                }
                l[i][j] = (matrix[i][j] - sum) / l[j][j];
            }
        }
    }

    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_three_asset_fit_matches_hand_calculation() {
        let ret_a = vec![0.01, 0.02, -0.01, 0.03, 0.00];
        let ret_b = vec![-0.02, 0.00, 0.01, -0.01, 0.02];
        let ret_c = vec![0.005, 0.015, -0.005, 0.020, 0.005];

        let fit_a = fit_asset_returns("A", &ret_a).unwrap();
        assert!((fit_a.mean_daily - 0.010).abs() < 1e-12);
        assert!((fit_a.volatility_daily - 0.00025f64.sqrt()).abs() < 1e-12);
        assert!((fit_a.mean_annualized - 0.010 * 252.0).abs() < 1e-12);
        assert!((fit_a.volatility_annualized - 0.00025f64.sqrt() * 252.0f64.sqrt()).abs() < 1e-12);

        let fit_b = fit_asset_returns("B", &ret_b).unwrap();
        assert!(fit_b.mean_daily.abs() < 1e-12);
        assert!((fit_b.volatility_daily - 0.00025f64.sqrt()).abs() < 1e-12);

        let fit_c = fit_asset_returns("C", &ret_c).unwrap();
        assert!((fit_c.mean_daily - 0.008).abs() < 1e-12);
        assert!((fit_c.volatility_daily - 0.000095f64.sqrt()).abs() < 1e-12);

        let cov_ab = covariance(&ret_a, &ret_b).unwrap();
        assert!((cov_ab - (-0.00015)).abs() < 1e-12);

        let corr_ab = correlation(&ret_a, &ret_b).unwrap();
        assert!((corr_ab - (-0.60)).abs() < 1e-12);

        let matrix = correlation_matrix(&[ret_a, ret_b, ret_c]).unwrap();
        assert_eq!(matrix.len(), 3);
        assert!((matrix[0][0] - 1.0).abs() < 1e-12);
        assert!((matrix[0][1] - (-0.60)).abs() < 1e-12);
        assert!((matrix[1][0] - (-0.60)).abs() < 1e-12);

        let chol = cholesky(&matrix).unwrap();
        assert_eq!(chol.len(), 3);
        for i in 0..3 {
            for j in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += chol[i][k] * chol[j][k];
                }
                assert!(
                    (sum - matrix[i][j]).abs() < 1e-6,
                    "Mismatch at ({i}, {j}): {sum} vs {}",
                    matrix[i][j]
                );
            }
        }
    }
}
