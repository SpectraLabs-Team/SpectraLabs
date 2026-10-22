//! M1b task 12: tests proving NO look-ahead bias and NO survivorship bias.
//!
//! A universe that contains:
//! 1. `REVISED`: A symbol whose bar on 2020-03-01 had close $100 (known_at 2020-03-01),
//!    which was revised to $50 on 2020-03-04 (known_at 2020-03-04).
//! 2. `DELISTED`: A symbol that traded daily on 2020-03-01..2020-03-03, then delisted.
//! 3. `ACTIVE`: A symbol that traded daily throughout 2020-03-01..2020-03-05.
//!
//! We prove:
//! - As of 2020-03-02 (D1):
//!   - No look-ahead bias: REVISED trades at $100 (the unrevised price known on D1).
//!   - No survivorship bias: DELISTED is included in the universe and traded.
//! - As of 2020-03-05 (D2):
//!   - REVISED trades at $50 (incorporating the revision known by D2).
//!   - DELISTED stopped trading after its delisting date.

use std::path::PathBuf;

use spectra_backtest::*;
use spectra_core::Json;
use spectra_data::{load_csv, CsvOptions, DataStore};
use spectra_engine::RunControl;

const VERSION: &str = "0192f0c4-7a3b-7c1d-8e2f-0123456789ab";

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spectra-bias-proof-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn payload(text: &str) -> Json {
    Json::parse(text).unwrap()
}

#[test]
fn test_no_look_ahead_and_no_survivorship_bias() {
    let root = temp_dir("bias-proof");

    // Daily bars for ACTIVE, DELISTED, and REVISED.
    // Day 1: 2020-03-01
    // Day 2: 2020-03-02
    // Day 3: 2020-03-03 (last bar for DELISTED before delisting)
    // Day 4: 2020-03-04 (REVISED bar for 2020-03-01 is revised to 50, known_at 2020-03-04)
    // Day 5: 2020-03-05
    let csv = "\
timestamp,symbol,open,high,low,close,volume,known_at
2020-03-01T00:00:00Z,ACTIVE,10,12,9,10,10000,2020-03-01T00:00:00Z
2020-03-01T00:00:00Z,DELISTED,20,22,18,20,10000,2020-03-01T00:00:00Z
2020-03-01T00:00:00Z,REVISED,100,105,95,100,10000,2020-03-01T00:00:00Z
2020-03-02T00:00:00Z,ACTIVE,10,12,9,10,10000,2020-03-02T00:00:00Z
2020-03-02T00:00:00Z,DELISTED,20,22,18,20,10000,2020-03-02T00:00:00Z
2020-03-02T00:00:00Z,REVISED,100,105,95,100,10000,2020-03-02T00:00:00Z
2020-03-03T00:00:00Z,ACTIVE,11,13,10,11,10000,2020-03-03T00:00:00Z
2020-03-03T00:00:00Z,DELISTED,21,23,19,21,10000,2020-03-03T00:00:00Z
2020-03-03T00:00:00Z,REVISED,101,106,96,101,10000,2020-03-03T00:00:00Z
2020-03-01T00:00:00Z,REVISED,50,55,45,50,10000,2020-03-04T00:00:00Z
2020-03-02T00:00:00Z,REVISED,50,55,45,50,10000,2020-03-04T00:00:00Z
2020-03-04T00:00:00Z,ACTIVE,12,14,11,12,10000,2020-03-04T00:00:00Z
2020-03-04T00:00:00Z,REVISED,102,107,97,102,10000,2020-03-04T00:00:00Z
2020-03-05T00:00:00Z,ACTIVE,13,15,12,13,10000,2020-03-05T00:00:00Z
2020-03-05T00:00:00Z,REVISED,103,108,98,103,10000,2020-03-05T00:00:00Z
";
    let opts = CsvOptions { known_at_column: "known_at".into(), ..Default::default() };
    let d = load_csv(csv.as_bytes(), &opts).unwrap();
    let checksum = DataStore::new(root.clone()).unwrap().put(&d).unwrap();

    // ------------------------------------------------------------------------
    // RUN 1: as of 2020-03-02 (D1)
    // - REVISED revision (known 2020-03-04) must NOT be visible (No Look-Ahead).
    // - Bars after 2020-03-02 must NOT be visible.
    // - DELISTED must be present and traded (No Survivorship Bias).
    // ------------------------------------------------------------------------
    let job_d1 = BacktestJob::from_payload(&payload(&format!(
        r#"{{
            "dataset_version_id": "{VERSION}",
            "strategy": "buy_and_hold",
            "as_of": "2020-03-02T00:00:00Z",
            "config": {{ "preset": "zero_cost" }}
        }}"#
    )))
    .unwrap();

    let mut ctl = RunControl::default();
    let prod_d1 = run_job_from_store(&root, &checksum, &job_d1, &mut ctl).unwrap();

    // 6 bars total (2 for ACTIVE, 2 for DELISTED, 2 for REVISED).
    assert_eq!(prod_d1.run.bar_count, 6);

    // Verify instrument symbols in summary
    let symbols_d1: Vec<&str> = prod_d1
        .summary
        .find("instruments")
        .unwrap()
        .items()
        .unwrap()
        .iter()
        .map(|r| r.find("symbol").unwrap().as_str().unwrap())
        .collect();

    // PROOF OF NO SURVIVORSHIP BIAS: DELISTED is included in the backtest!
    assert!(
        symbols_d1.contains(&"DELISTED"),
        "Universe as of D1 must include DELISTED symbol (no survivorship bias): got {symbols_d1:?}"
    );
    assert!(symbols_d1.contains(&"ACTIVE"));
    assert!(symbols_d1.contains(&"REVISED"));

    // PROOF OF NO LOOK-AHEAD BIAS:
    // Fills CSV must show REVISED filled at the unrevised price (100.0), NOT at the revised price (50.0).
    let fills_csv_d1 = prod_d1
        .files
        .iter()
        .find(|f| f.kind == "fills_csv")
        .map(|f| std::str::from_utf8(&f.bytes).unwrap())
        .unwrap();

    assert!(
        fills_csv_d1.contains("100"),
        "Fills on D1 must execute at unrevised price 100 (no look-ahead): {fills_csv_d1}"
    );
    assert!(
        !fills_csv_d1.contains(",50.") && !fills_csv_d1.contains(",50,"),
        "Fills on D1 must NOT see revised price 50 (look-ahead violation): {fills_csv_d1}"
    );

    // ------------------------------------------------------------------------
    // RUN 2: as of 2020-03-05 (D2)
    // - REVISED revision (known 2020-03-04) IS now visible.
    // - All 5 daily periods are included.
    // ------------------------------------------------------------------------
    let job_d2 = BacktestJob::from_payload(&payload(&format!(
        r#"{{
            "dataset_version_id": "{VERSION}",
            "strategy": "buy_and_hold",
            "as_of": "2020-03-05T00:00:00Z",
            "config": {{ "preset": "zero_cost" }}
        }}"#
    )))
    .unwrap();

    let mut ctl = RunControl::default();
    let prod_d2 = run_job_from_store(&root, &checksum, &job_d2, &mut ctl).unwrap();

    // In D2, 13 bars total (ACTIVE: 5, DELISTED: 3, REVISED: 5).
    assert_eq!(prod_d2.run.bar_count, 13);

    let fills_csv_d2 = prod_d2
        .files
        .iter()
        .find(|f| f.kind == "fills_csv")
        .map(|f| std::str::from_utf8(&f.bytes).unwrap())
        .unwrap();

    // After revision date, REVISED initial bar is 50.0!
    assert!(fills_csv_d2.contains("50"), "Fills on D2 must reflect revised price 50: {fills_csv_d2}");

    // Result fingerprints differ between point-in-time snapshots
    assert_ne!(
        prod_d1.run.result_sha256, prod_d2.run.result_sha256,
        "Backtest results must differ across point-in-time dates"
    );

    let _ = std::fs::remove_dir_all(root);
}
