#!/usr/bin/env python3
"""Reference implementation of the Baum‑Welch EM for the 2‑state and 3‑state HMM
used by `RegimeModel::fit` and `Regime3State::fit` in the Rust code.
It follows the exact same initialization, iteration count, clamps and density
formulae.
"""
import numpy as np

TRADING_DAYS_PER_YEAR = 252.0

def normal_pdf(x, m, s):
    s_clamped = max(s, 1e-6)
    diff = (x - m) / s_clamped
    exponent = -0.5 * diff * diff
    return (1.0 / (s_clamped * np.sqrt(2.0 * np.pi))) * np.exp(exponent)

def fit_2state(returns):
    t = len(returns)
    if t < 30:
        raise ValueError(f"requires at least 30 observations, got {t}")
    overall_mean = np.mean(returns)
    overall_std = np.sqrt(np.var(returns))
    mu = np.array([overall_mean + 0.1 * overall_std,
                   overall_mean - 0.2 * overall_std])
    sigma = np.array([0.7 * overall_std,
                      1.5 * overall_std])
    trans = np.array([[0.95, 0.05],
                      [0.10, 0.90]])
    pi = np.array([0.7, 0.3])
    for _ in range(50):
        # forward
        alpha = np.zeros((t, 2))
        c = np.zeros(t)
        b0 = normal_pdf(returns[0], mu[0], sigma[0])
        b1 = normal_pdf(returns[0], mu[1], sigma[1])
        alpha[0,0] = pi[0] * b0
        alpha[0,1] = pi[1] * b1
        c[0] = max(alpha[0,0] + alpha[0,1], 1e-18)
        alpha[0] /= c[0]
        for step in range(1, t):
            b0 = normal_pdf(returns[step], mu[0], sigma[0])
            b1 = normal_pdf(returns[step], mu[1], sigma[1])
            alpha[step,0] = (alpha[step-1,0] * trans[0,0] + alpha[step-1,1] * trans[1,0]) * b0
            alpha[step,1] = (alpha[step-1,0] * trans[0,1] + alpha[step-1,1] * trans[1,1]) * b1
            c[step] = max(alpha[step,0] + alpha[step,1], 1e-18)
            alpha[step] /= c[step]
        # backward
        beta = np.ones((t, 2))
        for step in range(t-2, -1, -1):
            b0 = normal_pdf(returns[step+1], mu[0], sigma[0])
            b1 = normal_pdf(returns[step+1], mu[1], sigma[1])
            beta[step,0] = (trans[0,0] * b0 * beta[step+1,0] + trans[0,1] * b1 * beta[step+1,1]) / c[step+1]
            beta[step,1] = (trans[1,0] * b0 * beta[step+1,0] + trans[1,1] * b1 * beta[step+1,1]) / c[step+1]
        # gamma
        gamma = np.zeros((t,2))
        for step in range(t):
            denom = max(alpha[step,0]*beta[step,0] + alpha[step,1]*beta[step,1], 1e-18)
            gamma[step,0] = (alpha[step,0]*beta[step,0]) / denom
            gamma[step,1] = (alpha[step,1]*beta[step,1]) / denom
        pi = gamma[0]
        # update transitions
        trans_num = np.zeros((2,2))
        trans_den = np.zeros(2)
        for step in range(t-1):
            b0 = normal_pdf(returns[step+1], mu[0], sigma[0])
            b1 = normal_pdf(returns[step+1], mu[1], sigma[1])
            denom = max(alpha[step,0]*beta[step,0] + alpha[step,1]*beta[step,1], 1e-18)
            xi_00 = (alpha[step,0] * trans[0,0] * b0 * beta[step+1,0] / c[step+1]) / denom
            xi_01 = (alpha[step,0] * trans[0,1] * b1 * beta[step+1,1] / c[step+1]) / denom
            xi_10 = (alpha[step,1] * trans[1,0] * b0 * beta[step+1,0] / c[step+1]) / denom
            xi_11 = (alpha[step,1] * trans[1,1] * b1 * beta[step+1,1] / c[step+1]) / denom
            trans_num[0,0] += xi_00
            trans_num[0,1] += xi_01
            trans_num[1,0] += xi_10
            trans_num[1,1] += xi_11
            trans_den[0] += gamma[step,0]
            trans_den[1] += gamma[step,1]
        for i in range(2):
            d = max(trans_den[i], 1e-12)
            trans[i,0] = np.clip(trans_num[i,0] / d, 0.001, 0.999)
            trans[i,1] = np.clip(trans_num[i,1] / d, 0.001, 0.999)
            row_sum = trans[i,0] + trans[i,1]
            trans[i] /= row_sum
        # update means & variances
        for k in range(2):
            g_sum = max(gamma[:,k].sum(), 1e-12)
            new_mu = (gamma[:,k] * returns).sum() / g_sum
            diff = returns - new_mu
            new_var = (gamma[:,k] * diff * diff).sum() / g_sum
            mu[k] = new_mu
            sigma[k] = max(np.sqrt(new_var), 1e-6)
    # reorder so sigma[0] <= sigma[1]
    if sigma[0] <= sigma[1]:
        s0_idx, s1_idx = 0,1
    else:
        s0_idx, s1_idx = 1,0
    state_0 = (mu[s0_idx], sigma[s0_idx])
    state_1 = (mu[s1_idx], sigma[s1_idx])
    transitions = np.array([[trans[s0_idx,s0_idx], trans[s0_idx,s1_idx]],
                            [trans[s1_idx,s0_idx], trans[s1_idx,s1_idx]]])
    init_probs = np.array([pi[s0_idx], pi[s1_idx]])
    return {
        "state_0": state_0,
        "state_1": state_1,
        "transitions": transitions,
        "initial_probs": init_probs,
    }

def fit_3state(returns):
    t = len(returns)
    if t < 30:
        raise ValueError(f"requires at least 30 observations, got {t}")
    total_mean = np.mean(returns)
    total_std = max(np.sqrt(np.var(returns)), 1e-6)
    s0 = {
        "mu_daily": total_mean + 0.0003,
        "sigma_daily": max(total_std * 0.6, 1e-6),
    }
    s1 = {
        "mu_daily": total_mean,
        "sigma_daily": max(total_std, 1e-6),
    }
    s2 = {
        "mu_daily": total_mean - 0.001,
        "sigma_daily": max(total_std * 2.0, 1e-6),
    }
    a = np.array([[0.85, 0.12, 0.03],
                  [0.10, 0.80, 0.10],
                  [0.05, 0.15, 0.80]])
    pi = np.array([0.6, 0.3, 0.1])
    def gaussian_density(x, s):
        z = (x - s["mu_daily"]) / s["sigma_daily"]
        return (1.0 / (s["sigma_daily"] * np.sqrt(2.0 * np.pi))) * np.exp(-0.5 * z * z)
    alpha = np.zeros((t,3))
    beta = np.zeros((t,3))
    scale = np.zeros(t)
    for _ in range(25):
        # forward
        b = np.array([
            max(gaussian_density(returns[0], s0), 1e-300),
            max(gaussian_density(returns[0], s1), 1e-300),
            max(gaussian_density(returns[0], s2), 1e-300),
        ])
        c0 = max(pi[0]*b[0] + pi[1]*b[1] + pi[2]*b[2], 1e-300)
        scale[0] = c0
        alpha[0] = np.array([pi[0]*b[0]/c0, pi[1]*b[1]/c0, pi[2]*b[2]/c0])
        for step in range(1, t):
            r = returns[step]
            b = np.array([
                max(gaussian_density(r, s0), 1e-300),
                max(gaussian_density(r, s1), 1e-300),
                max(gaussian_density(r, s2), 1e-300),
            ])
            alpha[step] = (alpha[step-1] @ a) * b
            c = max(alpha[step].sum(), 1e-300)
            scale[step] = c
            alpha[step] /= c
        # backward
        beta[t-1] = np.ones(3)
        for step in range(t-2, -1, -1):
            b = np.array([
                max(gaussian_density(returns[step+1], s0), 1e-300),
                max(gaussian_density(returns[step+1], s1), 1e-300),
                max(gaussian_density(returns[step+1], s2), 1e-300),
            ])
            beta[step] = (a @ (b * beta[step+1])) / scale[step+1]
        # gamma (state responsibilities)
        gamma = (alpha * beta)
        gamma /= gamma.sum(axis=1, keepdims=True)
        pi = gamma[0]
        # update transition matrix
        xi = np.zeros((t-1, 3, 3))
        for step in range(t-1):
            b_next = np.array([
                max(gaussian_density(returns[step+1], s0), 1e-300),
                max(gaussian_density(returns[step+1], s1), 1e-300),
                max(gaussian_density(returns[step+1], s2), 1e-300),
            ])
            denom = max((alpha[step][:,None] * a * b_next * beta[step+1]).sum(), 1e-18)
            xi[step] = (alpha[step][:,None] * a * b_next * beta[step+1]) / denom
        a = np.clip(xi.sum(axis=0), 0.01, 0.98)
        a /= a.sum(axis=1, keepdims=True)
        # update means & variances for each state
        for s, state in zip([s0, s1, s2], range(3)):
            g = gamma[:,state]
            g_sum = max(g.sum(), 1e-12)
            new_mu = (g * returns).sum() / g_sum
            diff = returns - new_mu
            new_var = (g * diff * diff).sum() / g_sum
            s["mu_daily"] = new_mu
            s["sigma_daily"] = max(np.sqrt(new_var), 1e-6)
    # assemble result
    states = [(s0["mu_daily"], s0["sigma_daily"]),
              (s1["mu_daily"], s1["sigma_daily"]),
              (s2["mu_daily"], s2["sigma_daily"])]
    return {
        "states": states,
        "transitions": a,
        "initial_probs": pi,
    }

if __name__ == "__main__":
    # deterministic synthetic series: a sinusoid with small trend
    t = 100
    x = np.arange(t)
    returns = 0.001 * np.sin(0.2 * x) + 0.0001 * x / t
    res2 = fit_2state(returns)
    res3 = fit_3state(returns)
    np.set_printoptions(precision=12, suppress=True)
    print("--- 2‑state fit ---")
    print("state_0 mean, sigma:", res2["state_0"])
    print("state_1 mean, sigma:", res2["state_1"])
    print("transitions:\n", res2["transitions"])
    print("initial_probs:", res2["initial_probs"])
    print("--- 3‑state fit ---")
    for i, (mu, sig) in enumerate(res3["states"]):
        print(f"state_{i} mean, sigma:", (mu, sig))
    print("transitions:\n", res3["transitions"])
    print("initial_probs:", res3["initial_probs"])
