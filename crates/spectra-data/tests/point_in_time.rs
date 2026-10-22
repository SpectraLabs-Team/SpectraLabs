//! Tests for point-in-time data ingest, storage, and as_of resolution (M1b Task 12).

use spectra_core::Price;
use spectra_data::{load_csv, CsvOptions, DataStore, TimeFormat, TimeOptions};
use std::fs;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("spectra-pit-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn test_pit_csv_ingest_revisions_and_resolution() {
    // CSV with an explicit known_at column.
    // Symbol AAPL has an initial close of 100 on 2024-01-02 (known at 2024-01-02),
    // and a revision of 105 known on 2024-01-10.
    // MSFT has normal bars on 2024-01-02 and 2024-01-03 without revisions.
    let csv = "\
date,known_at,symbol,open,high,low,close,volume
2024-01-02T16:00:00Z,2024-01-02T16:00:00Z,AAPL,99.0,101.0,98.5,100.0,1000
2024-01-02T16:00:00Z,2024-01-10T12:00:00Z,AAPL,99.0,106.0,98.5,105.0,1000
2024-01-02T16:00:00Z,2024-01-02T16:00:00Z,MSFT,370.0,375.0,369.0,372.0,2000
2024-01-03T16:00:00Z,2024-01-03T16:00:00Z,MSFT,372.0,376.0,371.0,374.0,2100
";

    let opt = CsvOptions {
        time: TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None },
        known_at_column: "known_at".to_string(),
        ..CsvOptions::default()
    };

    let dataset = load_csv(csv.as_bytes(), &opt).expect("CSV should parse successfully");
    assert_eq!(dataset.bars.len(), 4, "dataset holds all 4 bars including the revision");
    assert_eq!(dataset.symbols.symbols, vec!["AAPL", "MSFT"]);

    let aapl_id = dataset.symbols.id("AAPL").unwrap();
    let _msft_id = dataset.symbols.id("MSFT").unwrap();

    // Verify raw bars have correct known_at_ns
    let aapl_bars: Vec<_> = dataset.bars.iter().filter(|b| b.instrument == aapl_id).collect();
    assert_eq!(aapl_bars.len(), 2);
    assert_eq!(aapl_bars[0].close, Price::from_raw(100_0000_0000));
    assert_eq!(aapl_bars[1].close, Price::from_raw(105_0000_0000));
    assert!(aapl_bars[1].known_at_ns > aapl_bars[0].known_at_ns);

    // 1. As of 2024-01-05 (before the 2024-01-10 revision):
    // AAPL should have close = 100.0, NOT 105.0 (No Look-Ahead!)
    let ts_2024_01_05 = 1_704_499_200 * 1_000_000_000; // 2024-01-06 00:00:00 UTC
    let pit_early = dataset.as_of(ts_2024_01_05);
    assert_eq!(pit_early.bars.len(), 3, "2 MSFT bars + 1 unrevised AAPL bar");
    let aapl_early: Vec<_> = pit_early.bars.iter().filter(|b| b.instrument == aapl_id).collect();
    assert_eq!(aapl_early.len(), 1);
    assert_eq!(aapl_early[0].close, Price::from_raw(100_0000_0000), "must see unrevised close");

    // 2. As of 2024-01-15 (after the revision):
    // AAPL should have the updated close = 105.0
    let ts_2024_01_15 = 1_705_363_200 * 1_000_000_000;
    let pit_late = dataset.as_of(ts_2024_01_15);
    assert_eq!(pit_late.bars.len(), 3);
    let aapl_late: Vec<_> = pit_late.bars.iter().filter(|b| b.instrument == aapl_id).collect();
    assert_eq!(aapl_late.len(), 1);
    assert_eq!(aapl_late[0].close, Price::from_raw(105_0000_0000), "must see revised close");

    // 3. Test Storage round-trip in DataStore (QFBARS02)
    let tmp = temp_dir("pit-store");
    let store = DataStore::new(&tmp).unwrap();
    let id = store.put(&dataset).expect("storing PIT dataset should succeed");
    assert!(store.contains(&id));

    let loaded = store.get(&id).expect("retrieving PIT dataset should succeed");
    assert_eq!(loaded.bars.len(), 4);
    assert_eq!(loaded.bars, dataset.bars);

    // Verify as_of on loaded dataset produces identical result
    let loaded_pit_early = loaded.as_of(ts_2024_01_05);
    assert_eq!(loaded_pit_early.bars, pit_early.bars);

    fs::remove_dir_all(&tmp).ok();
}
