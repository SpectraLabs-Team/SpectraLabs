# Statistics reference — Sharpe family (M26 task 2)

Code: `crates/spectra-research/src/stats.rs` (all functions below), `sweep.rs`
(`TrialLedger::sharpe_variance`), `validation.rs` (`validate_selection`).
Callers: `spectra-api/src/optimization_api.rs`, `spectra-api/src/bin/spectra-worker.rs`,
`spectra-registry/src/vault.rs`. Source formulas: Bailey and López de Prado, "The Sharpe Ratio
Efficient Frontier" (2012), "The Deflated Sharpe Ratio" (2014).
Sign-off: pending (product owner, M26 decision 3). Reference-value tests: task 3, not yet written.

## Conventions (read first)

- **Per-period, not annualized.** `sharpe_per_period` = mean / sample stdev of the returns, in the
  return frequency. PSR, DSR and MinTRL all take this per-period value. Multiply by √periods
  only for display.
- **Raw kurtosis, not excess.** `moments().raw_kurtosis` = m4 / m2² (normal = 3). PSR's variance term
  uses (raw_kurt − 1)/4, which is correct for raw kurtosis. `performance::excess_kurtosis` is excess
  (normal = 0); never pass it where PSR expects raw.
- **Population skew and kurtosis; sample stdev.** `moments` divides m2, m3, m4 by n, and the
  stdev by n − 1. Mixing these changes results only in small samples; keep it as is.
- **Field naming (M26 task 4):** Field `raw_kurtosis` in `Moments` and `ValidationReport` is raw kurtosis (non-excess, normal = 3); `excess_kurtosis` in performance (`spectra-engine/performance.rs`) and metrics (`spectra-risk/metrics.rs`) is excess (normal = 0). Old name `kurt`. No value changed.

## Inputs

| Name | Meaning | Unit |
|---|---|---|
| `returns` | Periodic simple returns of one strategy | fraction per period |
| `sr` | Observed per-period Sharpe | per period |
| `sr_benchmark` | Benchmark Sharpe to beat (0 in most calls) | per period |
| `n_obs` | Number of returns | count |
| `skew`, `kurt` | Sample skewness; raw kurtosis (normal = 3) | — |
| `n_trials` | Number of strategies or parameter sets tried | count |
| `sr_variance` | Variance of per-period Sharpe across the trials | per period squared |
| `confidence` | Target probability, e.g. 0.95 | 0 to 1 |

## Formulas

**Sharpe (per period).** SR = mean(r) / stdev(r), stdev with n − 1.

**Probabilistic Sharpe ratio.**

```
PSR(SR*) = Φ( (SR − SR*) · √(n − 1) / √(1 − γ3·SR + (γ4 − 1)/4 · SR²) )
```

γ3 = skew, γ4 = raw kurtosis, Φ = normal CDF, SR* = benchmark.

**Expected maximum Sharpe among N trials** (zero true Sharpe, variance V = `sr_variance`):

```
E[max SR] = √V · ( (1 − γ)·Φ⁻¹(1 − 1/N) + γ·Φ⁻¹(1 − 1/(N·e)) )
```

γ = 0.5772… (Euler–Mascheroni), e = 2.71828…

**Deflated Sharpe ratio.** DSR = PSR(SR*) with SR* = E[max SR]. Uses the selected strategy's own
SR, skew, kurtosis and n.

**Minimum track record length** (observations needed for PSR to reach confidence c):

```
MinTRL = 1 + (1 − γ3·SR + (γ4 − 1)/4 · SR²) · (Φ⁻¹(c) / (SR − SR*))²
```

**Trial Sharpe variance** (`TrialLedger::sharpe_variance`): sample variance (n − 1) of the
per-period Sharpe of each trial. Zero with fewer than 2 trials.

**Normal CDF and inverse.** Φ via erfc. Φ⁻¹ by Acklam's rational approximation plus one Halley
step. Φ⁻¹(0) = −∞, Φ⁻¹(1) = +∞, NaN outside [0, 1].

## Edge cases (as coded)

| Function | Case | Returns |
|---|---|---|
| `sharpe_per_period` | stdev = 0 or n < 2 | 0 |
| `moments` | n < 2 | stdev 0, skew 0, kurt 3 |
| `probabilistic_sharpe` | n_obs < 2 | NaN |
| `probabilistic_sharpe` | variance term ≤ 0 | NaN |
| `expected_max_sharpe` | n_trials ≤ 1, or V ≤ 0 | 0 |
| `deflated_sharpe` | as PSR | as PSR (NaN propagates) |
| `min_track_record_length` | SR ≤ SR* | +∞ |
| `min_track_record_length` | variance term ≤ 0 | NaN |
| `TrialLedger::sharpe_variance` | fewer than 2 trials | 0 |

## Known problems (found in task 1; fixed in task 8)

1. **Hardcoded variance.** `optimization_api.rs:1387` calls `deflated_sharpe(&returns, n, 0.05)`:
   the trial variance is the constant 0.05 whatever the trials were. The DSR is wrong for every
   sweep that uses it. Replace with the ledger's `sharpe_variance`.
2. **NaN becomes 0.0.** The same call maps a NaN DSR to 0.0 (`if dsr.is_nan() { 0.0 }`). A
   0.0 DSR reads as "no evidence", not "undefined". Should be an explicit "not defined".
3. **Trial count as variance proxy.** `vault.rs:358` uses the evaluation count as N. Check that
   it is the number of trials, not evaluations of one trial.
4. **Benchmark defaults to 0.** `optimization_api.rs:1144` uses `unwrap_or(0.0)` for the
   benchmark Sharpe when the request omits it. Document it, or require it.
5. **Degenerate inputs.** Zero-variance returns, n = 2 to 5, and very high kurtosis can push the
   variance term ≤ 0 (PSR NaN). Tests for each are part of task 3.

## Test requirements for task 3 (not yet written)

- PSR, DSR and MinTRL against values computed at high precision (mpmath) for fixed return
  series, with skew and kurtosis from those series.
- E[max SR] against the closed form for N = 2, 10, 100 with a known V.
- Φ⁻¹ against published values: Φ⁻¹(0.95) = 1.6448536269514729, Φ⁻¹(0.975) = 1.959963984540054.
- Loud-failure tests: DSR with V = 0 and N = 1; MinTRL with SR ≤ SR*.

## Sources

- Bailey, D. and López de Prado, M. (2012). The Sharpe Ratio Efficient Frontier. *Journal of Risk*.
- Bailey, D. and López de Prado, M. (2014). The Deflated Sharpe Ratio. *Journal of Portfolio Management*.
- Acklam, P. J. Inverse normal CDF approximation (rational approximation with refinement).
