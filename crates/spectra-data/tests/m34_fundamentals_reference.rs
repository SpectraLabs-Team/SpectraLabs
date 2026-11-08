//! M34 Fundamentals Data & Point-in-Time Store Reference Tests.

use spectra_data::fundamentals::*;

#[test]
fn test_m34_point_in_time_restatement_isolation() {
    let mut store = PointInTimeFundamentalsStore::new();

    // MSFT FY2024 initial filing on 2024-07-30
    let ts_initial = 1_722_340_800_000_000_000i64;
    // MSFT FY2024 restatement filed on 2024-10-15
    let ts_restated = 1_728_993_600_000_000_000i64;

    let initial = FundamentalRecord {
        symbol: "MSFT".into(),
        period: "2024-FY".into(),
        fiscal_year: 2024,
        fiscal_quarter: 0,
        metric: "net_income".into(),
        value: 88_136_000_000.0,
        currency: "USD".into(),
        publication_ts_ns: ts_initial,
        restatement_index: 0,
        filing_ref: "0000950170-24-087843".into(),
    };

    let restatement = FundamentalRecord {
        symbol: "MSFT".into(),
        period: "2024-FY".into(),
        fiscal_year: 2024,
        fiscal_quarter: 0,
        metric: "net_income".into(),
        value: 88_150_000_000.0,
        currency: "USD".into(),
        publication_ts_ns: ts_restated,
        restatement_index: 1,
        filing_ref: "0000950170-24-099999".into(),
    };

    store.ingest(vec![initial, restatement]);

    // Query on 2024-08-15 (between initial and restatement): must return initial value
    let query_aug = store
        .query_point_in_time("MSFT", "2024-FY", "net_income", ts_initial + 86_400_000_000_000)
        .expect("should return record");
    assert_eq!(query_aug.value, 88_136_000_000.0);
    assert_eq!(query_aug.restatement_index, 0);

    // Query on 2024-11-01 (after restatement): must return restated value
    let query_nov = store
        .query_point_in_time("MSFT", "2024-FY", "net_income", ts_restated + 86_400_000_000_000)
        .expect("should return record");
    assert_eq!(query_nov.value, 88_150_000_000.0);
    assert_eq!(query_nov.restatement_index, 1);
}

#[test]
fn test_m34_symbol_master_lifecycle_and_delisting() {
    let mut master = SymbolMaster::new();

    master.add_entry(SymbolMetadata {
        symbol: "SVB".into(),
        company_name: "Silicon Valley Bank".into(),
        sector: "Financial Services".into(),
        industry: "Regional Banking".into(),
        exchange: "NASDAQ".into(),
        country: "USA".into(),
        status: ListingStatus::Active,
        effective_from_ns: 1_000_000,
        effective_to_ns: Some(5_000_000),
    });

    master.add_entry(SymbolMetadata {
        symbol: "SVB".into(),
        company_name: "Silicon Valley Bank".into(),
        sector: "Financial Services".into(),
        industry: "Regional Banking".into(),
        exchange: "OTC".into(),
        country: "USA".into(),
        status: ListingStatus::Delisted,
        effective_from_ns: 5_000_000,
        effective_to_ns: None,
    });

    assert!(master.is_active_at("SVB", 2_000_000));
    assert!(!master.is_active_at("SVB", 6_000_000));
    assert_eq!(master.symbol_history("SVB").len(), 2);
}

#[test]
fn test_m34_quality_checks_quarantine() {
    let mut store = PointInTimeFundamentalsStore::new();

    let records = vec![
        FundamentalRecord {
            symbol: "BAD".into(),
            period: "2024-Q1".into(),
            fiscal_year: 2024,
            fiscal_quarter: 1,
            metric: "total_assets".into(),
            value: -1_000_000.0, // impossible negative
            currency: "USD".into(),
            publication_ts_ns: 1000,
            restatement_index: 0,
            filing_ref: "err".into(),
        },
        FundamentalRecord {
            symbol: "GOOD".into(),
            period: "2024-Q1".into(),
            fiscal_year: 2024,
            fiscal_quarter: 1,
            metric: "total_assets".into(),
            value: 50_000_000.0,
            currency: "USD".into(),
            publication_ts_ns: 1000,
            restatement_index: 0,
            filing_ref: "ok".into(),
        },
    ];

    store.ingest(records);
    assert_eq!(store.accepted_count(), 1);
    assert_eq!(store.quarantined_records().len(), 1);
    assert_eq!(store.quarantined_records()[0].category, "IMPOSSIBLE_NEGATIVE");
}
