//! Tests for the `backtest` job: payload validation, running from a real
//! dataset store, the stored files, and failure classification.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use spectra_backtest::*;
use spectra_core::Json;
use spectra_data::{load_csv, CsvOptions, DataStore};
use spectra_engine::{run_backtest, Progress, RunControl, RunLimits};

const VERSION: &str = "0192f0c4-7a3b-7c1d-8e2f-0123456789ab";

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spectra-backtest-job-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A two-symbol daily CSV with enough movement for crossovers.
fn csv(days: usize) -> String {
    let mut s = String::from("timestamp,symbol,open,high,low,close,volume\n");
    let mut px = [50.0_f64, 120.0];
    let mut x: u64 = 7;
    for d in 0..days {
        let date = format!("{:04}-{:02}-{:02}T00:00:00Z", 2000 + d / 336, 1 + (d / 28) % 12, 1 + d % 28);
        for (k, sym) in ["AAA", "BBB"].iter().enumerate() {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let shock = ((x >> 33) % 2001) as f64 / 1000.0 - 1.0; // -1..1
            let open = px[k];
            let close = (open * (1.0 + 0.02 * shock + 0.0005)).max(1.0);
            let (hi, lo) = (open.max(close) * 1.002, open.min(close) * 0.998);
            s.push_str(&format!("{date},{sym},{open:.4},{hi:.4},{lo:.4},{close:.4},100000\n"));
            px[k] = close;
        }
    }
    s
}

/// A dataset store holding one dataset; returns (root, checksum).
fn store_with_dataset(name: &str, days: usize) -> (PathBuf, String) {
    let root = temp_dir(name);
    let d = load_csv(csv(days).as_bytes(), &CsvOptions::default()).unwrap();
    let id = DataStore::new(root.clone()).unwrap().put(&d).unwrap();
    (root, id)
}

fn payload(text: &str) -> Json {
    Json::parse(text).unwrap()
}

fn job(extra: &str) -> BacktestJob {
    BacktestJob::from_payload(&payload(&format!(r#"{{"dataset_version_id":"{VERSION}",{extra}}}"#))).unwrap()
}

#[test]
fn payload_validation_reports_every_problem() {
    let j = job(r#""strategy":"sma_cross","params":{"fast":5,"slow":20}"#);
    assert_eq!(j.dataset_version_id, VERSION);
    assert_eq!(j.request.params.canonical(), "fast=5;slow=20;qty=100;allow_short=0");

    let upper = VERSION.to_ascii_uppercase();
    let j = BacktestJob::from_payload(&payload(&format!(
        r#"{{"dataset_version_id":"{upper}","strategy":"buy_and_hold"}}"#
    )))
    .unwrap();
    assert_eq!(j.dataset_version_id, VERSION, "normalized to lowercase");

    let err = BacktestJob::from_payload(&payload(
        r#"{"dataset_version_id":"nope","strategy":"sma_cross","params":{"fast":40,"slow":20},"config":{"seed":-1},"x":1}"#,
    ))
    .unwrap_err();
    let fields: BTreeSet<String> = err.errors.iter().map(|e| e.field.clone()).collect();
    let expected: BTreeSet<String> =
        ["dataset_version_id", "params.fast", "config.seed", "x"].map(String::from).into();
    assert_eq!(fields, expected, "{err}");

    let err = BacktestJob::from_payload(&payload(r#"{"strategy":"sma_cross"}"#)).unwrap_err();
    assert_eq!(err.errors[0].field, "dataset_version_id");
    assert!(BacktestJob::from_payload(&payload("[]")).is_err());
    assert!(
        BacktestJob::from_payload(&payload(r#"{"dataset_version_id":5,"strategy":"sma_cross"}"#)).is_err()
    );
}

#[test]
fn job_run_matches_a_direct_engine_run_and_stores_every_file() {
    let (root, checksum) = store_with_dataset("match", 400);
    let j = job(r#""strategy":"sma_cross","params":{"fast":5,"slow":20,"qty":50},"config":{"seed":3}"#);

    let mut seen = Vec::new();
    let mut cb = |p: Progress| seen.push(p.fraction());
    let flag = AtomicBool::new(false);
    let mut ctl =
        RunControl { cancel: Some(&flag), progress: Some(&mut cb), progress_every: 50, ..Default::default() };
    let products = run_job_from_store(&root, &checksum, &j, &mut ctl).unwrap();

    // Same fingerprints as calling the engine directly on the same bars.
    let bars = load_bars(&root, &checksum).unwrap();
    let mut s = j.request.strategy.build(&j.request.params).unwrap();
    let direct = run_backtest(&bars, s.as_mut(), &j.request.config).unwrap();
    assert_eq!(products.run, direct.run);
    assert!(direct.fill_count() > 0);

    // Progress went up to 1.
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(seen.last().copied(), Some(1.0));

    let kinds: Vec<&str> = products.files.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            "summary",
            "equity_series",
            "drawdown_series",
            "request",
            "equity_csv",
            "orders_csv",
            "fills_csv",
            "returns_csv"
        ]
    );
    for f in &products.files {
        assert!(!f.bytes.is_empty(), "{}", f.kind);
        if f.media_type == "application/json" {
            Json::parse(std::str::from_utf8(&f.bytes).unwrap()).unwrap();
        }
    }
    assert_eq!(
        products.summary.find("run").unwrap().find("result_sha256").unwrap().as_str().unwrap(),
        direct.run.result_sha256
    );
    let symbols: Vec<&str> = products
        .summary
        .find("instruments")
        .unwrap()
        .items()
        .unwrap()
        .iter()
        .map(|r| r.find("symbol").unwrap().as_str().unwrap())
        .collect();
    assert!(!symbols.is_empty() && symbols.iter().all(|s| ["AAA", "BBB"].contains(s)), "{symbols:?}");
    assert_eq!(products.params.find("dataset_version_id").unwrap().as_str().unwrap(), VERSION);
    assert_eq!(products.params.find("dataset_checksum").unwrap().as_str().unwrap(), checksum);
    assert!(products.params.find("config_exact").is_some());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failures_are_classified_for_the_queue() {
    let (root, checksum) = store_with_dataset("fail", 200);
    let j = job(r#""strategy":"buy_and_hold""#);

    // Cancel before start, and part-way.
    let flag = AtomicBool::new(true);
    let mut ctl = RunControl { cancel: Some(&flag), ..Default::default() };
    assert_eq!(run_job_from_store(&root, &checksum, &j, &mut ctl).unwrap_err(), JobFailure::Cancelled);

    // Too large: permanent, with the limit in the message.
    let mut ctl =
        RunControl { limits: RunLimits { max_bars: Some(10), ..RunLimits::default() }, ..Default::default() };
    match run_job_from_store(&root, &checksum, &j, &mut ctl).unwrap_err() {
        JobFailure::Failed { retryable: false, message } => {
            assert!(message.contains("at most 10"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    // Unknown dataset: permanent, and says why.
    let missing = "0".repeat(64);
    match run_job_from_store(&root, &missing, &j, &mut RunControl::default()).unwrap_err() {
        JobFailure::Failed { retryable: false, message } => {
            assert!(message.contains("not in the dataset store"));
        }
        other => panic!("{other:?}"),
    }

    // Time limit: permanent, and suggests a fix.
    let failure = JobFailure::from(spectra_engine::RunError::TimedOut(Progress {
        bars_done: 5,
        bars_total: 10,
        timestamps_done: 5,
    }));
    assert!(
        matches!(&failure, JobFailure::Failed { retryable: false, message } if message.contains("5 of 10"))
    );

    // I/O trouble is worth retrying.
    let io = JobFailure::from(spectra_engine::RunError::Engine(spectra_core::Error::Io("disk".into())));
    assert!(matches!(io, JobFailure::Failed { retryable: true, .. }));
    let _ = std::fs::remove_dir_all(root);
}
