//! Behavioural tests for the registry and vault: tamper evidence, crash
//! repair, concurrent writers and the holdout budget. (The exact log format
//! is pinned by the C++ parity test.)

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

use spectra_core::*;
use spectra_data::{format_timestamp, load_csv, CsvOptions, DataStore, Dataset};
use spectra_engine::*;
use spectra_registry::*;
use spectra_research::*;

struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "spectra-reg-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn counting_clock() -> Clock {
    let t = Arc::new(AtomicI64::new(0));
    Box::new(move || 1_000 + t.fetch_add(1, Ordering::SeqCst))
}

fn populate(reg: &mut Registry) -> String {
    let e = reg.create_experiment("sma-trend", "trend persists", &["trend"], "").unwrap();
    reg.add_note(&e, "first look").unwrap();
    let f = reg.fork_experiment(&e, "sma-trend-2", "", &[]).unwrap();
    reg.add_note(&f, "second").unwrap();
    reg.create_experiment("other", "", &[], "").unwrap();
    e
}

fn log_of(dir: &Path) -> PathBuf {
    dir.join("registry.jsonl")
}

fn read_lines(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap().lines().map(str::to_string).collect()
}

fn write_lines(p: &Path, lines: &[String], final_newline: bool) {
    let mut s = lines.join("\n");
    if final_newline {
        s.push('\n');
    }
    std::fs::write(p, s).unwrap();
}

fn open_error(dir: &Path) -> String {
    Registry::open(dir).err().map(|e| e.to_string()).unwrap_or_default()
}

#[test]
fn persists_across_reopen_with_deterministic_content_derived_ids() {
    let a = Scratch::new("a");
    let b = Scratch::new("b");
    let ea = populate(&mut Registry::with_clock(&a.0, counting_clock()).unwrap());
    let eb = populate(&mut Registry::with_clock(&b.0, counting_clock()).unwrap());
    assert_eq!(ea, eb, "same history, same ids");
    assert_eq!(std::fs::read(log_of(&a.0)).unwrap(), std::fs::read(log_of(&b.0)).unwrap());
    let mut r = Registry::open(&a.0).unwrap();
    let xs = r.experiments().unwrap();
    assert_eq!(xs.len(), 3);
    assert_eq!(xs[0].notes, vec!["first look".to_string()]);
    assert_eq!(r.lineage(&xs[1].id).unwrap(), vec![xs[0].id.clone(), xs[1].id.clone()]);
    assert_eq!(xs.iter().map(|x| &x.id).collect::<std::collections::BTreeSet<_>>().len(), 3);
}

#[test]
fn rejects_invalid_writes() {
    let d = Scratch::new("inv");
    let mut r = Registry::open(&d.0).unwrap();
    assert!(r.create_experiment("", "", &[], "").is_err());
    assert!(r.create_experiment("x", "", &[], "exp-unknown").is_err());
    assert!(r.fork_experiment("", "x", "", &[]).is_err());
    assert!(r.add_note("nothing", "x").is_err());
    assert!(r.record_run(&RunInput { experiment_id: "nope".into(), ..RunInput::default() }).is_err());
    let e = r.create_experiment("x", "", &[], "").unwrap();
    let bad = RunInput {
        experiment_id: e,
        metrics: Metrics { sharpe: f64::NAN, ..Metrics::default() },
        ..RunInput::default()
    };
    let err = r.record_run(&bad).unwrap_err().to_string();
    assert!(err.contains("non-finite metric"), "{err}");
    assert_eq!(r.events().unwrap().len(), 1, "failed writes append nothing");
}

#[test]
fn configuration_round_trips_exactly_including_the_leverage_limit() {
    let mut cfg = BacktestConfig { seed: u64::MAX, ..BacktestConfig::default() };
    cfg.costs.slippage = Bps::from_f64(3.25).unwrap();
    cfg.exec.delay_bars = 2;
    cfg.risk_free_rate = 0.0123;
    let back = config_from_json(&Json::parse(&config_to_json(&cfg).unwrap().dump()).unwrap()).unwrap();
    assert_eq!(back, cfg);
    assert_eq!(config_fingerprint(&back), config_fingerprint(&cfg));
    let reference = BacktestConfig { ledger: LedgerConfig::reference(true, true), ..cfg };
    let j = config_to_json(&reference).unwrap();
    assert!(j.find("max_gross_leverage").is_none());
    assert_eq!(config_from_json(&j).unwrap(), reference);
}

#[test]
fn detects_an_edited_event() {
    let d = Scratch::new("edit");
    populate(&mut Registry::with_clock(&d.0, counting_clock()).unwrap());
    let mut lines = read_lines(&log_of(&d.0));
    lines[1] = lines[1].replace("first look", "first LOOK");
    write_lines(&log_of(&d.0), &lines, true);
    let err = open_error(&d.0);
    assert!(err.contains("hash chain broken") && err.contains("sequence 1"), "{err}");
}

#[test]
fn detects_deleted_reordered_inserted_and_forged_events() {
    let d = Scratch::new("reorder");
    populate(&mut Registry::with_clock(&d.0, counting_clock()).unwrap());
    let log = log_of(&d.0);
    let original = read_lines(&log);
    let mut deleted = original.clone();
    deleted.remove(2);
    let mut swapped = original.clone();
    swapped.swap(1, 2);
    let mut duplicated = original.clone();
    duplicated.insert(3, original[2].clone());
    let no_first = original[1..].to_vec();
    for broken in [deleted, swapped, duplicated, no_first] {
        write_lines(&log, &broken, true);
        assert!(open_error(&d.0).contains("hash chain broken"));
    }
    let mut forged = original.clone();
    let mut fake = Json::parse(original.last().unwrap()).unwrap();
    let head = fake.at("hash").unwrap().clone();
    fake.set("seq", Json::integer(original.len() as i64)).unwrap();
    fake.set("prev", head).unwrap();
    fake.set("hash", Json::string("f".repeat(64))).unwrap();
    forged.push(fake.dump());
    write_lines(&log, &forged, true);
    assert!(open_error(&d.0).contains("does not match its hash"));
    write_lines(&log, &original, true);
    assert_eq!(open_error(&d.0), "");
}

#[test]
fn notices_truncation_and_edits_after_opening() {
    let d = Scratch::new("trunc");
    let mut live = Registry::with_clock(&d.0, counting_clock()).unwrap();
    populate(&mut live);
    let log = log_of(&d.0);
    let original = read_lines(&log);
    write_lines(&log, &original[..original.len() - 1], true);
    assert!(live.experiments().is_err(), "a shrunken log is refused");
    let mut edited = original.clone();
    edited[0] = edited[0].replace("sma-trend", "sma-TREND");
    write_lines(&log, &edited, true);
    let v = live.verify();
    assert!(!v.ok && v.detail.contains("sequence 0"), "{}", v.detail);
}

#[test]
fn a_consistent_rewrite_is_caught_only_by_an_anchored_head() {
    let real = Scratch::new("real");
    let forged = Scratch::new("forged");
    let mut r = Registry::with_clock(&real.0, counting_clock()).unwrap();
    populate(&mut r);
    let anchored = r.verify().last_hash;
    drop(r);
    Registry::with_clock(&forged.0, counting_clock())
        .unwrap()
        .create_experiment("rewritten", "", &[], "")
        .unwrap();
    std::fs::copy(log_of(&forged.0), log_of(&real.0)).unwrap();
    let v = Registry::open(&real.0).unwrap().verify();
    assert!(v.ok);
    assert_ne!(v.last_hash, anchored);
}

#[test]
fn crash_repair_removes_only_an_incomplete_tail_and_records_it() {
    let d = Scratch::new("repair");
    populate(&mut Registry::with_clock(&d.0, counting_clock()).unwrap());
    let log = log_of(&d.0);
    let mut lines = read_lines(&log);
    let n = lines.len();
    let last = lines.pop().unwrap();
    lines.push(last[..last.len() / 2].to_string());
    write_lines(&log, &lines, false);
    let err = open_error(&d.0);
    assert!(err.contains("incomplete final event after sequence 4") && err.contains("repair"), "{err}");

    let rep = repair_incomplete_tail(&d.0, counting_clock()).unwrap();
    assert_eq!(rep.events_kept, n - 1);
    assert_eq!(rep.bytes_removed, (last.len() / 2) as u64);
    let mut r = Registry::open(&d.0).unwrap();
    let ev = r.events().unwrap();
    assert_eq!(ev.len(), n);
    assert_eq!(ev[n - 1].kind, "registry.repaired");
    assert_eq!(ev[n - 1].payload.at("removed_sha256").unwrap().as_str().unwrap(), rep.removed_sha256);
    assert!(r.verify().ok);
    assert_eq!(r.experiments().unwrap().len(), 2, "the half-written experiment is gone");

    // A healthy log is left alone.
    let again = repair_incomplete_tail(&d.0, counting_clock()).unwrap();
    assert_eq!(again.bytes_removed, 0);
    assert_eq!(Registry::open(&d.0).unwrap().events().unwrap().len(), n);
}

#[test]
fn crash_repair_refuses_a_log_broken_before_its_end() {
    let d = Scratch::new("norepair");
    populate(&mut Registry::with_clock(&d.0, counting_clock()).unwrap());
    let log = log_of(&d.0);
    let mut lines = read_lines(&log);
    lines[1] = lines[1].replace("first look", "edited");
    lines.push("{\"seq\":".into());
    write_lines(&log, &lines, false);
    let before = std::fs::read(&log).unwrap();
    let err = repair_incomplete_tail(&d.0, counting_clock()).unwrap_err().to_string();
    assert!(err.contains("refusing to repair"), "{err}");
    assert_eq!(std::fs::read(&log).unwrap(), before, "nothing was changed");
}

#[test]
fn two_writers_interleave_without_breaking_the_chain() {
    let d = Scratch::new("two");
    let mut a = Registry::with_clock(&d.0, counting_clock()).unwrap();
    let mut b = Registry::with_clock(&d.0, counting_clock()).unwrap();
    for i in 0..10 {
        a.create_experiment(&format!("from-a-{i}"), "", &[], "").unwrap();
        b.create_experiment(&format!("from-b-{i}"), "", &[], "").unwrap();
    }
    assert_eq!(a.experiments().unwrap().len(), 20);
    assert_eq!(b.experiments().unwrap().len(), 20);
    assert_eq!(a.verify().events, 20);
    assert_eq!(a.verify().last_hash, b.verify().last_hash);
}

#[test]
fn concurrent_threads_keep_one_valid_chain() {
    let d = Scratch::new("threads");
    drop(Registry::open(&d.0).unwrap());
    std::thread::scope(|s| {
        for t in 0..4 {
            let root = d.0.clone();
            s.spawn(move || {
                let mut reg = Registry::open(root).unwrap();
                for i in 0..15 {
                    reg.create_experiment(&format!("t{t}-{i}"), "concurrent", &[], "").unwrap();
                }
            });
        }
    });
    let mut check = Registry::open(&d.0).unwrap();
    assert_eq!(check.experiments().unwrap().len(), 60);
    let v = check.verify();
    assert!(v.ok, "{}", v.detail);
    assert_eq!(v.events, 60);
}

// ---------------------------------------------------------------------------
// Vault and reproduction
// ---------------------------------------------------------------------------

struct Sma(usize, usize);
impl Strategy for Sma {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let id = bars[0].instrument;
        let h = ctx.history(id);
        if h.len() < self.1.max(self.0) {
            return;
        }
        let sma =
            |n: usize| h[h.len() - n..].iter().map(|b| i128::from(b.close.raw())).sum::<i128>() / n as i128;
        let pos = ctx.position(id);
        if sma(self.0) > sma(self.1) && pos.is_zero() {
            ctx.market(id, Side::Buy, Qty::from_f64(10.0).unwrap());
        } else if sma(self.0) <= sma(self.1) && pos.raw() > 0 {
            ctx.market(id, Side::Sell, pos);
        }
    }
}

fn sma_factory(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(Sma(p.get_int("fast")? as usize, p.get_int("slow")? as usize)))
}

fn sma(fast: f64, slow: f64) -> ParamSet {
    ParamSet::from_pairs([("fast", fast), ("slow", slow)])
}

fn dataset(n: usize, seed: u64) -> Dataset {
    let mut rng = Rng::new(seed);
    let mut csv = String::from("timestamp,symbol,open,high,low,close,volume\n");
    let mut px = 50.0f64;
    for i in 0..n {
        let open = px;
        let close = (open * (1.0 + 0.012 * rng.normal())).max(1.0);
        let p = |v: f64| Price::from_f64(v).unwrap().to_string();
        csv += &format!(
            "{},ABC,{},{},{},{},50000\n",
            format_timestamp((1_600_000_000 + i as i64 * 86_400) * 1_000_000_000),
            p(open),
            p(open.max(close) * 1.001),
            p(open.min(close) * 0.999),
            p(close)
        );
        px = close;
    }
    load_csv(csv.as_bytes(), &CsvOptions::default()).unwrap()
}

struct World {
    dir: Scratch,
    store: DataStore,
    research: Dataset,
    holdout: Dataset,
}

fn world() -> World {
    let dir = Scratch::new("vault");
    let store = DataStore::new(dir.0.join("data")).unwrap();
    let research = dataset(300, 1);
    let holdout = dataset(250, 2);
    store.put(&research).unwrap();
    store.put(&holdout).unwrap();
    World { dir, store, research, holdout }
}

fn count_events(reg: &mut Registry, kind: &str) -> usize {
    reg.events().unwrap().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn vault_evaluate_matches_a_direct_run_and_spends_budget() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    let mut v = Vault::new(&mut reg, &w.store);
    v.seal("h", &w.holdout.info.id, 2, "final").unwrap();
    assert!(v.seal("h", &w.research.info.id, 1, "").unwrap_err().to_string().contains("already exists"));
    assert!(v.seal("z", &w.holdout.info.id, 0, "").is_err());
    assert!(v.seal("z", "missing", 1, "").is_err());
    let cfg = BacktestConfig::default();
    let r = v.evaluate("h", &sma_factory, "sma", "", &sma(5.0, 20.0), &cfg).unwrap();
    let direct = run_backtest(&w.holdout.bars, &mut Sma(5, 20), &cfg).unwrap();
    assert_eq!(r.result_sha256, direct.run.result_sha256);
    assert_eq!(r.metrics, direct.metrics);
    assert!(direct.fill_count() > 0, "the test strategy must trade");
    assert_eq!(r.dsr, r.psr_vs_zero);
    assert_eq!(v.status("h").unwrap().remaining(), 1);
    v.evaluate("h", &sma_factory, "sma", "", &sma(3.0, 10.0), &cfg).unwrap();
    let err = v.evaluate("h", &sma_factory, "sma", "", &sma(3.0, 10.0), &cfg).unwrap_err().to_string();
    assert!(err.contains("no evaluations left"), "{err}");
    let st = v.status("h").unwrap();
    assert_eq!((st.started, st.completed, st.denied), (2, 2, 1));
}

#[test]
fn vault_failed_evaluation_still_costs_a_unit() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    let mut v = Vault::new(&mut reg, &w.store);
    v.seal("h", &w.holdout.info.id, 3, "").unwrap();
    let boom = |_: &ParamSet| -> Result<Box<dyn Strategy>> { Err(Error::Strategy("boom".into())) };
    let err =
        v.evaluate("h", &boom, "broken", "", &ParamSet::default(), &BacktestConfig::default()).unwrap_err();
    let err = err.to_string();
    assert!(err.contains("boom") && err.contains("budget stays consumed"), "{err}");
    let st = v.status("h").unwrap();
    assert_eq!((st.started, st.failed, st.completed, st.remaining()), (1, 1, 0, 2));
}

#[test]
fn vault_corrupted_sealed_data_fails_and_is_never_used() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    let mut v = Vault::new(&mut reg, &w.store);
    v.seal("h", &w.holdout.info.id, 2, "").unwrap();
    let file = w.dir.0.join("data").join(&w.holdout.info.id).join("bars.qfb");
    let mut bytes = std::fs::read(&file).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    std::fs::write(&file, bytes).unwrap();
    assert!(v.evaluate("h", &sma_factory, "sma", "", &sma(5.0, 20.0), &BacktestConfig::default()).is_err());
    let st = v.status("h").unwrap();
    assert_eq!((st.failed, st.completed), (1, 0));
    assert!(v.seal("other", &w.holdout.info.id, 1, "").is_err());
}

#[test]
fn vault_deflates_by_the_number_of_looks() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    let mut v = Vault::new(&mut reg, &w.store);
    v.seal("h", &w.holdout.info.id, 5, "").unwrap();
    let cfg = BacktestConfig::default();
    let first = v.evaluate("h", &sma_factory, "sma", "", &sma(5.0, 20.0), &cfg).unwrap();
    v.evaluate("h", &sma_factory, "sma", "", &sma(3.0, 10.0), &cfg).unwrap();
    let third = v.evaluate("h", &sma_factory, "sma", "", &sma(8.0, 40.0), &cfg).unwrap();
    assert_eq!(third.evaluations_used, 3);
    assert!(third.expected_max_sharpe > 0.0);
    assert!(third.dsr <= third.psr_vs_zero);
    assert_eq!(first.dsr, first.psr_vs_zero);
}

#[test]
fn vault_checked_get_refuses_sealed_data_and_logs_it() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    {
        let mut v = Vault::new(&mut reg, &w.store);
        v.seal("h", &w.holdout.info.id, 1, "").unwrap();
        assert!(v.checked_get(&w.holdout.info.id).is_err());
        assert_eq!(v.checked_get(&w.research.info.id).unwrap().bars, w.research.bars);
    }
    assert_eq!(count_events(&mut reg, "holdout.access_denied"), 1);
}

#[test]
fn vault_budget_holds_when_threads_race_for_the_last_evaluations() {
    let w = world();
    let root = w.dir.0.join("reg");
    Vault::new(&mut Registry::open(&root).unwrap(), &w.store).seal("h", &w.holdout.info.id, 3, "").unwrap();
    let ok = AtomicUsize::new(0);
    let refused = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for t in 0..8 {
            let (root, store, ok, refused) = (&root, &w.store, &ok, &refused);
            s.spawn(move || {
                let mut reg = Registry::open(root).unwrap();
                let mut v = Vault::new(&mut reg, store);
                match v.evaluate(
                    "h",
                    &sma_factory,
                    "sma",
                    "",
                    &sma(3.0 + f64::from(t), 20.0),
                    &BacktestConfig::default(),
                ) {
                    Ok(_) => ok.fetch_add(1, Ordering::SeqCst),
                    Err(e) => {
                        assert!(e.to_string().contains("no evaluations left"), "{e}");
                        refused.fetch_add(1, Ordering::SeqCst)
                    }
                };
            });
        }
    });
    assert_eq!(ok.load(Ordering::SeqCst), 3);
    assert_eq!(refused.load(Ordering::SeqCst), 5);
    let mut reg = Registry::open(&root).unwrap();
    let st = Vault::new(&mut reg, &w.store).status("h").unwrap();
    assert_eq!((st.started, st.completed, st.denied), (3, 3, 5));
    assert!(reg.verify().ok);
}

#[test]
fn reproduce_run_recreates_the_result_and_explains_mismatches() {
    let w = world();
    let mut reg = Registry::open(w.dir.0.join("reg")).unwrap();
    let e = reg.create_experiment("repro", "", &[], "").unwrap();
    let cfg = BacktestConfig::default();
    let r = run_backtest(&w.research.bars, &mut Sma(5, 20), &cfg).unwrap();
    let id =
        reg.record_backtest(&e, &w.research.info.id, "sma", "src-v1", &sma(5.0, 20.0), &cfg, &r, "").unwrap();
    let ok = reproduce_run(&mut reg, &w.store, &id, &sma_factory, "src-v1").unwrap();
    assert!(ok.reproduced, "{:?}", ok.notes);
    assert!(ok.notes.is_empty());
    let changed = reproduce_run(&mut reg, &w.store, &id, &sma_factory, "src-v2").unwrap();
    assert!(changed.notes.iter().any(|n| n.contains("strategy source has changed")));
    let other = |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(Sma(3, 10))) };
    let diff = reproduce_run(&mut reg, &w.store, &id, &other, "").unwrap();
    assert!(!diff.reproduced);
    assert!(diff.notes.iter().any(|n| n.contains("differs from the recorded one")));
    let unstored = reg.record_backtest(&e, "", "sma", "", &sma(5.0, 20.0), &cfg, &r, "").unwrap();
    assert!(!reproduce_run(&mut reg, &w.store, &unstored, &sma_factory, "").unwrap().reproduced);
}
