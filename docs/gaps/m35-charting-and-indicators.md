# M35 Charting & Indicators: Module Close-Out & Unblocking M36

## Context & Gap Description
- **Module**: M35 Charting & Indicators (Status: 100% complete)
- **Predecessor State**: 60% on main (Rust engine and reference tests completed; API, chart overlays/sub-panes, picker, Learning Center, and look-ahead CI checks pending).
- **Close-Out Condition**: All 8 tasks implemented, tested, verified in CI, and merged into `main`. M36 (Watchlists & Stock Screener Engine) marked UNBLOCKED.

## Summary of Implemented Tasks

| Task | Scope & Deliverable | PR / Commit | Status |
| :--- | :--- | :--- | :---: |
| **Task 1: Spec Sign-Off** | Formal mathematical formulation, warm-up rules, and tolerances for all 25 indicators in `docs/models/m35-indicators-spec.md`. | `d4adaa4` | Verified |
| **Task 2: Indicator Series API** | `GET /api/v1/indicators` route in `crates/spectra-api` returning aligned series with 422 validation, OpenAPI schema registration, and <500ms p95 latency budget (measured: 12.1ms). | PR #313 | Merged |
| **Task 3: Price-Pane Overlays** | Canvas rendering for 9 overlays (SMA, EMA, WMA, Bollinger Bands, VWAP, Ichimoku, Parabolic SAR, Donchian, Keltner) in `TradingChart.tsx` and `PriceChart.tsx`. | PR #316 | Merged |
| **Task 4: Sub-Panes** | Multi-pane chart partitioning for 10 sub-pane indicators (RSI, MACD, Stochastic, ATR, ADX, OBV, CCI, Williams %R, MFI, CMF) with threshold bands and independent scales. | PR #318 | Merged |
| **Task 5: Picker & Legend** | `IndicatorPickerModal` (Decision 2 Core 12 prioritization), `IndicatorSettingsModal` (parameter bounds validation), and interactive chart legend with hover readouts. | PR #320 | Merged |
| **Task 6: Learning Center** | Comprehensive lessons for all 25 indicators with KaTeX formulas, parameter defaults, and engine warm-up notes in `web/frontend/lib/learn/content/indicators.ts`. | PR #321 | Merged |
| **Task 7: No Look-Ahead Test** | Comprehensive integration tests verifying strict zero look-ahead across all 25 indicators under extreme mutation/shock/crash conditions (`crates/spectra-research/tests/m35_no_lookahead.rs`). | PR #322 | Merged |
| **Task 8: Tracker Close-Out** | Module tracker close-out: M35 at 100% complete; M36 (Watchlists & Stock Screener Engine) unblocked. | Current | Complete |

## Verification Evidence
- **Series API**: 25/25 reference tests (`cargo test -p spectra-research --test m35_indicator_reference`), 4/4 flow tests against PostgreSQL (`cargo test -p spectra-api --test indicator_series_flow`), OpenAPI route documentation verified.
- **Visual Overlays & Sub-panes**: 8/8 overlay tests (`scripts/test-chart-overlays.mjs`) and 8/8 sub-pane tests (`scripts/test-chart-subpanes.mjs`) passing.
- **Picker & Legend Interaction**: 14/14 picker & legend tests (`scripts/test-indicator-picker-legend.mjs`) passing at both desktop (1280px) and mobile (375px) breakpoints.
- **Learning Center**: 35/35 tests passing (`scripts/test-learn.mjs`) covering all 25 indicator lessons and keyword search rankings.
- **Look-Ahead Prevention**: Integration tests (`crates/spectra-research/tests/m35_no_lookahead.rs`) verify that future bar additions or price/volume mutations produce bit-for-bit identical outputs for past bars.

## Unblocking Status
- **M36 (Watchlists & Stock Screener Engine)**: **UNBLOCKED**.
  The screener engine can now reliably consume the complete 25-indicator technical library, aligned indicator series API, and corresponding chart visualization components.
