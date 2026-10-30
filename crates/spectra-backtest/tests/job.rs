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

#[test]
fn job_runs_with_as_of_point_in_time_filter() {
    let root = temp_dir("as-of");
    let csv = "\
timestamp,symbol,open,high,low,close,volume,known_at
2024-01-02T00:00:00Z,AAA,100,105,95,100,1000,2024-01-02T00:00:00Z
2024-01-03T00:00:00Z,AAA,100,105,95,100,1000,2024-01-03T00:00:00Z
2024-01-02T00:00:00Z,AAA,120,125,115,120,1000,2024-01-05T00:00:00Z
2024-01-04T00:00:00Z,AAA,100,105,95,100,1000,2024-01-04T00:00:00Z
";
    let opts = CsvOptions { known_at_column: "known_at".into(), ..Default::default() };
    let d = load_csv(csv.as_bytes(), &opts).unwrap();
    let checksum = DataStore::new(root.clone()).unwrap().put(&d).unwrap();

    // 1. As of 2024-01-03: should only see unrevised bar for 2024-01-02 (price 100), and 2024-01-03 (2 bars).
    let j_early = BacktestJob::from_payload(&payload(&format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"buy_and_hold","as_of":"2024-01-03T00:00:00Z"}}"#
    )))
    .unwrap();
    let mut ctl = RunControl::default();
    let prod_early = run_job_from_store(&root, &checksum, &j_early, &mut ctl).unwrap();
    assert_eq!(prod_early.run.bar_count, 2);

    // 2. As of 2024-01-06 (after revision): sees revised bar for 2024-01-02 (price 120), 2024-01-03, and 2024-01-04 (3 bars).
    let j_late = BacktestJob::from_payload(&payload(&format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"buy_and_hold","as_of":"2024-01-06T00:00:00Z"}}"#
    )))
    .unwrap();
    let mut ctl = RunControl::default();
    let prod_late = run_job_from_store(&root, &checksum, &j_late, &mut ctl).unwrap();
    assert_eq!(prod_late.run.bar_count, 3);
    assert_ne!(prod_early.run.result_sha256, prod_late.run.result_sha256);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn payload_carries_an_optional_strategy_version() {
    let j = job(r#""strategy":"sma_cross""#);
    assert_eq!(j.strategy_version_id, None);

    let sv = "0192F0C4-0000-7000-8000-0000000000AB";
    let j = job(&format!(r#""strategy":"sma_cross","strategy_version_id":"{sv}""#));
    assert_eq!(j.strategy_version_id.as_deref(), Some(sv.to_ascii_lowercase().as_str()), "normalized");

    let j = job(r#""strategy":"sma_cross","strategy_version_id":null"#);
    assert_eq!(j.strategy_version_id, None, "null is the same as absent");

    let err = BacktestJob::from_payload(&payload(&format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"sma_cross","strategy_version_id":"v1"}}"#
    )))
    .unwrap_err();
    assert!(err.errors.iter().any(|e| e.field == "strategy_version_id"), "{err}");
}

#[test]
fn a_run_from_a_strategy_version_reproduces_the_direct_run() {
    // What the API queues for a saved version: the version's built-in and full
    // parameters plus its id. It must compute exactly what the same built-in and
    // parameters compute when given directly.
    let (root, checksum) = store_with_dataset("version-parity", 300);
    let sv = "0192f0c4-0000-7000-8000-0000000000ab";
    let from_version = job(&format!(
        r#""strategy_version_id":"{sv}","strategy":"sma_cross","params":{{"fast":10,"slow":50,"qty":100.0,"allow_short":false}}"#
    ));
    let direct = job(r#""strategy":"sma_cross","params":{"fast":10,"slow":50}"#);
    let a = run_job_from_store(&root, &checksum, &from_version, &mut RunControl::default()).unwrap();
    let b = run_job_from_store(&root, &checksum, &direct, &mut RunControl::default()).unwrap();
    assert_eq!(a.run.result_sha256, b.run.result_sha256, "same result");
    assert_eq!(from_version.request.strategy_sha256(), direct.request.strategy_sha256(), "same code");
    // Running the version again gives the same result.
    let again = run_job_from_store(&root, &checksum, &from_version, &mut RunControl::default()).unwrap();
    assert_eq!(again.run.result_sha256, a.run.result_sha256);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn failed_strategy_becomes_failed_job_with_readable_message() {
    struct PanickingStrategy;
    impl spectra_engine::Strategy for PanickingStrategy {
        fn on_bar(&mut self, _ctx: &mut spectra_engine::Context<'_>, _bars: &[spectra_core::Bar]) {
            panic!("division by zero in custom formula");
        }
    }

    static FAILING_STRATEGY: BuiltinStrategy = BuiltinStrategy::custom(
        "panicking_test_strat",
        "Panicking Test Strategy",
        "Panics on bar",
        &[],
        "// test strategy",
        |_| Ok(Box::new(PanickingStrategy)),
        |_| None,
    );

    let (root, checksum) = store_with_dataset("panic-test", 50);
    let job = BacktestJob {
        dataset_version_id: VERSION.into(),
        strategy_version_id: None,
        request: BacktestRequest {
            strategy: &FAILING_STRATEGY,
            params: spectra_research::ParamSet::default(),
            config: spectra_engine::BacktestConfig::default(),
            strategies: Vec::new(),
            corporate_actions: Vec::new(),
            as_of: None,
            dsl_code: None,
            instruments: spectra_backtest::RequestInstruments::default(),
        },
    };

    let mut ctl = RunControl::default();
    let err = run_job_from_store(&root, &checksum, &job, &mut ctl).unwrap_err();
    match err {
        JobFailure::Failed { retryable, message } => {
            assert!(!retryable, "panicking strategy must not be marked retryable");
            assert!(
                message.contains("The strategy panicked: division by zero in custom formula"),
                "expected readable panic error, got: {message}"
            );
        }
        JobFailure::Cancelled => panic!("expected JobFailure::Failed, got Cancelled"),
    }

    // Verify worker thread execution (e.g. tokio::task::spawn_blocking):
    // thread must not crash/unwind, and returns the structured failure.
    let (root_clone, checksum_clone, job_clone) = (root.clone(), checksum.clone(), job.clone());
    let handle = std::thread::spawn(move || {
        let mut ctl = RunControl::default();
        run_job_from_store(&root_clone, &checksum_clone, &job_clone, &mut ctl)
    });
    let thread_res = handle.join().expect("worker thread must not crash from strategy panic");
    assert!(
        matches!(thread_res, Err(JobFailure::Failed { retryable: false, ref message }) if message.contains("division by zero")),
        "expected structured non-retryable failure"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dsl_strategy_backtest_job_payload_validation_and_execution() {
    let dsl_code = r#"
param fast: int = 10, min = 2, max = 100, desc = "Fast period";
param slow: int = 30, min = 5, max = 300, desc = "Slow period";

on_bar {
    let fast_ma = sma(close, fast);
    let slow_ma = sma(close, slow);

    if fast_ma > slow_ma {
        market_buy(100.0);
    } else {
        market_sell(position);
    }
}
"#;

    let escaped_code = dsl_code.replace('\n', "\\n").replace('"', "\\\"");

    // 1. Valid DSL payload with parameters
    let valid_payload = format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"dsl","dsl_code":"{escaped_code}","params":{{"fast":50,"slow":30}}}}"#
    );
    let j = BacktestJob::from_payload(&payload(&valid_payload)).unwrap();
    assert_eq!(j.request.strategy.name, "dsl");
    assert_eq!(j.request.params.canonical(), "fast=50;slow=30");
    assert_eq!(j.request.dsl_code.as_deref(), Some(dsl_code));

    // 2. Unknown parameter on DSL strategy
    let bad_param_payload = format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"dsl","dsl_code":"{escaped_code}","params":{{"fast":50,"slow":30,"unknown_param":1}}}}"#
    );
    let err = BacktestJob::from_payload(&payload(&bad_param_payload)).unwrap_err();
    assert!(err.errors.iter().any(|e| e.field == "params.unknown_param" && e.message == "unknown parameter"));

    // 3. Out-of-range parameter on DSL strategy
    let out_of_range_payload = format!(
        r#"{{"dataset_version_id":"{VERSION}","strategy":"dsl","dsl_code":"{escaped_code}","params":{{"fast":999,"slow":30}}}}"#
    );
    let err = BacktestJob::from_payload(&payload(&out_of_range_payload)).unwrap_err();
    assert!(err
        .errors
        .iter()
        .any(|e| e.field == "params.fast" && e.message.contains("must be from 2 to 100")));

    // 4. Missing dsl_code
    let missing_code_payload =
        format!(r#"{{"dataset_version_id":"{VERSION}","strategy":"dsl","params":{{"fast":50,"slow":30}}}}"#);
    let err = BacktestJob::from_payload(&payload(&missing_code_payload)).unwrap_err();
    assert!(err.errors.iter().any(|e| e.field == "dsl_code"));

    // 5. Execution of DSL strategy backtest job
    let (root, checksum) = store_with_dataset("dsl-run", 100);
    let mut ctl = RunControl::default();
    let products = run_job_from_store(&root, &checksum, &j, &mut ctl).unwrap();
    assert_eq!(products.run.bar_count, 200);
    assert!(!products.run.result_sha256.is_empty());
    let _ = std::fs::remove_dir_all(root);
}
