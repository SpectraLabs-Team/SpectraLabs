//! Integration tests for holdout dataset tamper detection (M1 Data Studio).
//! Verifies that:
//! 1. Holdout slices are hashed separately and verified on load.
//! 2. Any byte mutation, truncation, or slice substitution in the stored holdout fails the load with a clear error.

use std::fs;
use std::path::PathBuf;

use spectra_core::{Bar, Price, Qty};
use spectra_data::{
    compute_dataset_id, hash_holdout_slice, verify_holdout_slice_hash, DataStore, Dataset, DatasetInfo,
    SymbolTable,
};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn temp_store() -> (TempDir, DataStore) {
    let id = TEST_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir =
        std::env::temp_dir().join(format!("spectra_test_holdout_tamper_{nanos}_{id}_{}", std::process::id()));
    let store = DataStore::new(dir.clone()).unwrap();
    (TempDir(dir), store)
}

fn sample_holdout_dataset(n_bars: usize) -> Dataset {
    let mut symbols = SymbolTable::default();
    symbols.symbols.push("SPY".into());

    let mut bars = Vec::with_capacity(n_bars);
    let mut ts = 1_700_000_000_000_000_000_i64;
    for i in 0..n_bars {
        bars.push(Bar {
            ts_ns: ts,
            known_at_ns: ts,
            instrument: 0,
            open: Price::from_f64(100.0 + (i as f64) * 0.1).unwrap(),
            high: Price::from_f64(101.0 + (i as f64) * 0.1).unwrap(),
            low: Price::from_f64(99.5 + (i as f64) * 0.1).unwrap(),
            close: Price::from_f64(100.5 + (i as f64) * 0.1).unwrap(),
            volume: Qty::from_f64(10_000.0).unwrap(),
        });
        ts += 86_400_000_000_000;
    }

    let id = compute_dataset_id(&bars, &symbols);
    let bars_sha = spectra_core::bars_sha256(&bars);

    Dataset {
        bars,
        symbols,
        info: DatasetInfo { id, bars_sha256: bars_sha, bar_count: n_bars, ..Default::default() },
        ..Default::default()
    }
}

#[test]
fn test_holdout_slice_hash_and_verified_load_success() {
    let (_tmp, store) = temp_store();
    let dataset = sample_holdout_dataset(100);
    let id = store.put(&dataset).unwrap();

    let holdout_hash = hash_holdout_slice(&dataset.bars);
    assert_eq!(holdout_hash.len(), 64);

    let loaded = store.load_and_verify_holdout(&id, &holdout_hash).unwrap();
    assert_eq!(loaded.bars.len(), 100);
    assert_eq!(loaded.info.id, id);
    assert!(verify_holdout_slice_hash(&loaded.bars, &holdout_hash).is_ok());
}

#[test]
fn test_tampered_holdout_byte_fails_load() {
    let (_tmp, store) = temp_store();
    let dataset = sample_holdout_dataset(100);
    let id = store.put(&dataset).unwrap();
    let holdout_hash = hash_holdout_slice(&dataset.bars);

    let bars_file = store.root().join(&id).join("bars.qfb");
    let mut bytes = fs::read(&bars_file).unwrap();
    let target_idx = bytes.len() / 2;
    bytes[target_idx] ^= 0x55; // Flip bits inside record body
    fs::write(&bars_file, bytes).unwrap();

    let load_res = store.get(&id);
    assert!(load_res.is_err(), "Loading a tampered holdout file must fail");
    let err_msg = load_res.unwrap_err().to_string();
    assert!(
        err_msg.contains("checksum mismatch")
            || err_msg.contains("corrupt or has been modified")
            || err_msg.contains("fingerprint mismatch"),
        "Unexpected error message: {err_msg}"
    );

    let verify_res = store.load_and_verify_holdout(&id, &holdout_hash);
    assert!(verify_res.is_err());
}

#[test]
fn test_tampered_holdout_truncated_fails_load() {
    let (_tmp, store) = temp_store();
    let dataset = sample_holdout_dataset(100);
    let id = store.put(&dataset).unwrap();
    let holdout_hash = hash_holdout_slice(&dataset.bars);

    let bars_file = store.root().join(&id).join("bars.qfb");
    let mut bytes = fs::read(&bars_file).unwrap();
    bytes.truncate(bytes.len() - 64); // Strip trailing record and trailer
    fs::write(&bars_file, bytes).unwrap();

    let load_res = store.get(&id);
    assert!(load_res.is_err(), "Truncated file must fail load");

    let verify_res = store.load_and_verify_holdout(&id, &holdout_hash);
    assert!(verify_res.is_err());
}

#[test]
fn test_holdout_slice_hash_mismatch_fails_with_clear_error() {
    let (_tmp, store) = temp_store();
    let dataset1 = sample_holdout_dataset(100);
    let id1 = store.put(&dataset1).unwrap();
    let hash1 = hash_holdout_slice(&dataset1.bars);

    let dataset2 = sample_holdout_dataset(120);
    let hash2 = hash_holdout_slice(&dataset2.bars);

    assert_ne!(hash1, hash2);

    let mismatch_res = store.load_and_verify_holdout(&id1, &hash2);
    assert!(mismatch_res.is_err());
    let err = mismatch_res.unwrap_err().to_string();
    assert!(
        err.contains("holdout slice checksum mismatch"),
        "Expected holdout slice checksum mismatch error, got: {err}"
    );
    assert!(err.contains("tampered with or modified"));
}
