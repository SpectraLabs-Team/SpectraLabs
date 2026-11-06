# Step 13: Margin Gap Analysis and Multi-Asset Requirements (M24)

## Context & Ownership
- **Modules**: M24
- **Owner**: Dev
- **Size**: TBD
- **Note**: Options findings 3 to 6 for M24 are tracked in M26.2 and are not in this list.

## Summary
Provide initial and maintenance margin calculations, leverage limits, interest accounting on negative cash balances, and margin liquidation breach monitoring across engine backtests and paper trading accounts.

## Implementation & Verification Checklist
- [ ] Initial margin and maintenance margin requirement formulas for equities, futures, and derivative contracts.
- [ ] Account margin interest calculations when negative cash is permitted (`margin_interest_rate` in backtest configs).
- [ ] Pre-trade margin sufficiency checks rejecting orders that exceed available buying power or portfolio leverage thresholds.
- [ ] Margin utilization gauges and liquidation warnings in Studio Paper Lab.
- [ ] Add known-answer margin verification fixtures and unit tests.
