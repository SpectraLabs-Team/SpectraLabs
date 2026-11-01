# Risk metrics and EVT — reference (M26 task 2)

Code: `crates/spectra-risk/src/metrics.rs` (VaR, CVaR, volatility, downside deviation, skewness,
excess kurtosis), `crates/spectra-risk/src/evt.rs` (peaks over threshold, GPD).
Sign-off: pending (product owner, M26 decision 3). Reference-value tests: task 3, not yet written.
Losses are positive numbers (a loss of 5 is 5, a gain is negative).

## Empirical VaR and CVaR (`metrics.rs`)

**Inputs:** `losses` (non-empty), `confidence` in [0, 1] (e.g. 0.99).

**VaR.** Sort the losses. Take the interpolated quantile at rank = c·(n − 1):
VaR = x_lower + frac·(x_upper − x_lower). Linear interpolation, the type-7 quantile.

**CVaR (expected shortfall).** Mean of the losses ≥ VaR. If no loss is ≥ VaR (only possible
when VaR is above every loss, e.g. c = 1), CVaR returns VaR.

**Edge cases:** empty → error. n = 1 → VaR is the single loss.

## Volatility and tail shape (`metrics.rs`)

- **Volatility:** sample standard deviation (n − 1) of the losses. Empty → error.
- **Downside deviation:** √(mean of (l − mean)² over losses above their mean), with a
  population denominator (count of those losses). Zero when no loss is above the mean. Empty →
  error. (Losses are positive; this is the upper-side deviation.)
- **Skewness:** Σ((l − m)/s)³ / n, with s = sample stdev. Needs ≥ 3 samples. Zero when s = 0.
- **Excess kurtosis:** the fourth-moment analogue minus 3. Needs ≥ 4 samples. Zero when s = 0.

**Note:** these use a different convention for skewness than `spectra-research`'s `moments`
(which divides m3 by m2^1.5 with population moments). The two can differ slightly in small
samples. They are not interchangeable.

## Extreme value theory: peaks over threshold (`evt.rs`)

**Model.** Exceedances y = l − u over a high threshold u follow a generalized Pareto
distribution with shape ξ and scale β > 0. The tail probability is P(l > u + y | l > u) =
(1 + ξy/β)^(−1/ξ) (ξ ≠ 0), or e^(−y/β) (ξ = 0).

**Fit (probability-weighted moments, Hosking and Wallis).** Let u = the threshold quantile,
exceedances y_1 ≤ … ≤ y_Nu, a0 = mean(y), a1 = Σ((Nu − j)/(Nu − 1))·y_j / Nu. Then

```
ξ = 2 − a0 / (a0 − 2·a1)
β = 2·a0·a1 / (a0 − 2·a1)
```

**Goodness of fit.** KS statistic D between the empirical and fitted exceedance CDFs. Passes
if D < 1.36/√Nu (the 5% critical value). The critical value assumes known parameters; the fit
uses estimated parameters, so the test is approximate.

**VaR at confidence p** (N = total losses, Nu = exceedances):

```
VaR_p = u + (β/ξ)·( (N/Nu)·(1 − p) )^(−ξ) − β/ξ          (ξ ≠ 0)
VaR_p = u − β·ln( (N/Nu)·(1 − p) )                         (ξ = 0)
```

**CVaR (expected shortfall) at p** (valid for ξ < 1):

```
CVaR_p = (VaR_p + β − ξ·u) / (1 − ξ)
```

**Inputs and limits:**
- at least 50 loss observations; threshold quantile in [0.70, 0.999); at least 8 exceedances.
- p is clamped to [0, 0.99999] inside `var`.

## Known problems (task 1 and this review; fixed in tasks 7–8)

1. **Silent ξ clamp.** `fit` clamps ξ to [−0.5, 0.9]. A fit that wants ξ outside this range is
   returned as the clamp, with no error and no flag. The reported tail is not the fitted one.
2. **Silent exponential fallback.** When β ≤ 0 the fit sets ξ = 0 and β = a0 (with a 1e-6
   floor), without an error. The model is then an exponential tail, not a fitted GPD.
3. **CVaR returns VaR when ξ ≥ 1.** The expected shortfall is infinite there, but `cvar`
   returns `var_val` silently. A reported EVT CVaR can be far too small with a heavy tail.
4. **KS flag is not enforced.** `passes_goodness_of_fit` is set but the model is still used
   when it fails. Callers must check it (check whether they do).
5. **CVaR of empirical tail with count 0.** `conditional_var` returns VaR when nothing is ≥ VaR.
   Only happens at c = 1, and is then the correct limit; document, do not change.

## Test requirements for task 3

- VaR and CVaR for a fixed loss vector against values computed by hand, with interpolation.
- EVT: a GPD sample with known ξ and β; the fit recovers them within a stated tolerance.
- ξ ≥ 1 in `cvar` returns an error (task 8), not VaR.
- A fit that hits the ξ clamp or the exponential fallback is reported (task 8).
- Loud failures: empty inputs, fewer than 50 observations, fewer than 8 exceedances.

## Sources

- Hosking, J. R. M. and Wallis, J. R. (1987). Parameter and quantile estimation for the generalized
  Pareto distribution. *Technometrics*.
- McNeil, A., Frey, R. and Embrechts, P. (2015). *Quantitative Risk Management*, ch. 5 (POT and ES).
- Hyndman, R. J. and Fan, Y. (1996). Sample quantiles in statistical packages (type 7 quantile).
