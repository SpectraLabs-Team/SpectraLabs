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

/// Three-state Hidden Markov Model (Low Volatility / Bull, Neutral, High Volatility / Crisis).
#[derive(Debug, Clone, PartialEq)]
pub struct Regime3State {
    /// Low volatility state (Bull / Growth).
    pub state_0: RegimeState,
    /// Moderate volatility state (Neutral / Transition).
    pub state_1: RegimeState,
    /// High volatility state (Crisis / Shock).
    pub state_2: RegimeState,
    /// Transition probability matrix: `transitions[i][j] = P(S_t = j | S_{t-1} = i)`.
    pub transitions: [[f64; 3]; 3],
    /// Initial state probabilities.
    pub initial_probs: [f64; 3],
    /// Filtered probability distribution at end of sample `[P(S_T=0), P(S_T=1), P(S_T=2)]`.
    pub filtered_probs: [f64; 3],
    /// Most likely current regime index (0, 1, or 2).
    pub current_regime: usize,
}

impl Regime3State {
    /// Fit a 3-state Gaussian HMM on historical returns using Baum-Welch EM.
    pub fn fit(returns: &[f64]) -> Result<Self> {
        let t = returns.len();
        if t < 30 {
            return Err(Error::InvalidArgument(format!(
                "3-state HMM requires at least 30 observations, got {t}"
            )));
        }

        let total_mean = mean(returns);
        let total_std = variance(returns).sqrt().max(1e-6);

        // Initial state clusters: low, med, high volatility
        let mut s0 = RegimeState {
            mu_daily: total_mean + 0.0003,
            sigma_daily: (total_std * 0.6).max(1e-6),
            volatility_annualized: (total_std * 0.6 * TRADING_DAYS_PER_YEAR.sqrt()).max(1e-6),
        };
        let mut s1 = RegimeState {
            mu_daily: total_mean,
            sigma_daily: total_std.max(1e-6),
            volatility_annualized: (total_std * TRADING_DAYS_PER_YEAR.sqrt()).max(1e-6),
        };
        let mut s2 = RegimeState {
            mu_daily: total_mean - 0.001,
            sigma_daily: (total_std * 2.0).max(1e-6),
            volatility_annualized: (total_std * 2.0 * TRADING_DAYS_PER_YEAR.sqrt()).max(1e-6),
        };

        let mut a = [[0.85, 0.12, 0.03], [0.10, 0.80, 0.10], [0.05, 0.15, 0.80]];
        let mut pi = [0.6, 0.3, 0.1];

        let gaussian_density = |x: f64, s: &RegimeState| -> f64 {
            let z = (x - s.mu_daily) / s.sigma_daily;
            (1.0 / (s.sigma_daily * (2.0 * std::f64::consts::PI).sqrt())) * (-0.5 * z * z).exp()
        };

        let mut alpha = vec![[0.0; 3]; t];
        let mut beta = vec![[0.0; 3]; t];
        let mut scale = vec![0.0; t];

        for _iter in 0..25 {
            // Forward pass
            let b0 = [
                gaussian_density(returns[0], &s0).max(1e-300),
                gaussian_density(returns[0], &s1).max(1e-300),
                gaussian_density(returns[0], &s2).max(1e-300),
            ];
            let c0 = (pi[0] * b0[0] + pi[1] * b0[1] + pi[2] * b0[2]).max(1e-300);
            scale[0] = c0;
            alpha[0] = [pi[0] * b0[0] / c0, pi[1] * b0[1] / c0, pi[2] * b0[2] / c0];

            for step in 1..t {
                let r = returns[step];
                let b = [
                    gaussian_density(r, &s0).max(1e-300),
                    gaussian_density(r, &s1).max(1e-300),
                    gaussian_density(r, &s2).max(1e-300),
                ];
                let mut sum_c = 0.0;
                for j in 0..3 {
                    let trans_sum = alpha[step - 1][0] * a[0][j]
                        + alpha[step - 1][1] * a[1][j]
                        + alpha[step - 1][2] * a[2][j];
                    alpha[step][j] = trans_sum * b[j];
                    sum_c += alpha[step][j];
                }
                let c = sum_c.max(1e-300);
                scale[step] = c;
                for j in 0..3 {
                    alpha[step][j] /= c;
                }
            }

            // Backward pass
            beta[t - 1] = [1.0, 1.0, 1.0];
            for step in (0..t - 1).rev() {
                let r_next = returns[step + 1];
                let b_next = [
                    gaussian_density(r_next, &s0).max(1e-300),
                    gaussian_density(r_next, &s1).max(1e-300),
                    gaussian_density(r_next, &s2).max(1e-300),
                ];
                let c_next = scale[step + 1];
                for i in 0..3 {
                    beta[step][i] = (a[i][0] * b_next[0] * beta[step + 1][0]
                        + a[i][1] * b_next[1] * beta[step + 1][1]
                        + a[i][2] * b_next[2] * beta[step + 1][2])
                        / c_next;
                }
            }

            // Gammas and Xis
            let mut gamma = vec![[0.0; 3]; t];
            for step in 0..t {
                let denom = (alpha[step][0] * beta[step][0]
                    + alpha[step][1] * beta[step][1]
                    + alpha[step][2] * beta[step][2])
                    .max(1e-300);
                for i in 0..3 {
                    gamma[step][i] = (alpha[step][i] * beta[step][i]) / denom;
                }
            }

            pi = gamma[0];

            for i in 0..3 {
                let mut denom = 0.0;
                let mut num = [0.0; 3];
                for step in 0..t - 1 {
                    let r_next = returns[step + 1];
                    let b_next = [
                        gaussian_density(r_next, &s0).max(1e-300),
                        gaussian_density(r_next, &s1).max(1e-300),
                        gaussian_density(r_next, &s2).max(1e-300),
                    ];
                    let c_next = scale[step + 1];
                    denom += gamma[step][i];
                    for j in 0..3 {
                        num[j] += (alpha[step][i] * a[i][j] * b_next[j] * beta[step + 1][j]) / c_next;
                    }
                }
                if denom > 1e-12 {
                    for j in 0..3 {
                        a[i][j] = (num[j] / denom).clamp(0.01, 0.98);
                    }
                    let row_sum: f64 = a[i].iter().sum();
                    for j in 0..3 {
                        a[i][j] /= row_sum;
                    }
                }
            }

            // Update emission parameters
            for (idx, state) in [(&mut s0, 0), (&mut s1, 1), (&mut s2, 2)] {
                let mut gamma_sum = 0.0;
                let mut mean_sum = 0.0;
                for step in 0..t {
                    let g = gamma[step][state];
                    gamma_sum += g;
                    mean_sum += g * returns[step];
                }
                if gamma_sum > 1e-8 {
                    let new_mu = mean_sum / gamma_sum;
                    let mut var_sum = 0.0;
                    for step in 0..t {
                        let diff = returns[step] - new_mu;
                        var_sum += gamma[step][state] * diff * diff;
                    }
                    let new_sigma = (var_sum / gamma_sum).sqrt().max(1e-6);
                    idx.mu_daily = new_mu;
                    idx.sigma_daily = new_sigma;
                    idx.volatility_annualized = new_sigma * TRADING_DAYS_PER_YEAR.sqrt();
                }
            }
        }

        // Sort states by volatility so state_0 = low, state_1 = med, state_2 = high
        let mut states = [s0, s1, s2];
        states.sort_by(|x, y| x.sigma_daily.partial_cmp(&y.sigma_daily).unwrap());

        let last_probs = alpha[t - 1];
        let current_regime = if last_probs[0] >= last_probs[1] && last_probs[0] >= last_probs[2] {
            0
        } else if last_probs[1] >= last_probs[2] {
            1
        } else {
            2
        };

        Ok(Self {
            state_0: states[0],
            state_1: states[1],
            state_2: states[2],
            transitions: a,
            initial_probs: pi,
            filtered_probs: last_probs,
            current_regime,
        })
    }

    /// Decode the most likely regime path using Viterbi algorithm.
    pub fn decode_viterbi(&self, returns: &[f64]) -> Vec<usize> {
        let t = returns.len();
        if t == 0 {
            return Vec::new();
        }

        let log_trans = [
            [self.transitions[0][0].ln(), self.transitions[0][1].ln(), self.transitions[0][2].ln()],
            [self.transitions[1][0].ln(), self.transitions[1][1].ln(), self.transitions[1][2].ln()],
            [self.transitions[2][0].ln(), self.transitions[2][1].ln(), self.transitions[2][2].ln()],
        ];

        let gaussian_log_density = |x: f64, s: &RegimeState| -> f64 {
            let z = (x - s.mu_daily) / s.sigma_daily;
            -s.sigma_daily.ln() - 0.5 * (2.0 * std::f64::consts::PI).ln() - 0.5 * z * z
        };

        let mut v = vec![[-f64::INFINITY; 3]; t];
        let mut backpointer = vec![[0usize; 3]; t];

        for i in 0..3 {
            let state = if i == 0 {
                &self.state_0
            } else if i == 1 {
                &self.state_1
            } else {
                &self.state_2
            };
            v[0][i] = self.initial_probs[i].max(1e-12).ln() + gaussian_log_density(returns[0], state);
        }

        for step in 1..t {
            let r = returns[step];
            for j in 0..3 {
                let state = if j == 0 {
                    &self.state_0
                } else if j == 1 {
                    &self.state_1
                } else {
                    &self.state_2
                };
                let emit = gaussian_log_density(r, state);
                let mut best_prev = 0;
                let mut best_val = -f64::INFINITY;
                for i in 0..3 {
                    let score = v[step - 1][i] + log_trans[i][j];
                    if score > best_val {
                        best_val = score;
                        best_prev = i;
                    }
                }
                v[step][j] = best_val + emit;
                backpointer[step][j] = best_prev;
            }
        }

        let mut path = vec![0usize; t];
        let mut best_last = 0;
        let mut max_val = v[t - 1][0];
        if v[t - 1][1] > max_val {
            max_val = v[t - 1][1];
            best_last = 1;
        }
        if v[t - 1][2] > max_val {
            best_last = 2;
        }

        path[t - 1] = best_last;
        for step in (1..t).rev() {
            best_last = backpointer[step][best_last];
            path[step - 1] = best_last;
        }

        path
    }

    /// Compute regime-conditional Value-at-Risk across all three states.
    /// Returns `(var_bull, var_neutral, var_crisis, current_var)` in currency units.
    pub fn regime_conditional_var(
        &self,
        portfolio_value: f64,
        horizon_years: f64,
        confidence: f64,
    ) -> (f64, f64, f64, f64) {
        let z = normal_quantile(confidence);
        let calc_var = |state: &RegimeState| -> f64 {
            let drift = (state.mu_daily * TRADING_DAYS_PER_YEAR - 0.5 * state.volatility_annualized.powi(2))
                * horizon_years;
            let diffusion = state.volatility_annualized * horizon_years.sqrt();
            let ret = (drift - diffusion * z).exp();
            (portfolio_value * (1.0 - ret)).max(0.0)
        };

        let v0 = calc_var(&self.state_0);
        let v1 = calc_var(&self.state_1);
        let v2 = calc_var(&self.state_2);
        let current = match self.current_regime {
            0 => v0,
            1 => v1,
            _ => v2,
        };

        (v0, v1, v2, current)
    }
}

/// Statistical Jump Model (Nystrup et al. 2024).
///
/// Discretizes returns into persistent market regimes by adding an explicit jump penalty $\lambda$
/// that prevents high-frequency whipsaws.
#[derive(Debug, Clone, PartialEq)]
pub struct StatisticalJumpModel {
    /// Jump penalty parameter $\lambda \ge 0$.
    pub jump_penalty: f64,
    /// Low-volatility calm state parameters.
    pub state_0: RegimeState,
    /// High-volatility crisis state parameters.
    pub state_1: RegimeState,
    /// Decoded state sequence (0 = Calm, 1 = Crisis).
    pub states: Vec<usize>,
    /// Number of regime transitions in the sample.
    pub jump_count: usize,
    /// Persistence ratio: $1.0 - (\text{jump\_count} / (T - 1))$.
    pub persistence_ratio: f64,
    /// Current regime at the end of the sample.
    pub current_state: usize,
}

impl StatisticalJumpModel {
    /// Fit a Statistical Jump Model with jump penalty $\lambda$.
    pub fn fit(returns: &[f64], jump_penalty: f64) -> Result<Self> {
        let t = returns.len();
        if t < 20 {
            return Err(Error::InvalidArgument(format!(
                "Statistical Jump Model requires at least 20 observations, got {t}"
            )));
        }

        let total_mean = mean(returns);
        let total_std = variance(returns).sqrt().max(1e-6);

        let mut s0 = RegimeState {
            mu_daily: total_mean + 0.0002,
            sigma_daily: (total_std * 0.7).max(1e-6),
            volatility_annualized: (total_std * 0.7 * TRADING_DAYS_PER_YEAR.sqrt()).max(1e-6),
        };
        let mut s1 = RegimeState {
            mu_daily: total_mean - 0.001,
            sigma_daily: (total_std * 1.8).max(1e-6),
            volatility_annualized: (total_std * 1.8 * TRADING_DAYS_PER_YEAR.sqrt()).max(1e-6),
        };

        let mut states = vec![0usize; t];

        // Coordinate descent iterations
        for _iter in 0..10 {
            // Dynamic programming pass with jump penalty
            let mut dp = vec![[f64::INFINITY; 2]; t];
            let mut bp = vec![[0usize; 2]; t];

            let loss = |x: f64, s: &RegimeState| -> f64 {
                let diff = x - s.mu_daily;
                (diff * diff) / (2.0 * s.sigma_daily * s.sigma_daily) + s.sigma_daily.ln()
            };

            dp[0][0] = loss(returns[0], &s0);
            dp[0][1] = loss(returns[0], &s1);

            for step in 1..t {
                let r = returns[step];
                let l0 = loss(r, &s0);
                let l1 = loss(r, &s1);

                for j in 0..2 {
                    let cost_j = if j == 0 { l0 } else { l1 };
                    let stay = dp[step - 1][j] + cost_j;
                    let jump = dp[step - 1][1 - j] + cost_j + jump_penalty;

                    if stay <= jump {
                        dp[step][j] = stay;
                        bp[step][j] = j;
                    } else {
                        dp[step][j] = jump;
                        bp[step][j] = 1 - j;
                    }
                }
            }

            let mut last = if dp[t - 1][0] <= dp[t - 1][1] { 0 } else { 1 };
            states[t - 1] = last;
            for step in (1..t).rev() {
                last = bp[step][last];
                states[step - 1] = last;
            }

            // Update state parameters from assigned clusters
            let mut count0 = 0usize;
            let mut sum0 = 0.0;
            let mut count1 = 0usize;
            let mut sum1 = 0.0;

            for step in 0..t {
                if states[step] == 0 {
                    count0 += 1;
                    sum0 += returns[step];
                } else {
                    count1 += 1;
                    sum1 += returns[step];
                }
            }

            if count0 >= 3 && count1 >= 3 {
                let mu0 = sum0 / count0 as f64;
                let mu1 = sum1 / count1 as f64;

                let var0 = returns
                    .iter()
                    .zip(&states)
                    .filter(|(_, &s)| s == 0)
                    .map(|(&r, _)| (r - mu0).powi(2))
                    .sum::<f64>()
                    / count0 as f64;
                let var1 = returns
                    .iter()
                    .zip(&states)
                    .filter(|(_, &s)| s == 1)
                    .map(|(&r, _)| (r - mu1).powi(2))
                    .sum::<f64>()
                    / count1 as f64;

                s0.mu_daily = mu0;
                s0.sigma_daily = var0.sqrt().max(1e-6);
                s0.volatility_annualized = s0.sigma_daily * TRADING_DAYS_PER_YEAR.sqrt();

                s1.mu_daily = mu1;
                s1.sigma_daily = var1.sqrt().max(1e-6);
                s1.volatility_annualized = s1.sigma_daily * TRADING_DAYS_PER_YEAR.sqrt();
            }
        }

        // Ensure state_0 is lower volatility
        if s0.sigma_daily > s1.sigma_daily {
            std::mem::swap(&mut s0, &mut s1);
            for s in &mut states {
                *s = 1 - *s;
            }
        }

        let mut jumps = 0usize;
        for i in 1..t {
            if states[i] != states[i - 1] {
                jumps += 1;
            }
        }

        let persistence_ratio = if t > 1 { 1.0 - (jumps as f64 / (t - 1) as f64) } else { 1.0 };

        let current_state = states[t - 1];

        Ok(Self {
            jump_penalty,
            state_0: s0,
            state_1: s1,
            states,
            jump_count: jumps,
            persistence_ratio,
            current_state,
        })
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

    #[test]
    fn test_regime_3_state_monotonic_volatility_ordering() {
        let mut returns = Vec::with_capacity(90);
        let pi2 = 2.0 * std::f64::consts::PI;
        // Bull: low vol
        for i in 0..30 {
            returns.push(0.0008 + 0.004 * ((i as f64 * pi2 / 10.0).sin()));
        }
        // Neutral: med vol
        for i in 30..60 {
            returns.push(0.0001 + 0.012 * ((i as f64 * pi2 / 10.0).sin()));
        }
        // Crisis: high vol
        for i in 60..90 {
            returns.push(-0.002 + 0.035 * ((i as f64 * pi2 / 10.0).sin()));
        }

        let model = Regime3State::fit(&returns).unwrap();
        assert!(model.state_0.sigma_daily <= model.state_1.sigma_daily);
        assert!(model.state_1.sigma_daily <= model.state_2.sigma_daily);

        let (v0, _v1, v2, cur) = model.regime_conditional_var(1_000_000.0, 1.0 / 252.0, 0.99);
        assert!(v2 > v0);
        assert!(cur > 0.0);

        let decoded = model.decode_viterbi(&returns);
        assert_eq!(decoded.len(), 90);
    }

    #[test]
    fn test_statistical_jump_model_penalizes_whipsaws() {
        let mut returns = Vec::with_capacity(60);
        for i in 0..30 {
            returns.push(0.0005 + 0.005 * ((i as f64 * 0.6).sin()));
        }
        for i in 30..60 {
            returns.push(-0.002 + 0.030 * ((i as f64 * 0.6).sin()));
        }

        let sjm_low_penalty = StatisticalJumpModel::fit(&returns, 0.1).unwrap();
        let sjm_high_penalty = StatisticalJumpModel::fit(&returns, 10.0).unwrap();

        assert!(sjm_high_penalty.jump_count <= sjm_low_penalty.jump_count);
        assert!(sjm_high_penalty.persistence_ratio >= sjm_low_penalty.persistence_ratio);
        assert_eq!(sjm_high_penalty.states.len(), 60);
    }
}
