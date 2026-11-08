//! M36 Watchlists & Screener Reference Tests.

use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_data::fundamentals::{FundamentalRecord, PointInTimeFundamentalsStore};
use spectra_research::screener::*;

fn test_bar(ts: i64, p: f64, v: f64) -> Bar {
    Bar::new(
        ts,
        0,
        Price::from_f64(p).unwrap(),
        Price::from_f64(p + 1.0).unwrap(),
        Price::from_f64(p - 1.0).unwrap(),
        Price::from_f64(p).unwrap(),
        Qty::from_f64(v).unwrap(),
    )
}

#[test]
fn test_m36_point_in_time_screener_lookahead_prevention() {
    let mut fund_store = PointInTimeFundamentalsStore::new();

    let ts_initial = 1_000_000_000i64;
    let ts_restated = 2_000_000_000i64;

    // AAPL filed revenue = 100M at ts_initial, restated to 50M at ts_restated
    fund_store.ingest(vec![
        FundamentalRecord {
            symbol: "AAPL".into(),
            period: "2024-Q1".into(),
            fiscal_year: 2024,
            fiscal_quarter: 1,
            metric: "revenue".into(),
            value: 100_000_000.0,
            currency: "USD".into(),
            publication_ts_ns: ts_initial,
            restatement_index: 0,
            filing_ref: "f1".into(),
        },
        FundamentalRecord {
            symbol: "AAPL".into(),
            period: "2024-Q1".into(),
            fiscal_year: 2024,
            fiscal_quarter: 1,
            metric: "revenue".into(),
            value: 50_000_000.0, // restatement
            currency: "USD".into(),
            publication_ts_ns: ts_restated,
            restatement_index: 1,
            filing_ref: "f2".into(),
        },
        FundamentalRecord {
            symbol: "AAPL".into(),
            period: "2024-Q1".into(),
            fiscal_year: 2024,
            fiscal_quarter: 1,
            metric: "net_income".into(),
            value: 15_000_000.0,
            currency: "USD".into(),
            publication_ts_ns: ts_initial,
            restatement_index: 0,
            filing_ref: "f1".into(),
        },
    ]);

    let bars: Vec<Bar> = (1..=30).map(|i| test_bar(i * 1000, 150.0, 10000.0)).collect();
    let universe = [SymbolScreenData { symbol: "AAPL", bars: &bars, shares_outstanding: Some(1_000_000.0) }];

    // Screen requiring net margin >= 20%
    // At ts_initial: margin = 15M / 100M = 15% -> fails!
    // At ts_restated: margin = 15M / 50M = 30% -> passes!
    let def = ScreenDefinition {
        name: "High Margin Screen".into(),
        technical_filters: vec![TechnicalFilter::MinPrice(100.0)],
        fundamental_filters: vec![FundamentalFilter::MinNetMargin(0.20)],
        rankings: vec![],
    };

    // Run screen as of 1_500_000_000 (before restatement)
    let res_before = run_screen(&def, &universe, &fund_store, 1_500_000_000, "ds1");
    assert_eq!(res_before.matches.len(), 0, "Must not see future restatement!");

    // Run screen as of 2_500_000_000 (after restatement)
    let res_after = run_screen(&def, &universe, &fund_store, 2_500_000_000, "ds1");
    assert_eq!(res_after.matches.len(), 1, "Must see restatement once published!");
}

#[test]
fn test_m36_ranking_reproducibility_and_csv_export() {
    let fund_store = PointInTimeFundamentalsStore::new();
    let bars1: Vec<Bar> = (1..=30).map(|i| test_bar(i * 1000, 200.0, 20000.0)).collect();
    let bars2: Vec<Bar> = (1..=30).map(|i| test_bar(i * 1000, 100.0, 10000.0)).collect();

    let universe = [
        SymbolScreenData { symbol: "HIGH_VOL", bars: &bars1, shares_outstanding: None },
        SymbolScreenData { symbol: "LOW_VOL", bars: &bars2, shares_outstanding: None },
    ];

    let def = ScreenDefinition {
        name: "Volume Rank".into(),
        technical_filters: vec![],
        fundamental_filters: vec![],
        rankings: vec![RankingWeight { name: "volume".into(), weight: 1.0, ascending: false }],
    };

    let res1 = run_screen(&def, &universe, &fund_store, 50_000, "ds_v1");
    let res2 = run_screen(&def, &universe, &fund_store, 50_000, "ds_v1");

    assert_eq!(res1.matches[0].symbol, "HIGH_VOL");
    assert_eq!(res1.matches[1].symbol, "LOW_VOL");
    assert_eq!(res1.matches, res2.matches, "Rankings must be 100% deterministic!");

    let csv = res1.to_csv();
    assert!(csv.starts_with("Symbol,Price,Volume,RSI14,PE,NetMargin,RankScore\n"));
}
