# Extreme Value Theory (EVT) CVaR Audit (`crates/spectra-risk/src/evt.rs`)

**Module**: `crates/spectra-risk/src/evt.rs`  
**Audit Date**: 2026-11-04  
**Context**: M26 Task 8 Statistical Inventory & Silent Defaults Audit  

---

## 1. Inventory Table

| Public Function / Method | Statistic Computed | Divisor or Formula | Empty Slice (`n = 0`) | Single Obs (`n = 1`) | Zero Variance | NaN / Inf in Input | Too Few Observations |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `pub fn EvtTailModel::fit(losses: &[f64], threshold_quantile: f64) -> Result<Self>` | Peaks-Over-Threshold (POT) Generalized Pareto Distribution (GPD) fit: threshold $u$, shape $\xi$, scale $\beta$, KS statistic $D$ | Hosking & Wallis (1987) PWM. Divisors: $N_u$ for $a_0$, $N_u(N_u-1)$ for $a_1$, $\text{denom} = a_0 - 2 a_1$ for $\xi, \beta$. | Returns `Err(Error::InvalidArgument("EVT POT requires at least 50 loss observations, got 0"))` | Returns `Err(Error::InvalidArgument("EVT POT requires at least 50 loss observations, got 1"))` | **In losses**: $N_u = 0 < 8 \to$ `Err(InvalidArgument)`. **In exceedances**: $\text{denom} = 0 \to$ silently defaults to $(\xi=0, \beta=a_0)$. | No check: NaNs sort to tail via `total_cmp`. $\text{denom.abs()} > 10^{-9}$ is false $\to$ returns `Ok(Self)` with `beta = NaN`, `threshold = NaN`. | $N < 50 \to \text{Err}$. $N_u < 8 \to \text{Err}$. $q \notin [0.70, 0.999) \to \text{Err}$. |
| `pub fn EvtTailModel::var(&self, p: f64) -> f64` | GPD Value-at-Risk at confidence $p$ | $u + \frac{\beta}{\xi} \left[\left(\frac{N}{N_u}(1 - p)\right)^{-\xi} - 1\right]$ (or $u - \beta \ln\left[\frac{N}{N_u}(1 - p)\right]$ if $\|\xi\| < 10^{-6}$) | N/A (method on fitted model) | N/A (method on fitted model) | N/A (method on fitted model) | If $p$ is NaN, returns `NaN`. | $p \ge 1.0$ or $p \le 0.0$ silently clamped to $[0.0, 0.99999]$. |
| `pub fn EvtTailModel::cvar(&self, p: f64) -> Result<f64>` | GPD Expected Shortfall (CVaR) at confidence $p$ | $\text{CVaR}_p = \frac{\text{VaR}_p + \beta - \xi u}{1 - \xi}$ (divisor: $1 - \xi$) | N/A (method on fitted model) | N/A (method on fitted model) | N/A (method on fitted model) | If $p$ is NaN, returns `Ok(NaN)`. | If $\xi \ge 1.0$: returns `Err(Error::Data("EVT CVaR undefined for shape >= 1"))`. |

### 1.1 Formulas & Estimators in `EvtTailModel`
- **Threshold**: $u = \text{quantile}(losses, q)$
- **Exceedances**: $y_j = l_j - u$ for $l_j > u$, sorted ascending $j = 0 \dots N_u - 1$
- **Probability-Weighted Moments**:
  - $a_0 = \frac{1}{N_u} \sum_{j=0}^{N_u - 1} y_j$ (divisor: $N_u$)
  - $a_1 = \frac{1}{N_u} \sum_{j=0}^{N_u - 1} \frac{N_u - 1 - j}{N_u - 1} y_j$
  - $\text{denom} = a_0 - 2 a_1$
  - $\xi = 2 - \frac{a_0}{\text{denom}}$
  - $\beta = \frac{2 a_0 a_1}{\text{denom}}$
  - If $|\text{denom}| \le 10^{-9}$: falls back to $\xi = 0.0, \beta = a_0$ (exponential tail).
- **Goodness-of-Fit**:
  - Empirical CDF: $F_n(y_j) = \frac{j+1}{N_u}$
  - Theoretical GPD CDF: $F(y) = 1 - \left(1 + \frac{\xi y}{\beta}\right)^{-1/\xi}$ (or $1 - e^{-y/\beta}$ if $|\xi| < 10^{-6}$)
  - KS test statistic: $D = \max |F_n(y_j) - F(y_j)|$
  - Critical value: $D_{\text{crit}} = \frac{1.36}{\sqrt{N_u}}$
  - Pass condition: $D < D_{\text{crit}}$

---

## 2. Silent Defaults Inventory

Under M26 rules, a returned zero is permitted only where strictly defined by the mathematical formula. Any missing input, degenerate condition, or undefined statistic must return an explicit `Err`.

| # | File | Line | Code Snippet | Value Returned | Flag / Classification | Rationale & M26 Defect Assessment |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 1 | `evt.rs` | 72 | `else { (0.0, a_0) }` | `xi = 0.0, beta = a_0` | **Silent default** | When $\text{denom} = a_0 - 2 a_1 \approx 0$ (e.g. constant/identical exceedances where $a_0 = 2 a_1$), the Hosking-Wallis PWM shape is mathematically undefined ($0/0$). Instead of returning an error, it silently assumes an exponential distribution ($\xi = 0$), masking tail model degeneracy. |
| 2 | `evt.rs` | 88 | `let term = (1.0 + xi * y / beta).max(1e-12);` | `1e-12` | **Silent default / clamping** | For negative shape $\xi < 0$, GPD has finite support $y \le -\beta / \xi$. If an exceedance exceeds the upper endpoint, $1 + \xi y / \beta \le 0$. Instead of flagging a support violation or clipping CDF to 1.0, it silently clamps the base to `1e-12`. |
| 3 | `evt.rs` | 110 | `let p_clamped = p.clamp(0.0, 0.99999);` | Clamped $p$ | **Silent default** | Silently clamps invalid confidence levels $p \ge 1.0$ or $p \le 0.0$ to $[0.0, 0.99999]$. If a caller asks for $p = 1.0$ (infinite VaR / upper tail boundary), it evaluates at 0.99999 without returning an error. |
| 4 | `evt.rs` | 114 | `ratio.max(1e-18).ln()` | `1e-18` | **Silent default / floor** | Artificial floor to prevent $\ln(0) = -\infty$, silently masking zero tail probability. |
| 5 | `evt.rs` | 116 | `ratio.max(1e-18).powf(-self.xi)` | `1e-18` | **Silent default / floor** | Artificial floor to prevent $0^{-\xi}$, silently masking zero tail probability. |
| 6 | `evt.rs` | 31, 108, 127 | `var` and `cvar` ignore `passes_goodness_of_fit` | Numerical VaR / CVaR | **Silent default / unvalidated state** | The model computes and stores `passes_goodness_of_fit = false` when the Kolmogorov-Smirnov test fails, but neither `var` nor `cvar` checks this flag. Downstream callers in `RiskEngine` (`engine.rs:309`) and API (`engine.rs:408`) emit VaR and CVaR without checking GoF. |
| 7 | `evt.rs` | 43–78 | Unchecked `NaN`/`inf` in `losses` | `Ok(EvtTailModel)` with NaNs | **Silent default / missing validation** | `losses` is sorted with `total_cmp` without validating that inputs are finite. NaNs in exceedances cause `denom` to be `NaN`, triggering line 72 fallback `(0.0, a_0)` where $a_0 = \text{NaN}$. `beta <= 0.0` is false for NaN, returning `Ok` with NaN threshold and beta. |

**Total Silent Defaults Flagged**: 7  
**Top 3 by Risk**:
1. **Line 72 (`else { (0.0, a_0) }` when `denom.abs() <= 1e-9`)**: Silently masks degenerate exceedance distributions (such as uniform or identical tail losses) as an exponential distribution ($\xi = 0$), severely underestimating tail risk by assuming thin tails when tail estimation is actually mathematically undefined.
2. **Line 110 (`p.clamp(0.0, 0.99999)`)**: Silently distorts user-requested confidence limits (e.g. $p = 1.0$, $p = 0.999999$, or $p \le 0$) rather than returning `Err(InvalidArgument)`.
3. **Lines 31/108/127 (Unchecked `passes_goodness_of_fit`)**: Callers in `spectra-risk` and `spectra-api` compute and report EVT VaR and CVaR without checking whether the fitted GPD passed the Kolmogorov-Smirnov test, presenting fitted tail statistics to risk managers even when the model fails basic statistical validation.

---

## 3. Callers Breakdown

### 3.1 `EvtTailModel::fit` (18 call sites)
1. `crates/spectra-risk/src/lib.rs:60`: Re-export `pub use evt::EvtTailModel;`
2. `crates/spectra-risk/src/engine.rs:309`: Risk engine `calculate` method fitting EVT on simulated losses if `losses.len() >= 50`.
3. `crates/spectra-api/src/engine.rs:408`: REST API endpoint `pub fn evt(j: &Json) -> Reply`.
4. `crates/spectra-risk/src/evt.rs:151`: Unit test `test_evt_tail_fit_and_goodness_of_fit`.
5. `crates/spectra-risk/tests/m26_evt_reference.rs:39`: Test `fit_recovers_the_pwm_shape_scale_and_threshold`.
6. `crates/spectra-risk/tests/m26_evt_reference.rs:49`: Test `goodness_of_fit_statistic_matches_the_reference`.
7. `crates/spectra-risk/tests/m26_evt_reference.rs:57`: Test `var_matches_the_gpd_tail_formula`.
8. `crates/spectra-risk/tests/m26_evt_reference.rs:65`: Test `cvar_matches_the_gpd_tail_formula`.
9. `crates/spectra-risk/tests/m26_evt_reference.rs:72`: Test `a_light_tail_fit_matches_the_reference`.
10. `crates/spectra-risk/tests/m26_evt_reference.rs:84`: Test `the_fit_does_not_depend_on_input_order` (call 1).
11. `crates/spectra-risk/tests/m26_evt_reference.rs:85`: Test `the_fit_does_not_depend_on_input_order` (call 2).
12. `crates/spectra-risk/tests/m26_evt_reference.rs:93`: Test `too_few_observations_or_exceedances_are_refused` (call 1).
13. `crates/spectra-risk/tests/m26_evt_reference.rs:98`: Test `too_few_observations_or_exceedances_are_refused` (call 2).
14. `crates/spectra-risk/tests/m26_evt_reference.rs:104`: Test `threshold_quantile_outside_the_range_is_refused` (call 1).
15. `crates/spectra-risk/tests/m26_evt_reference.rs:105`: Test `threshold_quantile_outside_the_range_is_refused` (call 2).
16. `crates/spectra-risk/tests/m26_evt_reference.rs:112`: Test `evt_fit_shape_outside_range_returns_error`.
17. `crates/spectra-risk/tests/risk.rs:326`: Indirectly called via `RiskEngine::calculate` in `correlated_copula_modes_produce_contributions_and_evt`.
18. `crates/spectra-risk/tests/risk.rs:342`: Indirectly called via `RiskEngine::run` in `risk_engine_run_with_60_simulations_fails_due_to_evt_fit_error`.

### 3.2 `EvtTailModel::var` (7 call sites)
1. `crates/spectra-risk/src/engine.rs:310`: `Some(e.var(config.confidence))` in risk calculation.
2. `crates/spectra-api/src/engine.rs:409`: `let evt_var = model.var(confidence);` in API handler.
3. `crates/spectra-risk/src/evt.rs:131`: Internal call within `EvtTailModel::cvar`.
4. `crates/spectra-risk/src/evt.rs:160`: Unit test `test_evt_tail_fit_and_goodness_of_fit`.
5. `crates/spectra-risk/tests/m26_evt_reference.rs:58`: Reference test for VaR at 95%.
6. `crates/spectra-risk/tests/m26_evt_reference.rs:59`: Reference test for VaR at 99%.
7. `crates/spectra-risk/tests/m26_evt_reference.rs:76`: Reference test for light tail VaR at 99%.

### 3.3 `EvtTailModel::cvar` (8 call sites)
1. `crates/spectra-risk/src/engine.rs:310`: `Some(e.cvar(config.confidence)?)` in risk calculation.
2. `crates/spectra-api/src/engine.rs:410`: `let evt_cvar = model.cvar(confidence)...` in API handler.
3. `crates/spectra-risk/src/evt.rs:161`: Unit test `test_evt_tail_fit_and_goodness_of_fit`.
4. `crates/spectra-risk/tests/m26_evt_reference.rs:66`: Reference test for CVaR at 95%.
5. `crates/spectra-risk/tests/m26_evt_reference.rs:67`: Reference test for CVaR at 99%.
6. `crates/spectra-risk/tests/m26_evt_reference.rs:77`: Reference test for light tail CVaR at 99%.
7. `crates/spectra-risk/tests/m26_evt_reference.rs:132`: Reference test asserting `Err` when $\xi = 1.0$.
8. `crates/spectra-risk/tests/m26_evt_reference.rs:144`: Reference test asserting `Err` when $\xi = 1.5$.

**Total Workspace Call Sites**: 33 call sites across `crates/`, `tests/`, and re-exports.

---

## 4. Golden and Fixture Files

The following test suites directly test or rely on the behaviour of EVT POT, including undefined and error cases:

1. **`crates/spectra-risk/tests/m26_evt_reference.rs`**:
   - Reference-value tests using 40-digit `mpmath` solutions for heavy-tail ($\xi = -0.2969$) and light-tail ($\xi = 1/21$) distributions.
   - Line 93: Asserts `EvtTailModel::fit(&short, 0.8).is_err()` for 49 observations ($N < 50$).
   - Line 98: Asserts `EvtTailModel::fit(&few, 0.9).is_err()` for 5 exceedances ($N_u < 8$).
   - Lines 104–105: Asserts `is_err()` when `threshold_quantile` is 0.69 ($< 0.70$) or 0.999 ($\ge 0.999$).
   - Line 112: Asserts `Error::Data` when fitted $\xi = 0.9347 > 0.9$ (outside supported range $[-0.5, 0.9]$).
   - Lines 123–146: Explicitly tests the undefined CVaR condition when $\xi \ge 1.0$: verifies that calling `cvar(0.99)` on synthetic models with $\xi = 1.0$ and $\xi = 1.5$ returns `Err(Error::Data("EVT CVaR undefined for shape >= 1"))`.

2. **`crates/spectra-risk/tests/risk.rs`**:
   - Line 315 (`test correlated_copula_modes_produce_contributions_and_evt`): Verifies `rep.evt_var.is_some()` and `rep.evt_cvar.is_some()` when running full risk calculations with copulas.
   - Line 339 (`test risk_engine_run_with_60_simulations_fails_due_to_evt_fit_error`): Verifies that running 60 simulations with quantile 0.95 produces only 3 exceedances ($< 8$), propagating an `Error::InvalidArgument("Too few exceedances")` up through the risk engine.

---

## 5. Divergence Entries

Documented divergences for each undefined case that previously returned a number or silent placeholder:

| Function | Input Condition | Old Value / Behavior | New Behavior | Mathematical / Rationale |
|---|---|---|---|---|
| `EvtTailModel::fit` | Zero variance in tail exceedances ($\text{denom} = a_0 - 2 a_1 \approx 0$, $\|\text{denom}\| \le 10^{-9}$) | Defaulted to $(\xi = 0.0, \beta = a_0)$ (exponential tail) | `Err(Error::Data("EVT PWM shape undefined: zero-variance tail exceedances"))` | When tail exceedances above threshold $u$ are identical or have zero sample variance, $a_0 = 2 a_1$ and $\text{denom} = 0$. The Hosking-Wallis PWM estimators $\xi = 2 - a_0 / \text{denom}$ and $\beta = 2 a_0 a_1 / \text{denom}$ involve division by zero and are mathematically indeterminate. Silently defaulting to an exponential distribution ($\xi = 0$) masks degenerate tail samples and misreports tail risk. |
| `EvtTailModel::var` | Confidence level $p \le 0.0$, $p \ge 1.0$, or NaN ($p \notin (0, 1)$) | Silently clamped $p$ to $[0.0, 0.99999]$ | `Err(Error::InvalidArgument("confidence level p must be in (0, 1), got {p}"))` | Cumulative tail probability $p$ is mathematically defined on the open interval $(0, 1)$. Values $p \le 0$ or $p \ge 1$ are invalid confidence levels (e.g., $p = 1.0$ represents infinite loss / upper bound). Clamping alters user intent and silently produces ungrounded VaR estimates. |

### Detailed Divergence Records by Undefined Case

#### 1. `EvtTailModel::fit` — Zero-Variance Tail Exceedances
- **Function:** `EvtTailModel::fit`
- **Input:** Degenerate tail sample where all exceedances above threshold $u$ are identical ($y_j = c$), causing $a_0 = c$, $a_1 = c/2$, and $\text{denom} = a_0 - 2 a_1 = 0 \le 10^{-9}$.
- **Old value:** Silently returned `Ok(Self)` with $\xi = 0.0$ and $\beta = a_0$ (exponential tail assumption).
- **New behavior:** Returns `Err(Error::Data("EVT PWM shape undefined: zero-variance tail exceedances"))`.
- **Mathematical / Rationale:** Hosking & Wallis (1987) probability-weighted moment estimators require non-zero variance in exceedances. When $\text{denom} = 0$, both shape $\xi$ and scale $\beta$ are division-by-zero operations ($0/0$). Substituting an exponential distribution ($\xi = 0$) without empirical evidence masks degenerate data (e.g. repeated constant ticks or synthetic artifacts) and underestimates tail risk.
- **Workspace Impact:** In callers (`RiskEngine::calculate`, `api/v1/risk/evt`), degenerate tail samples now fail with explicit `Error::Data` instead of producing synthetic exponential VaR/CVaR values.

#### 2. `EvtTailModel::var` & `EvtTailModel::cvar` — Confidence Level Outside $(0, 1)$
- **Function:** `EvtTailModel::var` (and propagated through `EvtTailModel::cvar`)
- **Input:** $p \le 0.0$, $p \ge 1.0$, or $p$ is NaN.
- **Old value:** Silently clamped $p$ into $[0.0, 0.99999]$ via `p.clamp(0.0, 0.99999)`. Calling `var(1.0)` returned the 99.999% quantile; calling `var(-0.5)` returned the 0.0% quantile.
- **New behavior:** Returns `Err(Error::InvalidArgument(format!("confidence level p must be in (0, 1), got {p}")))`.
- **Mathematical / Rationale:** Quantile estimation for extreme value distributions is valid only for probabilities $0 < p < 1$. Clamping $p = 1.0$ masks unbounded or endpoint extrapolation, and negative $p$ is physically meaningless for probability.
- **Workspace Impact:** Function signature of `EvtTailModel::var` changed from `fn var(&self, p: f64) -> f64` to `fn var(&self, p: f64) -> Result<f64>`. Callers in `RiskEngine` (`crates/spectra-risk/src/engine.rs`), API handler (`crates/spectra-api/src/engine.rs`), and reference tests propagate the `Result`.

> [!NOTE]
> **Open Item: Goodness-of-Fit Enforcement**
> In accordance with M26 Task 8 instructions, the Kolmogorov-Smirnov goodness-of-fit check (`passes_goodness_of_fit`) is calculated and reported on `EvtTailModel`, but enforcement remains unchanged in this PR as an open item for the user.
