# M41: Events, Macro & News Specification

**Status**: Implemented & Verified (M41 Tasks 1-6)  
**Implementation**: `crates/spectra-data/src/events.rs`  
**Tests**: `crates/spectra-data/tests/m41_events_reference.rs`

## 1. Earnings Calendar & Surprise History (Task 1)
- `EarningsEvent` tracks consensus vs actual EPS/revenue and surprise percentage.
- **Point-in-Time Protection**: Unannounced earnings are strictly excluded from queries prior to `announcement_ts_ns` (Task 6).

## 2. Economic Calendar & Macro Series (Task 2)
- Macroeconomic series (CPI, FOMC rates, GDP, Non-Farm Payrolls) with mandatory publisher source attribution (e.g. BLS, Federal Reserve, BEA).

## 3. Event-Aware Backtests (Task 6)
- Allows strategies to query `has_earnings_between(symbol, start, end)` to filter out high-volatility binary event risk without forward-looking leakage.
