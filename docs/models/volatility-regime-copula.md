# GARCH, regime models, copula and Monte Carlo — reference (M26 task 2)

Code: `crates/spectra-risk/src/garch.rs`, `regime.rs`, `copula.rs`, `engine.rs`, `simulation.rs`.
Sign-off: pending (product owner, M26 decision 3). Reference-value tests: task 3, not yet written.

## GARCH(1,1), GJR-GARCH(1,1), EGARCH(1,1) (`garch.rs`)

**Model (GARCH).** r_t = μ + ε_t, h_t = ω + α·ε²_{t−1} + β·h_{t−1}.
Persistence = α + β. Unconditional variance = ω / (1 − α − β).

**Model (GJR-GARCH).** Adds γ·ε²_{t−1}·1{ε_{t−1} < 0}: negative shocks raise variance more.

**Model (EGARCH, standard form; confirm the code matches before task 3).** log h_t = ω + β·log h_{t−1} + α·(|z| − E|z|) + γ·z, with z = ε/√h.

**Fit.** Gaussian quasi-maximum likelihood, by a coordinate search over (α, β) (and γ for GJR
and EGARCH). The search starts from the sample variance; each step shrinks or grows the
parameters and keeps the best negative log-likelihood. Infeasible points get a penalty of 1e12.
Stops when a full pass does not improve.

**Forecast.** One step ahead: h_1 = ω + α·ε² + β·h_last. k steps: the unconditional variance +
persistence^(k−1) · (h_1 − unconditional). Annualized volatility = √(h · periods per year).

**Inputs and limits:** at least 30 returns; non-zero variance (> 1e-12).

**Known problems:**
1. **Search, not a standard optimizer.** The coordinate search stops at the first pass without
   improvement and does not check gradient or Hessian convergence. A reported fit can be a
   local or early-stopped point. Task 3: compare against a reference fit (e.g. from `arch` in
   Python) on a fixed series, within a stated tolerance.
2. **Silent defaults.** `last_variance.unwrap_or(unconditional)` assumes the last variance is the
   unconditional one when the caller passes none. `persistence.powi(...).unwrap_or(1)` uses a
   power of 1 for an absurdly large horizon.
3. **Penalty values.** Infeasible parameter points return 1e12 instead of an error. This is
   harmless for the search but hides infeasibility in any caller that reads the likelihood.

## Regime models (`regime.rs`)

**Two-state Gaussian HMM.** States with means and variances; transitions; initial probabilities.
Fit by Baum–Welch (forward–backward, then re-estimation). Viterbi decoding gives the most likely
state path. Log-space Viterbi with floors at 1e-18.

**Three-state model.** Same structure. **Initialization is ad hoc:** the three states start at
total σ × 0.6, × 1 and × 2 (`regime.rs:334–350`), not from the data. Results can depend on this
starting point; task 3 must test a known simulated regime sequence.

**Regime-conditional VaR.** VaR of the losses drawn from one regime, using that regime's
variance.

**Statistical jump model.** Penalized segmentation with a jump penalty; no distributional
assumption beyond the segment mean.

**Known problems:**
1. **Silent clamps.** Transition probabilities are clamped to [0.001, 0.999] (`regime.rs:157`),
   so a row may not sum to 1 and no flag is raised. Standard deviations are floored at 1e-6.
2. **Floors everywhere.** Probabilities floored at 1e-18 and densities at 1e-300. These protect
   the logs but change results silently when they bind. Count how often they bind in task 3.
3. **Ad hoc initialization** (above).

## Copula dependence and component CVaR (`copula.rs`)

**Gaussian copula.** Correlated normal draws: Z·Lᵀ with L = Cholesky(correlation). Marginals
follow the asset model.

**Student-t dependence.** Draws scaled by √(ν / χ²_ν); `degrees_of_freedom.max(1)` (silent
clamp, `copula.rs:173`).

**Clayton copula** (lower-tail crash dependence). Archimedean with parameter θ; `theta.max(0.1)`
(silent clamp, `copula.rs:185`). Expected to amplify downside CVaR (test exists).

**Component CVaR (risk contributions).** Each asset's share of portfolio CVaR, from the
losses in a band of simulations around the tail. The band half-width is n/100 observations,
clamped to [1, 50] (`copula.rs:249`). The band size is a choice, not derived from the model. Shares sum to 1
(test exists). A band of 1 to 50 observations is coarse for a small simulation; document it.

**Known problems:**
1. **Silent clamps** on ν and θ (above). A user who sets ν = 0 or θ = 0 gets a different model
   with no error.
2. **`n_paths unwrap_or(usize::MAX)`** (`copula.rs:135`): a bad input becomes an unbounded run.

## Monte Carlo risk engine (`engine.rs`, `simulation.rs`)

**Model.** Simulate portfolio losses over N paths. Report VaR and CVaR by the empirical
quantile (see `risk-metrics.md`). Adaptive runs check convergence in batches.

**Convergence.** Relative standard error of the CVaR estimate ≤ `target_rel_std_err`, between
batches. Reports `converged` and the last relative standard error.

**Backends.** Reference (single thread) and parallel; results must agree on a fixed seed (the
RNG is MT19937-64 with a polar normal sampler, `rng.rs`).

**Known problems:**
1. **Non-finite values become null.** `Json::number(v).unwrap_or(Null)` (`engine.rs:284`): NaN
   in a report shows as `null`, not as an error.
2. **`losses` capacity `unwrap_or(0)`** (`engine.rs:186`): harmless here (only a capacity), but
   note the pattern.
3. **Convergence target** `verify_tolerance` default 1e-6: check it matches the stated accuracy.

## Test requirements for task 3

- GARCH: fit on a simulated GARCH series recovers (ω, α, β) within a stated tolerance; forecast
  matches the closed form for k = 1 and k → ∞.
- Regime: a known two-state simulated sequence is decoded with > 90% accuracy; the three-state
  fit does not depend on the initial scale (test two starts).
- Copula: Gaussian copula with correlation ρ reproduces ρ within sampling error; Student-t
  tail dependence matches the closed form.
- Monte Carlo: a single-asset portfolio's VaR matches the analytic normal VaR.
- Loud failures: ν < 1, θ ≤ 0, n_paths overflow, a NaN report field.
