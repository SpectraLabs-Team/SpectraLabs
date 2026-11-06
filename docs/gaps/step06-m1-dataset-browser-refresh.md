# Step 6: Dataset Browser, Scheduled Refresh, and Workspace Integrity Checks (M1)

## Context & Ownership
- **Modules**: M1
- **Owner**: Dev
- **Size**: Medium

## Summary
Complete the interactive Dataset Browser in Studio, automate recurring market data refreshes via Alpaca/provider connectors, and enforce workspace checksum verification before running backtests.

## Implementation & Verification Checklist
- [ ] Implement Dataset Browser component in Studio for reviewing bar intervals, outlier returns, stale data, and gaps.
- [ ] Connect background worker job pipeline to perform scheduled dataset refreshes (`AlpacaConnector` daily bars sync).
- [ ] Add workspace checksum and holdout integrity validation check (`grant_holdout_evaluation` and sealed dataset verification).
- [ ] Add automated tests for scheduled refresh execution and workspace integrity enforcement.
