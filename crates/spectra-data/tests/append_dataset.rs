//! Tests for dataset refresh and appending new bars into new versions (M1b Task 13 Step 2).

use spectra_core::{Bar, Price, Qty};
use spectra_data::{append_bars, load_csv, CsvOptions, DataStore, SymbolTable, TimeFormat, TimeOptions};
use std::fs;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("spectra-append-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn p(v: i64) -> Price {
    Price::from_units(v).unwrap()
}

fn q(v: i64) -> Qty {
    Qty::from_units(v).unwrap()
}

#[test]
fn test_append_bars_chronological_advancement() {
    let csv = "\
date,symbol,open,high,low,close,volume
2024-01-02T16:00:00Z,AAPL,100,105,99,102,1000
2024-01-03T16:00:00Z,AAPL,102,106,101,104,1100
";
    let opt = CsvOptions {
        time: TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None },
        ..CsvOptions::default()
    };
    let base = load_csv(csv.as_bytes(), &opt).expect("base CSV loads");
    assert_eq!(base.bars.len(), 2);

    // New bars for 2024-01-04
    let ts_day4 = 1_704_384_000 * 1_000_000_000 + 16 * 3_600 * 1_000_000_000;
    let new_bars = vec![Bar::new(ts_day4, 0, p(104), p(108), p(103), p(107), q(1200))];

    let refreshed = append_bars(&base, &new_bars, &base.symbols, "Daily refresh 2024-01-04")
        .expect("append should succeed");

    assert_eq!(refreshed.bars.len(), 3);
    assert_eq!(refreshed.symbols.symbols, vec!["AAPL"]);
    assert_eq!(refreshed.info.bar_count, 3);
    assert_eq!(refreshed.info.parent_id, base.info.id);
    assert_eq!(refreshed.info.note, "Daily refresh 2024-01-04");
    assert_eq!(refreshed.info.first_ts, base.bars[0].ts_ns);
    assert_eq!(refreshed.info.last_ts, ts_day4);
    assert_ne!(refreshed.info.id, base.info.id);

    // DataStore roundtrip
    let tmp = temp_dir("chrono");
    let store = DataStore::new(&tmp).unwrap();
    let stored_id = store.put(&refreshed).unwrap();
    assert_eq!(stored_id, refreshed.info.id);
    let loaded = store.get(&stored_id).unwrap();
    assert_eq!(loaded.bars, refreshed.bars);
}

#[test]
fn test_append_bars_deduplication_and_in_place_update() {
    let csv = "\
date,symbol,open,high,low,close,volume
2024-01-02T16:00:00Z,AAPL,100,105,99,102,1000
2024-01-03T16:00:00Z,AAPL,102,106,101,104,1100
";
    let opt = CsvOptions {
        time: TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None },
        ..CsvOptions::default()
    };
    let base = load_csv(csv.as_bytes(), &opt).unwrap();

    // 1. Identical bar to 2024-01-03 -> deduplicated, len stays 2
    let bar_dup = base.bars[1];
    let refreshed_dup = append_bars(&base, &[bar_dup], &base.symbols, "duplicate").unwrap();
    assert_eq!(refreshed_dup.bars.len(), 2);
    assert_eq!(refreshed_dup.bars[1].close, base.bars[1].close);

    // 2. Bar with same timestamp, instrument and known_at, but updated close -> replaces in place
    let bar_updated = Bar::new(base.bars[1].ts_ns, 0, p(102), p(109), p(101), p(108), q(1500));
    let refreshed_updated = append_bars(&base, &[bar_updated], &base.symbols, "update").unwrap();
    assert_eq!(refreshed_updated.bars.len(), 2);
    assert_eq!(refreshed_updated.bars[1].close, p(108));
    assert_eq!(refreshed_updated.bars[1].high, p(109));
}

#[test]
fn test_append_bars_point_in_time_revisions() {
    let csv = "\
date,symbol,open,high,low,close,volume
2024-01-02T16:00:00Z,AAPL,100,105,99,102,1000
";
    let opt = CsvOptions {
        time: TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None },
        ..CsvOptions::default()
    };
    let base = load_csv(csv.as_bytes(), &opt).unwrap();
    assert_eq!(base.bars.len(), 1);

    let ts = base.bars[0].ts_ns;
    let revision_known_at = ts + 7 * 86_400 * 1_000_000_000; // revised 7 days later
    let revision_bar = Bar::with_known_at(ts, revision_known_at, 0, p(100), p(110), p(99), p(109), q(1000));

    let pit_dataset = append_bars(&base, &[revision_bar], &base.symbols, "Revision 7 days later").unwrap();
    // Both bars preserved: initial and revision
    assert_eq!(pit_dataset.bars.len(), 2);
    assert_eq!(pit_dataset.bars[0].close, p(102));
    assert_eq!(pit_dataset.bars[1].close, p(109));

    // Verify as_of point-in-time resolution
    let early = pit_dataset.as_of(ts + 86_400 * 1_000_000_000);
    assert_eq!(early.bars.len(), 1);
    assert_eq!(early.bars[0].close, p(102), "as_of early date sees original close");

    let late = pit_dataset.as_of(revision_known_at + 1_000_000);
    assert_eq!(late.bars.len(), 1);
    assert_eq!(late.bars[0].close, p(109), "as_of late date sees revised close");
}

#[test]
fn test_append_bars_symbol_table_expansion_and_remapping() {
    // Base dataset only has MSFT (index 0)
    let csv = "\
date,symbol,open,high,low,close,volume
2024-01-02T16:00:00Z,MSFT,370,375,369,372,2000
";
    let opt = CsvOptions {
        time: TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 0, timezone: None },
        ..CsvOptions::default()
    };
    let base = load_csv(csv.as_bytes(), &opt).unwrap();
    assert_eq!(base.symbols.symbols, vec!["MSFT"]);
    assert_eq!(base.bars[0].instrument, 0);

    // New bars introduce AAPL.
    // In sorted order, AAPL is index 0 and MSFT becomes index 1.
    let new_symbols = SymbolTable::from_symbols(vec!["AAPL"]);
    let ts = base.bars[0].ts_ns;
    let aapl_bar = Bar::new(ts, 0, p(100), p(105), p(99), p(102), q(1000));

    let expanded = append_bars(&base, &[aapl_bar], &new_symbols, "Add AAPL").unwrap();
    assert_eq!(expanded.symbols.symbols, vec!["AAPL", "MSFT"]);
    assert_eq!(expanded.bars.len(), 2);

    let aapl_id = expanded.symbols.id("AAPL").unwrap();
    let msft_id = expanded.symbols.id("MSFT").unwrap();
    assert_eq!(aapl_id, 0);
    assert_eq!(msft_id, 1);

    // Bars should be sorted by (ts_ns, instrument)
    assert_eq!(expanded.bars[0].instrument, aapl_id);
    assert_eq!(expanded.bars[0].close, p(102));

    assert_eq!(expanded.bars[1].instrument, msft_id);
    assert_eq!(expanded.bars[1].close, p(372));
}

#[test]
fn test_append_bars_error_handling() {
    let base_symbols = SymbolTable::from_symbols(vec!["AAPL"]);
    let empty_base = spectra_data::Dataset { symbols: base_symbols.clone(), ..Default::default() };

    // 1. Empty base and empty new bars -> Error
    let res = append_bars(&empty_base, &[], &base_symbols, "empty");
    assert!(res.is_err());

    // 2. Out of bounds instrument ID in new_bars -> Error
    let invalid_bar = Bar::new(100, 5, p(100), p(105), p(99), p(102), q(1000));
    let res = append_bars(&empty_base, &[invalid_bar], &base_symbols, "out of bounds");
    assert!(res.is_err());

    // 3. Invalid bar values (low > high) -> Error
    let bad_bar = Bar::new(100, 0, p(100), p(90), p(110), p(102), q(1000));
    let res = append_bars(&empty_base, &[bad_bar], &base_symbols, "bad bar");
    assert!(res.is_err());
}
