# Engine models — reference (M26 task 2, second batch)

Code: `crates/spectra-engine/src/` as named in each section. Options are in `options.md`.
Sign-off: pending (product owner, M26 decision 3). Reference-value tests: task 3.

## Bonds (`bonds.rs`)

**Conventions.** Face value 100 (hardcoded: `coupon_per_period` = 100 × coupon rate / frequency).
Yield compounds `frequency` times a year. Times in the cash-flow schedule are in coupon periods.

**Price from yield.** With base = 1 + y/f and flows (t_i, CF_i) in periods:

```
P(y) = Σ CF_i / base^(t_i)
```

**Yield from price.** Solve P(y) = dirty price for y. Newton from the coupon rate, falling back
to bisection. Price decreases in y.

**Risk.** Macaulay duration = Σ t_i·PV_i / P (periods; the doc comment says years, check the
units in task 3). Modified duration = Macaulay / (1 + y/f). Convexity = Σ t_i(t_i + 1)·CF_i /
base^(t_i + 2) / P. DV01 = price change per 100 face for a 1 bp fall in yield.

**Accrued and clean/dirty.** Accrued interest by the day-count convention; clean = dirty − accrued.

**Edge cases.** No period found after maturity → `period_around` returns `None`. Period walk
stops after 2,400 steps (silent cap, `bonds.rs`: `k > 2_400`). Yield from price errors when no
yield in (−99%, +1000%) reprices the bond.

## Interest rate swaps (`swaps.rs`)

**Net coupon** (per period, on 100 notional): `100·(fixed rate − fixing)·τ`, where τ is the
accrual fraction. Errors when the period's fixing is not known yet.

**Mark** (value to the fixed-rate receiver, at `ts_ns`) = fixed leg − float leg, where fixed leg =
fixed rate × Σ τ·DF(payment), and float leg = (1 + fixing·τ)·DF(next) − DF(maturity). Also returns
the annuity and float leg, and a DV01 from ±1 bp parallel shifts of the curve. Returns `None`
before the current period's fixing is known.

**Known problems:** the mark is `None` before the first fixing rather than an error, so a caller
that treats `None` as zero mark would silently report no value. Check callers in task 3.

## Yield curves (`curves.rs`)

**Discount factors.** Log-linear interpolation between nodes: DF(t) = exp(w·ln DF₁ + (1 − w)·ln DF₂)
(verified in code). Node at t = 0 has DF = 1.

**Zero rate.** z(t) = −ln DF(t) / t.

**Bootstrap** from quotes, in increasing maturity:
- Deposit at rate r for t years: DF = 1 / (1 + r·t).
- Par swap at rate R, frequency f, maturity t: solve for the DF at t that makes the swap price
  zero, i.e. R·Σ τ·DF(t_k) = 1 − DF(t), by bisection on (1e-12, 2] then polished (200 steps).
  Intermediate payments are interpolated against the new node.
- Quote maturities must be positive and strictly increasing; swap frequency must be 1, 2, 4 or 12.

**Z-spread.** Constant spread added to the zero curve that reprices a price to the flows.

**Known problems:** `schedule` rounds the number of periods with `.round().max(1.0)`
(`curves.rs:58`): a maturity shorter than half a period silently becomes one period.

## Futures roll and continuous series (`futures.rs`)

**Roll date.** `roll_ts(expiry, n)`: move back n business days from expiry, using the exchange
calendar. Counting stops after 400 calendar days (`guard < 400`), and the date reached is
returned silently (`futures.rs`).

**Continuous series.** Stitches contracts at roll dates; `switches` records each roll.

## Performance ratios (`performance.rs`)

Per-period returns r_1..r_n, periods per year ppy, risk-free rate rf (annual).

| Measure | Formula | Undefined (None) when |
|---|---|---|
| CAGR | Π(1 + r_i)^(ppy/n) − 1 | — |
| Sortino | (mean(r) − rf/ppy) / DD · √ppy, DD = √(Σ min(r_i − rf/ppy, 0)² / n) | DD = 0 |
| Omega (threshold t) | Σ max(r_i − t, 0) / Σ max(t − r_i, 0) | nothing below t |
| Drawdown episodes | maximum depth of each stretch below the running peak of the compounded curve | — |
| Calmar | CAGR / max drawdown | no drawdown |
| Sterling | CAGR / mean episode depth | no episode |
| Burke | CAGR / √(Σ episode depth²) | no episode |
| Rolling Sharpe | mean/stdev over a window, annualized | zero volatility gives 0 |
| Stdev | sample (n − 1) | n < 2 |
| Skewness, excess kurtosis | population moments; excess = m4/m2² − 3 | n < 3 / n < 4 |

**Note:** `excess_kurtosis` here is excess (normal = 0). `spectra-research`'s `moments().kurt` is
raw (normal = 3). See `statistics.md`.

## Transaction costs (`costs.rs`)

**Commission.** fee = max(min fee, unit fee × qty + bps × notional).

**Slipped price.** Buys pay more, sells receive less, by the slippage in bps; never below one
raw unit (silent floor, `costs.rs`: `max(Price::from_raw(1))`).

**Effective slippage.** bps = flat slippage + half the bid–ask spread (if > 0) + volume term
+ extra impact. Volume term = volume_slippage × min(1, qty / bar volume), added only when bar
volume > 0.

**Known problems (task 6–7):**
1. **Zero bar volume turns off the volume term** silently. Zero volume should be an error or an
   explicit "unknown" result, not zero cost.
2. **Flat slippage defaults** in the paper path are hardcoded (0.0005 and fee 1.0 in
   `paper_api.rs`, see the M26 doc). The fix routes them through this model.

## Rebalancing (`rebalance.rs`)

**Due.** A rebalance is due when any holding's drift from target exceeds the threshold.

**Trade.** Desired quantity = target weight × equity / close, rounded to the effective lot
(the coarser of the strategy lot and the instrument lot).

**Known problems (task 7):** a missing target weight is treated as 0 (`rebalance.rs:147` and
`:161`, `unwrap_or(0.0)`). The position is then sold with no error. Should be an explicit error
for an instrument in the universe with no target.

## Test requirements for task 3

- Bonds: a par bond at its coupon rate prices to 100; price and yield round-trip; Macaulay and
  modified duration match a hand-computed bond; DV01 matches a finite difference.
- Swaps: a par swap has zero mark at inception; net coupon matches the formula.
- Curves: a flat curve reprices; bootstrap reprices every quote; log-linear interpolation at a
  midpoint matches the formula.
- Futures: roll dates across a holiday match the exchange calendar; the 400-day guard raises an error.
- Performance: CAGR and each ratio against hand-computed values on a short fixed series.
- Costs: commission at the minimum fee; slippage sign for buy and sell; zero volume is an error.
- Rebalance: a missing target is an error.
