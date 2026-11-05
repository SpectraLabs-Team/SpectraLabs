# Covariance, concentration, stress and pre-trade limits — reference (M26 task 2, last batch)

Code: `crates/spectra-risk/src/returns.rs` (covariance, correlation, Cholesky),
`portfolio.rs` (gross weights, HHI), `engine.rs` (stress scenarios), `enforcement.rs`
(pre-trade checks and limit utilization). Sign-off: pending (product owner, M26 decision 3).
Reference-value tests: task 3.

## Covariance and correlation (`returns.rs`)

- **Covariance** = Σ(x_i − x̄)(y_i − ȳ) / (n − 1). Errors when lengths differ or n < 2.
- **Correlation** = cov / (s_x · s_y), clamped to [−1, 1] to absorb rounding (silent clamp).
  Errors when either series has stdev ≤ 1e-12.
- **Correlation matrix** from several series: pairwise correlations. Errors when series lengths differ.

## Cholesky factor (`returns.rs`)

L with A = L·Lᵀ for a symmetric matrix A. Errors when A is not square, or a pivot is ≤ 0 (not
positive definite).

**Silent change to the input:** a diagonal jitter of 1e-8 is added when the matrix is positive
semi-definite but nearly singular (documented in the function, but the caller is not told).
The factor is then of A + 1e-8·I, not A. The size of the jitter should be reported, or the call
should fail for a matrix that needs it.

## Concentration: gross weights and HHI (`portfolio.rs`)

- **Gross weight** w_i = |value_i| / Σ|value_j|. Empty when total gross exposure is zero.
- **HHI** = Σ w_i², in [1/n, 1]. 1 means all exposure in one position.
- Errors when the portfolio has no exposure.

**Divergence (M26 task 4):**
- **Herfindahl-Hirschman Index (HHI) concentration calculation:**
  - **Old behaviour:** The legacy C++ engine returned a hard-coded `1.0` for HHI regardless of portfolio weights (documented in `PORTING.md:85` and the doc comment in `crates/spectra-risk/src/portfolio.rs:100-111`; no C++ engine source is in the repository).
  - **New behaviour:** Rust computes the Herfindahl index from gross position weights: $\text{HHI} = \sum_i w_i^2$, in range $[1/n, 1]$ where $n$ is the number of active positions. Example: two equal positions give $0.5$, four equal positions give $0.25$, and a single position gives $1.0$.
  - **Reference test values:**
    - Concrete example in `crates/spectra-risk/tests/risk.rs:147` (`hhi_comes_from_the_weights`, two equal positions: 1.0 @ 50.0 and -1.0 @ 50.0, asserts 0.5):
      - Old C++ value: `1.0` (hard-coded stub)
      - New Rust value: `0.5`
      - Assertion line:
        ```rust
        assert!((p.hhi().unwrap() - 0.5).abs() < 1e-15);
        ```
    - Four equal positions in `crates/spectra-risk/tests/m26_risk_controls_reference.rs` (`hhi_of_equal_positions_is_one_over_n`, assets A, B, C, D each 1.0 @ 100.0):
      - Old C++ value: `1.0` (hard-coded stub)
      - New Rust value: `0.25` ($\sum_{i=1}^4 (0.25)^2 = 4 \times 0.0625 = 0.25$)
      - Assertion line:
        ```rust
        close(p.hhi().unwrap(), 0.25, TOL, "HHI of four equal positions");
        ```
    - Single position in `crates/spectra-risk/tests/m26_risk_controls_reference.rs` (`hhi_of_one_position_is_one`, asset A 10.0 @ 50.0):
      - Old C++ value: `1.0`
      - New Rust value: `1.0`
      - Assertion line:
        ```rust
        close(p.hhi().unwrap(), 1.0, TOL, "HHI of one position");
        ```
  - **Parity golden status:** The frozen golden has no HHI line (`git grep -n -i "hhi" parity/` returns nothing), so no golden line changes in this PR.

## Stress scenarios (`engine.rs`)

Six named scenarios, each a **single price multiplier applied to every position**:

| Name | Factor | Description in code |
|---|---|---|
| market-crash | 0.70 | every price falls 30% |
| mild-correction | 0.90 | every price falls 10% |
| covid-crash-2020 | 0.66 | March 2020 shock (−34%) |
| gfc-2008 | 0.52 | 2008 financial crisis (−48%) |
| tech-bubble-2000 | 0.51 | 2000 dot-com crash (−49%) |
| black-monday-1987 | 0.77 | October 1987 (−23%) |

The stress test revalues the shocked portfolio and reports its VaR, the value before and after,
and the scenario loss (before − after).

**Known problems:**
1. **The named historical scenarios are not historical.** They apply one uniform factor to every
   asset. Real 2008 or 2020 moves differed by sector, country and asset class, and a uniform
   shock hides concentration and correlation effects. The names and descriptions should say
   "uniform shock of X%", or the historical paths should be used.
2. **Unknown scenario** is an error (good), but the error message lists the known names as a
   hardcoded string, which can drift from the `SCENARIOS` array.

## Pre-trade limits and utilization (`enforcement.rs`)

**Kill switch.** When active, opening orders are rejected. Closing orders (buying a short,
selling a long) are allowed. A closing order larger than the position is reduced to the position
size if size reduction is enabled. Rejections return `approved_qty = 0`.

**Limit utilization.** Each limit as a fraction of its maximum: gross exposure, net exposure,
leverage, drawdown, daily loss, VaR. Fraction = value / limit; **0 when the limit is not set or is
0**.

**VaR as a fraction of equity** (`var_pct_with_model`): 1-day 99% VaR of the positions, divided by
equity. **0 when equity ≤ 0 or there are no positions.**

**Known problems (task 7–8):**
1. **A limit of 0 reads as 0% used.** `utilization` returns 0 when `limit ≤ 0`. A limit of 0
   means "no exposure allowed", so a position against it is a 100%-plus breach, not 0%. The two
   meanings of 0 (unset, and zero allowed) must be separated.
2. **Positions with a price ≤ 0 are dropped from VaR** (`pos.price > 0.0` filter). An asset with
   a missing or zero price contributes no risk, with no error or flag.
3. **VaR is 0 when equity is ≤ 0.** A portfolio with negative equity, the most dangerous case,
   reports zero VaR.
4. **Kill-switch rejection reports `concentration_pct: 0.0`** and other projected metrics as zero
   placeholders, not as computed values. Callers that display them show false zeros.

**Divergence (M26 task 9): pre-trade audit inserts fail closed.**
- **Old behaviour:** the paper order paths (the checked order, the kill-switch rejection and
  each order of a paper strategy run) and the broker order path (rejected, and approved or
  reduced) called `record_decision` as `let _ = …`: when the audit insert failed, the order was
  still placed (or the rejection still returned) with no decision in `risk_decisions`.
- **New behaviour:** if the decision cannot be recorded, the order is not placed or stored and
  the API returns 500 ("The pre-trade risk decision could not be recorded, so the order was not
  placed."), logged as an error. In a paper strategy run the decision is now recorded before
  the order row, so a failed insert stores no order for it (orders earlier in the same run are
  already stored, as with any other database error mid-run).
- **Test:** `crates/spectra-api/tests/paper_trading_flow.rs`,
  `an_order_whose_decision_cannot_be_recorded_is_not_placed` (a trigger scoped to the test's
  workspace makes the insert fail: the checked paper order, the kill-switch paper order and the
  broker order all return 500, and no paper order is stored).
- The pre-trade check endpoint (`POST /api/v1/risk/check`) places no order and is unchanged.

## Test requirements for task 3

- Covariance and correlation against hand-computed values on a fixed pair of series; correlation
  of a series with itself is 1.
- Cholesky: L·Lᵀ reproduces A; a non-PSD matrix is an error; the jitter case is reported (task 8).
- HHI: equal weights of n positions give 1/n; a single position gives 1.
- Stress: a uniform 0.7 shock on a portfolio reduces value by 30% exactly; the VaR of the shocked
  portfolio matches the scaled analytic VaR.
- Pre-trade: kill switch rejects an opening order and allows a closing one; a closing order larger
  than the position is reduced to the position size.
- Utilization: a limit of 0 with a non-zero exposure is a breach, not 0%; equity ≤ 0 is an error
  for VaR (task 8).
