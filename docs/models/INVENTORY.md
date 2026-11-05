# Model and statistic inventory (M26 task 1)

Checked against `origin/main` at `73fe1f1`, 2026-11-01. Scope: the model and statistic functions
in `spectra-research`, `spectra-risk` and `spectra-engine`, plus the statistic callers in
`spectra-api` and `spectra-registry`. Helpers, types and plumbing are left out.

**Test column:** "Ref" = reference-value test against an independent value. "Parity" = matches the
frozen C++ engine's output (not an independent check). "Sanity" = property checks only (range,
sign, limit). "None" = no test found. Counts are `#[test]` functions in the module.

## spectra-research

| Model / statistic | Function(s) | Formula source (verified in code) | Doc | Tests |
|---|---|---|---|---|
| Normal CDF, erfc, inverse CDF | `stats.rs`: `normal_cdf`, `erfc`, `normal_inv_cdf` | Acklam + Halley step | `statistics.md` | Sanity (2 in stats.rs) |
| Moments (mean, stdev, skew, raw kurtosis) | `stats.rs`: `moments` | Population skew/kurt; sample stdev | `statistics.md` | Sanity |
| Per-period Sharpe | `stats.rs`: `sharpe_per_period` | mean / stdev | `statistics.md` | Parity only |
| PSR | `stats.rs`: `probabilistic_sharpe` | Bailey & López de Prado 2012 | `statistics.md` | Sanity |
| Expected max Sharpe | `stats.rs`: `expected_max_sharpe` | Bailey & López de Prado 2014 | `statistics.md` | Sanity |
| DSR | `stats.rs`: `deflated_sharpe` | PSR vs expected max | `statistics.md` | Parity (golden DSR 0.688) |
| Minimum track record length | `stats.rs`: `min_track_record_length` | Bailey & López de Prado 2012 | `statistics.md` | Sanity |
| Trial Sharpe variance | `sweep.rs`: `TrialLedger::sharpe_variance` | Sample variance (n−1) | `statistics.md` | 2 |
| CSCV PBO | `pbo.rs`: `cscv_pbo` | Bailey et al. 2016 (CSCV) | `research-modules.md` | 3 (sanity), parity golden |
| Validation report | `validation.rs`: `validate_selection` | Calls the stats above | `statistics.md` | None (parity via golden) |
| Benchmark metrics (beta, correlation, tracking error) | `benchmark.rs`: `compute_benchmark_metrics` | OLS beta; cov/var; tracking error = stdev of active | `research-modules.md` | 1 |
| Walk-forward efficiency | `walkforward.rs`: `walk_forward_efficiency` | Ratio of OOS mean to IS mean | `research-modules.md` | 1 |
| Block bootstrap of returns | `trades.rs`: `block_bootstrap_returns` | Block resampling, seeded | `research-modules.md` | 4 (trades.rs) |
| Trade statistics | `trades.rs`: `compute_trade_stats` | Counts, hit rate, averages | `research-modules.md` | (in trades.rs) |
| P&L attribution | `attribution.rs`: `compute_pnl_attribution` | Decomposition by instrument and cost | `research-modules.md` | 1 |
| Survival lab | `survival.rs`: `run_survival_lab` | Composite of cost, delay, regime and bootstrap checks | `research-modules.md` | 4 (+ parity golden) |
| Execution gap | `gap.rs`: `GapReport` | Paper vs backtest fill differences | `research-modules.md` | 2 |

## spectra-risk

| Model / statistic | Function(s) | Formula source | Doc | Tests |
|---|---|---|---|---|
| VaR, CVaR (empirical) | `metrics.rs`: `value_at_risk`, `conditional_var` | Type-7 quantile; tail mean | `risk-metrics.md` | Parity (`risk.golden`); `risk.rs`: 1 on a known sequence |
| Volatility, downside deviation, skew, excess kurtosis | `metrics.rs` | Sample stdev; above-mean RMS; m3/s³; m4/s⁴ − 3 | `risk-metrics.md` | Parity |
| EVT (POT, GPD) | `evt.rs`: `EvtTailModel::fit`, `var`, `cvar` | Hosking–Wallis PWM; GPD VaR and CVaR | `risk-metrics.md` | 1 |
| GARCH(1,1), GJR, EGARCH | `garch.rs` | Gaussian QMLE by coordinate search | `volatility-regime-copula.md` | 4 |
| Regime models (HMM 2 and 3 state, jump model) | `regime.rs` | Baum–Welch; Viterbi; penalized segmentation | `volatility-regime-copula.md` | 4 |
| Copula simulation and component CVaR | `copula.rs` | Cholesky Gaussian; Student-t; Clayton | `volatility-regime-copula.md` | 4 |
| Monte Carlo engine and convergence | `engine.rs`, `simulation.rs` | Simulated losses; relative standard error | `volatility-regime-copula.md` | Parity (`risk.golden`); `risk.rs`: 21 `#[test]` functions in total |
| Stress scenarios | `engine.rs`: `scenario`, `stress_test` | Uniform price multipliers (not historical paths) | `risk-controls.md` | Parity |
| Covariance, correlation, Cholesky | `returns.rs` | Sample (n−1); Cholesky with 1e-8 jitter | `risk-controls.md` | 1 |
| Portfolio concentration (HHI) | `portfolio.rs`: `hhi` | Sum of squared gross weights (C++ returned 1.0) | `risk-controls.md` | `risk.rs`: 1 |
| Pre-trade limits | `enforcement.rs`: `PreTradeChecker::evaluate`, `utilization`, `var_pct_with_model` | Kill switch; fraction of limit | `risk-controls.md` | None found |

## spectra-engine

| Model / statistic | Function(s) | Formula source | Doc | Tests |
|---|---|---|---|---|
| European option price, Greeks, implied vol | `options.rs` | BSM with dividend yield | `options.md` (signed off) | Ref (5a), 5 + 7 |
| Performance ratios (Sortino, Omega, Calmar, Sterling, Burke, CAGR, drawdowns, rolling Sharpe) | `performance.rs` | Formulas in each doc comment (verified) | `engine-models.md` (after task 5) | 3 |
| Bond price, yield, duration, convexity, DV01 | `bonds.rs`: `price_from_yield`, `yield_from_price`, `risk` | Discounting at yield compounded `frequency`/yr; Newton with bisection fallback | `engine-models.md` | 6 |
| Interest rate swaps (fixing, net coupon, mark) | `swaps.rs`: `net_coupon`, `mark` | 100·(fixed − fixing)·τ; discounted float leg | `engine-models.md` | 6 |
| Yield curve: discount factors, zero rates, bootstrap, z-spread | `curves.rs`: `df`, `zero_rate`, `bootstrap`, `z_spread` | Log-linear interpolation of discount factors (verified); bootstrap from quotes | `engine-models.md` | 4 |
| Futures roll and continuous series | `futures.rs`: `roll_ts`, `Continuous` | Roll n business days before expiry | `engine-models.md` | 5 |
| Transaction costs and slippage | `costs.rs`: `commission_fee`, `slipped_price`, `effective_slippage_bps` | Commission = max(min fee, unit + bps); slippage bps plus half spread plus volume term | `engine-models.md` (task 6) | 4 |
| Rebalancing | `rebalance.rs`: `due`, `trade_for` | Target weights to quantities | `engine-models.md` (task 7) | 1 |

## Statistic callers outside the model crates

| Caller | What it reports | Issue |
|---|---|---|
| `spectra-api/src/optimization_api.rs:1148–1152` | PSR, DSR, MinTRL, expected max Sharpe, PBO | Benchmark defaults to 0.0 (`unwrap_or(0.0)`); PBO error becomes 0.0 |
| `optimization_api.rs:1387` | DSR per evaluation | Hardcoded `0.05` as variance; NaN DSR becomes 0.0 |
| `spectra-api/src/bin/spectra-worker.rs:1673–1679` | PSR, DSR, MinTRL, PBO stored with runs | PBO error becomes 0.0 |
| `spectra-registry/src/vault.rs:343–369` | PSR, DSR in the vault | Evaluation count used as trial count |
| `spectra-api/src/paper_api.rs:573, 718, 1050` | Paper fill base price | Falls back to 100.0 when no price known |

## Findings

1. **Parity tests pin current behaviour.** `parity_research.rs` and `parity_risk.rs` compare the
   Rust output to a frozen C++ engine (`parity/golden/research.golden`, `risk.golden`). The golden
   files contain DSR, PBO and risk values produced by the C++ code. Any fix to a formula will
   break these tests unless the golden files are regenerated, and the C++ engine is frozen.
   **Decision needed:** after the audit, do we (a) keep the C++ golden as the reference and
   accept that Rust diverges where it was fixed, or (b) regenerate the golden files from the
   fixed Rust code and retire the C++ parity? This affects every fix in tasks 4, 7 and 8.
2. **Hardcoded Sharpe variance** `0.05` at `optimization_api.rs:1387`. Not in the first grep.
3. **PBO errors become 0.0** at `optimization_api.rs:1152` and `spectra-worker.rs:1679`.
4. **Paper fill price falls back to 100.0** in `paper_api.rs` (three places).
5. **Silent zero targets in rebalancing**: `rebalance.rs:147` and `:161` use `unwrap_or(0.0)` for
   a missing target weight, so a missing target sells the position with no error. This is task 7.
6. **Zero bar volume disables volume slippage** silently: `costs.rs` `effective_slippage_bps`
   adds the volume term only when bar volume is positive, so zero volume means zero volume cost.
7. **Benchmark returns zeros silently** for fewer than 2 points or zero benchmark variance
   (`benchmark.rs`). A beta of 0 is reported as if measured.
8. **Walk-forward efficiency returns 0.0** when in-sample mean is near zero (`walkforward.rs`).
9. **Futures roll gives up silently** after 400 days of guard (`futures.rs`: `roll_ts`).
10. **Silent defaults in models** (EVT clamp and fallback, regime clamps, Clayton and Student-t
    clamps, GARCH last-variance default): see `risk-metrics.md` and
    `volatility-regime-copula.md`.
11. **Excess vs raw kurtosis**: `moments().kurt` is raw; `performance::excess_kurtosis` is excess.
    Keep the names explicit.

## Silent defaults check (M26 task 9)

`scripts/check_silent_defaults.py` runs in the CI lint job and in `scripts/ci-local.sh`. It
scans the non-test code (it stops at `#[cfg(test)]` and skips `tests/`) of spectra-core,
spectra-data, spectra-engine, spectra-research, spectra-risk and spectra-backtest for:

- `.unwrap_or(<number>)`, `.unwrap_or(f64::…)`, `.unwrap_or(<Type>::ZERO|ONE|MAX|MIN|INFINITY|NAN…)`
  (including `i64::MAX` and other integer types)
- `.unwrap_or_default()`
- `.map_or(<number>, …)`
- `.ok().unwrap_or…`

and spectra-api for literal costs: `Bps::from_f64(<number>)` or `Money::from_f64(<number>)`
outside a named `const`.

Every hit must be in `scripts/silent_defaults_allowlist.txt`, and every entry there must still
match a hit, so the list cannot go stale. `--self-test` checks the script on a small fixture
(CI runs it first).

**Adding an entry.** First ask whether the default should be an error instead. If it is
approved, add one line:

```
crates/<crate>/src/<file>.rs | <category> | <code text> | <reason>
```

The code text is a substring of the flagged line (matched with the file, not the line number,
so edits elsewhere do not break it). Keep it specific enough to name one site. The reason may
not contain ` | `. Categories:

| Category | Meaning | The reason says |
|---|---|---|
| `defined` | The value is defined by the formula or data structure (no position = quantity 0, an empty list) | why that value is the right one |
| `parity` | An approved divergence from the C++ golden | what was approved and when |
| `unreachable` | The input is validated before this line | where it is validated |
| `follow-up` | A known silent default left for a later PR | what the fix is |

**Baseline (task 9):** 45 hits allowed: 28 defined, 9 follow-up, 1 parity, 7 unreachable; 0
literal costs in spectra-api. The follow-ups are the strategy `Context` money accessors
(`equity`, `account_equity`, `position_value`, `position_exposure`, 0 on overflow), DSL numbers
that do not convert to a quantity or price (0), and `pbo: 0.0` in the validation report when PBO
is unavailable. Not caught by the patterns and also a follow-up: research attribution reports a
win rate of 0 when there are no trades.

**Fixed in task 9** (no longer in the allowlist):

| Site | Old behaviour | New behaviour | Test |
|---|---|---|---|
| `spectra-backtest/src/output.rs` `fills_csv` | A multi-strategy fill with no recorded order id was written as order 0 (the engine's own fills) with no strategy | `fills_csv` and `artifacts` return `Error::InvalidArgument` ("fill n … has no order id") | `multi_strategy.rs`: `a_fill_without_an_order_id_is_an_error` |
| `spectra-risk/src/garch.rs` `forecast_variance` (GARCH, GJR, EGARCH) | A horizon with `k − 1 > i32::MAX` used exponent 1, so it returned the two-step forecast | `forecast_variance` returns `Result`; such a horizon is `Error::InvalidArgument`. Valid horizons give identical values | `m26_volatility_reference.rs`: `a_horizon_past_the_i32_exponent_is_an_error` |
| `spectra-data/src/settings.rs` `utc_offset_minutes` | Converted with `i32::try_from(m).unwrap_or(0)` (an offset that did not fit became UTC; unreachable behind the range check, but silent) | Range-checked, then converted; anything else is the `utc_offset_minutes` settings error | `settings.rs`: `an_offset_outside_the_range_is_an_error_not_utc` |
