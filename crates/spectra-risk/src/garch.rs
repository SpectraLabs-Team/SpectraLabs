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

/// Fitted GJR-GARCH(1,1) model parameters capturing asymmetric leverage effect.
///
/// Glosten, Jagannathan, and Runkle (1993):
/// $\sigma_t^2 = \omega + (\alpha + \gamma \cdot \mathbf{1}_{\{\epsilon_{t-1} < 0\}}) \epsilon_{t-1}^2 + \beta \sigma_{t-1}^2$
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GjrGarch11 {
    /// Long-run constant term: $\omega$.
    pub omega: f64,
    /// Symmetric ARCH coefficient: $\alpha$.
    pub alpha: f64,
    /// Asymmetric leverage coefficient: $\gamma$ ($\gamma > 0$ means negative shocks produce higher volatility).
    pub gamma: f64,
    /// GARCH persistence coefficient: $\beta$.
    pub beta: f64,
    /// Unconditional daily sample variance: $\sigma_L^2$.
    pub unconditional_variance: f64,
    /// Mean return: $\mu$.
    pub mu: f64,
    /// Log-likelihood of the fit.
    pub log_likelihood: f64,
    /// Effective persistence: $\alpha + \beta + \gamma / 2$.
    pub persistence: f64,
}

impl GjrGarch11 {
    /// Fit a GJR-GARCH(1,1) model on returns using variance targeting.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if fewer than 30 observations or series has zero variance.
    pub fn fit(returns: &[f64]) -> Result<Self> {
        let t = returns.len();
        if t < 30 {
            return Err(Error::InvalidArgument(format!(
                "GJR-GARCH(1,1) requires at least 30 observations, got {t}"
            )));
        }

        let mu = mean(returns);
        let sample_var = variance(returns);
        if sample_var <= 1e-12 {
            return Err(Error::InvalidArgument("Returns series has zero or negligible variance".into()));
        }

        let residuals: Vec<f64> = returns.iter().map(|&r| r - mu).collect();

        let nll = |alpha: f64, gamma: f64, beta: f64| -> f64 {
            let persistence = alpha + beta + 0.5 * gamma;
            if alpha < 0.0 || gamma < 0.0 || beta < 0.0 || persistence >= 0.999 {
                return 1e12;
            }
            let omega = sample_var * (1.0 - persistence);
            let mut h = sample_var;
            let mut sum_ll = 0.0;

            for eps in &residuals {
                if h <= 1e-12 {
                    return 1e12;
                }
                let eps_sq = eps * eps;
                sum_ll += h.ln() + eps_sq / h;
                let leverage = if *eps < 0.0 { gamma } else { 0.0 };
                h = omega + (alpha + leverage) * eps_sq + beta * h;
            }

            0.5 * sum_ll
        };

        let mut best_nll = f64::INFINITY;
        let mut best_alpha = 0.05;
        let mut best_gamma = 0.08;
        let mut best_beta = 0.85;

        let mut a = 0.02;
        while a <= 0.15 {
            let mut g = 0.02;
            while g <= 0.20 {
                let mut b = 0.65;
                while b <= 0.92 {
                    if a + b + 0.5 * g < 0.999 {
                        let score = nll(a, g, b);
                        if score < best_nll {
                            best_nll = score;
                            best_alpha = a;
                            best_gamma = g;
                            best_beta = b;
                        }
                    }
                    b += 0.05;
                }
                g += 0.04;
            }
            a += 0.03;
        }

        let mut step = 0.01;
        for _ in 0..5 {
            let candidates = [
                (best_alpha + step, best_gamma, best_beta),
                (best_alpha - step, best_gamma, best_beta),
                (best_alpha, best_gamma + step, best_beta),
                (best_alpha, best_gamma - step, best_beta),
                (best_alpha, best_gamma, best_beta + step),
                (best_alpha, best_gamma, best_beta - step),
            ];
            let mut improved = false;
            for (ca, cg, cb) in candidates {
                if ca >= 0.001 && cg >= 0.0 && cb >= 0.001 && (ca + cb + 0.5 * cg) < 0.999 {
                    let score = nll(ca, cg, cb);
                    if score < best_nll {
                        best_nll = score;
                        best_alpha = ca;
                        best_gamma = cg;
                        best_beta = cb;
                        improved = true;
                    }
                }
            }
            if !improved {
                step *= 0.5;
            }
        }

        let persistence = best_alpha + best_beta + 0.5 * best_gamma;
        let omega = sample_var * (1.0 - persistence);
        let log_likelihood = -best_nll - 0.5 * t as f64 * (2.0 * std::f64::consts::PI).ln();

        Ok(Self {
            omega,
            alpha: best_alpha,
            gamma: best_gamma,
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
        let lev = if eps < 0.0 { self.gamma } else { 0.0 };
        let h_1 = self.omega + (self.alpha + lev) * eps * eps + self.beta * last_h;

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

/// Fitted Nelson (1991) Exponential GARCH(1,1) model parameters.
///
/// $\ln(\sigma_t^2) = \omega + \alpha (|\epsilon_{t-1}/\sigma_{t-1}| - \sqrt{2/\pi}) + \gamma (\epsilon_{t-1}/\sigma_{t-1}) + \beta \ln(\sigma_{t-1}^2)$
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Egarch11 {
    /// Constant in log-variance equation: $\omega$.
    pub omega: f64,
    /// ARCH magnitude coefficient: $\alpha$.
    pub alpha: f64,
    /// Asymmetry / leverage coefficient: $\gamma$ ($\gamma < 0$ means negative returns increase volatility).
    pub gamma: f64,
    /// GARCH log-variance persistence: $\beta$.
    pub beta: f64,
    /// Unconditional daily sample variance.
    pub unconditional_variance: f64,
    /// Mean return: $\mu$.
    pub mu: f64,
    /// Log-likelihood of the fit.
    pub log_likelihood: f64,
    /// Persistence: $\beta$.
    pub persistence: f64,
}

impl Egarch11 {
    /// Fit an EGARCH(1,1) model on returns.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] if fewer than 30 observations or series has zero variance.
    pub fn fit(returns: &[f64]) -> Result<Self> {
        let t = returns.len();
        if t < 30 {
            return Err(Error::InvalidArgument(format!(
                "EGARCH(1,1) requires at least 30 observations, got {t}"
            )));
        }

        let mu = mean(returns);
        let sample_var = variance(returns);
        if sample_var <= 1e-12 {
            return Err(Error::InvalidArgument("Returns series has zero or negligible variance".into()));
        }

        let residuals: Vec<f64> = returns.iter().map(|&r| r - mu).collect();
        let sqrt_2_over_pi = (2.0 / std::f64::consts::PI).sqrt();
        let log_sample_var = sample_var.ln();

        let nll = |alpha: f64, gamma: f64, beta: f64| -> f64 {
            if beta.abs() >= 0.999 || alpha < 0.0 {
                return 1e12;
            }
            let omega = log_sample_var * (1.0 - beta);
            let mut log_h = log_sample_var;
            let mut sum_ll = 0.0;

            for eps in &residuals {
                let h = log_h.exp();
                if !h.is_finite() || h <= 1e-14 {
                    return 1e12;
                }
                let std_res = *eps / h.sqrt();
                sum_ll += log_h + (eps * eps) / h;
                log_h = omega + alpha * (std_res.abs() - sqrt_2_over_pi) + gamma * std_res + beta * log_h;
            }

            0.5 * sum_ll
        };

        let mut best_nll = f64::INFINITY;
        let mut best_alpha = 0.12;
        let mut best_gamma = -0.06;
        let mut best_beta = 0.92;

        let mut a = 0.04;
        while a <= 0.25 {
            let mut g = -0.15;
            while g <= 0.05 {
                let mut b = 0.80;
                while b <= 0.98 {
                    let score = nll(a, g, b);
                    if score < best_nll {
                        best_nll = score;
                        best_alpha = a;
                        best_gamma = g;
                        best_beta = b;
                    }
                    b += 0.04;
                }
                g += 0.05;
            }
            a += 0.05;
        }

        let mut step = 0.01;
        for _ in 0..5 {
            let candidates = [
                (best_alpha + step, best_gamma, best_beta),
                (best_alpha - step, best_gamma, best_beta),
                (best_alpha, best_gamma + step, best_beta),
                (best_alpha, best_gamma - step, best_beta),
                (best_alpha, best_gamma, best_beta + step),
                (best_alpha, best_gamma, best_beta - step),
            ];
            let mut improved = false;
            for (ca, cg, cb) in candidates {
                if ca >= 0.0 && cb.abs() < 0.999 {
                    let score = nll(ca, cg, cb);
                    if score < best_nll {
                        best_nll = score;
                        best_alpha = ca;
                        best_gamma = cg;
                        best_beta = cb;
                        improved = true;
                    }
                }
            }
            if !improved {
                step *= 0.5;
            }
        }

        let omega = log_sample_var * (1.0 - best_beta);
        let log_likelihood = -best_nll - 0.5 * t as f64 * (2.0 * std::f64::consts::PI).ln();

        Ok(Self {
            omega,
            alpha: best_alpha,
            gamma: best_gamma,
            beta: best_beta,
            unconditional_variance: sample_var,
            mu,
            log_likelihood,
            persistence: best_beta,
        })
    }

    /// Forecast daily variance $k$ steps ahead from the last observation.
    pub fn forecast_variance(&self, last_return: f64, last_variance: Option<f64>, k: usize) -> f64 {
        let last_h = last_variance.unwrap_or(self.unconditional_variance);
        let log_last_h = last_h.ln();
        let eps = last_return - self.mu;
        let std_res = eps / last_h.sqrt().max(1e-8);
        let sqrt_2_over_pi = (2.0 / std::f64::consts::PI).sqrt();

        let log_h1 = self.omega
            + self.alpha * (std_res.abs() - sqrt_2_over_pi)
            + self.gamma * std_res
            + self.beta * log_last_h;
        let h1 = log_h1.exp();

        if k <= 1 {
            return h1;
        }

        let log_unconditional = self.unconditional_variance.ln();
        let term = self.persistence.powi(i32::try_from(k - 1).unwrap_or(1));
        (log_unconditional + term * (log_h1 - log_unconditional)).exp()
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

    #[test]
    fn test_gjr_garch_captures_asymmetric_leverage() {
        let mut returns = Vec::with_capacity(200);
        let mut h: f64 = 0.0001;
        for i in 0..200 {
            let z = ((i as f64 * 0.22).sin()) * 0.9;
            let eps = z * h.sqrt();
            returns.push(eps);
            let lev = if eps < 0.0 { 0.12 } else { 0.0 };
            h = 0.000005 + (0.04 + lev) * eps * eps + 0.85 * h;
        }

        let model = GjrGarch11::fit(&returns).unwrap();
        assert!(model.alpha > 0.0);
        assert!(model.beta > 0.6);
        assert!(model.persistence < 1.0);
        assert!(model.gamma >= 0.0);

        let f1 = model.forecast_variance(returns[returns.len() - 1], None, 1);
        assert!(f1 > 0.0 && f1.is_finite());
    }

    #[test]
    fn test_egarch_strictly_positive_variance() {
        let mut returns = Vec::with_capacity(180);
        for i in 0..180 {
            returns.push(((i as f64 * 0.43).sin() + (i as f64 * 0.88).cos()) * 0.012);
        }

        let model = Egarch11::fit(&returns).unwrap();
        assert!(model.beta.abs() < 1.0);
        assert!(model.unconditional_variance > 0.0);

        for k in 1..=21 {
            let f = model.forecast_variance(returns[returns.len() - 1], None, k);
            assert!(f > 0.0 && f.is_finite());
        }
    }
}
