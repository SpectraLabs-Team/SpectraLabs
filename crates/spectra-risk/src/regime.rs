//! Two-state Hidden Markov Model (HMM) for market regime detection and regime-conditional risk.
//!
//! State 0: Low volatility (Normal / Calm / Bull).
//! State 1: High volatility (Turbulent / Crisis / Bear).
//!
//! Standard-library only: zero external dependencies.

use crate::returns::{mean, variance, TRADING_DAYS_PER_YEAR};
use spectra_core::{Error, Result};

/// State index for regime modeling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegime {
    /// Regime 0: Low volatility / Normal.
    LowVolatility = 0,
    /// Regime 1: High volatility / Crisis.
    HighVolatility = 1,
}

impl MarketRegime {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LowVolatility => "low_volatility",
            Self::HighVolatility => "high_volatility",
        }
    }
}

/// Parameters for one Gaussian state in the HMM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegimeState {
    /// Daily mean return $\mu_k$.
    pub mu_daily: f64,
    /// Daily volatility $\sigma_k$.
    pub sigma_daily: f64,
    /// Annualized volatility $\sigma_k \times \sqrt{252}$.
    pub volatility_annualized: f64,
}

/// Fitted Two-State Hidden Markov Model.
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeModel {
    /// Low volatility state (Regime 0).
    pub state_0: RegimeState,
    /// High volatility state (Regime 1).
    pub state_1: RegimeState,
    /// Transition probability matrix: `transitions[i][j] = P(S_t = j | S_{t-1} = i)`.
    pub transitions: [[f64; 2]; 2],
    /// Initial state probabilities `[P(S_1 = 0), P(S_1 = 1)]`.
    pub initial_probs: [f64; 2],
    /// Filtered probability of being in state 1 at the end of the sample `P(S_T = 1 | r_{1:T})`.
    pub current_high_vol_prob: f64,
    /// Classified current regime based on filtered probability.
    pub current_regime: MarketRegime,
}

impl RegimeModel {
    /// Fit a 2-state Gaussian HMM on historical returns using in-sample Baum-Welch (EM).
    pub fn fit(returns: &[f64]) -> Result<Self> {
        let t = returns.len();
        if t < 30 {
            return Err(Error::InvalidArgument(format!(
                "HMM regime detection requires at least 30 observations, got {t}"
            )));
        }

        let overall_mean = mean(returns);
        let overall_var = variance(returns);
        if overall_var <= 1e-12 {
            return Err(Error::InvalidArgument("Returns series has zero variance".into()));
        }
        let overall_std = overall_var.sqrt();

        let mut mu = [overall_mean + 0.1 * overall_std, overall_mean - 0.2 * overall_std];
        let mut sigma = [0.7 * overall_std, 1.5 * overall_std];
        let mut trans = [[0.95, 0.05], [0.10, 0.90]];
        let mut pi = [0.7, 0.3];

        let normal_pdf = |x: f64, m: f64, s: f64| -> f64 {
            let s_clamped = s.max(1e-6);
            let diff = (x - m) / s_clamped;
            let exponent = -0.5 * diff * diff;
            (1.0 / (s_clamped * (2.0 * std::f64::consts::PI).sqrt())) * exponent.exp()
        };

        for _iter in 0..50 {
            let mut alpha = vec![[0.0, 0.0]; t];
            let mut c = vec![0.0; t];

            let b0_0 = normal_pdf(returns[0], mu[0], sigma[0]);
            let b1_0 = normal_pdf(returns[0], mu[1], sigma[1]);
            alpha[0][0] = pi[0] * b0_0;
            alpha[0][1] = pi[1] * b1_0;
            c[0] = (alpha[0][0] + alpha[0][1]).max(1e-18);
            alpha[0][0] /= c[0];
            alpha[0][1] /= c[0];

            for step in 1..t {
                let b0 = normal_pdf(returns[step], mu[0], sigma[0]);
                let b1 = normal_pdf(returns[step], mu[1], sigma[1]);

                alpha[step][0] = (alpha[step - 1][0] * trans[0][0] + alpha[step - 1][1] * trans[1][0]) * b0;
                alpha[step][1] = (alpha[step - 1][0] * trans[0][1] + alpha[step - 1][1] * trans[1][1]) * b1;
                c[step] = (alpha[step][0] + alpha[step][1]).max(1e-18);
                alpha[step][0] /= c[step];
                alpha[step][1] /= c[step];
            }

            let mut beta = vec![[0.0, 0.0]; t];
            beta[t - 1][0] = 1.0;
            beta[t - 1][1] = 1.0;

            for step in (0..t - 1).rev() {
                let b0 = normal_pdf(returns[step + 1], mu[0], sigma[0]);
                let b1 = normal_pdf(returns[step + 1], mu[1], sigma[1]);

                beta[step][0] = (trans[0][0] * b0 * beta[step + 1][0] + trans[0][1] * b1 * beta[step + 1][1])
                    / c[step + 1];
                beta[step][1] = (trans[1][0] * b0 * beta[step + 1][0] + trans[1][1] * b1 * beta[step + 1][1])
                    / c[step + 1];
            }

            let mut gamma = vec![[0.0, 0.0]; t];
            for step in 0..t {
                let denom = (alpha[step][0] * beta[step][0] + alpha[step][1] * beta[step][1]).max(1e-18);
                gamma[step][0] = (alpha[step][0] * beta[step][0]) / denom;
                gamma[step][1] = (alpha[step][1] * beta[step][1]) / denom;
            }

            pi[0] = gamma[0][0];
            pi[1] = gamma[0][1];

            let mut trans_num = [[0.0; 2]; 2];
            let mut trans_den = [0.0; 2];

            for step in 0..t - 1 {
                let b0 = normal_pdf(returns[step + 1], mu[0], sigma[0]);
                let b1 = normal_pdf(returns[step + 1], mu[1], sigma[1]);
                let denom = (alpha[step][0] * beta[step][0] + alpha[step][1] * beta[step][1]).max(1e-18);

                let xi_00 = (alpha[step][0] * trans[0][0] * b0 * beta[step + 1][0] / c[step + 1]) / denom;
                let xi_01 = (alpha[step][0] * trans[0][1] * b1 * beta[step + 1][1] / c[step + 1]) / denom;
                let xi_10 = (alpha[step][1] * trans[1][0] * b0 * beta[step + 1][0] / c[step + 1]) / denom;
                let xi_11 = (alpha[step][1] * trans[1][1] * b1 * beta[step + 1][1] / c[step + 1]) / denom;

                trans_num[0][0] += xi_00;
                trans_num[0][1] += xi_01;
                trans_num[1][0] += xi_10;
                trans_num[1][1] += xi_11;

                trans_den[0] += gamma[step][0];
                trans_den[1] += gamma[step][1];
            }

            for i in 0..2 {
                let d = trans_den[i].max(1e-12);
                trans[i][0] = (trans_num[i][0] / d).clamp(0.001, 0.999);
                trans[i][1] = (trans_num[i][1] / d).clamp(0.001, 0.999);
                let row_sum = trans[i][0] + trans[i][1];
                trans[i][0] /= row_sum;
                trans[i][1] /= row_sum;
            }

            for k in 0..2 {
                let g_sum = gamma.iter().map(|g| g[k]).sum::<f64>().max(1e-12);
                let new_mu = gamma.iter().enumerate().map(|(i, g)| g[k] * returns[i]).sum::<f64>() / g_sum;
                let new_var = gamma
                    .iter()
                    .enumerate()
                    .map(|(i, g)| {
                        let diff = returns[i] - new_mu;
                        g[k] * diff * diff
                    })
                    .sum::<f64>()
                    / g_sum;

                mu[k] = new_mu;
                sigma[k] = new_var.sqrt().max(1e-6);
            }
        }

        let (s0_idx, s1_idx) = if sigma[0] <= sigma[1] { (0, 1) } else { (1, 0) };

        let state_0 = RegimeState {
            mu_daily: mu[s0_idx],
            sigma_daily: sigma[s0_idx],
            volatility_annualized: sigma[s0_idx] * TRADING_DAYS_PER_YEAR.sqrt(),
        };
        let state_1 = RegimeState {
            mu_daily: mu[s1_idx],
            sigma_daily: sigma[s1_idx],
            volatility_annualized: sigma[s1_idx] * TRADING_DAYS_PER_YEAR.sqrt(),
        };

        let transitions =
            [[trans[s0_idx][s0_idx], trans[s0_idx][s1_idx]], [trans[s1_idx][s0_idx], trans[s1_idx][s1_idx]]];
        let initial_probs = [pi[s0_idx], pi[s1_idx]];

        let b0_t = normal_pdf(returns[t - 1], state_0.mu_daily, state_0.sigma_daily);
        let b1_t = normal_pdf(returns[t - 1], state_1.mu_daily, state_1.sigma_daily);
        let current_p0 = (transitions[0][0] * b0_t).max(1e-18);
        let current_p1 = (transitions[0][1] * b1_t).max(1e-18);
        let p1_norm = current_p1 / (current_p0 + current_p1);

        let current_regime =
            if p1_norm >= 0.5 { MarketRegime::HighVolatility } else { MarketRegime::LowVolatility };

        Ok(Self {
            state_0,
            state_1,
            transitions,
            initial_probs,
            current_high_vol_prob: p1_norm,
            current_regime,
        })
    }

    /// Decode most likely regime path $S_1, \dots, S_T$ using the Viterbi algorithm.
    pub fn decode_viterbi(&self, returns: &[f64]) -> Vec<MarketRegime> {
        let t = returns.len();
        if t == 0 {
            return Vec::new();
        }

        let normal_log_pdf = |x: f64, m: f64, s: f64| -> f64 {
            let s_clamped = s.max(1e-6);
            let diff = (x - m) / s_clamped;
            -0.5 * (2.0 * std::f64::consts::PI).ln() - s_clamped.ln() - 0.5 * diff * diff
        };

        let mut v = vec![[0.0, 0.0]; t];
        let mut backpointer = vec![[0usize, 0usize]; t];

        let log_pi = [self.initial_probs[0].max(1e-18).ln(), self.initial_probs[1].max(1e-18).ln()];
        let log_trans = [
            [self.transitions[0][0].max(1e-18).ln(), self.transitions[0][1].max(1e-18).ln()],
            [self.transitions[1][0].max(1e-18).ln(), self.transitions[1][1].max(1e-18).ln()],
        ];

        v[0][0] = log_pi[0] + normal_log_pdf(returns[0], self.state_0.mu_daily, self.state_0.sigma_daily);
        v[0][1] = log_pi[1] + normal_log_pdf(returns[0], self.state_1.mu_daily, self.state_1.sigma_daily);

        for step in 1..t {
            for j in 0..2 {
                let mu = if j == 0 { self.state_0.mu_daily } else { self.state_1.mu_daily };
                let sigma = if j == 0 { self.state_0.sigma_daily } else { self.state_1.sigma_daily };
                let emit = normal_log_pdf(returns[step], mu, sigma);

                let from0 = v[step - 1][0] + log_trans[0][j];
                let from1 = v[step - 1][1] + log_trans[1][j];

                if from0 >= from1 {
                    v[step][j] = from0 + emit;
                    backpointer[step][j] = 0;
                } else {
                    v[step][j] = from1 + emit;
                    backpointer[step][j] = 1;
                }
            }
        }

        let mut path = vec![MarketRegime::LowVolatility; t];
        let mut last_state = if v[t - 1][0] >= v[t - 1][1] { 0 } else { 1 };
        path[t - 1] =
            if last_state == 0 { MarketRegime::LowVolatility } else { MarketRegime::HighVolatility };

        for step in (1..t).rev() {
            last_state = backpointer[step][last_state];
            path[step - 1] =
                if last_state == 0 { MarketRegime::LowVolatility } else { MarketRegime::HighVolatility };
        }

        path
    }

    /// Compute regime-conditional Value-at-Risk for portfolio value $V$.
    /// Returns `(var_regime_0, var_regime_1, current_regime_var)` in currency units.
    pub fn regime_conditional_var(
        &self,
        portfolio_value: f64,
        horizon_years: f64,
        confidence: f64,
    ) -> (f64, f64, f64) {
        let z = normal_quantile(confidence);
        let calc_var = |state: &RegimeState| -> f64 {
            let drift = (state.mu_daily * TRADING_DAYS_PER_YEAR - 0.5 * state.volatility_annualized.powi(2))
                * horizon_years;
            let diffusion = state.volatility_annualized * horizon_years.sqrt();
            let ret = (drift - diffusion * z).exp();
            (portfolio_value * (1.0 - ret)).max(0.0)
        };

        let var_0 = calc_var(&self.state_0);
        let var_1 = calc_var(&self.state_1);
        let current_var = match self.current_regime {
            MarketRegime::LowVolatility => var_0,
            MarketRegime::HighVolatility => var_1,
        };

        (var_0, var_1, current_var)
    }
}

/// Rational approximation of inverse standard normal CDF $\Phi^{-1}(p)$ (Acklam's algorithm).
pub fn normal_quantile(p: f64) -> f64 {
    let p_clamped = p.clamp(1e-15, 1.0 - 1e-15);
    let a = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    let d = [7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00, 3.754408661907416e+00];

    let p_low = 0.02425;
    let p_high = 1.0 - p_low;

    if p_clamped < p_low {
        let q = (-2.0 * p_clamped.ln()).sqrt();
        (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    } else if p_clamped <= p_high {
        let q = p_clamped - 0.5;
        let r = q * q;
        (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p_clamped).ln()).sqrt();
        -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regime_labels_match_hand_labelled_sample_50_days() {
        let mut returns = Vec::with_capacity(50);
        let mut expected = Vec::with_capacity(50);

        for i in 0..25 {
            let r = 0.0005 + 0.004 * ((i as f64 * 1.7).sin());
            returns.push(r);
            expected.push(MarketRegime::LowVolatility);
        }
        for i in 25..50 {
            let r = -0.002 + 0.035 * ((i as f64 * 1.9).sin());
            returns.push(r);
            expected.push(MarketRegime::HighVolatility);
        }

        let model = RegimeModel::fit(&returns).unwrap();
        assert!(model.state_0.sigma_daily < model.state_1.sigma_daily);
        assert!(model.state_1.volatility_annualized > model.state_0.volatility_annualized * 2.0);

        let decoded = model.decode_viterbi(&returns);
        assert_eq!(decoded.len(), 50);

        let matches = decoded.iter().zip(&expected).filter(|(a, b)| a == b).count();
        let accuracy = matches as f64 / 50.0;
        assert!(accuracy >= 0.90, "Regime decoding accuracy was {accuracy}, expected >= 0.90");

        assert_eq!(decoded[0], MarketRegime::LowVolatility);
        assert_eq!(decoded[49], MarketRegime::HighVolatility);
    }

    #[test]
    fn test_regime_conditional_var_higher_in_turbulent_state() {
        let mut returns = Vec::with_capacity(60);
        for i in 0..30 {
            returns.push(0.0008 + 0.005 * ((i as f64 * 0.8).cos()));
        }
        for i in 30..60 {
            returns.push(-0.003 + 0.030 * ((i as f64 * 1.1).cos()));
        }

        let model = RegimeModel::fit(&returns).unwrap();
        let portfolio_value = 1_000_000.0;
        let horizon_years = 1.0 / 252.0;
        let confidence = 0.99;

        let (var_0, var_1, current_var) =
            model.regime_conditional_var(portfolio_value, horizon_years, confidence);

        assert!(
            var_1 > var_0 * 1.5,
            "Crisis VaR ({var_1}) should be significantly higher than Calm VaR ({var_0})"
        );
        assert!(current_var > 0.0);
    }
}
