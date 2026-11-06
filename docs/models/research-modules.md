# Research modules — reference (M26 task 2, second batch)

Code: `crates/spectra-research/src/` as named. Statistics and PBO are in `statistics.md`.
Sign-off: pending (product owner, M26 decision 3). Reference-value tests: task 3.

## Benchmark-relative metrics (`benchmark.rs`)

Strategy returns s and benchmark returns b, aligned, n ≥ 2, periods per year ppy.

- Beta = cov(s, b) / var(b). Correlation = cov / √(var(s)·var(b)), 0 if var(s) = 0.
- Tracking error = sample stdev(s − b) × √ppy (annualized). Information ratio = mean active return / tracking error × √ppy (0 when tracking error ≤ 1e-15).

**Edge cases (as coded):** fewer than 2 points, or ppy ≤ 0, or zero benchmark variance → returns
the default metrics (all zero) with no error. A beta of 0 is then indistinguishable from a
measured zero.

**Known problems (task 8):** silent zeros on invalid input. Should return an error or an explicit
"not defined".

## Walk-forward (`walkforward.rs`)

- Folds: `make_folds(n, cfg)` splits n observations into in-sample and out-of-sample windows.
- **Efficiency** = mean OOS / mean IS. Returns 0.0 when |mean IS| ≤ 1e-9.

**Known problems (task 8):** returns 0.0 for a near-zero in-sample mean, which reads as "no
efficiency" when the ratio is undefined. Should be "not defined".

## Block bootstrap (`trades.rs`: `block_bootstrap_returns`, `resample_trades`)

**Method.** Circular moving-block bootstrap. Blocks of length L start at uniformly random
positions; each resample is built from ⌈n/L⌉ blocks, truncated to n. Each resample compounds its
returns from `initial_equity` to a terminal return and a max drawdown; Sharpe per resample is
annualized with √ppy.

**Inputs:** returns (n ≥ 2), L in [1, n], resamples ≥ 1, initial equity > 0, ppy > 0, all returns
finite and > −1.

**Edge cases:** any invalid input returns an empty result with `valid = false`. This is correctly
flagged, unlike the metrics above.

**Note:** the RNG is seeded, so results are reproducible for a fixed seed (seed-dependent output
must be tested against the parity golden, not against an independent value).

## Attribution (`attribution.rs`: `compute_pnl_attribution`)

Breaks P&L into components from the ledger journal:
- Fees: sum of fill fees.
- Borrow fees: sum of borrow entries.
- Financing: sum of **negative** interest entries only; interest income: sum of positive ones.
- Dividends and coupons: sums of their entries (memo lines: they are already in trade gross).
- Slippage cost = total notional × (slippage bps / 10,000). A memo line: fill prices already
  include slippage, so it is not in `total_costs` or the waterfall.
- `total_costs` = commissions + borrow fees + financing interest (costs charged).
- Per-instrument and per-direction aggregation of gross, fees, net, win and loss counts (closed
  trades).

**Waterfall (M11 Task 3, decided by the product owner 6 Nov 2026).** A two-stage bridge, summed
in fixed point so both identities are exact:

```
Gross Trading P&L                     (closed trades: price P&L + dividends + coupons while held)
− Fees on Closed Trades
= Trading Net P&L                     = total_net_pnl
+ Open Positions (Realized So Far)    (partial closes, dividends, coupons on open positions, less their fees)
− Borrow Fees
− Financing Interest
+ Interest Income
= Net Account P&L                     = total_account_pnl
```

Zero steps are left out; the start and the two subtotals (`subtotal: true`) are always there.
Unrealized P&L is in neither total. `total_account_pnl` equals the journal's realized P&L:
fills' and expiries' realized P&L, plus dividends, coupons and interest, less fees and borrow
fees.

**Divergence (M11 Task 3):** the old bridge started from closed-trade gross but then subtracted
every fill's fee (open positions too), subtracted the slippage estimate (already in prices),
added dividends and coupons a second time, and left out interest income. Its last line did not
equal `total_net_pnl`: in the M26 reference scenario it ended at 939 against a net of 990.
`total_costs` there drops from 61 to 30 (slippage no longer counted). Tests:
`crates/spectra-research/tests/m11_attribution_totals.rs` (13, including 300 random ledgers),
and the backtest summaries of the futures, bonds, options, corporate-actions and service tests.

**Known problems (task 8, plus a check):**
1. **Unknown slippage defaults to zero.** `slippage_bps.unwrap_or(0.0)` (`attribution.rs`): a
   report without a slippage figure says "no slippage cost", not "unknown".
2. **Positive interest was previously dropped (fixed in Task 4).** See divergence below.

**Divergence (M26 task 4):**
- **Positive interest reported as interest income:** Previously, positive `EntryType::Interest` entries were dropped completely, so a strategy earning cash deposit interest had that income omitted from the attribution breakdown. Now, positive interest entries are accumulated in `CostDecomposition::interest_income` (reported in the `costs` JSON object as `"interest_income"`), while `financing_interest` remains strictly negative financing charges. In the reference test scenario with 45.0 positive interest:
  - Old behaviour: `interest_income` was omitted/0.0, and positive interest vanished from the report.
  - New behaviour: `interest_income` is 45.0, `financing_interest` is 0.0, and `total_net_pnl` is unchanged (990.0).

## Execution gap (`gap.rs`)

Compares paper fills with backtest fills per matched trade. Default thresholds:

| Threshold | Default | Alert when |
|---|---|---|
| Slippage drift | 10 bps | drift above 10 bps |
| Timing latency | 1,000 ms | latency above 1 s |
| Tracking error | 3% | tracking error above 3% |
| Fill ratio | 0.95 minimum | fill ratio below 0.95 |

Each breach is a `GapAlert` with a code. Tests: 2.

## Strategy Survival Lab (`survival.rs`)

Tries to break one strategy configuration. Nine components, each with its own numbers and a
verdict (Pass, Warn, Insufficient, Fail): 1 cost sensitivity, 2 execution delay, 3 parameter
stability, 4 time-period robustness, 5 trade bootstrap, 6 sample size and concentration, 7 regime
sensitivity, 8 benchmark-relative, 9 drawdown and tail risk. Thresholds are rules of thumb, not
statistical tests. **By design there is no aggregate score**; the report's `worst` field is the
most severe component verdict.

**Known problems:** thresholds are not statistical tests and have no reference values. Task 3
must test each component against a case with a known answer. Output text is identical to the
C++ reference, so changes here show up in the parity golden.

## Sweep and trial ledger (`sweep.rs`)

Covered in `statistics.md`: the trial Sharpe variance used by DSR (sample variance, n − 1).

## Test requirements for task 3

- Benchmark: a strategy equal to the benchmark has beta 1, correlation 1, tracking error 0.
- Walk-forward: efficiency matches the ratio on a fixed fold set; zero IS mean is an error (task 8).
- Bootstrap: fixed seed gives the same output across runs; L = 1 reproduces i.i.d. resampling
  statistics; invalid inputs return `valid = false`.
- Attribution: components sum to total P&L on a ledger with fees, borrow, dividends and interest
  income (done in M11 Task 3); unknown slippage is an error (task 8, done in #206).
- Gap: each threshold fires exactly at its boundary.
- Survival: each component's verdict on a fixed case with a known answer.
