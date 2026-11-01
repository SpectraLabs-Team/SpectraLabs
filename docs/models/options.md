# Options model — reference document

Model: European option, Black-Scholes-Merton with a continuous dividend (carry) yield.
Code: `crates/spectra-engine/src/options.rs` (price, Greeks, implied volatility);
settlement: `settle_options` in `crates/spectra-engine/src/backtest.rs`.
Reference tests: `crates/spectra-engine/tests/m26_options_reference.rs`,
`crates/spectra-backtest/tests/m24_options.rs`.
Audit: M26 task 5a, 2026-11-01. Signed off by the product owner on 2026-11-01 (M26 decision 3).

A change to any formula below needs this document and the reference tests updated in the same PR.

## Inputs

| Symbol | Field | Unit | Valid range |
|---|---|---|---|
| S | `spot` | price | finite, > 0 |
| K | `strike` | price | finite, > 0 |
| T | `years` | years | finite, >= 0 |
| r | `rate` | continuously compounded, per year | finite |
| q | `dividend_yield` | continuous, per year | finite |
| σ | `volatility` | per year | finite, 0 to 10 |

Anything outside these ranges is an error (`InvalidArgument`), never a default.

## Formulas (T > 0, σ > 0)

```
d1 = (ln(S/K) + (r − q + σ²/2)·T) / (σ·√T)
d2 = d1 − σ·√T
N  = standard normal CDF (Hart 1968 / West 2005, double precision, ~1e-14)
n  = standard normal density
```

| | Call | Put |
|---|---|---|
| Price | S·e^(−qT)·N(d1) − K·e^(−rT)·N(d2) | K·e^(−rT)·N(−d2) − S·e^(−qT)·N(−d1) |
| Delta | e^(−qT)·N(d1) | −e^(−qT)·N(−d1) |
| Gamma | e^(−qT)·n(d1) / (S·σ·√T) | same |
| Vega | S·e^(−qT)·n(d1)·√T | same |
| Theta | −S·e^(−qT)·n(d1)·σ/(2√T) − r·K·e^(−rT)·N(d2) + q·S·e^(−qT)·N(d1) | −S·e^(−qT)·n(d1)·σ/(2√T) + r·K·e^(−rT)·N(−d2) − q·S·e^(−qT)·N(−d1) |
| Rho | K·T·e^(−rT)·N(d2) | −K·T·e^(−rT)·N(−d2) |

Units: vega per 1.00 of volatility (÷100 for one vol point); theta per year, as calendar time passes (÷365 for a day); rho per 1.00 of rate.

Source: Hull, *Options, Futures, and Other Derivatives*, ch. 15 and 19; Haug, *The Complete Guide to Option Pricing Formulas*, §1.1 (generalized BSM).

## Edge cases

| Case | Result |
|---|---|
| T = 0 or σ = 0, in the money | A forward: price = ±(S·e^(−qT) − K·e^(−rT)); delta = ±e^(−qT); theta = q·S·e^(−qT) − r·K·e^(−rT) (call) or its negative (put); rho = ±T·K·e^(−rT); gamma = vega = 0 |
| T = 0 or σ = 0, out of the money | Price and all Greeks 0 |
| T = 0, at the money | Price 0, Greeks 0 (theta is undefined in the limit; 0 is the convention) |
| Implied volatility at T = 0 | Error |
| Implied volatility for a price outside [price at σ = 1e-9, price at σ = 10] | Error |
| Implied volatility that does not converge in 200 steps | Error (Newton with bisection fallback; normal cases converge in 3–5 steps) |

## Settlement at expiry (backtest)

European, cash-settled. At the first bar at or after expiry, each held option settles at its intrinsic value — max(S − K, 0) for a call, max(K − S, 0) for a put — against the underlying's last close at or before expiry (never a later one), times the contract multiplier, converted to the base currency. Out of the money it expires at zero.

## Reference values

Computed independently at 40 significant digits (mpmath); each Greek by numerical differentiation of the price, not from the closed forms above. The model matches all of them to within 2e-14.

| Case | S | K | T | r | q | σ | Call | Put | Published |
|---|---|---|---|---|---|---|---|---|---|
| Hull, at the money | 100 | 100 | 1 | 5% | 0 | 20% | 10.450584 | 5.573526 | 10.4506 / 5.5735 |
| Hull, Example 15.6 | 42 | 40 | 0.5 | 10% | 0 | 20% | 4.759422 | 0.808599 | 4.76 / 0.81 |
| Haug, generalized BSM | 100 | 95 | 0.5 | 10% | 5% | 20% | 9.628984 | 2.464788 | put 2.4648 |

Greeks for the Hull at-the-money case:

| | Delta | Gamma | Vega | Theta | Rho |
|---|---|---|---|---|---|
| Call | 0.636831 | 0.018762 | 37.524035 | −6.414028 | 53.232482 |
| Put | −0.363169 | 0.018762 | 37.524035 | −1.657880 | −41.890461 |

Full-precision values for all three cases are in the reference test file.

## Audit findings (M26 task 5a)

| # | Finding | Old | New | Status |
|---|---|---|---|---|
| 1 | Zero volatility or expiry, in the money: theta and rho returned 0 | S=120, K=100, T=1, r=5%, q=2%, call: theta 0, rho 0 | theta −2.403670, rho 95.122942 | Fixed in `options.rs` |
| 2 | Implied volatility returned its last guess, without an error, after 200 steps | `Ok(last guess)` | Error | Fixed in `options.rs` (no value change in normal cases) |
| 3 | `compute_option_greeks` (broker path) clamps volatility to at least 0.0001 and time to at least 0, so a NaN or zero volatility, or an expired contract, is priced silently | NaN volatility → 0.01% | — | Open, M26.2 (decision 1) |
| 4 | `settle_options`: no underlying price at or before expiry closes the position at zero with a note, not an error; a long in-the-money option loses its value silently | closed at zero | — | Open, M26.2 (decision 1) |
| 5 | `settle_options` takes the last close at or before expiry with no limit on how old it is | — | — | Open, low |
| 6 | An option with no registered multiplier settles as 1×, not 100× (`Ledger::multiplier` defaults to 1) | — | — | Open, low; needs a rule for options |

The formulas for price, delta, gamma, vega, theta and rho are otherwise correct.
