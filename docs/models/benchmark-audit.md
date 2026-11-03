# Benchmark Module Audit (`crates/spectra-research/src/benchmark.rs`)

**Module**: `crates/spectra-research/src/benchmark.rs`  
**Audit Date**: 2026-11-04  
**Context**: M26 Task 8 Statistical Inventory & Silent Defaults Audit  

---

## 1. Inventory Table

| Public Function / Type | Statistic Computed | Divisor or Formula | Empty Slice (`n = 0`) | Single Obs (`n = 1`) | Zero Variance | NaN / Inf in Input | Too Few Observations |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `pub fn aligned_returns(curve: &[EquityPoint]) -> Vec<f64>` | Consecutive period return series of an equity curve | $r_t = \frac{E_t}{E_{t-1}} - 1.0$ (when $E_{t-1} > 0.0$; else returns `0.0`) | `[]` (empty vector) | `[]` (empty vector) | Constant equity: returns `[0.0, ...]` | If `prev` is NaN, returns `0.0`. If `curr` is NaN, returns `NaN`. | Returns `[]` if `n < 2` (no error). |
| `pub fn benchmark_returns(bars: &[Bar]) -> Vec<f64>` | Equal-weight buy-and-hold returns per unique timestamp | $r_t = \frac{1}{\|K_t\|} \sum_{k \in K_t} \left(\frac{P_{k,t}}{P_{k,t-1}} - 1\right)$ (divisor: $\|K_t\|$; returns `0.0` if $\|K_t\| = 0$) | `[]` (empty vector) | `[]` (empty vector; first timestamp has no prior close) | Constant prices: returns `[0.0, ...]` | If any bar close is NaN, return becomes `NaN`. If prior close is `0.0`, produces `+inf`. | Returns `[]` if `< 2` distinct timestamps (no error). |
| `pub fn compute_benchmark_metrics(s: &[f64], b: &[f64], ppy: f64) -> Result<BenchmarkMetrics>` | Beta, annualized alpha, correlation, tracking error, information ratio, up/down capture | Sample cov/var (divisor $n - 1$). See formulas in Section 1.1 below. | Returns `Ok(BenchmarkMetrics::default())` with `valid = false`, all fields `0.0` | Returns `Ok(BenchmarkMetrics::default())` with `valid = false`, all fields `0.0` | **Benchmark $v_b = 0$**: `Ok(default())` (`valid = false`). **Strategy $v_s = 0$**: `Ok` with `valid = true`, `beta = 0.0`, `corr = 0.0`. **$TE \le 10^{-15}$**: `IR = 0.0`. | **NaN in $b$**: `!(vb > 0.0)` triggers, returning `Ok(default())` with `valid = false`. **NaN in $s$**: returns `Ok` with `valid = true` and metric fields as `NaN`. | $n < 2$ or `ppy <= 0.0`: returns `Ok(default())` (`valid = false`). Length mismatch: `Err(InvalidArgument)`. |

### 1.1 Formulas & Divisors in `compute_benchmark_metrics`
- **Sample means**: $\bar{s} = \frac{1}{n} \sum s_i$, $\bar{b} = \frac{1}{n} \sum b_i$ (divisor: $n$)
- **Sample covariances/variances**: (divisor: $n - 1$)
  - $\text{cov}(s, b) = \frac{1}{n-1} \sum (s_i - \bar{s})(b_i - \bar{b})$
  - $\text{var}(s) = \frac{1}{n-1} \sum (s_i - \bar{s})^2$
  - $\text{var}(b) = \frac{1}{n-1} \sum (b_i - \bar{b})^2$
  - $\text{var}(d) = \frac{1}{n-1} \sum ((s_i - b_i) - (\bar{s} - \bar{b}))^2$
- **Metrics**:
  - $\text{beta} = \frac{\text{cov}(s, b)}{\text{var}(b)}$
  - $\text{alpha\_annualized} = (\bar{s} - \text{beta} \cdot \bar{b}) \cdot \text{ppy}$
  - $\text{correlation} = \frac{\text{cov}(s, b)}{\sqrt{\text{var}(s) \cdot \text{var}(b)}}$ (falls back to `0.0` if $\text{var}(s) \le 0.0$)
  - $\text{tracking\_error} = \sqrt{\text{var}(d)} \cdot \sqrt{\text{ppy}}$
  - $\text{information\_ratio} = \frac{\bar{s} - \bar{b}}{\sqrt{\text{var}(d)}} \cdot \sqrt{\text{ppy}}$ (falls back to `0.0` if $\sqrt{\text{var}(d)} \le 10^{-15}$)
  - $\text{up\_capture} = \frac{\text{mean}(s \mid b > 0)}{\text{mean}(b \mid b > 0)}$ (defaults to `0.0` if no up periods or sum is 0)
  - $\text{down\_capture} = \frac{\text{mean}(s \mid b < 0)}{\text{mean}(b \mid b < 0)}$ (defaults to `0.0` if no down periods or sum is 0)

---

## 2. Silent Defaults Inventory

Under M26 rules, a returned zero is permitted only where strictly defined by the mathematical formula. Any missing input, degenerate condition, or undefined statistic must return an explicit `Err`.

| # | File | Line | Code Snippet | Value Returned | Flag / Classification | Rationale & M26 Defect Assessment |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 1 | `benchmark.rs` | 31 | `if prev > 0.0 { ... } else { 0.0 }` | `0.0` | **Silent default** | When prior equity is non-positive ($E_{t-1} \le 0.0$ due to bankruptcy, liquidation, or deficit), the return is undefined or $-100\%$, but is silently reported as flat `0.0` return. |
| 2 | `benchmark.rs` | 57 | `if count > 0 { sum / f64::from(count) } else { 0.0 }` | `0.0` | **Silent default** | When no instruments have prior close data for a consecutive timestamp pair, silently pushes `0.0` return instead of omitting the period or returning an error. |
| 3 | `benchmark.rs` | 100 | `if n < 2 \|\| ppy <= 0.0 { return Ok(m); }` | `Ok(BenchmarkMetrics::default())` | **Silent default** | $n < 2$ (insufficient observations) and `ppy <= 0.0` (invalid period frequency) silently return `Ok` with default zero fields and `valid = false` rather than `Err(Error::InvalidArgument)`. |
| 4 | `benchmark.rs` | 124 | `if !(vb > 0.0) { return Ok(m); }` | `Ok(BenchmarkMetrics::default())` | **Silent default** | When benchmark variance is zero ($v_b = 0.0$) or NaN, beta, correlation, and capture ratios are mathematically undefined ($0/0$). Silently returns `Ok` with all zeros instead of returning an error. |
| 5 | `benchmark.rs` | 129 | `if vs > 0.0 { cov / (vs * vb).sqrt() } else { 0.0 }` | `0.0` | **Silent default** | When strategy return variance $v_s = 0.0$, correlation is undefined ($0/0$), but silently returns `0.0`. |
| 6 | `benchmark.rs` | 132 | `if te > 1e-15 { md / te * ppy.sqrt() } else { 0.0 }` | `0.0` | **Silent default** | When tracking error $TE \le 10^{-15}$, information ratio is undefined ($0/0$ when active return is 0) or infinite ($\pm \infty$), but silently defaults to `0.0`. |
| 7 | `benchmark.rs` | 148 | `if up_n > 0 && up_b != 0.0 { m.up_capture = ...; }` | `0.0` (unassigned default) | **Silent default** | When benchmark has no up periods ($N_{\text{up}} = 0$) or cumulative up return is zero, up capture is undefined, but silently remains `0.0`. |
| 8 | `benchmark.rs` | 151 | `if dn_n > 0 && dn_b != 0.0 { m.down_capture = ...; }` | `0.0` (unassigned default) | **Silent default** | When benchmark has no down periods ($N_{\text{down}} = 0$) or cumulative down return is zero, down capture is undefined, but silently remains `0.0`. |

**Total Silent Defaults Flagged**: 8  
**Top 3 by Risk**:
1. **Line 124 (`!(vb > 0.0)` returning `Ok(default())`)**: Masks zero-variance benchmarks and silently swallows NaNs present in the benchmark series, returning `Ok` with zeroed metrics rather than failing loudly.
2. **Line 100 (`n < 2 || ppy <= 0.0` returning `Ok(default())`)**: Bypasses input validation for empty/short inputs and negative `ppy`, returning a valid `Result::Ok` holding zero metrics.
3. **Line 31 (`prev <= 0.0` returning `0.0`)**: In an equity curve where account equity hits 0 or goes negative, masks bankruptcy as a 0% return, distorting downstream performance and risk metrics.

---

## 3. Callers Breakdown

### 3.1 `aligned_returns` (4 call sites)
1. `crates/spectra-research/src/lib.rs:21`: Module re-export `pub use benchmark::{aligned_returns, ...}`
2. `crates/spectra-backtest/src/output.rs:937`: Backtest report generator computing strategy period returns.
3. `crates/spectra-research/src/survival.rs:1678`: Component 8 (benchmark relative survival test).
4. `crates/spectra-research/src/survival.rs:1861`: Regime survival evaluation.

### 3.2 `benchmark_returns` (4 call sites)
1. `crates/spectra-research/src/lib.rs:21`: Module re-export `pub use benchmark::{benchmark_returns, ...}`
2. `crates/spectra-backtest/src/run.rs:655`: Backtest execution engine generating benchmark curve.
3. `crates/spectra-research/src/survival.rs:1302`: Survival engine data preparation.
4. `crates/spectra-research/src/survival.rs:1679`: Component 8 (benchmark relative survival test).

### 3.3 `compute_benchmark_metrics` (15 call sites)
1. `crates/spectra-research/src/lib.rs:21`: Module re-export `pub use benchmark::{compute_benchmark_metrics, ...}`
2. `crates/spectra-backtest/src/output.rs:942`: Backtest output builder computing benchmark metrics.
3. `crates/spectra-research/src/survival.rs:1690`: Survival Lab Component 8 computing benchmark statistics.
4. `crates/spectra-cli/tests/learn_formula_drift.rs:155`: Learn formula drift test harness verifying benchmark formulas.
5. `crates/spectra-research/src/benchmark.rs:163`: Unit test `identical_series_have_beta_one_and_no_tracking_error`.
6. `crates/spectra-research/src/benchmark.rs:168`: Unit test checking length mismatch error.
7. `crates/spectra-research/src/benchmark.rs:170`: Unit test checking doubled return series beta.
8. `crates/spectra-research/tests/m26_benchmark_reference.rs:29`: Reference test for beta and correlation.
9. `crates/spectra-research/tests/m26_benchmark_reference.rs:38`: Reference test for alpha.
10. `crates/spectra-research/tests/m26_benchmark_reference.rs:45`: Reference test for tracking error and IR.
11. `crates/spectra-research/tests/m26_benchmark_reference.rs:52`: Reference test for capture ratios.
12. `crates/spectra-research/tests/m26_benchmark_reference.rs:61`: Reference test for identical series (zero tracking error).
13. `crates/spectra-research/tests/m26_benchmark_reference.rs:70`: Reference test for series length mismatch.
14. `crates/spectra-research/tests/m26_benchmark_reference.rs:77`: Reference test for constant benchmark (`!m.valid`).
15. `crates/spectra-research/tests/m26_survival_components_6_8_reference.rs:705`: Integration test for Component 8 with zero-variance benchmark.

**Total Workspace Call Sites**: 23 call sites across `crates/`, `tests/`, and re-exports.

---

## 4. Golden and Fixture Files

The following reference test files directly test or rely on the behaviour of these functions, including undefined/degenerate cases:

1. **`crates/spectra-research/tests/m26_benchmark_reference.rs`**:
   - Lines 11–13 state: *"Not covered here: invalid-input zeros. Returning zeros without an error is a documented defect (task 8), so a test would lock in the wrong behaviour; the `valid` flag is checked instead."*
   - Line 61 (`test a_strategy_identical_to_the_benchmark_has_no_tracking_error`): Explicitly asserts `assert_eq!(m.information_ratio, 0.0, "IR with no active return");`, relying on the silent default fallback when $TE \le 10^{-15}$.
   - Line 74 (`test a_constant_benchmark_is_flagged_invalid`): Feeds `flat = [0.01; 5]` (zero variance) and asserts `assert!(!m.valid);`, testing the degenerate early return of `Ok(BenchmarkMetrics::default())`.

2. **`crates/spectra-research/tests/m26_survival_components_6_8_reference.rs`**:
   - Line 704 (`test benchmark_relative_insufficient_when_benchmark_has_zero_variance`): Feeds constant prices (`closes = vec![100.0; 80]`) causing benchmark variance = 0.0. Asserts `assert!(!bm.metrics.valid);` and `assert_eq!(bm.verdict, Verdict::Insufficient);`, relying on the silent default `valid = false` flag in Component 8.

3. **`crates/spectra-cli/tests/learn_formula_drift.rs`**:
   - Line 155 (`test learn_benchmark_metrics_formulas`): Validates formulas against reference series; expects successful execution without error.

---

## 5. Divergence Records (M26 Enforced Rules)

Summary of divergences between the legacy behavior (returning default numbers/structs) and the M26 enforced behavior (explicit errors or formula-defined values):

| Function | Case / Input Condition | Old Value | New Behavior | Rationale / Mathematical Rule |
| :--- | :--- | :--- | :--- | :--- |
| `compute_benchmark_metrics` | Insufficient observations ($n < 2$) | `Ok(BenchmarkMetrics::default())` (`valid = false`, zeros) | `Err(Error::InvalidArgument("benchmark metrics: need at least 2 observations, got {n}"))` | Covariance and sample variance divisors $n - 1 \le 0$ require at least 2 observations; returning zeros conflated missing data with zero risk. |
| `compute_benchmark_metrics` | Non-positive or non-finite frequency factor (`ppy <= 0.0`) | `Ok(BenchmarkMetrics::default())` (`valid = false`, zeros) | `Err(Error::InvalidArgument("benchmark metrics: periods_per_year must be positive, got {ppy}"))` | Annualization factor must be strictly positive and finite; invalid config must fail loudly. |
| `compute_benchmark_metrics` | Zero benchmark variance ($v_b \le 0.0$) or NaN in benchmark | `Ok(BenchmarkMetrics::default())` (`valid = false`, zeros) | `Err(Error::InvalidArgument("benchmark metrics: zero benchmark variance or NaN in benchmark"))` | OLS beta $\text{cov}(s, b) / \text{var}(b)$ and correlation have $0/0$ division by zero; must return explicit error instead of Ok with zero fields. |
| `aligned_returns` | Non-positive prior equity ($E_{t-1} \le 0.0$ or non-finite) | `0.0` return | `Err(Error::InvalidArgument("aligned_returns: non-positive prior equity {prev}"))` | Return calculation $\frac{E_t}{E_{t-1}} - 1$ has non-positive or undefined divisor; masking as 0.0 hid account deficit/insolvency. |
| `aligned_returns` | Prior equity positive and current equity non-positive ($E_{t-1} > 0 \land E_t \le 0$) | `0.0` return | `-1.0` return | Total loss is formula-defined: wiping out account equity represents a $-100\%$ return ($-1.0$). |
| `run_survival_lab` | Benchmark evaluation error from `compute_benchmark_metrics` | Silent default `Verdict::Insufficient` via unassigned field | Explicit `bm.verdict = Verdict::Insufficient;` assignment | Explicitly handles benchmark calculation errors, setting finding and verdict deterministically. |

### Detailed Divergence Records by Case

#### 1. `compute_benchmark_metrics()` — Insufficient Observations ($n < 2$)
- **Function:** `compute_benchmark_metrics`
- **Input:** `s.len() < 2` or `b.len() < 2` (e.g. `n = 0` or `n = 1`)
- **Old value:** `Ok(BenchmarkMetrics::default())` (with `valid = false` and all statistics defaulted to `0.0`)
- **New behavior:** `Err(Error::InvalidArgument("benchmark metrics: need at least 2 observations, got {n}"))`

#### 2. `compute_benchmark_metrics()` — Non-Positive Annualization Factor (`ppy <= 0.0`)
- **Function:** `compute_benchmark_metrics`
- **Input:** `ppy <= 0.0` or non-finite `ppy`
- **Old value:** `Ok(BenchmarkMetrics::default())` (with `valid = false` and all statistics defaulted to `0.0`)
- **New behavior:** `Err(Error::InvalidArgument("benchmark metrics: periods_per_year must be positive, got {ppy}"))`

#### 3. `compute_benchmark_metrics()` — Zero Benchmark Variance or NaN in Benchmark
- **Function:** `compute_benchmark_metrics`
- **Input:** Constant benchmark returns $b_i = c$ (producing $v_b = 0.0$) or any $b_i = \text{NaN}$
- **Old value:** `Ok(BenchmarkMetrics::default())` (with `valid = false` and all statistics defaulted to `0.0`)
- **New behavior:** `Err(Error::InvalidArgument("benchmark metrics: zero benchmark variance or NaN in benchmark"))`

#### 4. `aligned_returns()` — Non-Positive Prior Equity
- **Function:** `aligned_returns`
- **Input:** Equity curve containing consecutive points where $E_{t-1} \le 0.0$ or $E_{t-1}$ is non-finite
- **Old value:** Pushed `0.0` into the return series
- **New behavior:** `Err(Error::InvalidArgument("aligned_returns: non-positive prior equity {prev}"))`

#### 5. `aligned_returns()` — Formula-Defined Total Loss
- **Function:** `aligned_returns`
- **Input:** Equity curve where $E_{t-1} > 0.0$ and $E_t \le 0.0$
- **Old value:** Pushed `0.0` into the return series
- **New behavior:** Pushes `-1.0` (representing total capital loss / $-100\%$)

#### 6. `run_survival_lab()` — Explicit Insufficient Mapping
- **Function:** `run_survival_lab_controlled` / Component 8 (`survival.rs`)
- **Input:** Strategies or benchmarks producing an error in `compute_benchmark_metrics`
- **Old value:** Swallowed error via `bm.metrics.valid == false` branch, leaving `bm.verdict` as default
- **New behavior:** Explicitly matches on `compute_benchmark_metrics` and sets `bm.verdict = Verdict::Insufficient` and finding message
