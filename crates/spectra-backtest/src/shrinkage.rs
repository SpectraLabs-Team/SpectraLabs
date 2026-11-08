//! Shrunk expected returns and covariance for mean-variance portfolios (M4 mean-variance, Task 1).
//!
//! Both estimates start from the holdings' daily log returns, with the same history rule as risk
//! parity ([`crate::portfolio::risk_parity_weights_from_closes`]): the caller passes only closes
//! known on the `as_of` date, at least [`MIN_VOLATILITY_PRICES`](crate::portfolio::MIN_VOLATILITY_PRICES) finite positive closes per
//! holding, every series on the same dates. Estimates are annualized by `periods_per_year`
//! (252 for daily closes).
//!
//!  * [`shrunk_mean_from_closes`]: each holding's mean daily log return, shrunk toward the grand
//!    mean across holdings with the positive-part James-Stein weight computed from the data.
//!  * [`shrunk_covariance_from_closes`]: the sample covariance of daily log returns, shrunk toward
//!    the constant-correlation target (Ledoit and Wolf, 2003, "Honey, I shrunk the sample
//!    covariance matrix"), with their estimate of the optimal intensity.
//!
//! Each result keeps the sample estimate, the target and the intensity, so a caller can show how
//! far the estimate was moved. Everything here is pure: no I/O, no clock.

use crate::portfolio::{aligned_log_returns, PortfolioError};

/// The `3` in the James-Stein weight `(N - 3) * sigma^2 / sum_i (m_i - m_bar)^2`: one degree for
/// the dimension bound of the classic estimator and one more for estimating the grand mean it
/// shrinks toward. With three holdings or fewer the weight is not positive, so the means are not
/// shrunk.
pub const JAMES_STEIN_OFFSET: f64 = 3.0;

/// Expected returns, annualized: the sample means, the grand mean they are shrunk toward, the
/// intensity in `[0, 1]`, and `shrunk = intensity * target + (1 - intensity) * sample`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeanEstimate {
    pub symbols: Vec<String>,
    /// Mean daily log return of each holding, times `periods_per_year`.
    pub sample: Vec<f64>,
    /// The grand mean: the average of `sample`.
    pub target: f64,
    /// 0 keeps the sample means, 1 replaces them with the grand mean.
    pub intensity: f64,
    pub shrunk: Vec<f64>,
}

/// Covariance, annualized: the sample covariance, the constant-correlation target, the target's
/// average correlation, the intensity in `[0, 1]`, and
/// `shrunk = intensity * target + (1 - intensity) * sample`.
#[derive(Debug, Clone, PartialEq)]
pub struct CovarianceEstimate {
    pub symbols: Vec<String>,
    /// Sample covariance (N - 1) of daily log returns, times `periods_per_year`.
    pub sample: Vec<Vec<f64>>,
    /// The sample variances on the diagonal, `mean_correlation * sd_i * sd_j` off it.
    pub target: Vec<Vec<f64>>,
    /// The average of the sample correlations between distinct holdings (0 with one holding).
    pub mean_correlation: f64,
    /// 0 keeps the sample covariance, 1 replaces it with the target.
    pub intensity: f64,
    pub shrunk: Vec<Vec<f64>>,
}

/// Expected returns from closes: the mean daily log return of each holding, shrunk toward the
/// grand mean, annualized.
///
/// With `m_i` the sample mean of holding `i`'s `T` daily log returns, `m_bar` their average,
/// `s_i^2` the sample variance (T - 1) of holding `i`'s returns and `sigma^2 = mean_i(s_i^2) / T`
/// (the pooled variance of a sample mean), the intensity is the positive-part James-Stein weight
/// `c = clamp((N - 3) * sigma^2 / sum_i (m_i - m_bar)^2, 0, 1)`, and
/// `shrunk_i = c * m_bar + (1 - c) * m_i`. If every mean is equal the sample already is the
/// target and the intensity is 1. Everything is then multiplied by `periods_per_year`.
///
/// # Errors
/// - [`PortfolioError::Empty`], [`PortfolioError::LengthMismatch`] (a symbol without a close
///   series) or [`PortfolioError::DuplicateSymbol`].
/// - [`PortfolioError::InsufficientHistory`] with fewer than [`MIN_VOLATILITY_PRICES`](crate::portfolio::MIN_VOLATILITY_PRICES) closes,
///   [`PortfolioError::InvalidPrice`] for a close that is not finite and positive,
///   [`PortfolioError::UnequalHistory`] if the series differ in length, and
///   [`PortfolioError::InvalidVolatility`] for a `periods_per_year` that is not finite and positive.
pub fn shrunk_mean_from_closes(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<MeanEstimate, PortfolioError> {
    let returns = aligned_log_returns(symbols, closes, periods_per_year)?;
    let (sample, target, intensity) = james_stein(&returns);
    let shrunk = shrink_vector(&sample, target, intensity);
    let scale = |v: &[f64]| v.iter().map(|x| x * periods_per_year).collect::<Vec<f64>>();
    Ok(MeanEstimate {
        symbols: symbols.to_vec(),
        sample: scale(&sample),
        target: target * periods_per_year,
        intensity,
        shrunk: scale(&shrunk),
    })
}

/// Covariance from closes: the sample covariance of daily log returns, shrunk toward the
/// constant-correlation target with the Ledoit-Wolf (2003) intensity, annualized.
///
/// Follows Ledoit and Wolf's reference implementation (`covCor`): returns are demeaned and
/// `S = Y'Y / (T - 1)` over `T` returns. The target `F` keeps the variances and puts
/// `r_bar * sd_i * sd_j` off the diagonal, `r_bar` being the average sample correlation. The
/// optimal intensity is `clamp(kappa / T, 0, 1)` with `kappa = (pi - rho) / gamma`, where
///
/// - `pi = sum_ij [ mean_t(y_ti^2 y_tj^2) - S_ij^2 ]` (sum of asymptotic variances of `S`),
/// - `rho = sum_i pi_ii + r_bar * sum_{i != j} (sd_j / sd_i) * [ mean_t(y_ti^3 y_tj) - S_ii S_ij ]`,
/// - `gamma = ||S - F||^2` (Frobenius), the misspecification of the target,
///
/// and each `mean_t` divides by `T - 1` like `S`. If `gamma` is 0 the target equals the sample and
/// the intensity is 1. The intensity does not change with the scale of the returns, so the result
/// is shrunk on daily returns and then multiplied by `periods_per_year`. At intensity 0 the
/// estimate is [`crate::portfolio::annualized_covariance`].
///
/// # Errors
/// As [`shrunk_mean_from_closes`], and [`PortfolioError::SingularCovariance`] naming a holding
/// whose closes never move (zero variance, so it has no correlation with the others).
pub fn shrunk_covariance_from_closes(
    symbols: &[String],
    closes: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<CovarianceEstimate, PortfolioError> {
    let returns = aligned_log_returns(symbols, closes, periods_per_year)?;
    let lw = ledoit_wolf(&returns)
        .map_err(|k| PortfolioError::SingularCovariance { symbol: symbols[k].clone() })?;
    let shrunk = shrink_matrix(&lw.sample, &lw.target, lw.intensity);
    let scale = |m: &[Vec<f64>]| -> Vec<Vec<f64>> {
        m.iter().map(|row| row.iter().map(|x| x * periods_per_year).collect()).collect()
    };
    Ok(CovarianceEstimate {
        symbols: symbols.to_vec(),
        sample: scale(&lw.sample),
        target: scale(&lw.target),
        mean_correlation: lw.mean_correlation,
        intensity: lw.intensity,
        shrunk: scale(&shrunk),
    })
}

/// `intensity * target + (1 - intensity) * sample_i` for each entry: exactly the sample at 0 and
/// exactly the target at 1.
fn shrink_vector(sample: &[f64], target: f64, intensity: f64) -> Vec<f64> {
    sample.iter().map(|m| intensity * target + (1.0 - intensity) * m).collect()
}

/// `intensity * target + (1 - intensity) * sample`, entry by entry.
fn shrink_matrix(sample: &[Vec<f64>], target: &[Vec<f64>], intensity: f64) -> Vec<Vec<f64>> {
    sample
        .iter()
        .zip(target)
        .map(|(s, f)| s.iter().zip(f).map(|(s, f)| intensity * f + (1.0 - intensity) * s).collect())
        .collect()
}

/// The daily sample means, the grand mean and the James-Stein intensity for `returns[i]` (one
/// series per holding, all the same length, at least two returns).
fn james_stein(returns: &[Vec<f64>]) -> (Vec<f64>, f64, f64) {
    let n = returns.len() as f64;
    let t = returns[0].len() as f64;
    let means: Vec<f64> = returns.iter().map(|r| r.iter().sum::<f64>() / t).collect();
    let grand = means.iter().sum::<f64>() / n;
    let pooled = returns
        .iter()
        .zip(&means)
        .map(|(r, m)| r.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (t - 1.0))
        .sum::<f64>()
        / n;
    let spread: f64 = means.iter().map(|m| (m - grand).powi(2)).sum();
    let intensity =
        if spread > 0.0 { ((n - JAMES_STEIN_OFFSET) * (pooled / t) / spread).clamp(0.0, 1.0) } else { 1.0 };
    (means, grand, intensity)
}

/// Daily sample covariance, constant-correlation target, average correlation and intensity.
struct LedoitWolf {
    sample: Vec<Vec<f64>>,
    target: Vec<Vec<f64>>,
    mean_correlation: f64,
    intensity: f64,
}

/// Ledoit-Wolf (2003) on `returns[i]` (one series per holding, all the same length, at least two
/// returns). Fails with the index of a holding whose returns have zero variance.
fn ledoit_wolf(returns: &[Vec<f64>]) -> Result<LedoitWolf, usize> {
    let n = returns.len();
    let t = returns[0].len();
    let dof = t as f64 - 1.0;
    let y: Vec<Vec<f64>> = returns
        .iter()
        .map(|r| {
            let mean = r.iter().sum::<f64>() / t as f64;
            r.iter().map(|x| x - mean).collect()
        })
        .collect();
    // mean_t(a_t * b_t) with the T - 1 divisor of the sample covariance.
    let avg = |f: &dyn Fn(usize) -> f64| (0..t).map(f).sum::<f64>() / dof;

    let mut sample = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let c = avg(&|k| y[i][k] * y[j][k]);
            sample[i][j] = c;
            sample[j][i] = c;
        }
    }
    let sd: Vec<f64> = (0..n).map(|i| sample[i][i].sqrt()).collect();
    if let Some(k) = sd.iter().position(|s| !(*s > 0.0)) {
        return Err(k);
    }

    let mut mean_correlation = 0.0;
    if n > 1 {
        let mut total = 0.0;
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    total += sample[i][j] / (sd[i] * sd[j]);
                }
            }
        }
        mean_correlation = total / (n * (n - 1)) as f64;
    }
    let mut target = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            target[i][j] = if i == j { sample[i][i] } else { mean_correlation * sd[i] * sd[j] };
        }
    }

    let mut pi = 0.0;
    let mut rho = 0.0;
    let mut gamma = 0.0;
    for i in 0..n {
        for j in 0..n {
            let pi_ij = avg(&|k| (y[i][k] * y[j][k]).powi(2)) - sample[i][j].powi(2);
            pi += pi_ij;
            gamma += (sample[i][j] - target[i][j]).powi(2);
            if i == j {
                rho += pi_ij;
            } else {
                let theta = avg(&|k| y[i][k].powi(3) * y[j][k]) - sample[i][i] * sample[i][j];
                rho += mean_correlation * (sd[j] / sd[i]) * theta;
            }
        }
    }
    let intensity = if gamma > 0.0 { ((pi - rho) / gamma / t as f64).clamp(0.0, 1.0) } else { 1.0 };
    Ok(LedoitWolf { sample, target, mean_correlation, intensity })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{annualized_covariance, MIN_VOLATILITY_PRICES};

    fn syms(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    /// Closes from 100 following the given log returns.
    fn closes(returns: &[f64]) -> Vec<f64> {
        let mut p = 100.0;
        let mut out = vec![p];
        for r in returns {
            p *= f64::exp(*r);
            out.push(p);
        }
        out
    }

    /// Deterministic daily returns: a common factor plus one idiosyncratic term per holding and a
    /// drift, from a 64-bit LCG.
    fn returns(spec: &[(f64, f64, f64)], days: usize) -> Vec<Vec<f64>> {
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let mut out = vec![Vec::with_capacity(days); spec.len()];
        for _ in 0..days {
            let f = next();
            for (k, (drift, beta, idio)) in spec.iter().enumerate() {
                out[k].push(drift + beta * f + idio * next());
            }
        }
        out
    }

    fn five() -> (Vec<String>, Vec<Vec<f64>>) {
        let spec = [
            (0.0006, 0.02, 0.006),
            (0.0002, 0.025, 0.009),
            (-0.0001, -0.006, 0.01),
            (0.0004, 0.004, 0.012),
            (0.0, 0.015, 0.02),
        ];
        let series = returns(&spec, 250).iter().map(|r| closes(r)).collect();
        (syms(&["SPY", "QQQ", "TLT", "GLD", "IWM"]), series)
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + a.abs().max(b.abs()))
    }

    #[test]
    fn intensity_zero_gives_the_sample_and_one_gives_the_target() {
        let sample = vec![0.3, -0.1, 0.05];
        assert_eq!(shrink_vector(&sample, 0.08, 0.0), sample);
        assert!(shrink_vector(&sample, 0.08, 1.0).iter().all(|x| *x == 0.08));

        let s = vec![vec![0.04, 0.01], vec![0.01, 0.09]];
        let f = vec![vec![0.04, 0.03], vec![0.03, 0.09]];
        assert_eq!(shrink_matrix(&s, &f, 0.0), s);
        assert_eq!(shrink_matrix(&s, &f, 1.0), f);
    }

    #[test]
    fn the_estimates_are_the_blend_their_intensity_says() {
        let (symbols, series) = five();
        let m = shrunk_mean_from_closes(&symbols, &series, 252.0).unwrap();
        assert!(m.intensity > 0.0 && m.intensity < 1.0, "{}", m.intensity);
        for (x, e) in m.shrunk.iter().zip(shrink_vector(&m.sample, m.target, m.intensity)) {
            assert!(close(*x, e, 1e-15));
        }
        let c = shrunk_covariance_from_closes(&symbols, &series, 252.0).unwrap();
        assert!(c.intensity > 0.0 && c.intensity < 1.0, "{}", c.intensity);
        for ((row, s), f) in c.shrunk.iter().zip(&c.sample).zip(&c.target) {
            for ((x, s), f) in row.iter().zip(s).zip(f) {
                assert!(close(*x, c.intensity * f + (1.0 - c.intensity) * s, 1e-15));
            }
        }
        // The sample covariance is risk parity's, and the target keeps its variances.
        let rp = annualized_covariance(&symbols, &series, 252.0).unwrap();
        for i in 0..5 {
            assert_eq!(c.target[i][i], c.sample[i][i]);
            for j in 0..5 {
                assert!(close(c.sample[i][j], rp[i][j], 1e-14), "{i},{j}");
                assert_eq!(c.sample[i][j], c.sample[j][i]);
                assert_eq!(c.shrunk[i][j], c.shrunk[j][i]);
            }
        }
    }

    #[test]
    fn three_holdings_or_fewer_keep_their_sample_means() {
        // N - 3 <= 0: the positive-part weight is 0, so the result is the sample estimate.
        let (symbols, series) = five();
        let m = shrunk_mean_from_closes(&symbols[..3], &series[..3], 252.0).unwrap();
        assert_eq!(m.intensity, 0.0);
        assert_eq!(m.shrunk, m.sample);
    }

    #[test]
    fn equal_means_are_already_the_target() {
        // Every holding has the same returns in a different order: equal means, spread 0.
        let base: Vec<f64> = (0..60).map(|k| if k % 3 == 0 { 0.01 } else { -0.004 }).collect();
        let series: Vec<Vec<f64>> = (0..4)
            .map(|s| {
                let mut r = base.clone();
                r.rotate_left(s);
                closes(&r)
            })
            .collect();
        let m = shrunk_mean_from_closes(&syms(&["A", "B", "C", "D"]), &series, 252.0).unwrap();
        assert_eq!(m.intensity, 1.0);
        for x in &m.shrunk {
            assert!(close(*x, m.target, 1e-12));
        }
    }

    #[test]
    fn james_stein_matches_a_hand_calculation() {
        // Four holdings, four daily returns each: drift m_k plus the shared noise +1, -1, +1, -1.
        //   m = (3, 1, -1, -3), grand mean 0, spread sum (m - 0)^2 = 9 + 1 + 1 + 9 = 20.
        //   Each holding's sample variance: (1 + 1 + 1 + 1) / (4 - 1) = 4/3; pooled 4/3.
        //   sigma^2 = (4/3) / 4 = 1/3.
        //   c = (4 - 3) * (1/3) / 20 = 1/60.
        //   shrunk = (1/60) * 0 + (59/60) * m = (2.95, 0.98333.., -0.98333.., -2.95).
        let noise = [1.0, -1.0, 1.0, -1.0];
        let r: Vec<Vec<f64>> =
            [3.0, 1.0, -1.0, -3.0].iter().map(|m| noise.iter().map(|e| m + e).collect()).collect();
        let (means, grand, c) = james_stein(&r);
        assert_eq!(means, vec![3.0, 1.0, -1.0, -3.0]);
        assert_eq!(grand, 0.0);
        assert!(close(c, 1.0 / 60.0, 1e-15), "{c}");
        let shrunk = shrink_vector(&means, grand, c);
        for (x, e) in shrunk.iter().zip([2.95, 59.0 / 60.0, -59.0 / 60.0, -2.95]) {
            assert!(close(*x, e, 1e-14), "{shrunk:?}");
        }
    }

    #[test]
    fn ledoit_wolf_matches_a_hand_calculation() {
        // Three holdings, T = 4 daily returns (already mean 0), divisor T - 1 = 3:
        //   t:   A    B    C
        //   1:   1    1    2
        //   2:  -1    1    0
        //   3:   1   -1    0
        //   4:  -1   -1   -2
        // Sample S: s_AA = s_BB = 4/3, s_CC = (4 + 4)/3 = 8/3, s_AB = (1 - 1 - 1 + 1)/3 = 0,
        //   s_AC = s_BC = (2 + 2)/3 = 4/3.
        // Correlations: AB 0, AC = BC = (4/3) / sqrt(4/3 * 8/3) = 1/sqrt 2.
        //   r_bar = (0 + 2/sqrt 2) / 3 = sqrt 2 / 3.
        // Target F off the diagonal: f_AB = r_bar * 4/3 = 4 sqrt 2 / 9,
        //   f_AC = f_BC = r_bar * sqrt(32)/3 = 8/9.
        // gamma = 2 * [(4 sqrt 2/9)^2 + 2 * (8/9 - 4/3)^2] = 2 * (32 + 16 + 16)/81 = 128/81.
        // pi_ij = mean(y_i^2 y_j^2) - s_ij^2:
        //   AA, BB: 4/3 - 16/9 = -4/9;  CC: 32/3 - 64/9 = 32/9;
        //   AB: 4/3 - 0 = 12/9;  AC, BC: 8/3 - 16/9 = 8/9.
        //   pi = -4/9 - 4/9 + 32/9 + 2 * (12/9 + 8/9 + 8/9) = 80/9;  rho_diag = 24/9.
        // theta_ij = mean(y_i^3 y_j) - s_ii s_ij:
        //   AB = BA = 0;  AC = BC = 4/3 - 16/9 = -4/9;  CA = CB = 16/3 - 32/9 = 16/9.
        //   sd_C / sd_A = sqrt 2, so rho_off = r_bar * [2 * sqrt 2 * (-4/9) + 2 * (1/sqrt 2) * 16/9]
        //   = (sqrt 2/3) * (8 sqrt 2/9) = 16/27.
        // rho = 24/9 + 16/27 = 88/27.  kappa = (80/9 - 88/27) / (128/81) = (152/27) * (81/128) = 3.5625.
        // intensity = kappa / T = 3.5625 / 4 = 0.890625 = 57/64.
        // Shrunk: diagonal unchanged; AB = (57/64) * 4 sqrt 2/9 = 19 sqrt 2/48;
        //   AC = BC = (57/64) * 8/9 + (7/64) * 4/3 = 15/16.
        let r = vec![vec![1.0, -1.0, 1.0, -1.0], vec![1.0, 1.0, -1.0, -1.0], vec![2.0, 0.0, 0.0, -2.0]];
        let lw = ledoit_wolf(&r).unwrap();
        let s2 = 2.0_f64.sqrt();
        assert!(close(lw.mean_correlation, s2 / 3.0, 1e-15));
        assert!(close(lw.target[0][1], 4.0 * s2 / 9.0, 1e-15) && close(lw.target[0][2], 8.0 / 9.0, 1e-15));
        assert!(close(lw.intensity, 57.0 / 64.0, 1e-14), "{}", lw.intensity);
        let shrunk = shrink_matrix(&lw.sample, &lw.target, lw.intensity);
        let expected = [
            [4.0 / 3.0, 19.0 * s2 / 48.0, 15.0 / 16.0],
            [19.0 * s2 / 48.0, 4.0 / 3.0, 15.0 / 16.0],
            [15.0 / 16.0, 15.0 / 16.0, 8.0 / 3.0],
        ];
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    close(shrunk[i][j], expected[i][j], 1e-14),
                    "({i},{j}) {} vs {}",
                    shrunk[i][j],
                    expected[i][j]
                );
            }
        }
    }

    #[test]
    fn annualizing_scales_the_estimates_but_not_the_intensity() {
        let (symbols, series) = five();
        let (d, y) = (
            shrunk_covariance_from_closes(&symbols, &series, 1.0).unwrap(),
            shrunk_covariance_from_closes(&symbols, &series, 252.0).unwrap(),
        );
        assert_eq!(d.intensity, y.intensity);
        assert!(close(y.shrunk[1][2], 252.0 * d.shrunk[1][2], 1e-15));
        let (d, y) = (
            shrunk_mean_from_closes(&symbols, &series, 1.0).unwrap(),
            shrunk_mean_from_closes(&symbols, &series, 252.0).unwrap(),
        );
        assert_eq!(d.intensity, y.intensity);
        assert!(close(y.shrunk[0], 252.0 * d.shrunk[0], 1e-15));
    }

    #[test]
    fn the_same_closes_give_the_same_estimates() {
        let (symbols, series) = five();
        assert_eq!(
            shrunk_mean_from_closes(&symbols, &series, 252.0),
            shrunk_mean_from_closes(&symbols, &series, 252.0)
        );
        assert_eq!(
            shrunk_covariance_from_closes(&symbols, &series, 252.0),
            shrunk_covariance_from_closes(&symbols, &series, 252.0)
        );
    }

    #[test]
    fn short_unequal_and_non_finite_histories_are_refused() {
        let (symbols, series) = five();
        let both = |s: &[String], c: &[Vec<f64>], p: f64| {
            let m = shrunk_mean_from_closes(s, c, p).map(|_| ());
            let v = shrunk_covariance_from_closes(s, c, p).map(|_| ());
            assert_eq!(format!("{m:?}"), format!("{v:?}"), "both estimates refuse alike");
            m.unwrap_err()
        };

        let mut short: Vec<Vec<f64>> = series.iter().map(|c| c[..MIN_VOLATILITY_PRICES].to_vec()).collect();
        short[2].pop();
        assert_eq!(
            both(&symbols, &short, 252.0),
            PortfolioError::InsufficientHistory { symbol: "TLT".into(), need: 60, have: 59 }
        );

        let mut uneven = series.clone();
        uneven[3].push(120.0);
        assert_eq!(
            both(&symbols, &uneven, 252.0),
            PortfolioError::UnequalHistory { symbol: "GLD".into(), need: 251, have: 252 }
        );

        for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut prices = series.clone();
            prices[1][40] = bad;
            assert!(matches!(
                both(&symbols, &prices, 252.0),
                PortfolioError::InvalidPrice { ref symbol, .. } if symbol == "QQQ"
            ));
        }
        for bad in [f64::NAN, f64::INFINITY, 0.0] {
            assert!(matches!(both(&symbols, &series, bad), PortfolioError::InvalidVolatility { .. }));
        }

        assert_eq!(
            both(&symbols[..2], &series[..1], 252.0),
            PortfolioError::LengthMismatch { symbols: 2, values: 1 }
        );
        assert_eq!(both(&[], &[], 252.0), PortfolioError::Empty);
    }

    #[test]
    fn a_holding_that_never_moves_has_no_covariance_estimate() {
        let (symbols, mut series) = five();
        series[4] = vec![42.0; series[0].len()];
        assert_eq!(
            shrunk_covariance_from_closes(&symbols, &series, 252.0),
            Err(PortfolioError::SingularCovariance { symbol: "IWM".into() })
        );
        // Its mean is still defined: 0.
        let m = shrunk_mean_from_closes(&symbols, &series, 252.0).unwrap();
        assert_eq!(m.sample[4], 0.0);
    }
}
