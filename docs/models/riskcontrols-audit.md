# Risk Controls & Pre-Trade Enforcement Audit (`crates/spectra-risk/src/enforcement.rs`)

**Module**: `crates/spectra-risk/src/enforcement.rs`  
**Audit Date**: 2026-11-04  
**Context**: M26 Task 8 Statistical Inventory & Silent Defaults Audit  
**Cross-Reference**: PR #202 `PositionSnapshot` Inventory & `docs/models/risk-controls.md`  

---

## 1. Inventory Table

| Public Function / Method | Metric Computed | Formula or Divisor | Empty Input | Zero Limit (`limit <= 0`) | Zero Equity (`equity <= 0`) | Price <= 0 (`P <= 0`) | Missing Volatility |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `OrderCandidate::notional(&self) -> f64` | Nominal candidate order value | $\text{qty} \times \text{price}$ | Returns `0.0` if `qty == 0.0` | N/A (limits not evaluated) | N/A | Returns $\le 0.0$ (negative or zero) | N/A |
| `PositionSnapshot::value(&self) -> f64` | Marked market value of position | $\text{qty} \times \text{price}$ | Returns `0.0` if `qty == 0.0` | N/A | N/A | Returns $\le 0.0$ | N/A |
| `PortfolioSnapshot::gross_exposure(&self) -> f64` | Sum of absolute position values | $\sum_{i} \|\text{pos}_i.\text{value}()\|$ | Returns `0.0` when `positions` is empty | N/A | Returns $\sum \|\text{value}\|$ regardless of equity | Contributes $\|P \times \text{qty}\|$ (0.0 if $P=0$, $>0$ if $P<0$) | N/A |
| `PortfolioSnapshot::net_exposure(&self) -> f64` | Sum of signed position values | $\sum_{i} \text{pos}_i.\text{value}()$ | Returns `0.0` when `positions` is empty | N/A | Returns $\sum \text{value}$ regardless of equity | Contributes $P \times \text{qty}$ (0.0 if $P=0$, inverted sign if $P<0$) | N/A |
| `PortfolioSnapshot::leverage(&self) -> f64` | Gross exposure divided by equity | $\frac{\text{gross\_exposure}}{\text{equity}}$ (Divisor: $\text{equity}$) | Returns `0.0` if `positions` is empty and $\text{equity} > 0$ | N/A | Returns `f64::INFINITY` if $\text{equity} \le 0.0$ | Handled via gross exposure | N/A |
| `PortfolioSnapshot::drawdown_pct(&self) -> f64` | Drawdown from peak equity fraction | $\frac{\text{peak} - \text{equity}}{\text{peak}}$ (Divisor: $\text{peak}$) | N/A | N/A | **Silent default `0.0`** if $\text{peak} \le 0.0$; if $\text{peak} > 0$ and $\text{equity} \le 0$, returns $\ge 1.0$ | N/A | N/A |
| `PortfolioSnapshot::daily_loss(&self) -> f64` | Day-to-date equity loss | $(\text{start\_equity} - \text{equity}).\max(0.0)$ | N/A | N/A | Returns $\text{start\_equity}.\max(0.0)$ | N/A | N/A |
| `PortfolioSnapshot::var_pct(&self) -> f64` | 1-day 99% VaR as fraction of equity | Delegates to `var_pct_with_model(TCopula { dof: 4 })` | **Silent default `0.0`** if `positions.is_empty()` | N/A | **Silent default `0.0`** if $\text{equity} \le 0.0$ | Silently filters out positions with $P \le 0.0$; returns `0.0` if all filtered | Silently defaults all asset volatilities to `DEFAULT_VOLATILITY` (0.20) |
| `PortfolioSnapshot::var_pct_with_model(&self, model) -> f64` | 1-day 99% VaR under dependence model | 2,000 Monte Carlo runs at 99% confidence on 1-day horizon ($1/252$); fallback: $(L \times 0.0165 \times 2.33)$ | **Silent default `0.0`** if `positions.is_empty()` or `assets.is_empty()` | N/A | **Silent default `0.0`** if $\text{equity} \le 0.0$; masks insolvency | Filter `pos.price > 0.0` silently ignores non-positive prices; if all $\le 0$, returns `0.0` | Silently defaults volatility to `DEFAULT_VOLATILITY` (0.20) via `add_asset`; assumes identity correlation (`None`) |
| `pub fn utilization(value: f64, limit: f64) -> f64` | Limit utilization fraction | $\frac{\text{value}}{\text{limit}}$ (Divisor: $\text{limit}$) | Returns `0.0` if `value == 0.0` | **Silent default `0.0`** if $\text{limit} \le 0.0$ (masks breach of zero limit) | N/A | N/A | N/A |
| `LimitUtilization::new(...) -> LimitUtilization` | Bundle of 6 limit utilization ratios | Calls `utilization()` for gross, net, leverage, drawdown, daily loss, VaR | All `0.0` if input values are `0.0` | **Silent default `0.0`** for any metric where $\text{limit} \le 0.0$ | Leverage utilization is `INFINITY` (or `0.0` if leverage limit $\le 0$) | Inherits from underlying metrics | Inherits from `var_pct` |
| `PreTradeChecker::fail_closed(...) -> PreTradeResult` | Rejected verdict for system failures | Construct `PreTradeResult` with `Rejected` verdict and 0 approved | `requested_qty` preserved, `approved_qty = 0.0` | N/A | N/A | N/A | N/A |
| `PreTradeChecker::evaluate(...) -> PreTradeResult` | Pre-trade risk validation against all limits | Stage 1: Kill switch; Stage 2: Drawdown & daily loss; Stage 3: Exposure projection; Stage 4: Breach checks; Stage 5: Size reduction | Evaluates with zero existing position for unheld symbols | If `max_var_pct <= 0.0`, **silently bypasses VaR check**. In size reduction, non-zero order breaches zero limit. | Leverage is `INFINITY`. Concentration defaults to **silent default `0.0`** (bypasses check!). VaR defaults to **`0.0`**. | $P \le 0$ silently dropped from projected VaR calculation; in size reduction causes division by zero. | `PositionSnapshot` lacks volatility; projected portfolio silently defaults volatility to 0.20 and correlation to identity. |

---

## 2. Silent Defaults Inventory

Under M26 rules, a returned zero is permitted only where strictly defined by mathematical formulas. Missing inputs, invalid configurations, non-positive equity, missing prices, or undefined statistics must return an explicit `Err`.

| # | File | Line | Code Snippet | Value Returned | Flag / Classification | Rationale & M26 Defect Assessment |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 1 | `enforcement.rs` | 130 | `if self.peak_equity <= 0.0 { 0.0 }` | `0.0` | **Silent default** | When peak equity is non-positive ($E_{\text{peak}} \le 0$), drawdown is mathematically undefined ($0/0$ or negative denominator). Returning `0.0` indicates zero drawdown (peak health) when the account is actually depleted or bankrupt. |
| 2 | `enforcement.rs` | 149 | `if self.equity <= 0.0 \|\| self.positions.is_empty() { return 0.0; }` (equity branch) | `0.0` | **Silent default** | When equity is non-positive ($E \le 0$), risk as a fraction of equity is infinite or undefined. Returning `0.0` (0% VaR) silently masks extreme bankruptcy risk, allowing orders to pass pre-trade VaR checks. |
| 3 | `enforcement.rs` | 154 | `if pos.qty.abs() > 1e-8 && pos.price > 0.0 { p.add_asset(...); }` | Position dropped | **Silent default** | Positions with non-positive prices ($P \le 0.0$) are silently dropped from the VaR portfolio without warning or error, causing VaR to underreport portfolio risk. |
| 4 | `enforcement.rs` | 160 | `if p.assets().is_empty() { return 0.0; }` | `0.0` | **Silent default** | If all positions have $P \le 0.0$ and were filtered out, `var_pct_with_model` returns `0.0` rather than signaling that positions could not be valued. |
| 5 | `enforcement.rs` | 155 | `p.add_asset(sym.as_str(), pos.qty, pos.price)` | Volatility `0.0` $\to$ `0.20` | **Silent default** | `PositionSnapshot` has no volatility field. `add_asset` passes `0.0`, which causes the simulation engine to silently default every position's annualized volatility to `DEFAULT_VOLATILITY` (20%). |
| 6 | `enforcement.rs` | 164 | `simulate_correlated(&p, None, model, ...)` | Correlation `None` $\to$ $I$ | **Silent default** | Passing `None` for correlation silently assumes an identity correlation matrix (complete independence between assets), severely understating joint portfolio tail risk. |
| 7 | `enforcement.rs` | 167 | `match metrics::value_at_risk(...) { Err(_) => (self.leverage() * 0.0165 * 2.33).max(0.0) }` | Parametric fallback | **Silent default** | When simulation-based VaR calculation fails, instead of propagating the error, it silently falls back to a crude parametric heuristic $(L \times 0.0165 \times 2.33)$. |
| 8 | `enforcement.rs` | 169 | `match copula::simulate_correlated(...) { Err(_) => (self.leverage() * 0.0165 * 2.33).max(0.0) }` | Parametric fallback | **Silent default** | When copula simulation fails (e.g., singular correlation matrix or numerical error), it silently falls back to the parametric heuristic rather than returning an error. |
| 9 | `enforcement.rs` | 180 | `if limit > 0.0 { value / limit } else { 0.0 }` | `0.0` | **Silent default** | When `limit <= 0.0` (zero exposure allowed, or unset limit), any non-zero value is a 100%+ breach. `utilization` returns `0.0` (0% utilized), hiding the violation. |
| 10 | `enforcement.rs` | 321 | `concentration_pct: 0.0` (kill-switch rejection) | `0.0` | **Silent default** | Kill-switch rejection reports `concentration_pct: 0.0` as a placeholder rather than calculating the actual portfolio concentration. |
| 11 | `enforcement.rs` | 364 | `concentration_pct: 0.0` (kill-switch size reduction) | `0.0` | **Silent default** | Kill-switch size reduction reports `concentration_pct: 0.0` in projected metrics rather than calculating the projected concentration of the reduced position. |
| 12 | `enforcement.rs` | 367 | `var_pct: 0.0` (kill-switch size reduction) | `0.0` | **Silent default** | Kill-switch size reduction reports `var_pct: 0.0` in projected metrics rather than computing the post-reduction portfolio VaR. |
| 13 | `enforcement.rs` | 391 | `concentration_pct: 0.0` (kill-switch size exceeded rejection) | `0.0` | **Silent default** | Reports `concentration_pct: 0.0` as placeholder in rejected result metrics. |
| 14 | `enforcement.rs` | 428 | `concentration_pct: if portfolio.equity <= 0.0 { 0.0 } else { ... }` | `0.0` | **Silent default** | When equity is non-positive in kill-switch approval, concentration is reported as `0.0` rather than undefined / infinite. |
| 15 | `enforcement.rs` | 478 | `concentration_pct: 0.0` (drawdown / daily loss rejection) | `0.0` | **Silent default** | When an order is rejected for drawdown or daily loss breach, concentration is reported as `0.0` rather than the true portfolio concentration. |
| 16 | `enforcement.rs` | 528 | `let projected_concentration = if portfolio.equity <= 0.0 { 0.0 } else { ... }` | `0.0` | **Silent default** | In general order evaluation with $E \le 0$, projected concentration is set to `0.0`. Since `0.0 <= max_concentration_pct`, the concentration check at line 600 passes undetected despite negative equity. |
| 17 | `enforcement.rs` | 613 | `if limits.max_var_pct > 0.0 && projected_var > limits.max_var_pct` | Bypassed check | **Silent default** | If `limits.max_var_pct <= 0.0` (strict 0 VaR tolerance or unconfigured 0.0), the check evaluates to `false`, silently disabling pre-trade VaR enforcement entirely. |

### Formula-Defined Values (Explicitly Not Silent Defaults)
- **Line 121 (`leverage`)**: `if self.equity <= 0.0 { f64::INFINITY }` — Defined by arithmetic division by zero: $\lim_{E \to 0^+} \frac{\text{gross}}{E} = +\infty$.
- **Line 132 (`drawdown_pct`)**: `.max(0.0)` — Clamps new equity peaks ($E > E_{\text{peak}}$) to 0% drawdown.
- **Line 138 (`daily_loss`)**: `.max(0.0)` — Clamps profitable days ($E > E_{\text{start}}$) to zero loss.
- **Line 149 (`var_pct_with_model`)**: `if self.positions.is_empty() { return 0.0; }` — Zero positions mathematically have zero market risk.
- **Line 295 (`evaluate`)**: `portfolio.positions.get(&order.symbol).map_or(0.0, |p| p.qty)` — A symbol not present in the map holds exactly 0.0 shares.
- **Line 477 (`evaluate`)**: `portfolio.positions.get(&order.symbol).map_or(0.0, |p| p.value().abs())` — An unheld symbol has exactly $0.00 market value.
- **Lines 640, 642, 644, 648 (`evaluate` size reduction)**: `.max(0.0)` — Permissible order quantities cannot be negative; clamping prevents order inversion.
- **Line 654 (`evaluate` size reduction)**: `.min(...)` chain — Order sizing must satisfy the most constraining active risk limit.

**Total Silent Defaults Flagged**: 17  

---

## 3. Overlap Check with PR #202 `PositionSnapshot` Sites

PR #202 audited 10 `PositionSnapshot` instantiation and consumption sites across `crates/spectra-engine`, `crates/spectra-api`, and `crates/spectra-risk`. All 10 sites directly overlap with the silent defaults uncovered in `crates/spectra-risk/src/enforcement.rs`:

| PR #202 Site | File & Line | Code Context | Silent Default at Site | Overlap with `enforcement.rs` Silent Defaults | Overlap Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **1** | `crates/spectra-engine/src/backtest.rs:2494` | `PositionSnapshot { qty: pos.qty.to_f64(), price: unit(inst, mark_price) }` | `mark_price.map_or(fill_px, ...)` defaults missing mark price to fill price. Constructed without volatility. | Feeds `PortfolioSnapshot::var_pct()`, triggering silent default volatility (`0.20`, line 155) and independence (`None`, line 164). | **Overlaps** |
| **2** | `crates/spectra-api/src/paper_api.rs:96` | `PositionSnapshot { qty: pos.qty, price }` in `PaperAccountView::new` | Multiplier applied without validating positive price. Constructed without volatility. | Feeds `PortfolioSnapshot::var_pct()` on line 105, triggering silent default volatility (`0.20`, line 155) and correlation (`None`, line 164). | **Overlaps** |
| **3** | `crates/spectra-api/src/paper_api.rs:571` | `PositionSnapshot { qty: p.qty, price: p.price * mult(&p.symbol) }` in paper pre-trade check | `eval_price = req.fill_price.or(req.limit_price).or(current_sym_price).unwrap_or(100.0)`. Silent dummy price fallback of `100.0`. | Fictitious price `100.0` corrupts `OrderCandidate::notional` and `PreTradeChecker::evaluate` (lines 518, 537). Triggers line 155 volatility default. | **Overlaps** |
| **4** | `crates/spectra-api/src/broker_api.rs:439` | `PositionSnapshot { qty: p.qty, price: p.price * mult(&p.symbol) }` in broker pre-trade check | `order_req.parse_limit_price().unwrap_or(100.0)` dummy price fallback; missing `db_balance` defaults to `(100_000.0, 100_000.0 + gross, ...)`. | Corrupted `PositionSnapshot` and synthetic 100k equity feed into `PreTradeChecker::evaluate` (lines 518, 525, 528, 537). Triggers line 155 vol default. | **Overlaps** |
| **5** | `crates/spectra-api/src/broker_api.rs:987` | `PositionSnapshot { qty: p.qty, price: p.price }` in margin pre-check | `db_positions.unwrap_or_default()`, `db_balance.unwrap_or(None)`, missing balance defaults to `(100_000.0, 100_000.0, 100_000.0, 100_000.0)`. | Missing database balance silently manufactures 100k equity in `PortfolioSnapshot`, artificially lowering `leverage()` (line 123) and masking breaches. | **Overlaps** |
| **6** | `crates/spectra-api/src/broker_api.rs:1097` | `PositionSnapshot { qty, price: 100.0 }` in reconciliation handler | Hardcodes dummy price `100.0` for all venue/local positions. | Directly manufactures `PositionSnapshot` with a silent default price `100.0`. | **Overlaps** |
| **7** | `crates/spectra-api/src/risk_center_api.rs:743` | `PositionSnapshot { qty: p.qty, price: p.price }` in risk center order eval | Missing `db_balance` silently defaults to `(100_000.0, 100_000.0 + gross, ...)`. Lacks volatility. | Synthetic equity and unvolatized `PositionSnapshot` feed into `PreTradeChecker::evaluate` (lines 518, 525, 528, 537). | **Overlaps** |
| **8** | `crates/spectra-api/src/risk_center_api.rs:1132` | `PositionSnapshot { qty: p.qty, price: p.price }` in risk alerts check | `db_positions.unwrap_or_default()`, missing balance fallback to 100k. | Feeds into `utilization(gross, limits.max_gross_exposure)`, triggering line 180 silent default for `limit <= 0.0`. | **Overlaps** |
| **9** | `crates/spectra-api/src/risk_center_api.rs:1356` | `PositionSnapshot { qty: p.qty, price: p.price }` in risk center summary | Missing positions/balance falls back to `PortfolioSnapshot::default()` (100k cash/equity). Lacks volatility. | Calls `LimitUtilization::new` and `p.var_pct()`, triggering silent default volatility (`0.20`, line 155), identity correlation (line 164), and limit utilization `0.0` (line 180). | **Overlaps** |
| **10** | `crates/spectra-risk/src/enforcement.rs:518` | `PositionSnapshot { qty: projected_pos_qty, price: order.price }` | Constructed inside `enforcement.rs::evaluate` for projected positions without volatility. | **Direct origin**: this site constructs `PositionSnapshot` internally, triggering lines 154, 155, 164 during projected VaR evaluation (`projected_portfolio.var_pct()`). | **Overlaps** |

### Summary of Overlap Analysis
Every single one of the 10 PR #202 `PositionSnapshot` sites overlaps with the silent defaults identified in `enforcement.rs`. The two primary vectors of overlap are:
1. **Missing Volatility & Independence Assumption**: `PositionSnapshot` contains only `(qty, price)`, omitting `volatility`. Whenever a `PortfolioSnapshot` is passed to `var_pct()`, `enforcement.rs:155` silently assigns `0.0` volatility (defaulting to 20% in the simulation engine) and `enforcement.rs:164` silently assumes an identity correlation matrix.
2. **Upstream Dummy Fallbacks Passing Downstream**: Callers at `broker_api.rs:439`, `broker_api.rs:1097`, and `paper_api.rs:571` inject dummy prices (`unwrap_or(100.0)`) or synthetic equity balances (`(100_000.0, 100_000.0 + gross, ...)`), which bypass `enforcement.rs` checks by artificially inflating equity or suppressing true portfolio volatility.

---

## 4. Callers and Golden / Fixture Coverage

### 4.1 Caller Inventory across `crates/`, `tests/`, and `examples/`

| Public Function | External Callers | Internal Callers (`enforcement.rs`) | Total Calls | External Caller Locations |
| :--- | :--- | :--- | :--- | :--- |
| `OrderCandidate::notional` | 3 | 0 | 3 | `broker_adapters/mod.rs:275`, `broker_adapters/mod.rs:290`, `broker_api.rs:1002` |
| `PositionSnapshot::value` | 7 | 7 | 14 | `broker_adapters/mod.rs:271`, `broker_adapters/mod.rs:289`, `broker_api.rs:444`, `risk_center_api.rs:748`, `risk_center_api.rs:1137`, `risk_center_api.rs:1361`, `learn_formula_drift.rs:389` |
| `PortfolioSnapshot::gross_exposure` | 5 | 4 | 9 | `risk_center_api.rs:1143`, `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `learn_formula_drift.rs:213`, `learn_formula_drift.rs:351` |
| `PortfolioSnapshot::net_exposure` | 4 | 5 | 9 | `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `learn_formula_drift.rs:214`, `learn_formula_drift.rs:352` |
| `PortfolioSnapshot::leverage` | 5 | 5 | 10 | `risk_center_api.rs:1156`, `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `learn_formula_drift.rs:215`, `learn_formula_drift.rs:353` |
| `PortfolioSnapshot::drawdown_pct` | 4 | 5 | 9 | `risk_center_api.rs:1169`, `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `learn_formula_drift.rs:216` |
| `PortfolioSnapshot::daily_loss` | 4 | 5 | 9 | `risk_center_api.rs:1182`, `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `learn_formula_drift.rs:217` |
| `PortfolioSnapshot::var_pct` | 4 | 5 | 9 | `paper_api.rs:105`, `risk_center_api.rs:1365`, `risk_center_api.rs:1368`, `m26_copula_reference.rs:559` |
| `PortfolioSnapshot::var_pct_with_model` | 2 | 1 | 3 | `m26_copula_reference.rs:560`, `m26_copula_reference.rs:561` |
| `utilization` | 6 | 6 | 12 | `risk_center_api.rs:1144`, `risk_center_api.rs:1157`, `risk_center_api.rs:1170`, `risk_center_api.rs:1183`, `m26_risk_controls_reference.rs:110`, `m26_risk_controls_reference.rs:111` |
| `LimitUtilization::new` | 2 | 0 | 2 | `risk_center_api.rs:1371`, `learn_formula_drift.rs:212` |
| `PreTradeChecker::fail_closed` | 0 | 0 | 0 | None (API routes implement independent `fail_closed_response`) |
| `PreTradeChecker::evaluate` | 8 | 11 | 19 | Production: `broker_api.rs:456`, `paper_api.rs:608`, `risk_center_api.rs:756`, `risk_center_api.rs:1258`, `backtest.rs:2517`. Tests: `m26_risk_gap_reference.rs:176, 184, 193` |
| **Total Call Sites** | **54** | **49** | **103** | |

### 4.2 Golden and Fixture Coverage

- **Parity Golden Files (`parity/golden/`)**:
  - `git grep -i "enforcement\|pretrade\|pre_trade\|utilization\|positionsnapshot" parity/` returns zero matches.
  - The frozen C++ parity suite does not cover `enforcement.rs` or `PreTradeChecker`, as pre-trade risk enforcement and limit utilization were added in the Rust rewrite (M26 tasks 2, 5, and 9) rather than existing in the legacy CPU/CUDA simulation core.
- **Reference Test Suites (`tests/`)**:
  - `crates/spectra-risk/tests/m26_risk_controls_reference.rs`: Asserts `utilization(50.0, 100.0) == 0.5` and `utilization(150.0, 100.0) == 1.5` (does not test undefined case `limit <= 0.0`).
  - `crates/spectra-risk/tests/m26_risk_gap_reference.rs`: Asserts kill switch rejection, closing order approval, and size reduction under kill switch.
  - `crates/spectra-risk/tests/m26_copula_reference.rs`: Asserts `var_pct()` matches `var_pct_with_model(TCopula { degrees_of_freedom: 4 })` on valid 2-asset portfolio.
  - `crates/spectra-cli/tests/learn_formula_drift.rs`: Asserts formula drift metrics against `PortfolioSnapshot` and `LimitUtilization::new`.
  - `crates/spectra-risk/src/enforcement.rs` (unit tests): 11 tests covering within-limits approval, kill-switch behavior, size reduction, drawdown rejection, arithmetic invariants, and VaR limit enforcement.
- **Undefined Case Test Gaps**:
  - No existing test asserts the behavior of `utilization(val, 0.0)` or `utilization(val, -10.0)`.
  - No existing test asserts `var_pct()` when `equity <= 0.0` or when all positions have `price <= 0.0`.
  - No existing test asserts `PreTradeChecker::evaluate` when `limits.max_var_pct == 0.0` or `equity <= 0.0`.

---

## 5. Divergence Entries & Behavior Changes

#### 1. `PreTradeChecker::evaluate` & `PortfolioSnapshot::var_pct_with_model` — Non-Positive Equity ($E \le 0$)
- **Function:** `PreTradeChecker::evaluate` and `PortfolioSnapshot::var_pct_with_model`
- **Input:** `portfolio.equity <= 0.0` (or non-finite)
- **Old value / Old behavior:** `var_pct_with_model` silently returned `Ok(0.0)`. In `evaluate`, concentration was defaulted to `0.0`, allowing insolvent accounts to bypass VaR and concentration checks.
- **New behavior:** `PreTradeChecker::evaluate` immediately rejects through `PreTradeChecker::fail_closed` with verdict `Rejected`, approved quantity `0.0`, and breach code `system_fail_closed` ("portfolio equity must be positive; VaR and concentration are undefined"). Direct calls to `var_pct_with_model` return `Err(Error::InvalidArgument("portfolio equity must be positive to compute VaR as a fraction of equity, got ..."))`.

#### 2. `PortfolioSnapshot::drawdown_pct` — Non-Positive Peak Equity ($E_{\text{peak}} \le 0$)
- **Function:** `PortfolioSnapshot::drawdown_pct`
- **Input:** `self.peak_equity <= 0.0` (or non-finite)
- **Old value:** `0.0` (indicated 0% drawdown when peak was uninitialized or account bankrupt)
- **New behavior:** Returns `Err(Error::InvalidArgument("drawdown is undefined when peak_equity <= 0.0, got ..."))`. Propagated by `PreTradeChecker::evaluate` instead of masking bankruptcy.

#### 3. `PortfolioSnapshot::var_pct_with_model` — Non-Positive Position Price ($P \le 0$)
- **Function:** `PortfolioSnapshot::var_pct_with_model`
- **Input:** Position with `pos.qty.abs() > 1e-8` and `pos.price <= 0.0` (or non-finite)
- **Old value / Old behavior:** Positions with non-positive prices were silently filtered out and dropped from simulation; returned `Ok(0.0)` if all positions were filtered out.
- **New behavior:** Returns `Err(Error::InvalidArgument("position for {sym} has non-positive or non-finite price: ..."))`.

#### 4. `utilization` & `LimitUtilization::new` — Non-Positive Limit ($\text{limit} \le 0$)
- **Function:** `utilization` and `LimitUtilization::new`
- **Input:** `limit <= 0.0` (or non-finite limit)
- **Old value:** `0.0` (reported 0% utilization, masking breaches of strict zero limits)
- **New behavior:** `utilization` returns `Err(Error::InvalidArgument("limit must be positive and finite, got ..."))`. `LimitUtilization::new` propagates the error as `Result<LimitUtilization>`.

#### 5. `PreTradeChecker::evaluate` — Non-Positive VaR Limit (`max_var_pct <= 0`)
- **Function:** `PreTradeChecker::evaluate`
- **Input:** `limits.max_var_pct <= 0.0` (or non-finite)
- **Old value / Old behavior:** The condition `if limits.max_var_pct > 0.0` evaluated to `false`, silently disabling pre-trade VaR enforcement entirely.
- **New behavior:** `PreTradeChecker::evaluate` rejects the configuration with `Err(Error::InvalidArgument("RiskLimits max_var_pct must be positive and finite, got ..."))`.

#### 6. `PortfolioSnapshot::var_pct_with_model` — Copula Simulation and Quantile Failures
- **Function:** `PortfolioSnapshot::var_pct_with_model`
- **Input:** Simulation failure (e.g. non-finite quantities/losses) or quantile calculation error
- **Old value / Old behavior:** Silently fell back to crude parametric heuristic `(self.leverage() * 0.0165 * 2.33).max(0.0)`.
- **New behavior:** Parametric fallback eliminated; propagates `Err(Error::Data("var_pct simulation failed: ..."))` or `Err(Error::Data("var_pct quantile calculation failed: ..."))`.

#### 7. `PortfolioSnapshot::var_pct_with_model` — Multi-Asset Portfolio without Correlation Matrix
- **Function:** `PortfolioSnapshot::var_pct_with_model`
- **Input:** $N > 1$ assets with correlation matrix `None`
- **Old value / Old behavior:** Silently defaulted to identity matrix ($I$), assuming complete asset independence.
- **New behavior:** Returns `Err(Error::InvalidArgument("multi-asset VaR requires a correlation matrix; none available for {n} assets"))`.

#### 8. `PreTradeChecker::evaluate` — Placeholder Zeros in Reject and Reduce Branches
- **Function:** `PreTradeChecker::evaluate`
- **Input:** Orders evaluated during kill-switch activation (rejected or reduced), or rejected during drawdown/daily loss limit breaches
- **Old value / Old behavior:** Reported placeholder `0.0` for `concentration_pct` and `var_pct` in `ProjectedMetrics`.
- **New behavior:** Computes the actual portfolio state metrics: `concentration_pct` is computed from the actual position value divided by equity (`current_pos_val / portfolio.equity`), and `var_pct` is computed from the actual existing or post-reduction portfolio. Correction (Task 7b): the merged #221 code (commit 5a6d5f5, `eval_var_pct`) still reported `var_pct = 0.0` when a position had no volatility; see entry 10.

#### 9. `PortfolioSnapshot::var_pct` — Degrees of Freedom Literal in Enforcement Path
- **Function:** `PortfolioSnapshot::var_pct`
- **Location:** `crates/spectra-risk/src/enforcement.rs`
- **Old value:** Hardcoded integer literal `4` in `DependenceModel::TCopula { degrees_of_freedom: 4 }`.
- **New behavior:** Extracted and referenced as named constant `pub const ENFORCEMENT_T_COPULA_DOF: u32 = 4;`.

#### 10. `PreTradeChecker::evaluate` — Missing Volatility (Task 7b)
- **Function:** `PreTradeChecker::evaluate`, `ProjectedMetrics::var_pct`
- **Input:** A portfolio or projected portfolio with an open position whose volatility is `None` (the workspace has no dataset to estimate it from).
- **Old value / Old behavior:** VaR was treated as `0.0`. On the approval path (earlier commit d548777) this passed any `max_var_pct`, so the VaR limit was never applied to positions without data. In size reduction, a missing volatility set the VaR quantity cap to `f64::INFINITY`. In kill-switch, reduce and drawdown branches (#221 commit 5a6d5f5) the reported VaR was `0.0`. Detection matched the error text "has no volatility".
- **New behavior:** `ProjectedMetrics::var_pct` is `Option<f64>`; `None` means VaR could not be computed (API: `null`). An order that adds exposure gets breach `var_unavailable` and is rejected; it is never size-reduced. An order that only reduces an existing position (same side, smaller size) is still allowed, so positions can always be closed. Kill-switch and drawdown results report `var_pct: None` instead of `0.0`. Detection checks `PositionSnapshot::volatility` directly; no error-text matching. The `var_unavailable` breach reports `projected: 0.0` because there is no projected value; the code and message carry the meaning.

#### 11. `pre_trade_check` — Synthetic Balance (risk_center_api.rs)
- **Function:** `risk_center_api::pre_trade_check`
- **Input:** No balance row in the database and no `portfolio` in the request.
- **Old value / Old behavior:** Used a synthetic balance of cash 100,000 and equity 100,000 + gross exposure.
- **New behavior:** Fails closed through `fail_closed_response` (verdict `rejected`, breach `system_fail_closed`, reason "no portfolio balance in database and no portfolio supplied"). Matches the paper and broker paths after #202.

#### 12. `replay_decision` — Fabricated Replay Portfolio (risk_center_api.rs)
- **Function:** `risk_center_api::replay_decision`
- **Input:** Any recorded pre-trade decision.
- **Old value / Old behavior:** Rebuilt a made-up portfolio from the stored metrics: a fixed position of 10 AAPL at 150 with 20% volatility, cash 50,000, equity 100,000 when gross or leverage was zero, and `unwrap_or` defaults for every missing metric. The replay verdict did not reflect the original inputs.
- **New behavior:** Risk check, paper and broker decisions store the evaluated portfolio (balance and positions with volatility) in their metrics under `portfolio_snapshot`. Replay evaluates against that snapshot. A decision without a snapshot (recorded before this change, or a fail-closed decision) returns 422 "Decision has no stored portfolio snapshot, so it cannot be replayed exactly." Replay still uses the current limits and kill-switch state, not those at decision time.

#### 13. Broker Order Pre-Trade — Synthetic Balance (broker_api.rs)
- **Function:** `broker_api` order submission pre-trade check
- **Input:** No balance row in the database.
- **Old value / Old behavior:** Used a synthetic balance of cash 100,000 and equity 100,000 + gross exposure.
- **New behavior:** Returns 422 "Broker risk check failed: no portfolio balance initialized in database."

#### 14. `run_backtest` — Volatility Skipped for Short Histories (backtest.rs)
- **Function:** `spectra_engine::run_backtest`, when `cfg.risk_limits` is set
- **Input:** An instrument with fewer bars than `volatility_from_bars` needs (60).
- **Old value / Old behavior:** The volatility error was discarded (`if let Ok(vol)`), so the instrument had no volatility and its VaR counted as 0.0 in the pre-trade check.
- **New behavior:** The backtest returns the volatility error. A backtest or paper step with risk limits needs at least 60 bars per instrument. Note: volatility is still estimated from the whole input bar series, including bars after the decision time; that look-ahead is a known issue for a later task.


### Paper path (Task 6b)

#### 15. `PaperAccountView` — VaR Fallback (paper_api.rs)
- **Function:** `PaperAccountView::new` (GET `/api/v1/paper/account`, account reset, paper step)
- **Input:** A position with no volatility, or any other VaR failure (multi-asset without a correlation matrix, equity ≤ 0, invalid price).
- **Old value / Old behavior:** `snap.var_pct().unwrap_or(0.0)`: VaR, VaR in currency and CVaR all showed 0.
- **New behavior:** `var_pct`, `var_dollar` and `cvar_dollar` are `null`, with `var_unavailable_reason` naming the cause ("no volatility for SYM (no dataset with at least 60 bars)", or the VaR error text). The account itself still loads: this is a read-only view. The pre-trade check is unchanged and still rejects or errors as before.

#### 16. `PaperAccountView` — Made-up CVaR (paper_api.rs)
- **Function:** `PaperAccountView::new`
- **Input:** Any account with VaR available.
- **Old value / Old behavior:** `cvar_dollar = var_dollar * 1.25`, a fixed multiple not computed from anything.
- **New behavior:** CVaR is the mean simulated loss at or beyond the 99% VaR, read from the same 2,000-run simulation as VaR (`PortfolioSnapshot::var_and_cvar_pct`, `metrics::conditional_var`). VaR values are unchanged (same simulation, same seed); a unit test checks they are identical.

#### 17. `get_paper_attribution` — Synthetic Attribution Report (paper_api.rs)
- **Function:** GET `/api/v1/paper/attribution`
- **Input:** Any paper account.
- **Old value / Old behavior:** Gross P&L was today's equity change (equity − start-of-day equity) split across open positions by market value; fees were 0 although every fill stores its fee; each open position counted as one trade; per-position win rate 1.0 or 0.0; overall win rate always 1.0 when any position existed; every position counted as long; the cost breakdown was all zeros.
- **New behavior:** The account's fills since it started are replayed into a ledger and the backtest attribution (`spectra_research::compute_pnl_attribution`) runs over it with the paper slippage rate (`PAPER_SLIPPAGE_BPS`). Fees, trades, win rates, long/short split and costs are real. As for backtests, it covers closed trades; open positions are not in it. "Since the account started" means since creation or the last reset (`portfolio_balances.started_at`, new column; existing rows backfilled to the epoch). Any fill that cannot be replayed returns 422.

#### 18. Paper Kill-Switch Rejection — Decision Not Recorded Without a Price (paper_api.rs)
- **Function:** paper order submission while the kill switch is active
- **Input:** A market order with no fill price, no limit price and no position in the symbol.
- **Old value / Old behavior:** The decision was recorded with price `0.0`; `risk_decisions.price` has `CHECK (price > 0)`, so the insert failed and `let _ =` hid the error. The rejection never reached the audit trail.
- **New behavior:** `risk_decisions.price` allows NULL (migration; the CHECK still rejects zero and negative prices). The rejection is recorded with `price: null`. Replay of a decision with no price returns 422 "Decision has no price, so it cannot be replayed." The audit table in the Risk Center shows "—".

#### 19. Paper Step — Volatility Errors Dropped (paper_api.rs)
- **Function:** POST `/api/v1/paper/step`, account view after the step
- **Input:** A volatility error on the step's own bars, or a failed dataset lookup.
- **Old value / Old behavior:** Both errors were discarded (`if let Ok`); the symbol simply had no volatility.
- **New behavior:** With fewer than 60 step bars for a symbol, its volatility comes from the stored dataset, as before. Any other volatility error, and any dataset lookup error, returns 422.
