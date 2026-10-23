//! GARCH(1,1) volatility modeling and forecasting with variance targeting.
//!
//! Standard-library only: zero external dependencies.

use crate::returns::{mean, variance, TRADING_DAYS_PER_YEAR};
use spectra_core::{Error, Result};

/// Fitted GARCH(1,1) model parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Garch11 {
    /// Long-run constant term: $\omega = \sigma_L^2 (1 - \alpha - \beta)$.
    pub omega: f64,
    /// ARCH coefficient (reaction to shocks): $\alpha$.
    pub alpha: f64,
    /// GARCH coefficient (persistence): $\beta$.
    pub beta: f64,
    /// Unconditional daily sample variance: $\sigma_L^2$.
    pub unconditional_variance: f64,
    /// Mean return: $\mu$.
    pub mu: f64,
    /// Log-likelihood of the fit.
    pub log_likelihood: f64,
    /// Persistence: $\alpha + \beta$.
    pub persistence: f64,
}

impl Garch11 {
    /// Fit a GARCH(1,1) model on a return series using variance targeting.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if fewer than 30 observations or series has zero variance.
    pub fn fit(returns: &[f64]) -> Result<Self> {
        let t = returns.len();
        if t < 30 {
            return Err(Error::InvalidArgument(format!(
                "GARCH(1,1) requires at least 30 observations, got {t}"
            )));
        }

        let mu = mean(returns);
        let sample_var = variance(returns);
        if sample_var <= 1e-12 {
            return Err(Error::InvalidArgument("Returns series has zero or negligible variance".into()));
        }

        let residuals: Vec<f64> = returns.iter().map(|&r| r - mu).collect();

        let nll = |alpha: f64, beta: f64| -> f64 {
            if alpha < 0.0 || beta < 0.0 || (alpha + beta) >= 0.999 {
                return 1e12;
            }
            let omega = sample_var * (1.0 - alpha - beta);
            let mut h = sample_var;
            let mut sum_ll = 0.0;

            for eps in &residuals {
                if h <= 1e-12 {
                    return 1e12;
                }
                let eps_sq = eps * eps;
                sum_ll += h.ln() + eps_sq / h;
                h = omega + alpha * eps_sq + beta * h;
            }

            0.5 * sum_ll
        };

        let mut best_nll = f64::INFINITY;
        let mut best_alpha = 0.08;
        let mut best_beta = 0.88;

        let mut a = 0.01;
        while a <= 0.25 {
            let mut b = 0.60;
            while b <= 0.98 {
                if a + b < 0.999 {
                    let score = nll(a, b);
                    if score < best_nll {
                        best_nll = score;
                        best_alpha = a;
                        best_beta = b;
                    }
                }
                b += 0.02;
            }
            a += 0.01;
        }

        let mut step = 0.01;
        for _ in 0..6 {
            let candidates = [
                (best_alpha + step, best_beta),
                (best_alpha - step, best_beta),
                (best_alpha, best_beta + step),
                (best_alpha, best_beta - step),
                (best_alpha + step, best_beta + step),
                (best_alpha - step, best_beta - step),
            ];
            let mut improved = false;
            for (ca, cb) in candidates {
                if ca >= 0.001 && cb >= 0.001 && ca + cb < 0.999 {
                    let score = nll(ca, cb);
                    if score < best_nll {
                        best_nll = score;
                        best_alpha = ca;
                        best_beta = cb;
                        improved = true;
                    }
                }
            }
            if !improved {
                step *= 0.5;
            }
        }

        let omega = sample_var * (1.0 - best_alpha - best_beta);
        let persistence = best_alpha + best_beta;
        let log_likelihood = -best_nll - 0.5 * t as f64 * (2.0 * std::f64::consts::PI).ln();

        Ok(Self {
            omega,
            alpha: best_alpha,
            beta: best_beta,
            unconditional_variance: sample_var,
            mu,
            log_likelihood,
            persistence,
        })
    }

    /// Forecast daily variance $k$ steps ahead from the last observation.
    pub fn forecast_variance(&self, last_return: f64, last_variance: Option<f64>, k: usize) -> f64 {
        let last_h = last_variance.unwrap_or(self.unconditional_variance);
        let eps = last_return - self.mu;
        let h_1 = self.omega + self.alpha * eps * eps + self.beta * last_h;

        if k <= 1 {
            return h_1;
        }

        let term = self.persistence.powi(i32::try_from(k - 1).unwrap_or(1));
        self.unconditional_variance + term * (h_1 - self.unconditional_variance)
    }

    /// Forecast annualized volatility for 1-step ahead.
    pub fn forecast_volatility_annualized(&self, last_return: f64, last_variance: Option<f64>) -> f64 {
        let h = self.forecast_variance(last_return, last_variance, 1);
        (h * TRADING_DAYS_PER_YEAR).sqrt()
    }

    /// Unconditional annualized volatility.
    pub fn unconditional_volatility_annualized(&self) -> f64 {
        (self.unconditional_variance * TRADING_DAYS_PER_YEAR).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_garch_fit_synthetic_series() {
        let mut returns = Vec::with_capacity(200);
        let mut h: f64 = 0.0001; // ~15.8% vol
        let omega = 0.000005;
        let alpha = 0.10;
        let beta = 0.85;

        for i in 0..200 {
            let eps = ((i as f64 * 0.137).sin()) * h.sqrt();
            returns.push(eps);
            h = omega + alpha * eps * eps + beta * h;
        }

        let model = Garch11::fit(&returns).unwrap();
        assert!(model.alpha > 0.0);
        assert!(model.beta > 0.5);
        assert!(model.persistence < 1.0);
        assert!(model.omega > 0.0);

        let forecast_1d = model.forecast_variance(returns[returns.len() - 1], None, 1);
        assert!(forecast_1d > 0.0);

        let vol_ann = model.forecast_volatility_annualized(returns[returns.len() - 1], None);
        assert!(vol_ann > 0.0 && vol_ann.is_finite());
    }

    #[test]
    fn test_garch_parameter_stability_across_rolling_windows() {
        let mut returns = Vec::with_capacity(150);
        let mut h: f64 = 0.0001;
        for i in 0..150 {
            let z = ((i as f64 * 0.31).sin() + (i as f64 * 0.77).cos()) * 0.7071;
            let eps = z * h.sqrt();
            returns.push(eps);
            h = 0.000004 + 0.08 * eps * eps + 0.88 * h;
        }

        let window1 = &returns[0..80];
        let window2 = &returns[20..100];

        let fit1 = Garch11::fit(window1).unwrap();
        let fit2 = Garch11::fit(window2).unwrap();

        assert!(
            (fit1.persistence - fit2.persistence).abs() < 0.10,
            "Persistence must be stable across rolling windows"
        );
        assert!((fit1.beta - fit2.beta).abs() < 0.12, "Beta must be stable across rolling windows");
    }
}
