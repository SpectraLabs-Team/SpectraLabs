//! M42 Advanced Signals & Pattern Detection Reference Tests.

use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_research::patterns::*;

fn bar(ts: i64, h: f64, l: f64, c: f64) -> Bar {
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
fn test_m42_pivot_point_and_sr_clustering() {
    let bars = vec![
        bar(1000, 10.0, 8.0, 9.0),
        bar(2000, 15.0, 9.0, 14.0),
        bar(3000, 20.0, 14.0, 19.0), // High pivot at 20.0 (idx 2)
        bar(4000, 15.0, 11.0, 13.0),
        bar(5000, 12.0, 8.0, 9.0), // Low pivot at 8.0 (idx 4)
        bar(6000, 16.0, 10.0, 15.0),
        bar(7000, 20.2, 14.0, 19.0), // Second high pivot near 20.0 (idx 6)
        bar(8000, 15.0, 11.0, 12.0),
        bar(9000, 11.0, 7.8, 8.5), // Second low pivot near 8.0 (idx 8)
        bar(10000, 14.0, 9.0, 11.0),
    ];

    let pivots = find_pivots(&bars, 1, 1);
    assert!(pivots.len() >= 4);

    let sr_levels = cluster_support_resistance(&pivots, 0.03);
    // Should identify a resistance level near ~20.1 with 2 touches
    let res_level = sr_levels.iter().find(|l| l.is_resistance).unwrap();
    assert_eq!(res_level.touches, 2);
    assert!((res_level.price - 20.1).abs() < 0.2);

    // Should identify a support level near ~7.9 with 2 touches
    let sup_level = sr_levels.iter().find(|l| !l.is_resistance).unwrap();
    assert_eq!(sup_level.touches, 2);
    assert!((sup_level.price - 7.9).abs() < 0.2);
}

#[test]
fn test_m42_double_bottom_pattern() {
    let pivots = vec![
        PivotPoint { bar_index: 2, ts_ns: 2000, price: 50.0, kind: PivotKind::Low },
        PivotPoint { bar_index: 5, ts_ns: 5000, price: 65.0, kind: PivotKind::High },
        PivotPoint { bar_index: 8, ts_ns: 8000, price: 49.5, kind: PivotKind::Low },
    ];

    let patterns = detect_double_tops_and_bottoms(&pivots, 0.02);
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].kind, ChartPatternKind::DoubleBottom);
    assert_eq!(patterns[0].neckline_price, 65.0);
    assert!(patterns[0].confidence > 0.95);
}
