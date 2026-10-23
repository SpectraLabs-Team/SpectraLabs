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
}
