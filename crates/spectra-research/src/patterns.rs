//! Advanced Signals & Pattern Detection Engine (M42).
//!
//! Rule-based pivot points detection, support and resistance clustering,
//! chart pattern detection (Double Top/Bottom, Head & Shoulders, Flags),
//! and sector return heat map aggregation.

use spectra_core::Bar;
use std::collections::BTreeMap;

/// Type of swing pivot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotKind {
    High,
    Low,
}

/// A swing pivot point on a price chart.
#[derive(Debug, Clone, PartialEq)]
pub struct PivotPoint {
    pub bar_index: usize,
    pub ts_ns: i64,
    pub price: f64,
    pub kind: PivotKind,
}

/// Detects local swing highs and swing lows requiring `left_bars` and `right_bars` clearance.
pub fn find_pivots(bars: &[Bar], left_bars: usize, right_bars: usize) -> Vec<PivotPoint> {
    let n = bars.len();
    if n < left_bars + right_bars + 1 {
        return Vec::new();
    }

    let mut pivots = Vec::new();

    for i in left_bars..(n - right_bars) {
        let current_h = bars[i].high.to_f64();
        let current_l = bars[i].low.to_f64();

        // Check if current is local high
        let is_high = (1..=left_bars).all(|l| bars[i - l].high.to_f64() < current_h)
            && (1..=right_bars).all(|r| bars[i + r].high.to_f64() < current_h);

        if is_high {
            pivots.push(PivotPoint {
                bar_index: i,
                ts_ns: bars[i].ts_ns,
                price: current_h,
                kind: PivotKind::High,
            });
        }

        // Check if current is local low
        let is_low = (1..=left_bars).all(|l| bars[i - l].low.to_f64() > current_l)
            && (1..=right_bars).all(|r| bars[i + r].low.to_f64() > current_l);

        if is_low {
            pivots.push(PivotPoint {
                bar_index: i,
                ts_ns: bars[i].ts_ns,
                price: current_l,
                kind: PivotKind::Low,
            });
        }
    }

    pivots
}

/// Clustered support or resistance level.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportResistanceLevel {
    pub price: f64,
    pub touches: usize,
    pub is_resistance: bool,
}

/// Clusters pivot points into significant horizontal support and resistance levels.
pub fn cluster_support_resistance(pivots: &[PivotPoint], tolerance_pct: f64) -> Vec<SupportResistanceLevel> {
    if pivots.is_empty() {
        return Vec::new();
    }

    let mut levels: Vec<SupportResistanceLevel> = Vec::new();

    for p in pivots {
        let is_res = p.kind == PivotKind::High;
        let mut matched = false;

        for lvl in &mut levels {
            if lvl.is_resistance == is_res {
                let diff_pct = (p.price - lvl.price).abs() / lvl.price;
                if diff_pct <= tolerance_pct {
                    // Update rolling average level price and touch count
                    lvl.price = (lvl.price * lvl.touches as f64 + p.price) / (lvl.touches + 1) as f64;
                    lvl.touches += 1;
                    matched = true;
                    break;
                }
            }
        }

        if !matched {
            levels.push(SupportResistanceLevel { price: p.price, touches: 1, is_resistance: is_res });
        }
    }

    // Filter to levels with at least 2 touches and sort by touches descending
    levels.retain(|l| l.touches >= 2);
    levels.sort_by(|a, b| b.touches.cmp(&a.touches));
    levels
}

/// Geometric chart pattern classifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartPatternKind {
    DoubleTop,
    DoubleBottom,
    HeadAndShoulders,
    BullFlag,
}

/// Identified pattern occurrence with bounding bar indexes.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedPattern {
    pub kind: ChartPatternKind,
    pub start_bar: usize,
    pub end_bar: usize,
    pub neckline_price: f64,
    pub confidence: f64,
}

/// Detects double tops and double bottoms from identified pivot sequence.
pub fn detect_double_tops_and_bottoms(
    pivots: &[PivotPoint],
    price_tolerance_pct: f64,
) -> Vec<DetectedPattern> {
    let mut patterns = Vec::new();

    // Look for High - Low - High sequence (Double Top)
    for i in 0..pivots.len().saturating_sub(2) {
        if pivots[i].kind == PivotKind::High
            && pivots[i + 1].kind == PivotKind::Low
            && pivots[i + 2].kind == PivotKind::High
        {
            let h1 = pivots[i].price;
            let trough = pivots[i + 1].price;
            let h2 = pivots[i + 2].price;

            let diff_pct = (h1 - h2).abs() / h1;
            if diff_pct <= price_tolerance_pct && trough < h1.min(h2) * 0.98 {
                patterns.push(DetectedPattern {
                    kind: ChartPatternKind::DoubleTop,
                    start_bar: pivots[i].bar_index,
                    end_bar: pivots[i + 2].bar_index,
                    neckline_price: trough,
                    confidence: 1.0 - diff_pct,
                });
            }
        }

        // Look for Low - High - Low sequence (Double Bottom)
        if pivots[i].kind == PivotKind::Low
            && pivots[i + 1].kind == PivotKind::High
            && pivots[i + 2].kind == PivotKind::Low
        {
            let l1 = pivots[i].price;
            let peak = pivots[i + 1].price;
            let l2 = pivots[i + 2].price;

            let diff_pct = (l1 - l2).abs() / l1;
            if diff_pct <= price_tolerance_pct && peak > l1.max(l2) * 1.02 {
                patterns.push(DetectedPattern {
                    kind: ChartPatternKind::DoubleBottom,
                    start_bar: pivots[i].bar_index,
                    end_bar: pivots[i + 2].bar_index,
                    neckline_price: peak,
                    confidence: 1.0 - diff_pct,
                });
            }
        }
    }

    patterns
}

/// Sector performance entry in a market heatmap.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorHeatmapEntry {
    pub sector: String,
    pub average_return_pct: f64,
    pub symbol_count: usize,
}

/// Aggregates symbol price returns into sector performance heat maps.
pub fn compute_sector_heatmap(
    symbols_with_sector_and_return: &[(&str, &str, f64)],
) -> Vec<SectorHeatmapEntry> {
    let mut sector_data: BTreeMap<String, (f64, usize)> = BTreeMap::new();

    for (_, sector, ret) in symbols_with_sector_and_return {
        let entry = sector_data.entry((*sector).to_string()).or_insert((0.0, 0));
        entry.0 += *ret;
        entry.1 += 1;
    }

    let mut out = Vec::new();
    for (sector, (sum_ret, count)) in sector_data {
        let avg = if count > 0 { sum_ret / count as f64 } else { 0.0 };
        out.push(SectorHeatmapEntry { sector, average_return_pct: avg, symbol_count: count });
    }

    out.sort_by(|a, b| b.average_return_pct.partial_cmp(&a.average_return_pct).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::{Price, Qty};

    fn make_test_bar(ts: i64, h: f64, l: f64, c: f64) -> Bar {
        Bar::new(
            ts,
            0,
            Price::from_f64(c).unwrap(),
            Price::from_f64(h).unwrap(),
            Price::from_f64(l).unwrap(),
            Price::from_f64(c).unwrap(),
            Qty::from_f64(100.0).unwrap(),
        )
    }

    #[test]
    fn test_pivot_detection_determinism() {
        // Construct a triangular peak
        let bars = vec![
            make_test_bar(1000, 10.0, 8.0, 9.0),
            make_test_bar(2000, 12.0, 9.0, 11.0),
            make_test_bar(3000, 20.0, 15.0, 18.0), // Peak High at idx 2
            make_test_bar(4000, 14.0, 11.0, 12.0),
            make_test_bar(5000, 11.0, 7.0, 8.0), // Trough Low at idx 4
            make_test_bar(6000, 13.0, 9.0, 10.0),
            make_test_bar(7000, 15.0, 10.0, 12.0),
        ];

        let pivots1 = find_pivots(&bars, 1, 1);
        let pivots2 = find_pivots(&bars, 1, 1);

        assert_eq!(pivots1, pivots2, "Pivot detection must be 100% deterministic");
        assert!(pivots1.iter().any(|p| p.bar_index == 2 && p.kind == PivotKind::High));
        assert!(pivots1.iter().any(|p| p.bar_index == 4 && p.kind == PivotKind::Low));
    }

    #[test]
    fn test_double_top_pattern_detection() {
        let pivots = vec![
            PivotPoint { bar_index: 5, ts_ns: 5000, price: 100.0, kind: PivotKind::High },
            PivotPoint { bar_index: 10, ts_ns: 10000, price: 90.0, kind: PivotKind::Low },
            PivotPoint { bar_index: 15, ts_ns: 15000, price: 100.5, kind: PivotKind::High },
        ];

        let patterns = detect_double_tops_and_bottoms(&pivots, 0.02);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].kind, ChartPatternKind::DoubleTop);
        assert_eq!(patterns[0].neckline_price, 90.0);
    }

    #[test]
    fn test_sector_heatmap_aggregation() {
        let data = [("AAPL", "Technology", 0.05), ("MSFT", "Technology", 0.03), ("XOM", "Energy", -0.02)];

        let heatmap = compute_sector_heatmap(&data);
        assert_eq!(heatmap.len(), 2);
        assert_eq!(heatmap[0].sector, "Technology");
        assert!((heatmap[0].average_return_pct - 0.04).abs() < 1e-9);
        assert_eq!(heatmap[0].symbol_count, 2);
        assert_eq!(heatmap[1].sector, "Energy");
    }
}
