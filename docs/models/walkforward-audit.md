# Walk-Forward Validation Module Audit (`crates/spectra-research/src/walkforward.rs`)

**Module**: `crates/spectra-research/src/walkforward.rs`  
**Audit Date**: 2026-11-04  
**Context**: M26 Task 8 Statistical Inventory & Silent Defaults Audit  

---

## 1. Inventory Table

| Public Function / Type | Statistic / Output Computed | Divisor or Formula | Empty Input (`n = 0`) | Single Obs (`n = 1`) | Zero Variance | NaN / Inf in Input | Too Few Observations |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `pub fn make_folds(n: usize, cfg: &WalkForwardConfig) -> Result<Vec<Fold>>` | Sequence of half-open timestamp index ranges `[train_begin, train_end)` and `[test_begin, test_end)` for each fold | Sliding or anchored window bounds over $n$ timestamps; step advance $s \mathrel{+}= \text{step}$ (where $\text{step}=0$ means `cfg.test`). Stops when $\text{test\_end} > n$. | Returns `Ok([])` (empty vector; no error) | Returns `Ok([])` (empty vector; no error) | N/A (operates on timestamp count $n$ and window configuration) | N/A (inputs are unsigned integers and enum) | If $n < \text{train} + \text{embargo} + \text{test}$, loop immediately terminates and returns `Ok([])` (empty vector) without returning an error. Requires `train > 0` and `test > 0`; else `Err(InvalidArgument)`. |
| `pub fn walk_forward_efficiency(mean_in_sample: f64, mean_out_of_sample: f64) -> f64` | Walk-Forward Efficiency (WFE): ratio of mean out-of-sample objective to mean in-sample objective | $\text{WFE} = \frac{\bar{O}_{\text{OOS}}}{\bar{O}_{\text{IS}}}$ (divisor: $\bar{O}_{\text{IS}}$). If $\|\bar{O}_{\text{IS}}\| \le 10^{-9}$, returns `0.0`. | Returns `0.0` (since $\|0.0\| \le 10^{-9}$) | Evaluates ratio directly for $n=1$ fold (if $\|\bar{O}_{\text{IS}}\| > 10^{-9}$) | Returns constant ratio if $\|\bar{O}_{\text{IS}}\| > 10^{-9}$; else `0.0` | **$\bar{O}_{\text{IS}}$ is NaN**: `!(NaN.abs() > 1e-9)` is false, entering `else` and returning `0.0`! (Silently swallows NaN). **$\bar{O}_{\text{OOS}}$ is NaN**: returns `NaN`. **$\pm\infty$**: returns `0.0` if IS is infinite; $\pm\infty$ if OOS is infinite. | If denominator $\|\bar{O}_{\text{IS}}\| \le 10^{-9}$, returns `0.0` instead of `Err` or `NaN`. |
| `pub fn WalkForwardResult::efficiency(&self) -> f64` | Walk-Forward Efficiency of the completed walk-forward result | Delegates to `walk_forward_efficiency(self.mean_in_sample_objective, self.mean_out_of_sample_objective)` | Default struct has $\bar{O}_{\text{IS}} = 0.0, \bar{O}_{\text{OOS}} = 0.0$; returns `0.0` | Evaluates efficiency of single fold | Evaluates efficiency of constant objectives | Same as `walk_forward_efficiency`: if `mean_in_sample_objective` is NaN, silently returns `0.0`. | For 0 folds, mean IS objective is `0.0`, silently returning `0.0`. |
| `pub fn run_walk_forward(bars: &[Bar], factory: &StrategyFactory<'_>, config: &BacktestConfig, params: &[ParamSet], wf: &WalkForwardConfig, objective: &Objective<'_>, options: SweepOptions, mut ledger: Option<&mut TrialLedger>) -> Result<WalkForwardResult>` | Multi-fold parameter optimization, out-of-sample backtest evaluation, stitched return compounding, and aggregated performance metrics | Slices bars by unique timestamp offsets; runs `run_sweep` per fold; evaluates best trial out-of-sample; averages objectives (divisor: $K = \text{folds.len()}$); compounds stitched returns $E_t = E_{t-1}(1 + r_t)$. | `validate_bars(&[])` returns `None` (valid); `timestamp_offsets` returns `[0]`; $n_{\text{ts}} = 0$; `make_folds` returns `Ok([])`; loop is skipped; returns `Ok(WalkForwardResult::default())` with 0 folds, empty returns, and invalid metrics (`valid = false`). No error! | $n_{\text{ts}} = 1 < \text{train} + \text{test}$; `make_folds` returns `Ok([])`; returns `Ok(WalkForwardResult::default())`. No error! | Constant prices: returns 0.0; metrics show 0 volatility; `objective_sharpe` returns 0.0; `efficiency()` returns 0.0. Identical param objectives: best trial index is 0. | Prices are fixed-point `Price` (i64, non-positive rejected by `validate_bars`). If objective function returns NaN: `sweep.rs` maps NaN to $-\infty$; if all trials return NaN, best trial is 0 with NaN objective; mean IS objective becomes NaN; `efficiency()` returns `0.0`! | If unique timestamps $n_{\text{ts}} < \text{train} + \text{embargo} + \text{test}$, `run_walk_forward` silently returns `Ok(WalkForwardResult::default())` with 0 folds, 0 trials, and `oos_metrics.valid = false` instead of an `Err`. |

### 1.1 Structural Types in `walkforward.rs`

- **`WalkForwardMode`**: Enum controlling window movement:
  - `Rolling` (`#[default]`): Train window slides forward with each fold step: `train_begin = s`.
  - `Anchored`: Train window always begins at timestamp 0: `train_begin = 0`.
- **`WalkForwardConfig`**: Configuration struct specifying:
  - `train`: Number of unique timestamps in training window (`usize`, must be $> 0$).
  - `test`: Number of unique timestamps in test window (`usize`, must be $> 0$).
  - `step`: Advance step between folds (`usize`; `0` defaults to `test`).
  - `embargo`: Number of unique timestamps skipped between train end and test begin (`usize`).
  - `mode`: `WalkForwardMode` (`Rolling` or `Anchored`).
- **`Fold`**: Half-open index ranges into the sorted array of unique timestamps:
  - `train_begin`: Start index of training data (inclusive).
  - `train_end`: End index of training data (exclusive).
  - `test_begin`: Start index of test data (inclusive; equals `train_end + embargo`).
  - `test_end`: End index of test data (exclusive; equals `test_begin + test`).
- **`FoldResult`**: Per-fold outcome storing chosen parameters, in-sample/out-of-sample objectives, out-of-sample metrics, out-of-sample returns, backtest run record, and trials count.
- **`WalkForwardResult`**: Aggregated validation result storing all fold results, concatenated stitched out-of-sample returns, aggregated equity metrics, mean in-sample objective, mean out-of-sample objective, and total trials run.

### 1.2 Mathematical Formulas & Divisors

1. **Timestamp Offset Slicing**:
   - Unique timestamps: $U = \{ \text{ts}_0 < \text{ts}_1 < \dots < \text{ts}_{n_{\text{ts}}-1} \}$
   - Slice bounds for fold $k$: $\text{bars}[\text{offsets}[\text{begin}] \dots \text{offsets}[\text{end}]]$
2. **Fold Step & Window Traversal**:
   - Step size: $\Delta = \begin{cases} \text{cfg.test} & \text{if } \text{cfg.step} = 0 \\ \text{cfg.step} & \text{otherwise} \end{cases}$
   - At step $k \ge 0$, start offset $s_k = k \cdot \Delta$:
     - Train end: $e_{\text{train}} = s_k + \text{cfg.train}$
     - Test begin: $b_{\text{test}} = e_{\text{train}} + \text{cfg.embargo}$
     - Test end: $e_{\text{test}} = b_{\text{test}} + \text{cfg.test}$
     - Stopping condition: $e_{\text{test}} > n_{\text{ts}}$
3. **Mean Objectives**:
   - In-sample mean: $\bar{O}_{\text{IS}} = \frac{1}{K} \sum_{k=1}^K O_{k, \text{IS}}$ (divisor: $K = \text{folds.len()}$)
   - Out-of-sample mean: $\bar{O}_{\text{OOS}} = \frac{1}{K} \sum_{k=1}^K O_{k, \text{OOS}}$ (divisor: $K = \text{folds.len()}$)
4. **Walk-Forward Efficiency**:
   - $\text{WFE} = \begin{cases} \frac{\bar{O}_{\text{OOS}}}{\bar{O}_{\text{IS}}} & \text{if } |\bar{O}_{\text{IS}}| > 10^{-9} \\ 0.0 & \text{if } |\bar{O}_{\text{IS}}| \le 10^{-9} \text{ or } \bar{O}_{\text{IS}} \text{ is NaN} \end{cases}$
5. **Stitched Return Compounding**:
   - Stitched returns: $R_{\text{OOS}} = [r_1, r_2, \dots, r_M]$ (concatenation of fold test returns)
   - Equity compounding: $E_0 = 1.0, \quad E_t = E_{t-1} \cdot (1 + r_t) \quad \forall t \in \{1, \dots, M\}$
   - Stitched metrics: computed via `compute_metrics_from_equity(&equity, ppy, rf)`.

---

## 2. Silent Defaults Inventory

Under M26 rules, a returned zero or default value is permitted only where strictly defined by the mathematical formula. Any missing input, degenerate condition, or undefined statistic must return an explicit `Err`.

| # | File | Line | Code Snippet | Value Returned | Flag / Classification | Rationale & M26 Defect Assessment |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 1 | `crates/spectra-research/src/walkforward.rs` | 67 | `let step = if cfg.step == 0 { cfg.test } else { cfg.step };` | `cfg.test` | **Formula-defined** | Explicitly specified in `WalkForwardConfig.step` contract: *"How far each fold advances; 0 means test."* |
| 2 | `crates/spectra-research/src/walkforward.rs` | 74–76 | `if test_end > n { break; }` | `Ok([])` (empty vector) | **Silent default** | When timestamps $n < \text{train} + \text{embargo} + \text{test}$, `make_folds` terminates without constructing any folds, silently returning `Ok([])` instead of `Err(Error::InvalidArgument)`. |
| 3 | `crates/spectra-research/src/walkforward.rs` | 132 | `if mean_in_sample.abs() > 1e-9 { ... } else { 0.0 }` | `0.0` | **Silent default** | When $|\bar{O}_{\text{IS}}| \le 10^{-9}$, ratio is mathematically undefined ($0/0$ or $x/0$). Silently returns `0.0` instead of `NaN` or error. Furthermore, if `mean_in_sample` is `NaN`, `NaN.abs() > 1e-9` evaluates to `false`, silently returning `0.0` and swallowing NaN values. |
| 4 | `crates/spectra-research/src/walkforward.rs` | 139 | `walk_forward_efficiency(self.mean_in_sample_objective, ...)` | `0.0` | **Silent default** | When `WalkForwardResult` has 0 folds or zero in-sample objective, `efficiency()` delegates to line 132 and silently returns `0.0`. |
| 5 | `crates/spectra-research/src/walkforward.rs` | 168 | `let mut out = WalkForwardResult::default();` | Zero-initialized struct | **Silent default** | Initializes result with all zero fields and empty fold vector; if loop does not run, this unpopulated default is returned directly in `Ok(out)`. |
| 6 | `crates/spectra-research/src/walkforward.rs` | 169 | `let mut next_id = ledger.as_ref().map_or(1, \|l\| l.next_trial_id());` | `1` | **Formula-defined** | Optional state default: when no external trial ledger is supplied, trial IDs begin counting at 1. |
| 7 | `crates/spectra-research/src/walkforward.rs` | 197–201 | `if !out.folds.is_empty() { ... }` | `0.0` | **Silent default** | When `out.folds` is empty, mean objectives are skipped and remain silently `0.0` rather than raising an error for an empty walk-forward run. |
| 8 | `crates/spectra-research/src/walkforward.rs` | 210–211 | `out.oos_metrics = compute_metrics_from_equity(&equity, ...); Ok(out)` | `Ok(out)` with `valid = false` | **Silent default** | When `out.oos_returns` is empty, `equity` is `[1.0]`. `compute_metrics_from_equity` returns `Metrics::default()` with `valid = false` and all 0.0 metrics. `run_walk_forward` returns `Ok(out)` instead of `Err(Error::InvalidArgument("walk-forward: no folds generated"))`. |
| 9 | `crates/spectra-api/src/bin/spectra-worker.rs` | 1963 | `.and_then(\|id\| Uuid::parse_str(id).ok())` | `None` leading to `fail_job` | **Formula-defined** | Idiomatic Option parse; failures are caught and logged with explicit job termination. |
| 10 | `crates/spectra-api/src/bin/spectra-worker.rs` | 1973 | `.and_then(\|id\| Uuid::parse_str(id).ok())` | `None` leading to `fail_job` | **Formula-defined** | Idiomatic Option parse; failures are caught and logged with explicit job termination. |
| 11 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2033 | `job.payload.get("param_grid").cloned().unwrap_or_else(\|\| json!({}))` | `json!({})` | **Silent default** | Missing `param_grid` silently defaults to an empty object instead of failing payload validation. |
| 12 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2034 | `.get("mode").and_then(...).unwrap_or("rolling")` | `"rolling"` | **Silent default** | Unspecified mode silently defaults to `"rolling"` rather than requiring an explicit choice. |
| 13 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2036 | `.get("train_size").and_then(...).unwrap_or(200)` | `200` | **Silent default** | Missing train window size silently defaults to arbitrary 200 bars/timestamps. |
| 14 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2037 | `.get("test_size").and_then(...).unwrap_or(50)` | `50` | **Silent default** | Missing test window size silently defaults to arbitrary 50 bars/timestamps. |
| 15 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2039 | `.get("step_size").and_then(...).unwrap_or(test_size as u64)` | `test_size` | **Formula-defined** | Config-defined: step size defaults to test size when omitted. |
| 16 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2040 | `.get("embargo").and_then(...).unwrap_or(0)` | `0` | **Formula-defined** | Embargo gap defaults to 0 timestamps skipped. |
| 17 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2042 | `.get("objective").and_then(...).unwrap_or("sharpe")` | `"sharpe"` | **Silent default** | Missing optimization objective silently defaults to `"sharpe"`. |
| 18 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2045 | `.get("strategy").and_then(...).unwrap_or("sma_cross")` | `"sma_cross"` | **Silent default** | Missing strategy identifier silently defaults to `"sma_cross"`. |
| 19 | `crates/spectra-api/src/bin/spectra-worker.rs` | 2200 | `optimization::update_walk_forward(..., "succeeded", ...)` | Job marked `"succeeded"` | **Silent default** | Worker records empty walk-forward results (0 folds, invalid metrics, 0.0 efficiency) as a successful job in the database without warning. |

### Summary of Defaults
- **In `crates/spectra-research/src/walkforward.rs`**: **6 flagged silent defaults** (#2, #3, #4, #5, #7, #8) and **2 formula-defined defaults** (#1, #6).
- **In `crates/spectra-api/src/bin/spectra-worker.rs`**: **7 flagged silent defaults** (#11, #12, #13, #14, #17, #18, #19) and **4 formula-defined defaults** (#9, #10, #15, #16).
- **Total Flagged Silent Defaults**: **13** across the walk-forward pipeline (**6** strictly within the `walkforward.rs` research module).

### Top 3 Silent Defaults by Risk

1. **Line 132 in `walkforward.rs` (`if mean_in_sample.abs() > 1e-9 { ... } else { 0.0 }`)**:
   - **Risk**: Masks mathematical singularity ($0/0$ or $x/0$) when in-sample optimization yields near-zero objective, but even more critically, **silently swallows NaNs**. Because IEEE 754 comparisons with NaN return false, `!(NaN.abs() > 1e-9)` evaluates to `false`, directing execution into the `else` branch. Any NaN objective arising from backtest errors, non-finite Sharpe, or zero-variance returns silently produces a clean `0.0` efficiency ratio rather than propagating `NaN` or returning an `Err`.
2. **Lines 74–76 and 210–211 in `walkforward.rs` (Degenerate fold loop and returning `Ok(WalkForwardResult::default())`)**:
   - **Risk**: When a dataset contains fewer timestamps than $\text{train} + \text{embargo} + \text{test}$, `make_folds` returns `Ok([])` and `run_walk_forward` completes with `Ok(WalkForwardResult::default())` (0 folds, 0 trials, empty returns, `valid = false`). Callers receive `Ok`, assuming validation succeeded. In `spectra-worker`, this is persisted to the database as `"succeeded"`, reporting misleading zeroed metrics to users rather than failing with an informative error stating the dataset is too short.
3. **Lines 2036 & 2037 in `spectra-worker.rs` (Payload `.unwrap_or(200)` and `.unwrap_or(50)`)**:
   - **Risk**: Silently invents arbitrary walk-forward window parameters (200 train timestamps, 50 test timestamps) when omitted from API requests. On daily data, this represents ~10 months of training and ~2.5 months of testing; on 1-minute data, it represents ~3 hours of training and ~50 minutes of testing. Running optimizations on unvalidated, silently defaulted window parameters produces arbitrary model selections without caller awareness.

---

## 3. Callers Breakdown

### 3.1 `run_walk_forward` (7 call sites: 5 Rust direct, 1 re-export, 1 C++ parity)
1. `crates/spectra-research/src/lib.rs:45`: Module re-export `pub use walkforward::{run_walk_forward, ...};`.
2. `crates/spectra-api/src/bin/spectra-worker.rs:2098`: Worker execution for DSL strategy walk-forward jobs.
3. `crates/spectra-api/src/bin/spectra-worker.rs:2116`: Worker execution for builtin strategy walk-forward jobs.
4. `crates/spectra-cli/examples/overfitting.rs:99`: Overfitting demonstration CLI running walk-forward validation on synthetic random walk bars.
5. `crates/spectra-research/tests/parity_research.rs:182`: Parity test comparing Rust walk-forward output against C++ golden files.
6. `crates/spectra-research/tests/research.rs:109`: Integration test verifying walk-forward out-of-sample data separation and trial ledger recording.
7. `parity/cpp/research_golden.cpp:131`: C++ QuantForge golden generator running `run_walk_forward`.

### 3.2 `walk_forward_efficiency` (6 call sites: 5 Rust direct, 1 re-export)
1. `crates/spectra-research/src/lib.rs:45`: Module re-export `pub use walkforward::{walk_forward_efficiency, ...};`.
2. `crates/spectra-research/src/walkforward.rs:139`: Internal method implementation of `WalkForwardResult::efficiency(&self)`.
3. `crates/spectra-cli/tests/learn_formula_drift.rs:229`: Formula drift test asserting `wf.efficiency() == walk_forward_efficiency(mean_is, mean_oos)`.
4. `crates/spectra-research/tests/m26_walkforward_reference.rs:78`: Reference test for standard positive efficiency ratio ($0.01 / 0.02 = 0.5$).
5. `crates/spectra-research/tests/m26_walkforward_reference.rs:79`: Reference test for efficiency ratio above unity ($0.03 / 0.02 = 1.5$).
6. `crates/spectra-research/tests/m26_walkforward_reference.rs:81`: Reference test for efficiency with negative in-sample mean ($0.01 / -0.04 = -0.25$).

### 3.3 `WalkForwardResult::efficiency` (3 call sites)
1. `crates/spectra-api/src/bin/spectra-worker.rs:2164`: Worker extracting efficiency ratio from `wf_res` for database update.
2. `crates/spectra-cli/tests/learn_formula_drift.rs:229`: Learn formula drift assertion verifying `wf.efficiency()` against direct formula.
3. `crates/spectra-cli/tests/learn_formula_drift.rs:385`: Learn formula drift table entry recording efficiency ratio calculation.

### 3.4 `make_folds` (13 call sites: 12 Rust direct, 1 re-export)
1. `crates/spectra-research/src/lib.rs:45`: Module re-export `pub use walkforward::{make_folds, ...};`.
2. `crates/spectra-research/src/walkforward.rs:167`: Core generation of fold boundaries inside `run_walk_forward`.
3. `crates/spectra-research/src/walkforward.rs:221`: Unit test verifying rolling folds count and bounds for $n=10$.
4. `crates/spectra-research/src/walkforward.rs:225`: Unit test verifying anchored folds bounds and train start at 0.
5. `crates/spectra-research/src/walkforward.rs:227`: Unit test asserting error when `train == 0` or `test == 0`.
6. `crates/spectra-research/tests/m26_walkforward_reference.rs:34`: Reference test verifying rolling folds slice progression and terminal fold truncation.
7. `crates/spectra-research/tests/m26_walkforward_reference.rs:47`: Reference test verifying anchored folds retain `train_begin == 0`.
8. `crates/spectra-research/tests/m26_walkforward_reference.rs:56`: Reference test verifying step 0 advances by test window length (call 1).
9. `crates/spectra-research/tests/m26_walkforward_reference.rs:57`: Reference test verifying step 0 advances by test window length (call 2).
10. `crates/spectra-research/tests/m26_walkforward_reference.rs:64`: Reference test verifying series shorter than one complete fold ($n=6 < 7$) yields empty folds vector.
11. `crates/spectra-research/tests/m26_walkforward_reference.rs:65`: Reference test verifying minimum series length ($n=7$) yields exactly 1 fold.
12. `crates/spectra-research/tests/m26_walkforward_reference.rs:72`: Reference test verifying zero training window returns error.
13. `crates/spectra-research/tests/m26_walkforward_reference.rs:73`: Reference test verifying zero test window returns error.

### 3.5 API and Database Orchestration Endpoints
1. `crates/spectra-api/src/optimization_api.rs:986`: `queue_walk_forward` enqueues `"walk_forward"` job.
2. `crates/spectra-api/src/optimization_api.rs:1077`: `list_walk_forwards` lists walk-forward reports in workspace.
3. `crates/spectra-api/src/optimization_api.rs:1099`: `get_walk_forward` fetches specific report by ID.
4. `crates/spectra-api/src/engine.rs:83, 140, 161, 252`: Job kind registration and dataset validation for `"walk_forward"`.
5. `crates/spectra-api/tests/optimization_flow.rs:142, 149, 153, 172`: API integration test testing queueing and querying walk-forward reports.
6. `crates/spectra-db/src/optimization.rs:12, 28, 44, 60`: Database CRUD queries (`insert_walk_forward`, `get_walk_forward`, `list_walk_forwards`, `update_walk_forward`).

**Total Call Sites across `crates/`, `tests/`, and `examples/`**: **25 direct function call sites** (or **28** including re-exports in `lib.rs`; **29** including C++ parity).

---

## 4. Golden and Fixture Files

The following reference and fixture test files directly test or rely on the behaviour of walk-forward functions, including undefined and degenerate cases:

1. **`crates/spectra-research/tests/m26_walkforward_reference.rs`**:
   - Lines 12–13 state explicitly:
     ```rust
     //! Not covered here: the 1e-9 zero-denominator case is a documented defect (task 8: it
     //! should be "not defined"), so only the ratio itself is tested.
     ```
     This confirms that the `else { 0.0 }` branch in `walk_forward_efficiency` is an acknowledged M26 Task 8 defect.
   - Lines 61–66 (`test too_short_a_series_gives_no_folds`):
     ```rust
     assert!(make_folds(6, &cfg(WalkForwardMode::Rolling, 2)).unwrap().is_empty());
     ```
     Explicitly asserts that when $n$ is too short to complete even one fold ($n = 6 < \text{train}(4) + \text{embargo}(1) + \text{test}(2) = 7$), `make_folds` silently returns an empty vector without an error.
   - Lines 68–74 (`test zero_train_or_test_windows_are_refused`): Asserts that `train == 0` and `test == 0` return `Err`.
   - Lines 77–82 (`test efficiency_is_the_out_of_sample_mean_over_the_in_sample_mean`): Tests standard and negative in-sample ratios, avoiding denominator $\le 10^{-9}$.

2. **`parity/golden/research.golden` & `crates/spectra-research/tests/parity_research.rs`**:
   - Golden output generated by `parity/cpp/research_golden.cpp` containing exact outputs for 3 walk-forward scenarios:
     - **"noise, long only"** (lines 96–105): 1000 bars, train 300, test 100, embargo 0, Rolling $\to$ 7 folds, 413 trials, ledger 472.
     - **"drift, long/short, two instruments"** (lines 286–293): 600 bars (300 timestamps for 2 instruments), train 200, test 80, embargo 5, Anchored $\to$ 4 folds, 236 trials, ledger 295. Golden records `wf_mean_is 0.86877618143695623`.
     - **"short sample"** (lines 472–478): 160 bars, train 60, test 30, embargo 0, Rolling $\to$ 3 folds, 177 trials, ledger 236. Golden records `wf_mean_is 2.2152692910118246`, `wf_mean_oos 0.00043712861599728053`.

3. **`crates/spectra-research/tests/research.rs`**:
   - Line 105 (`test walk_forward_never_trains_on_test_data_and_records_every_trial`): Runs rolling walk-forward on 800 random-walk bars (train 200, test 100, embargo 10, step 100). Verifies that 5 folds are generated, that `fold.train_end + 10 == fold.test_begin` across all folds, and that all 80 trials are logged in the ledger.

4. **`crates/spectra-cli/tests/learn_formula_drift.rs`**:
   - Lines 223–230: Asserts parity between `WalkForwardResult::efficiency()` and direct formula call `walk_forward_efficiency(mean_is, mean_oos)`.
   - Lines 384–386: Drift assertion comparing `mean_oos / mean_is` with `wf.efficiency()`.

5. **`crates/spectra-api/tests/optimization_flow.rs`**:
   - Integration test `walk_forward_flow` exercising job creation, execution, and REST report retrieval via `/api/v1/optimization/walkforward`.
   - Integration test `walk_forward_missing_window_field_returns_400` verifying that requests missing `train_size` or `test_size` fail with HTTP 400 naming the missing field.

---

## 5. Divergence Records (M26 Enforced Rules)

Summary of divergences between the legacy behavior (returning default numbers, empty structs, or silent fallbacks) and the M26 enforced behavior (explicit errors and strict payload validation):

| Function / Component | Case / Input Condition | Old Value | New Behavior | Rationale / Mathematical Rule |
| :--- | :--- | :--- | :--- | :--- |
| `make_folds` | Timestamps cannot fit one fold ($n < \text{train} + \text{embargo} + \text{test}$) | `Ok([])` (empty vector) | `Err(Error::InvalidArgument("walk-forward: timestamps ({n}) cannot fit one fold..."))` | A walk-forward validation run requires at least one complete fold; silently returning an empty vector masked insufficient sample lengths. |
| `walk_forward_efficiency` | Undefined ratio: zero or flat in-sample objective ($|\bar{O}_{\text{IS}}| \le 10^{-9}$) | `0.0` (silent fallback) | `Err(Error::InvalidArgument("walk-forward: undefined efficiency with zero or flat in-sample objective"))` | Ratio is mathematically undefined ($x / 0$). Returning 0.0 conflated zero performance with an undefined statistic. |
| `walk_forward_efficiency` | NaN in objective input ($\bar{O}_{\text{IS}}$ or $\bar{O}_{\text{OOS}}$ is NaN) | Silently returned `0.0` when $\bar{O}_{\text{IS}} = \text{NaN}$ | `Err(Error::InvalidArgument("walk-forward: NaN objective in efficiency calculation"))` | Masked data corruption, failed optimizations, or invalid Sharpe ratios as a clean numeric efficiency of 0.0. |
| `WalkForwardResult::efficiency` | Default or zero in-sample objective ($\bar{O}_{\text{IS}} = 0.0$) | `0.0` | `Err(Error::InvalidArgument("walk-forward: undefined efficiency with zero or flat in-sample objective"))` | Delegates to `walk_forward_efficiency` and propagates error instead of returning 0.0. |
| `run_walk_forward` | Timestamps cannot fit one fold ($n_{\text{ts}} < \text{train} + \text{embargo} + \text{test}$) | `Ok(WalkForwardResult::default())` (0 folds, invalid metrics) | `Err(Error::InvalidArgument)` propagated from `make_folds` | Prevents returning an `Ok` result when no folds or returns were produced. |
| `POST /api/v1/optimization/walkforward` | Missing window parameters (`train_size` or `test_size`) | 422 or worker default (`unwrap_or(200)`, `unwrap_or(50)`) | HTTP 400 Bad Request naming the missing field | Window sizes are fundamental experimental parameters that must be provided explicitly by the client. |
| `spectra-worker.rs` | Missing payload properties (`mode`, `param_grid`, `objective`, `strategy/dsl_code`) | Silently defaulted via `unwrap_or` | Explicit job failure via `fail_job` naming the missing property | Ensures queued worker tasks fail loudly on malformed payloads rather than proceeding under arbitrary defaults. |
| `spectra-worker.rs` | Empty folds produced by walk-forward | Marked job as `"succeeded"` and stored 0 folds | Checks `wf_res.folds.is_empty()`, marks report as `"failed"`, and calls `fail_job` | A validation run with 0 folds is a pipeline failure and must not be reported as a success. |

### Detailed Divergence Records by Case

#### 1. `make_folds()` — Insufficient Timestamps for Single Fold
- **Function:** `make_folds`
- **Input:** $n < \text{train} + \text{embargo} + \text{test}$ (e.g. $n = 0$, $n = 1$, or $n = 6$ with window requirement 7)
- **Old value:** `Ok([])` (empty vector)
- **New behavior:** `Err(Error::InvalidArgument("walk-forward: timestamps ({n}) cannot fit one fold (need at least {required})"))`

#### 2. `walk_forward_efficiency()` — Undefined Efficiency with Flat/Zero In-Sample Objective
- **Function:** `walk_forward_efficiency`
- **Input:** $|\bar{O}_{\text{IS}}| \le 10^{-9}$ (e.g. `mean_in_sample = 0.0` or `1e-10`)
- **Old value:** `0.0`
- **New behavior:** `Err(Error::InvalidArgument("walk-forward: undefined efficiency with zero or flat in-sample objective"))`

#### 3. `walk_forward_efficiency()` — NaN Objective Inputs
- **Function:** `walk_forward_efficiency`
- **Input:** `mean_in_sample.is_nan() || mean_out_of_sample.is_nan()`
- **Old value:** `0.0` (when `mean_in_sample` is NaN, because `!(NaN.abs() > 1e-9)` evaluated to false)
- **New behavior:** `Err(Error::InvalidArgument("walk-forward: NaN objective in efficiency calculation"))`

#### 4. `WalkForwardResult::efficiency()` — Propagation of Undefined Result
- **Function:** `WalkForwardResult::efficiency`
- **Input:** Result where `mean_in_sample_objective` is 0.0 or NaN
- **Old value:** `0.0`
- **New behavior:** Returns `Result<f64>` containing `Err(Error::InvalidArgument)`

#### 5. `run_walk_forward()` — Propagation of Insufficient Data Error
- **Function:** `run_walk_forward`
- **Input:** `bars` slice where unique timestamps count cannot fit one fold
- **Old value:** `Ok(WalkForwardResult::default())` with `folds.is_empty()`, `oos_metrics.valid = false`, all fields `0.0`
- **New behavior:** Returns `Err(Error::InvalidArgument)` propagated via `?` from `make_folds`

#### 6. `queue_walk_forward` API Endpoint — HTTP 400 for Missing Window Fields
- **Endpoint:** `POST /api/v1/optimization/walkforward`
- **Input:** JSON payload omitting `train_size` or `test_size` (or supplying `<= 0`)
- **Old value:** Returned 422 Unprocessable Entity or relied on worker fallback (`unwrap_or(200)`, `unwrap_or(50)`)
- **New behavior:** Returns HTTP 400 Bad Request with error body naming the missing/invalid field (`train_size` or `test_size`)

#### 7. `spectra-worker.rs` — Elimination of Silent Defaults in Job Execution
- **Component:** `run_walk_forward_job` in `spectra-worker.rs`
- **Input:** Job payload missing `param_grid`, `mode`, `objective`, or `strategy`/`dsl_code`
- **Old value:** Replaced missing values with `"rolling"`, `json!({})`, `"sharpe"`, `"sma_cross"`
- **New behavior:** Calls `fail_job` immediately, logging the missing field and marking the job as failed

#### 8. `spectra-worker.rs` — Elimination of False Succeeded Status for Empty Folds
- **Component:** `run_walk_forward_job` in `spectra-worker.rs`
- **Input:** Walk-forward result containing empty folds
- **Old value:** Recorded `optimization::update_walk_forward(..., "succeeded", ...)` with empty folds and `eff_ratio = 0.0`
- **New behavior:** Checks `wf_res.folds.is_empty()`, marks report as `"failed"`, and calls `fail_job`
