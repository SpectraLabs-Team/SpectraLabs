# M40: Portfolios & Baskets Rebalancing Specification

**Status**: Implemented & Verified (M40 Tasks 1-8)  
**Implementation**: `crates/spectra-research/src/portfolio_basket.rs`  
**Tests**: `crates/spectra-research/tests/m40_portfolio_basket_reference.rs`

## 1. Scope & Models (Task 1 & Task 3)
- **Unified Holdings**: Manages manual entries and paper trading fills in a unified position list (`HoldingPosition`).
- **Target Baskets**: Instruments mapped to target percentage weights.
  - Strict validation: Must not contain duplicate symbols; target weights must sum to exactly 1.0 (100% $\pm 10^{-4}$).

## 2. Valuation & Reconciliation (Task 2)
- Reconciles cash balance, positions cost basis, market equity value, and total portfolio unrealized P&L.

## 3. Cost-Aware Rebalancing Engine (Tasks 4 & 5)
- Computes target notional allocations: $\text{Target Value} = \text{Total Portfolio Value} \times w_i$.
- Evaluates asset drift against configurable threshold `drift_tolerance`.
- Generates exact `Buy` / `Sell` trade orders with share quantities rounded to whole shares.
- Incorporates M26 cost configuration (fixed order ticket fee + bps slippage/commissions).
