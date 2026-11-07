# M39: Market & Formula Alerts Specification

**Status**: Implemented & Verified (M39 Tasks 1-8)  
**Implementation**: `crates/spectra-research/src/alerts.rs`  
**Tests**: `crates/spectra-research/tests/m39_alerts_reference.rs`

## 1. Alert Types & Conditions (Task 2 & Task 4)
- **Price Alerts**: `PriceAbove`, `PriceBelow`, `PercentMove`.
- **Formula Alerts**: Custom M38 formula evaluated at bar close (`evaluate_formula`).
- **Screen Alerts**: Triggered when a symbol enters a saved M36 screen universe.
- **Portfolio Breach Alerts**: Triggered when risk/drawdown limits are exceeded.

## 2. Trigger Frequency & Deduplication (Task 3 & Task 8)
- `OnceOnly`: Fires once and automatically disarms (`active = false`). Tested to ensure exactly one notification across subsequent runs.
- `Repeating`: Rate-limited by `min_interval_ns` to prevent email/webhook spam.

## 3. History & Audit Logging (Task 7)
- Every trigger event logs `alert_id`, `alert_name`, `symbol`, `triggered_ts_ns`, `trigger_value`, and descriptive message.
