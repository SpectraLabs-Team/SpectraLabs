//! Extreme Value Theory (EVT) Peaks-Over-Threshold (POT) tail risk estimation.
//!
//! Fits a Generalized Pareto Distribution (GPD) to extreme losses exceeding a high threshold.
//! Standard-library only: zero external dependencies.

use crate::metrics::quantile_sorted;
use spectra_core::{Error, Result};

/// Fitted Extreme Value Theory model on the upper tail of losses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvtTailModel {
    /// Threshold $u$ chosen for POT.
    pub threshold: f64,
    /// Shape parameter (tail index $\xi$): positive indicates heavy-tailed Frechet-type tail.
    pub xi: f64,
    /// Scale parameter ($\beta > 0$).
    pub beta: f64,
    /// Total number of loss observations $N$.
    pub total_observations: usize,
    /// Number of exceedances $N_u = |\{l_i : l_i > u\}|$.
    pub exceedances_count: usize,
    /// Kolmogorov-Smirnov test statistic $D$ measuring goodness-of-fit.
    pub ks_statistic: f64,
    /// Whether the model passed the Kolmogorov-Smirnov goodness-of-fit check.
    pub passes_goodness_of_fit: bool,
}

impl EvtTailModel {
    /// Fit a GPD to the upper tail of `losses` using Peaks-Over-Threshold (POT).
    pub fn fit(losses: &[f64], threshold_quantile: f64) -> Result<Self> {
        let n = losses.len();
        if n < 50 {
            return Err(Error::InvalidArgument(format!(
                "EVT POT requires at least 50 loss observations, got {n}"
            )));
        }
        if !(threshold_quantile >= 0.70 && threshold_quantile < 0.999) {
            return Err(Error::InvalidArgument("threshold_quantile must be in [0.70, 0.999)".into()));
        }

        let mut sorted = losses.to_vec();
        sorted.sort_by(f64::total_cmp);

        let u = quantile_sorted(&sorted, threshold_quantile);
        let mut exceedances: Vec<f64> = sorted.iter().copied().filter(|&l| l > u).map(|l| l - u).collect();
        let n_u = exceedances.len();

        if n_u < 8 {
            return Err(Error::InvalidArgument(format!(
                "Too few exceedances ({n_u}) above threshold {u} for reliable EVT fit (minimum 8)"
            )));
        }

        exceedances.sort_by(f64::total_cmp);

        let a_0 = exceedances.iter().sum::<f64>() / n_u as f64;
        let mut a_1_sum = 0.0;
        for (j, &y) in exceedances.iter().enumerate() {
            let weight = (n_u - 1 - j) as f64 / (n_u - 1) as f64;
            a_1_sum += weight * y;
        }
        let a_1 = a_1_sum / n_u as f64;

        let denom = a_0 - 2.0 * a_1;
        let (mut xi, mut beta) = if denom.abs() > 1e-9 {
            let xi_est = 2.0 - a_0 / denom;
            let beta_est = (2.0 * a_0 * a_1) / denom;
            (xi_est, beta_est)
        } else {
            (0.0, a_0)
        };

        if beta <= 0.0 {
            beta = a_0.max(1e-6);
            xi = 0.0;
        } else {
            xi = xi.clamp(-0.5, 0.9);
        }

        let mut ks_stat = 0.0f64;
        for (j, &y) in exceedances.iter().enumerate() {
            let empirical_cdf = (j + 1) as f64 / n_u as f64;
            let theoretical_cdf = if xi.abs() < 1e-6 {
                1.0 - (-y / beta).exp()
            } else {
                let term = (1.0 + xi * y / beta).max(1e-12);
                1.0 - term.powf(-1.0 / xi)
            };
            ks_stat = ks_stat.max((empirical_cdf - theoretical_cdf).abs());
        }

        let ks_crit = 1.36 / (n_u as f64).sqrt();
        let passes = ks_stat < ks_crit;

        Ok(Self {
            threshold: u,
            xi,
            beta,
            total_observations: n,
            exceedances_count: n_u,
            ks_statistic: ks_stat,
            passes_goodness_of_fit: passes,
        })
    }

    /// Calculate EVT Value-at-Risk at confidence level `p` (e.g. 0.99 or 0.999).
    pub fn var(&self, p: f64) -> f64 {
        let p_clamped = p.clamp(0.0, 0.99999);
        let ratio = (self.total_observations as f64 / self.exceedances_count as f64) * (1.0 - p_clamped);

        if self.xi.abs() < 1e-6 {
            self.threshold - self.beta * ratio.max(1e-18).ln()
        } else {
            let term = ratio.max(1e-18).powf(-self.xi);
            self.threshold + (self.beta / self.xi) * (term - 1.0)
        }
    }

    /// Calculate EVT Expected Shortfall (CVaR) at confidence level `p`.
    pub fn cvar(&self, p: f64) -> f64 {
        let var_val = self.var(p);
        if self.xi >= 1.0 {
            return var_val;
        }
        (var_val + self.beta - self.xi * self.threshold) / (1.0 - self.xi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evt_tail_fit_and_goodness_of_fit() {
        let mut losses = Vec::with_capacity(1000);
        for i in 0..1000 {
            let z = ((i as f64 * 0.17).sin() + (i as f64 * 0.53).cos()) * 0.7071;
            let chi = (1.0 + (i as f64 * 0.31).sin().powi(2) + (i as f64 * 0.71).cos().powi(2)) / 3.0;
            let t_val = z / chi.sqrt();
            let loss = 100.0 * (t_val * 0.02);
            losses.push(loss);
        }

        let model = EvtTailModel::fit(&losses, 0.90).unwrap();
        assert!(model.beta > 0.0);
        assert!(model.exceedances_count >= 50);
        assert!(
            model.passes_goodness_of_fit,
            "EVT fit must pass KS goodness-of-fit test: stat = {}",
            model.ks_statistic
        );

        let var99 = model.var(0.99);
        let cvar99 = model.cvar(0.99);

        assert!(var99 > model.threshold);
        assert!(cvar99 >= var99, "CVaR ({cvar99}) must be >= VaR ({var99})");
    }
}
