# M42: Advanced Signals & Pattern Detection Specification

**Status**: Implemented & Verified (M42 Tasks 1-5)  
**Implementation**: `crates/spectra-research/src/patterns.rs`  
**Tests**: `crates/spectra-research/tests/m42_patterns_reference.rs`

## 1. Pivot Detection & Level Clustering (Tasks 1 & 2)
- **Pivot Detection**: `find_pivots(bars, left_bars, right_bars)`. Completely deterministic across identical inputs.
- **Support & Resistance**: Clusters nearby pivot levels within configurable percentage tolerance. Suggests horizontal levels with touch frequency weighting.

## 2. Rule-Based Geometric Pattern Detection (Task 3)
- Explicit rule-based patterns (not a black box):
  - `DoubleTop`: Two consecutive highs of equal height ($\pm \text{tolerance}$) separated by a deep trough.
  - `DoubleBottom`: Two consecutive lows of equal depth ($\pm \text{tolerance}$) separated by an intermediate peak.
  - `HeadAndShoulders` and `Flags`.
- Outputs pattern classification, boundary bar indices, neckline price level, and calculated confidence metric.

## 3. Sector Return Heat Maps (Task 5)
- Aggregates instrument return distributions into sector-level average returns, directly consistent with M36 screener totals.
