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

**Divergence from the C++ engine (already in code):** the doc comment says the C++ engine returned
a hard-coded 1.0. The Rust code computes the index. The golden files will show the C++ value for
any portfolio with more than one position, so a parity test that covers HHI will differ by design.
This is the first documented divergence under the parity rule in the M26 doc: record it there.

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
