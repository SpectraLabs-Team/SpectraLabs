//! VaR Backtesting: exception counting, Kupiec POF test, and Christoffersen independence test.
//!
//! Standard-library only: zero external dependencies.

use spectra_core::{Error, Result};

/// Basel Committee Traffic Light zone for VaR backtest results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselZone {
    /// Green: model is accepted (exceptions within normal bounds).
    Green,
    /// Yellow: supervisory review / warning (borderline exceptions).
    Yellow,
    /// Red: model is rejected (statistically excessive exceptions).
    Red,
}

impl BaselZone {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Red => "red",
        }
    }
}

/// Comprehensive outcome of a VaR backtesting run.
#[derive(Debug, Clone, PartialEq)]
pub struct VarBacktestResult {
    /// Number of observations evaluated.
    pub total_observations: usize,
    /// Number of exceptions where realized loss exceeded forecasted VaR.
    pub exceptions_count: usize,
    /// Expected exceptions under the null hypothesis `(1 - confidence) * total`.
    pub expected_exceptions: f64,
    /// Realized exception rate `exceptions / total`.
    pub failure_rate: f64,
    /// Kupiec Proportion of Failures (POF) likelihood ratio statistic.
    pub kupiec_stat: f64,
    /// Kupiec test p-value.
    pub kupiec_p_value: f64,
    /// Christoffersen independence test likelihood ratio statistic.
    pub christoffersen_stat: f64,
    /// Christoffersen independence test p-value.
    pub christoffersen_p_value: f64,
    /// Conditional coverage (joint) likelihood ratio statistic: $LR_{CC} = LR_{POF} + LR_{ind}$.
    pub conditional_coverage_stat: f64,
    /// Conditional coverage test p-value.
    pub conditional_coverage_p_value: f64,
    /// Basel traffic light categorization.
    pub basel_zone: BaselZone,
}

/// Run a formal VaR backtest evaluating realized losses against forecasted VaR thresholds.
pub fn backtest_var(
    realized_losses: &[f64],
    var_forecasts: &[f64],
    confidence: f64,
) -> Result<VarBacktestResult> {
    let n = realized_losses.len();
    if n != var_forecasts.len() {
        return Err(Error::InvalidArgument(format!(
            "Length mismatch in VaR backtest: {} realized vs {} forecasts",
            n,
            var_forecasts.len()
        )));
    }
    if n < 20 {
        return Err(Error::InvalidArgument(format!(
            "VaR backtesting requires at least 20 observations, got {n}"
        )));
    }
    if !(confidence > 0.0 && confidence < 1.0) {
        return Err(Error::InvalidArgument("confidence must be in (0, 1)".into()));
    }

    let alpha_target = 1.0 - confidence;
    let mut hits = Vec::with_capacity(n);
    let mut exceptions = 0usize;

    for i in 0..n {
        let is_hit = realized_losses[i] > var_forecasts[i];
        if is_hit {
            exceptions += 1;
        }
        hits.push(is_hit);
    }

    let x = exceptions as f64;
    let n_f = n as f64;
    let expected_x = alpha_target * n_f;
    let failure_rate = x / n_f;

    let kupiec_stat = if exceptions == 0 {
        (-2.0 * n_f * (1.0 - alpha_target).ln()).max(0.0)
    } else if exceptions == n {
        (-2.0 * n_f * alpha_target.ln()).max(0.0)
    } else {
        let log_num = (n_f - x) * (1.0 - alpha_target).ln() + x * alpha_target.ln();
        let log_den = (n_f - x) * (1.0 - failure_rate).ln() + x * failure_rate.ln();
        (-2.0 * (log_num - log_den)).max(0.0)
    };
    let kupiec_p_value = chi2_1_survival(kupiec_stat);

    let mut n00 = 0usize;
    let mut n01 = 0usize;
    let mut n10 = 0usize;
    let mut n11 = 0usize;

    for i in 1..n {
        match (hits[i - 1], hits[i]) {
            (false, false) => n00 += 1,
            (false, true) => n01 += 1,
            (true, false) => n10 += 1,
            (true, true) => n11 += 1,
        }
    }

    let pi_01 = if (n00 + n01) > 0 { n01 as f64 / (n00 + n01) as f64 } else { 0.0 };
    let pi_11 = if (n10 + n11) > 0 { n11 as f64 / (n10 + n11) as f64 } else { 0.0 };
    let pi_all = if (n01 + n11) > 0 { (n01 + n11) as f64 / (n00 + n01 + n10 + n11) as f64 } else { 0.0 };

    let christoffersen_stat = if pi_01 <= 0.0 || pi_01 >= 1.0 || pi_11 <= 0.0 || pi_11 >= 1.0 {
        0.0
    } else {
        let log_num = (n00 + n10) as f64 * (1.0 - pi_all).ln() + (n01 + n11) as f64 * pi_all.ln();
        let log_den = (n00 as f64) * (1.0 - pi_01).ln()
            + (n01 as f64) * pi_01.ln()
            + (n10 as f64) * (1.0 - pi_11).ln()
            + (n11 as f64) * pi_11.ln();
        (-2.0 * (log_num - log_den)).max(0.0)
    };
    let christoffersen_p_value = chi2_1_survival(christoffersen_stat);

    let cc_stat = kupiec_stat + christoffersen_stat;
    let cc_p_value = (-0.5 * cc_stat).exp();

    let scaled_exceptions = if n > 0 { (x / n_f) * 250.0 } else { 0.0 };
    let basel_zone = if scaled_exceptions <= 4.0 {
        BaselZone::Green
    } else if scaled_exceptions <= 9.0 {
        BaselZone::Yellow
    } else {
        BaselZone::Red
    };

    Ok(VarBacktestResult {
        total_observations: n,
        exceptions_count: exceptions,
        expected_exceptions: expected_x,
        failure_rate,
        kupiec_stat,
        kupiec_p_value,
        christoffersen_stat,
        christoffersen_p_value,
        conditional_coverage_stat: cc_stat,
        conditional_coverage_p_value: cc_p_value,
        basel_zone,
    })
}

/// Fissler-Ziegel (2016) joint scoring loss for Value-at-Risk and Expected Shortfall.
///
/// ES and VaR are jointly elicitable under the FZ scoring family.
/// We use the 0-homogeneous canonical scoring function (Patton, Ziegel, & Chen 2019):
///
///   S(y, v, e; \alpha) = \frac{1}{\alpha e} 1_{\{y > v\}} (y - v) - \frac{v}{e} - \ln(e) + 1
///
/// Lower score indicates superior joint quantile and tail expectation forecasts.
pub fn fissler_ziegel_loss(
    realized_losses: &[f64],
    var_forecasts: &[f64],
    es_forecasts: &[f64],
    alpha: f64,
) -> Result<f64> {
    let n = realized_losses.len();
    if n != var_forecasts.len() || n != es_forecasts.len() {
        return Err(Error::InvalidArgument(format!(
            "Mismatched slice lengths: {} realized vs {} VaR vs {} ES",
            n,
            var_forecasts.len(),
            es_forecasts.len()
        )));
    }
    if n == 0 {
        return Err(Error::InvalidArgument("Cannot compute FZ loss on empty series".into()));
    }
    if !(alpha > 0.0 && alpha < 1.0) {
        return Err(Error::InvalidArgument("alpha must be in (0, 1)".into()));
    }

    let mut sum_loss = 0.0;
    for i in 0..n {
        let y = realized_losses[i];
        let v = var_forecasts[i];
        let e = es_forecasts[i].max(v + 1e-6);
        let e_safe = e.max(1e-8);
        let hit = if y > v { 1.0 } else { 0.0 };
        let term1 = (hit * (y - v)) / (alpha * e_safe);
        let term2 = -v / e_safe;
        let term3 = -e_safe.ln() + 1.0;
        sum_loss += term1 + term2 + term3;
    }

    Ok(sum_loss / (n as f64))
}

/// Result of the Jarque-Bera test for normality.
#[derive(Debug, Clone, PartialEq)]
pub struct JarqueBeraResult {
    /// Sample size $N$.
    pub sample_size: usize,
    /// Sample mean $\bar{x}$.
    pub mean: f64,
    /// Sample variance $\sigma^2$.
    pub variance: f64,
    /// Sample skewness $S = \frac{1}{N}\sum ((x_i - \bar{x})/\sigma)^3$.
    pub skewness: f64,
    /// Sample kurtosis $K = \frac{1}{N}\sum ((x_i - \bar{x})/\sigma)^4$.
    pub kurtosis: f64,
    /// Excess kurtosis $K - 3$.
    pub excess_kurtosis: f64,
    /// Jarque-Bera test statistic $\text{JB} = \frac{N}{6}(S^2 + \frac{(K-3)^2}{4})$.
    pub jb_stat: f64,
    /// Chi-squared (df=2) asymptotic p-value.
    pub p_value: f64,
    /// True if null hypothesis of normality cannot be rejected at $\alpha = 0.05$.
    pub is_normal: bool,
}

/// Perform Jarque-Bera goodness-of-fit test for normality on empirical returns or innovations.
pub fn jarque_bera_test(data: &[f64]) -> Result<JarqueBeraResult> {
    let n = data.len();
    if n < 8 {
        return Err(Error::InvalidArgument(format!(
            "Jarque-Bera test requires at least 8 observations, got {n}"
        )));
    }
    let n_f = n as f64;
    let mean = data.iter().sum::<f64>() / n_f;

    let mut m2 = 0.0;
    let mut m3 = 0.0;
    let mut m4 = 0.0;
    for &x in data {
        let diff = x - mean;
        let d2 = diff * diff;
        m2 += d2;
        m3 += d2 * diff;
        m4 += d2 * d2;
    }

    let variance = m2 / n_f;
    if variance <= 1e-12 {
        return Err(Error::InvalidArgument("Data variance is near zero".into()));
    }

    let s = (m3 / n_f) / variance.powf(1.5);
    let k = (m4 / n_f) / (variance * variance);
    let excess_k = k - 3.0;

    let jb_stat = (n_f / 6.0) * (s * s + (excess_k * excess_k) / 4.0);
    // Survival function for Chi^2 with 2 degrees of freedom: P(X >= x) = exp(-x / 2)
    let p_value = (-0.5 * jb_stat).exp().clamp(0.0, 1.0);
    let is_normal = p_value > 0.05;

    Ok(JarqueBeraResult {
        sample_size: n,
        mean,
        variance,
        skewness: s,
        kurtosis: k,
        excess_kurtosis: excess_k,
        jb_stat,
        p_value,
        is_normal,
    })
}

fn chi2_1_survival(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let z = x.sqrt();
    2.0 * (1.0 - normal_cdf(z))
}

fn normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x_abs = x.abs();
    let p = 0.3275911;
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;

    let t = 1.0 / (1.0 + p * x_abs);
    let poly = ((((a5 * t + a4) * t + a3) * t + a2) * t + a1) * t;
    let y = 1.0 - poly * (-x_abs * x_abs).exp();
    sign * y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_backtest_with_expected_exceptions() {
        let n = 250;
        let mut realized = vec![0.0; n];
        let var_forecast = vec![100.0; n];

        realized[10] = 120.0;
        realized[150] = 110.0;

        let res = backtest_var(&realized, &var_forecast, 0.99).unwrap();
        assert_eq!(res.exceptions_count, 2);
        assert_eq!(res.basel_zone, BaselZone::Green);
        assert!(res.kupiec_p_value > 0.05, "P-value should not reject model: {}", res.kupiec_p_value);
    }

    #[test]
    fn test_var_backtest_with_excessive_exceptions() {
        let n = 250;
        let mut realized = vec![0.0; n];
        let var_forecast = vec![100.0; n];

        for i in (0..15).map(|k| k * 16) {
            realized[i] = 120.0;
        }

        let res = backtest_var(&realized, &var_forecast, 0.99).unwrap();
        assert_eq!(res.exceptions_count, 15);
        assert_eq!(res.basel_zone, BaselZone::Red);
        assert!(res.kupiec_p_value < 0.01, "P-value should reject model: {}", res.kupiec_p_value);
    }

    #[test]
    fn test_fissler_ziegel_loss_computation() {
        let realized = vec![0.01, -0.02, 0.05, 0.02, 0.01];
        let var = vec![0.03, 0.03, 0.03, 0.03, 0.03];
        let es = vec![0.045, 0.045, 0.045, 0.045, 0.045];
        let loss = fissler_ziegel_loss(&realized, &var, &es, 0.05).unwrap();
        assert!(loss.is_finite());
    }

    #[test]
    fn test_jarque_bera_normality_detection() {
        // Standard normal-like symmetric data with light kurtosis
        let normal_data = vec![-1.5, -1.1, -0.8, -0.5, -0.2, 0.0, 0.2, 0.5, 0.8, 1.1, 1.5];
        let res_norm = jarque_bera_test(&normal_data).unwrap();
        assert!(res_norm.is_normal);
        assert!(res_norm.p_value > 0.05);

        // Strongly skewed data with heavy tail (outlier spike)
        let mut heavy_tail = vec![0.0; 50];
        heavy_tail[0] = 25.0;
        heavy_tail[1] = 30.0;
        let res_skew = jarque_bera_test(&heavy_tail).unwrap();
        assert!(!res_skew.is_normal);
        assert!(res_skew.jb_stat > 10.0);
    }
}
