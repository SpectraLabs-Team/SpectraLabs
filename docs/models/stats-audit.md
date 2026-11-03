# M26 Task 8: Statistics Audit (`crates/spectra-research/src/stats.rs`)

Audit of all statistics in `crates/spectra-research/src/stats.rs` under the M26 Model Correctness Audit silent-default rule:
> *A missing input or undefined statistic must return an explicit error. Zero is allowed only where the formula defines it.*

---

## 1. Inventory Table

| Function / Statistic | Formula / Definition | Divisor | Empty Input ($n=0$) | Single Input ($n=1$) | Zero Variance ($s=0$ or $s \le 10^{-12} \cdot \text{scale}$) | Status / Flag |
|---|---|---|---|---|---|---|
| `mean` (`moments`) | $\frac{1}{n} \sum x_i$ | $n$ | `0.0` (line 255) | `0.0` (lines 255, 264) | $\bar{x}$ (exact constant value) | ⚠️ **Silent default**: returns `0.0` on empty and single input (for $n=1$, returns before computing mean) |
| `stdev` (`moments`) | $\sqrt{\frac{1}{n-1} \sum (x_i - \bar{x})^2}$ | $n - 1$ (sample) | `0.0` (line 255) | `0.0` (line 255) | `0.0` (line 285) | ⚠️ **Silent default**: undefined standard deviation reported as `0.0` |
| `skew` (`moments`) | $m_3 / m_2^{1.5}$ | $n$ (population) | `0.0` (line 255) | `0.0` (line 255) | `0.0` (lines 255, 285) | ⚠️ **Silent default**: undefined skewness reported as `0.0` (falsely implying symmetry) |
| `raw_kurtosis` (`moments`) | $m_4 / m_2^2$ | $n$ (population) | `3.0` (line 255) | `3.0` (line 255) | `3.0` (lines 255, 285) | ⚠️ **Silent default**: undefined kurtosis reported as `3.0` (falsely implying normal distribution) |
| `sharpe_per_period` | $\bar{r} / s_r$ | $n - 1$ (sample stdev) | `0.0` (line 304) | `0.0` (line 304) | `0.0` (line 304) | ⚠️ **Silent default**: undefined Sharpe ratio returns `0.0` with no error or `None` |
| `probabilistic_sharpe` (PSR) | $\Phi\left( \frac{(SR - SR^*) \sqrt{n-1}}{\sqrt{1 - \gamma_3 SR + \frac{\gamma_4 - 1}{4} SR^2}} \right)$ | $n - 1$ (numerator) | `NaN` (line 313) | `NaN` (line 313) | Evaluates with sample moments | ⚠️ **NaN return**: returns raw `f64::NAN` on $n < 2$ or non-positive variance term instead of `Result`/`Option` |
| `expected_max_sharpe` | $\sqrt{V} ((1-\gamma)\Phi^{-1}(1-\frac{1}{N}) + \gamma\Phi^{-1}(1-\frac{1}{Ne}))$ | $N$ (trials) | `0.0` (line 327) | `0.0` (line 327) | `0.0` when $V \le 0$ (line 327) | ⚠️ **Silent default**: returns `0.0` for $N \le 1$ or $V \le 0$ instead of an error |
| `deflated_sharpe` (DSR) | $\text{PSR}(SR, E[\max SR], n, \gamma_3, \gamma_4)$ | $n - 1$ | `NaN` (via PSR) | `NaN` (via PSR) | Evaluates at $SR = 0.0$ (line 338) | ⚠️ **Silent default**: line 338 defaults $SR$ to `0.0` when stdev is zero, computing a pseudo-DSR rather than returning undefined |
| `min_track_record_length` (MinTRL) | $1 + \text{var\_term} \left( \frac{\Phi^{-1}(\alpha)}{SR - SR^*} \right)^2$ | $SR - SR^*$ | `NaN` (if $SR$ NaN) | `NaN` (if $SR$ NaN) | Evaluates or returns `+inf` / `NaN` | ℹ️ Returns `f64::INFINITY` when $SR \le SR^*$ (mathematical boundary); returns `NaN` when variance term $\le 0$ |
| `normal_inv_cdf` | Acklam approximation + Halley step | N/A | `NaN` outside $[0, 1]$ | N/A | N/A | ℹ️ Returns `f64::NEG_INFINITY` at 0.0, `f64::INFINITY` at 1.0, and `NaN` outside $[0, 1]$ |

---

## 2. Flagged Silent Defaults & Line References

### 1. `Moments::default()` (line 255)
```rust
impl Default for Moments {
    fn default() -> Self {
        Self { n: 0, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }
    }
}
```
- **Issue**: For an empty sample, `stdev = 0.0`, `skew = 0.0`, and `raw_kurtosis = 3.0` are placeholders that mimic a standard normal distribution. Under M26, an empty return series has no defined moments.

### 2. `moments()` early return on $n < 2$ (lines 263–265)
```rust
    if x.len() < 2 {
        return m;
    }
```
- **Issue**: When `x.len() == 1`, this returns `Moments::default()`, meaning `m.n = 1` but `m.mean = 0.0` (never calculated!), `m.stdev = 0.0`, `m.skew = 0.0`, and `m.raw_kurtosis = 3.0`. For a single element `x = [5.0]`, the reported mean is `0.0` instead of `5.0`.

### 3. `moments()` constant-sample fallback (lines 284–286)
```rust
    if sample_sd <= 1e-12 * scale {
        return m;
    }
```
- **Issue**: If the sample is constant up to float residue ($s \le 10^{-12} \cdot \text{scale}$), `m` is returned with `stdev = 0.0`, `skew = 0.0`, and `raw_kurtosis = 3.0`. Skewness and kurtosis are mathematically undefined when variance is zero.

### 4. `sharpe_per_period()` silent zero fallback (lines 301–305)
```rust
    if m.stdev > 0.0 {
        m.mean / m.stdev
    } else {
        0.0
    }
```
- **Issue**: When $n < 2$ or stdev is zero, Sharpe returns `0.0`. A Sharpe ratio of 0.0 represents zero excess return with positive volatility, whereas an undefined Sharpe has zero volatility. Callers cannot distinguish between a strategy that earned 0 return and one with no data or zero risk.

### 5. `expected_max_sharpe()` silent zero fallback (lines 326–328)
```rust
    if n_trials <= 1 || !(sr_variance > 0.0) {
        return 0.0;
    }
```
- **Issue**: With $N \le 1$ trials or non-positive variance across trials, returns `0.0`. For $N = 1$, expected maximum Sharpe among trials is undefined (no multiple-testing selection bias exists to correct for).

### 6. `deflated_sharpe()` silent zero Sharpe fallback (lines 338–339)
```rust
    let sr = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
```
- **Issue**: When returns have zero variance, `deflated_sharpe` substitutes $SR = 0.0$ and evaluates `probabilistic_sharpe(0.0, ...)`, returning a numerical probability instead of indicating that DSR is undefined.

---

## 3. Fix Plan

### Fix 1: `Moments::default()` and `moments()` (lines 253–296)

1. **Current behavior:**
   - Lines 253–257: `Moments::default()` sets `mean = 0.0`, `stdev = 0.0`, `skew = 0.0`, `raw_kurtosis = 3.0`.
   - Lines 263–265: If `x.len() < 2`, returns `m` (from `Default`) without computing mean:
     - Empty ($n=0$): returns `Moments { n: 0, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }`.
     - Single ($n=1$): returns `Moments { n: 1, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` (drops actual value).
   - Lines 284–286: If `sample_sd <= 1e-12 * scale` (zero variance), returns `Moments { n, mean: \bar{x}, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }`.
2. **Proposed behavior:**
   - Change signature to: `pub fn moments(x: &[f64]) -> Result<Moments>`
   - Return `Err(Error::InvalidArgument("moments require at least 2 observations"))` when `x.len() < 2`.
   - Return `Err(Error::InvalidArgument("sample has zero variance; higher moments undefined"))` when `sample_sd <= 1e-12 * scale`.
   - Remove `impl Default for Moments` (or restrict it to test fixtures with `#[cfg(test)]`) to prevent uninitialized or placeholder instances from circulating.
   - **Justification**: A sample of 0 or 1 point has no defined sample variance ($n - 1 = 0$), and a zero-variance series has undefined skewness ($0/0$) and kurtosis ($0/0$). Silently defaulting these to a standard normal distribution ($\text{skew}=0, \text{kurtosis}=3$) violates the silent-default rule.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1146`: `let m = moments(best_returns)?;` — signature change breaks it; caller should propagate error or return 400.
   - `crates/spectra-api/src/optimization_api.rs:1148`: `let sr_moments = moments(&sharpes)?;` — breaks; caller should propagate error.
   - `crates/spectra-api/src/bin/spectra-worker.rs:1707, 1751, 1752`: `moments(&...)` — breaks; worker should record trial evaluation error instead of silent run.
   - `crates/spectra-research/src/survival.rs:1769, 1832`: `moments(values)` — breaks; regime bucket should check `mo.is_ok()` and report insufficient data if error.
   - `crates/spectra-research/src/validation.rs:56`: `moments(&selected.returns)` — breaks; `validate_selection` signature should return `Result<ValidationReport>`.
   - `crates/spectra-research/src/stats.rs:300, 337`: internal calls in `sharpe_per_period` and `deflated_sharpe` — propagate with `?`.
   - `crates/spectra-registry/src/vault.rs:343`: `moments(&returns)` — breaks; vault holdout evaluation should fail with `Error::Data` / `Error::InvalidArgument`.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`: lines 46, 55, 72, 89, 105, 120 (`let m = moments(&RETURNS).unwrap();`).
   - `crates/spectra-research/src/stats.rs`: line 381 (`moments(&[1.0, 2.0, 3.0, 4.0]).unwrap()`), line 384 (`assert!(moments(&[1.0]).is_err())`).
   - `parity/golden/research.golden`: No lines changed. In `parity_research.rs:175, 214`, `validate_selection` operates on sweep trials with 1000, 600, or 160 returns (all $n \ge 2$, non-zero variance), so `moments()` succeeds with `Ok`.
5. **Output-changing assessment & divergence:**
   - On valid inputs (e.g. `[-4, -2, 2, 4]`), values are identical:
     - Mean: `0.0` (unchanged)
     - Stdev: `3.6514837167011076` (unchanged)
     - Skewness: `0.0` (unchanged)
     - Raw kurtosis: `1.360` (unchanged)
   - On degenerate inputs ($n < 2$ or zero variance), the output changes from a placeholder struct to an explicit error.

---

### Fix 2: `sharpe_per_period()` (lines 299–306)

1. **Current behavior:**
   - Lines 299–306:
     ```rust
     pub fn sharpe_per_period(returns: &[f64]) -> f64 {
         let m = moments(returns);
         if m.stdev > 0.0 {
             m.mean / m.stdev
         } else {
             0.0
         }
     }
     ```
   - Empty input ($n=0$): returns `0.0`.
   - Single input ($n=1$): returns `0.0`.
   - Zero-variance input ($s_r = 0.0$): returns `0.0`.
2. **Proposed behavior:**
   - Change signature to: `pub fn sharpe_per_period(returns: &[f64]) -> Result<f64>`
   - If `returns.len() < 2`: returns `Err(Error::InvalidArgument("sharpe_per_period requires at least 2 return observations"))`.
   - If `stdev <= 0.0`: returns `Err(Error::InvalidArgument("sharpe_per_period undefined: return variance is zero"))`.
   - **Justification**: A Sharpe ratio of 0.0 represents zero excess return under non-zero risk. An undefined Sharpe (no data or zero volatility) is not 0.0; conflating the two allows unviable or degenerate strategies to masquerade as break-even.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1137`:
     `req.returns_matrix.iter().map(|r| sharpe_per_period(r)).collect::<Result<Vec<f64>>>()?` — breaks; handle with `?`.
   - `crates/spectra-research/src/sweep.rs:331`:
     `sharpe_per_period: sharpe_per_period(&t.returns)?` — breaks; propagate with `?`.
   - `crates/spectra-research/src/trades.rs:342`:
     `sharpe_per_period(&sampled)?` — breaks; handle with `?`.
   - `crates/spectra-registry/src/registry.rs:1009`:
     `finite_metric(sharpe_per_period(&input.returns)?, "sharpe_per_period")?` — breaks; update with `?`.
   - `crates/spectra-registry/src/pg.rs:476`:
     `let sharpe_period = sharpe_per_period(&input.returns)?;` — breaks; propagate.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`:
     - Lines 67, 73, 90, 106, 121: `sharpe_per_period(&RETURNS).unwrap()`
     - Line 113: `assert!(sharpe_per_period(&[0.01; 10]).is_err())` (was `assert_eq!(..., 0.0)`).
   - `crates/spectra-research/tests/m26_bootstrap_reference.rs`: line 42 (`sharpe_per_period(&R).unwrap()`).
   - `parity/golden/research.golden`: No lines changed. Parity sweeps have full return series ($n \ge 60$) with non-zero volatility.
5. **Output-changing assessment & divergence:**
   - Hand-computed sample `[-4, -2, 2, 4]`:
     - Mean: `0.0`, Stdev: `3.6514837167011076`
     - Old: `0.0`
     - New: `Ok(0.0)` (exact match).
   - On zero-volatility sample `[0.01; 10]`:
     - Old: `0.0`
     - New: `Err(Error::InvalidArgument)` (divergence from silent default).

---

### Fix 3: `expected_max_sharpe()` (lines 325–333)

1. **Current behavior:**
   - Lines 325–333:
     ```rust
     pub fn expected_max_sharpe(sr_variance: f64, n_trials: usize) -> f64 {
         if n_trials <= 1 || !(sr_variance > 0.0) {
             return 0.0;
         }
         ...
     }
     ```
   - Zero trials ($N=0$): returns `0.0`.
   - Single trial ($N=1$): returns `0.0`.
   - Non-positive or zero variance ($V \le 0.0$): returns `0.0`.
2. **Proposed behavior:**
   - Change signature to: `pub fn expected_max_sharpe(sr_variance: f64, n_trials: usize) -> Result<f64>`
   - If `n_trials == 0`: returns `Err(Error::InvalidArgument("n_trials must be at least 1"))`.
   - If `!sr_variance.is_finite() || sr_variance < 0.0`: returns `Err(Error::InvalidArgument("sr_variance must be non-negative and finite"))`.
   - If `n_trials == 1` or `sr_variance == 0.0`: returns `Ok(0.0)`.
   - **Justification**:
     - $N=0$ and negative or non-finite variance are invalid arguments that must error loudly.
     - $N=1$ (single trial) and $V=0$ (zero spread between trials) mathematically have zero selection bias: $E[\max SR] = 0.0$. Here `0.0` is the exact mathematical limit defined by the formula, not an ungrounded fallback.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1155`: `let exp_max_sr = expected_max_sharpe(sr_var, n_trials)?;` — breaks; propagate with `?`.
   - `crates/spectra-research/src/validation.rs:60`: `let ems = expected_max_sharpe(sr_var, n_trials)?;` — breaks; propagate with `?`.
   - `crates/spectra-registry/src/vault.rs:358`: `let ems = expected_max_sharpe(sr_variance, evaluation_no)?;` — breaks; propagate with `?`.
   - `crates/spectra-cli/tests/learn_formula_drift.rs:397`: `expected_max_sharpe(...).unwrap()` — breaks; update with `.unwrap()`.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`:
     - Lines 79, 110, 111, 131, 132: update to `.unwrap()`.
   - `crates/spectra-research/src/stats.rs`: lines 378, 379: update to `.unwrap()`.
   - `parity/golden/research.golden`: No lines changed ($V > 0$ and $N > 1$ in all golden runs).
5. **Output-changing assessment & divergence:**
   - On valid inputs: identical output.
   - On $N=0$ or negative variance: returns `Err` instead of `0.0`.

---

### Fix 4: `deflated_sharpe()` (lines 336–340)

1. **Current behavior:**
   - Lines 336–340:
     ```rust
     pub fn deflated_sharpe(returns: &[f64], n_trials: usize, sr_variance: f64) -> f64 {
         let m = moments(returns);
         let sr = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
         probabilistic_sharpe(sr, expected_max_sharpe(sr_variance, n_trials), m.n, m.skew, m.raw_kurtosis)
     }
     ```
   - Empty input ($n=0$): returns `NaN` (via PSR on $n=0$).
   - Single input ($n=1$): returns `NaN` (via PSR on $n=1$).
   - Zero-variance input: silently defaults $SR$ to `0.0` and evaluates PSR against $E[\max SR]$.
2. **Proposed behavior:**
   - Change signature to: `pub fn deflated_sharpe(returns: &[f64], n_trials: usize, sr_variance: f64) -> Result<f64>`
   - Propagate errors from `moments(returns)?` and `expected_max_sharpe(sr_variance, n_trials)?`.
   - When return variance is zero or $n < 2$, return `Err(Error::InvalidArgument(...))` instead of evaluating with $SR = 0.0$.
   - **Justification**: DSR requires an observed Sharpe ratio. If variance is zero, Sharpe is undefined; computing a pseudo-DSR from $SR = 0.0$ misreports statistical confidence for an undefined metric.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1153`: `let dsr = deflated_sharpe(best_returns, n_trials, sr_var)?;` — breaks; propagate with `?`.
   - `crates/spectra-api/src/bin/spectra-worker.rs:1761`: `let dsr = spectra_research::deflated_sharpe(...)?;` — breaks; propagate with `?`.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`:
     - Line 84: `deflated_sharpe(&RETURNS, 10, 0.01).unwrap()`
     - Line 124: `deflated_sharpe(&RETURNS, 10, 0.0).unwrap()`
   - `parity/golden/research.golden`: No lines changed.
5. **Output-changing assessment & divergence:**
   - Hand-computed sample `[-4, -2, 2, 4]` ($N = 10, V = 0.01$):
     - $SR = 0.0$, $E[\max SR] \approx 0.15746$, $n = 4, \gamma_3 = 0.0, \gamma_4 = 1.36$
     - Old: `0.3929007...`
     - New: `Ok(0.3929007...)` (exact match).
   - Zero-variance returns: Old returns float probability, New returns `Err`.

---

### Treatment of `probabilistic_sharpe()` and `min_track_record_length()`

#### `probabilistic_sharpe()` (lines 311–321)

1. **Current behavior:**
   - Lines 312–314: returns `f64::NAN` when $n_{\text{obs}} < 2$.
   - Lines 316–318: returns `f64::NAN` when variance term $1 - \gamma_3 SR + \frac{\gamma_4 - 1}{4}SR^2 \le 0$.
2. **Proposed behavior:**
   - Change signature to: `pub fn probabilistic_sharpe(sr: f64, sr_benchmark: f64, n_obs: usize, skew: f64, raw_kurt: f64) -> Result<f64>`
   - Return `Err(Error::InvalidArgument(format!("probabilistic_sharpe requires n_obs >= 2, got {n_obs}")))` when $n_{\text{obs}} < 2$.
   - Return `Err(Error::InvalidArgument(format!("probabilistic_sharpe variance term ({var_term}) must be positive")))` when $\text{var\_term} \le 0$.
   - Return `Err(Error::InvalidArgument("probabilistic_sharpe inputs must be finite"))` if any float parameter is non-finite.
   - **Justification**: Returning `NaN` allows callers (e.g. `optimization_api.rs:1168`: `if psr.is_nan() { 0.0 } else { psr }`) to silently convert undefined results to `0.0`. Under the silent-default rule, undefined inputs must fail loudly.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1152`: `probabilistic_sharpe(...)` — breaks; update with `?`.
   - `crates/spectra-api/src/bin/spectra-worker.rs:1754`: `probabilistic_sharpe(...)` — breaks; update with `?`.
   - `crates/spectra-research/src/validation.rs:70, 71`: breaks; update with `?`.
   - `crates/spectra-research/src/stats.rs:339`: internal call in `deflated_sharpe` — update with `?`.
   - `crates/spectra-registry/src/vault.rs:367, 368`: breaks; update with `?`.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`:
     - Lines 74, 125: update to `.unwrap()`.
     - Lines 107, 109: update from `.is_nan()` to `.is_err()`.
   - `crates/spectra-research/src/stats.rs`: lines 376 (`.unwrap()`), 377 (`.is_err()`).
   - `parity/golden/research.golden`: No lines changed.
5. **Output-changing assessment & divergence:**
   - Valid inputs: unchanged.
   - Invalid / degenerate inputs: returns `Err` instead of `NaN`.

#### `min_track_record_length()` (lines 344–355)

1. **Current behavior:**
   - Line 345: returns `f64::INFINITY` when $SR \le SR_{\text{benchmark}}$.
   - Line 349: returns `f64::NAN` when variance term $\le 0$.
2. **Proposed behavior:**
   - Change signature to: `pub fn min_track_record_length(sr: f64, sr_benchmark: f64, skew: f64, raw_kurt: f64, confidence: f64) -> Result<f64>`
   - Return `Err(Error::InvalidArgument("confidence must be in (0, 1)"))` if $confidence \le 0$ or $confidence \ge 1$ or non-finite.
   - Return `Err(Error::InvalidArgument("min_track_record_length variance term must be positive"))` if variance term $\le 0$.
   - **For $SR \le SR_{\text{benchmark}}$**: Keep `Ok(f64::INFINITY)`.
     - **Justification**: $+\infty$ is the mathematically exact limit of the formula $\text{MinTRL} = 1 + \text{var\_term}\left(\frac{\Phi^{-1}(\alpha)}{SR - SR^*}\right)^2$ as $SR \to SR^*$. If a strategy does not beat the benchmark in sample, no finite amount of track record can prove that its true Sharpe exceeds the benchmark with confidence $\alpha > 0.5$. It is a valid mathematical limit, not a silent fallback.
     - Furthermore, `parity/golden/research.golden` lines 285 and 471 record `mintrl inf`. Returning `Ok(f64::INFINITY)` preserves C++ golden parity.
     - Callers that convert infinity to `0.0` (such as `optimization_api.rs:1170`) must be updated to return `null` or explicit infinity in JSON.
3. **Workspace callers:**
   - `crates/spectra-api/src/optimization_api.rs:1154`: `min_track_record_length(...)` — breaks; update with `?`.
   - `crates/spectra-api/src/bin/spectra-worker.rs:1762`: `min_track_record_length(...)` — breaks; update with `?`.
   - `crates/spectra-research/src/validation.rs:72`: breaks; update with `?`.
4. **Golden and test files affected:**
   - `crates/spectra-research/tests/m26_stats_reference.rs`: lines 91, 112 (update with `.unwrap()`).
   - `crates/spectra-research/src/stats.rs`: line 380 (update with `.unwrap()`).
   - `parity/golden/research.golden`: Lines 285, 471 (`mintrl inf`) remain completely unchanged.
5. **Output-changing assessment & divergence:**
   - Identical on all valid inputs including $SR \le SR_{\text{benchmark}}$ ($+\infty$).
   - Returns `Err` on invalid confidence or non-positive variance term.

---

## 4. Summary of Impact

- **Affected Functions in `stats.rs`:** 6 (`moments`, `sharpe_per_period`, `expected_max_sharpe`, `deflated_sharpe`, `probabilistic_sharpe`, `min_track_record_length`).
- **Total Affected Callers in Workspace:** 19 call sites across 6 crates/binaries (`spectra-api`, `spectra-worker`, `spectra-research`, `spectra-registry`, `spectra-cli`).
- **Total Test Call Sites to Update:** 18 in `tests/m26_stats_reference.rs`, `tests/m26_bootstrap_reference.rs`, `tests/learn_formula_drift.rs`, and inline unit tests.
- **Affected Golden File Lines:** **0** lines in `parity/golden/research.golden` (all golden scenarios have $n \ge 160$, non-zero volatility, positive trial variance, and retain `mintrl inf`).

---

## 5. Divergence Entries

Documented divergences for each undefined case that previously returned a number or silent placeholder:

| Function | Input Condition | Old Value / Behavior | New Behavior | Mathematical / Rationale |
|---|---|---|---|---|
| `moments` | Empty series (`x = []`, $n=0$) | `Moments { n: 0, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` | `Err(Error::InvalidArgument("moments require at least 2 observations, got 0"))` | Sample variance divisor $n-1 = -1$ is undefined; skew/kurtosis undefined. |
| `moments` | Single observation (`x = [c]`, $n=1$) | `Moments { n: 1, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` | `Err(Error::InvalidArgument("moments require at least 2 observations, got 1"))` | Sample variance divisor $n-1 = 0$ is division by zero; mean was silently discarded as 0.0. |
| `moments` | Zero variance (`s \le 10^{-12} \cdot \text{scale}`) | `Moments { n, mean: \bar{x}, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` | `Err(Error::InvalidArgument("sample has zero variance; higher moments undefined"))` | Higher moments ($m_3 / m_2^{1.5}$ and $m_4 / m_2^2$) are $0/0$ (indeterminate); normal placeholders (0.0, 3.0) falsely implied Gaussian distribution. |
| `sharpe_per_period` | Fewer than 2 returns ($n < 2$) | `0.0` | `Err(Error::InvalidArgument(...))` | Sample standard deviation is undefined with $n < 2$. Conflating with Sharpe 0.0 masks missing data. |
| `sharpe_per_period` | Zero volatility ($s_r = 0.0$) | `0.0` | `Err(Error::InvalidArgument("stdev is zero"))` | Sharpe ratio $\bar{r} / s_r$ has division by zero ($s_r = 0$). Conflating with 0.0 masks zero-risk assets as zero-return under risk. |
| `expected_max_sharpe` | Zero trials ($N = 0$) | `0.0` | `Err(Error::InvalidArgument("n_trials must be at least 1"))` | Evaluation of maximum over empty set of trials is undefined. |
| `expected_max_sharpe` | Negative or non-finite variance ($V < 0.0$ or NaN) | `0.0` | `Err(Error::InvalidArgument("sr_variance must be finite and non-negative..."))` | Trial Sharpe variance cannot be negative or non-finite. |
| `deflated_sharpe` | Zero volatility / constant returns | Evaluated PSR with substituted $SR = 0.0$ returning a pseudo-probability | `Err(Error::InvalidArgument(...))` | Strategy Sharpe is undefined; computing DSR from synthetic $SR = 0$ misreports statistical confidence for an undefined metric. |
| `deflated_sharpe` | Fewer than 2 returns ($n < 2$) | `NaN` (via PSR) | `Err(Error::InvalidArgument(...))` | Insufficient return observations to compute moments or Sharpe. |
| `deflated_sharpe` | Zero trials ($N = 0$) or negative variance | Evaluated PSR against $E[\max SR] = 0.0$ | `Err(Error::InvalidArgument(...))` | Invalid trial ledger parameters propagate explicit errors rather than defaulting benchmark to 0. |
| `probabilistic_sharpe` | $n_{\text{obs}} < 2$ | `NaN` | `Err(Error::InvalidArgument("probabilistic_sharpe requires n_obs >= 2, got {n_obs}"))` | Degrees of freedom in numerator $\sqrt{n-1} \le 0$ requires at least 2 observations; errors loudly rather than passing raw NaN. |
| `probabilistic_sharpe` | Non-positive variance term ($\text{var\_term} \le 0$) | `NaN` | `Err(Error::InvalidArgument("probabilistic_sharpe variance term ({var_term}) must be positive"))` | Square root of non-positive variance term is undefined in real numbers. |
| `probabilistic_sharpe` | Non-finite inputs (NaN / $\pm\infty$) | `NaN` | `Err(Error::InvalidArgument("probabilistic_sharpe inputs must be finite"))` | Invalid arguments rejected explicitly. |
| `min_track_record_length` | Non-positive variance term ($\text{var\_term} \le 0$) | `NaN` | `Err(Error::InvalidArgument("min_track_record_length variance term ({var_term}) must be positive"))` | Variance term in asymptotic variance formula must be strictly positive. |
| `min_track_record_length` | Invalid confidence level ($p \le 0$ or $p \ge 1$ or non-finite) | Float or `NaN` | `Err(Error::InvalidArgument("confidence must be in (0, 1), got {confidence}"))` | Confidence probability must be strictly within $(0, 1)$. |
| `normal_inv_cdf` | Argument outside $[0, 1]$ or non-finite | `NaN` | `Err(Error::InvalidArgument("normal_inv_cdf argument must be in [0, 1], got {p}"))` | Quantile function $\Phi^{-1}(p)$ is defined on $[0, 1]$; returns `Ok(-inf)` at 0, `Ok(+inf)` at 1, and `Err` outside. |

### Detailed Divergence Records by Undefined Case

#### 1. `moments()` — Empty Sample
- **Function:** `moments`
- **Input:** `&[]` ($n = 0$)
- **Old value:** `Moments { n: 0, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }`
- **New behavior:** `Err(Error::InvalidArgument("moments require at least 2 observations, got 0"))`

#### 2. `moments()` — Single Observation
- **Function:** `moments`
- **Input:** `&[c]` ($n = 1$)
- **Old value:** `Moments { n: 1, mean: 0.0, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` (silently dropped `c` as mean)
- **New behavior:** `Err(Error::InvalidArgument("moments require at least 2 observations, got 1"))`

#### 3. `moments()` — Zero Variance
- **Function:** `moments`
- **Input:** Constant slice `&[c, c, ...]` ($s \le 10^{-12} \cdot \text{scale}$)
- **Old value:** `Moments { n, mean: c, stdev: 0.0, skew: 0.0, raw_kurtosis: 3.0 }` (silent normal distribution placeholder)
- **New behavior:** `Err(Error::InvalidArgument("sample has zero variance; higher moments undefined"))`

#### 4. `sharpe_per_period()` — Empty Sample ($n = 0$)
- **Function:** `sharpe_per_period`
- **Input:** `&[]` ($n = 0$)
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("sharpe_per_period requires at least 2 return observations, got 0"))`

#### 5. `sharpe_per_period()` — Single Observation ($n = 1$)
- **Function:** `sharpe_per_period`
- **Input:** `&[c]` ($n = 1$)
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("sharpe_per_period requires at least 2 return observations, got 1"))`

#### 6. `sharpe_per_period()` — Zero Volatility
- **Function:** `sharpe_per_period`
- **Input:** `&[c; n]` where all returns are identical ($s_r = 0.0$)
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("stdev is zero"))`

#### 7. `expected_max_sharpe()` — Zero Trials ($N = 0$)
- **Function:** `expected_max_sharpe`
- **Input:** `expected_max_sharpe(V, 0)`
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("n_trials must be at least 1"))`

#### 8. `expected_max_sharpe()` — Negative or Non-Finite Variance
- **Function:** `expected_max_sharpe`
- **Input:** `expected_max_sharpe(-0.01, 10)` or `expected_max_sharpe(f64::NAN, 10)`
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("sr_variance must be finite and non-negative, got ..."))`

#### 9. `deflated_sharpe()` — Zero Volatility / Constant Returns
- **Function:** `deflated_sharpe`
- **Input:** `deflated_sharpe(&[0.01; 10], 10, 0.01)`
- **Old value:** Evaluated PSR with substituted $SR = 0.0$ against $E[\max SR]$ ($\approx 0.3929$)
- **New behavior:** `Err(Error::InvalidArgument("stdev is zero"))`

#### 10. `deflated_sharpe()` — Zero Trials ($N = 0$)
- **Function:** `deflated_sharpe`
- **Input:** `deflated_sharpe(&RETURNS, 0, 0.01)`
- **Old value:** Evaluated PSR against benchmark $0.0$ (returning unadjusted PSR)
- **New behavior:** `Err(Error::InvalidArgument("n_trials must be at least 1"))`

#### 11. `deflated_sharpe()` — Negative Trial Sharpe Variance ($V < 0.0$)
- **Function:** `deflated_sharpe`
- **Input:** `deflated_sharpe(&RETURNS, 10, -0.01)`
- **Old value:** Evaluated PSR against benchmark $0.0$
- **New behavior:** `Err(Error::InvalidArgument("sr_variance must be finite and non-negative, got -0.01"))`

#### 12. `block_bootstrap_returns()` — Zero Volatility Sample
- **Function:** `block_bootstrap_returns`
- **Input:** `block_bootstrap_returns(&[-0.01; 4], ...)`
- **Old value:** `ReturnBlockBootstrap` with empty `annualized_sharpe` band
- **New behavior:** `Err(Error::InvalidArgument("stdev is zero"))`

