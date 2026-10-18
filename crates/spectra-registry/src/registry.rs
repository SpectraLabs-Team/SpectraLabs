//! Experiment and run registry: an append-only, hash-chained event log.
//!
//! Everything the platform remembers about research lives in ONE file,
//! `<root>/registry.jsonl`, one JSON event per line:
//! `{"seq":N,"type":"...","ts_ns":T,"payload":{...},"prev":"<hash>","hash":"<hash>"}`
//! where `hash = SHA-256(prev + "\n" + dump({"seq","type","ts_ns","payload"}))`.
//! Each event commits to the one before it, so editing, deleting, reordering
//! or inserting any past event breaks the chain and is detected by
//! [`Registry::verify`] (and on every open). This is TAMPER-EVIDENCE, not
//! access control: someone who can rewrite the file can rewrite the whole
//! chain. Anchor the latest hash somewhere you trust.
//!
//! The file format, hashing and ids are identical to the C++ reference, so a
//! registry written by either engine can be opened and extended by the other.
//!
//! Concurrency: appends take an exclusive lock on `<root>/registry.lock` and
//! re-read the tail first, so several processes can share one registry; one
//! `Registry` value is not itself shared between threads. Locks use the
//! standard library's portable file locking, so this works on Windows too (the
//! C++ engine needed POSIX `flock`).
//!
//! Limits: the whole log is loaded in memory. A crashed writer can leave an
//! incomplete final line; opening then fails with a clear error and
//! [`repair_incomplete_tail`] removes it (new in the Rust port).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use spectra_core::{sha256_hex, Error, Json, Result};
use spectra_data::DataStore;
use spectra_engine::{
    config_fingerprint, returns_from_curve, run_backtest, BacktestConfig, BacktestResult, CommissionModel,
    CostModel, ExecutionConfig, Metrics, RunRecord,
};
use spectra_research::{
    sharpe_per_period, ParamSet, StrategyFactory, SweepResult, TrialLedger, TrialSummary,
};

use spectra_core::{Bps, LedgerConfig, Money, Qty, Ratio};

fn reg_err(msg: impl Into<String>) -> Error {
    Error::Registry(msg.into())
}

/// Nanoseconds since the Unix epoch.
pub type Clock = Box<dyn Fn() -> i64 + Send + Sync>;

/// The system clock.
pub fn system_clock_ns() -> Clock {
    Box::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
    })
}

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An exclusive or shared lock on the registry's lock file, released on drop.
struct FileLock(File);

impl FileLock {
    fn acquire(path: &Path, exclusive: bool) -> Result<Self> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| reg_err(format!("cannot open lock file {}", path.display())))?;
        let locked = if exclusive { f.lock() } else { f.lock_shared() };
        locked.map_err(|_| reg_err(format!("cannot lock {}", path.display())))?;
        Ok(Self(f))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

// ---------------------------------------------------------------------------
// Serialization of run components
// ---------------------------------------------------------------------------

/// A configuration as JSON (exact: round-trips fingerprints). The leverage
/// limit is written only when set, so records of reference-policy runs are
/// identical to the C++ engine's.
///
/// # Errors
/// If a float field is not finite.
pub fn config_to_json(c: &BacktestConfig) -> Result<Json> {
    let mut j = Json::object()
        .with("initial_cash", Json::integer(c.initial_cash.raw()))
        .with("allow_short", Json::boolean(c.ledger.allow_short))
        .with("allow_negative_cash", Json::boolean(c.ledger.allow_negative_cash));
    if let Some(l) = c.ledger.max_gross_leverage {
        j = j.with("max_gross_leverage", Json::integer(l.raw()));
    }
    Ok(j.with("commission_per_unit", Json::integer(c.costs.commission.per_unit.raw()))
        .with("commission_bps", Json::integer(c.costs.commission.bps_of_notional.raw()))
        .with("commission_min", Json::integer(c.costs.commission.min_fee.raw()))
        .with("slippage_bps", Json::integer(c.costs.slippage.raw()))
        .with("max_participation", Json::integer(c.exec.max_participation.raw()))
        .with("delay_bars", Json::integer(c.exec.delay_bars as i64))
        .with("seed", Json::string(c.seed.to_string())) // u64 may not fit an i64
        .with("periods_per_year", Json::number(c.periods_per_year)?)
        .with("risk_free_rate", Json::number(c.risk_free_rate)?))
}

/// Parses [`config_to_json`] output (and C++ records, which have no leverage limit).
///
/// # Errors
/// [`Error::Registry`] for missing or malformed fields.
pub fn config_from_json(j: &Json) -> Result<BacktestConfig> {
    let wrap = |e: Error| reg_err(format!("invalid stored configuration: {e}"));
    let int = |k: &str| j.at(k).and_then(Json::as_int).map_err(wrap);
    let delay = int("delay_bars")?;
    if delay < 0 {
        return Err(reg_err("negative delay_bars"));
    }
    let seed_text = j.at("seed").and_then(Json::as_str).map_err(wrap)?;
    let seed = seed_text.parse::<u64>().map_err(|_| reg_err("invalid seed"))?;
    let leverage = match j.find("max_gross_leverage") {
        Some(v) => Some(Ratio::from_raw(v.as_int().map_err(wrap)?)),
        None => None,
    };
    Ok(BacktestConfig {
        initial_cash: Money::from_raw(int("initial_cash")?),
        ledger: LedgerConfig {
            allow_short: j.at("allow_short").and_then(Json::as_bool).map_err(wrap)?,
            allow_negative_cash: j.at("allow_negative_cash").and_then(Json::as_bool).map_err(wrap)?,
            max_gross_leverage: leverage,
        },
        costs: CostModel {
            commission: CommissionModel {
                per_unit: Money::from_raw(int("commission_per_unit")?),
                bps_of_notional: Bps::from_raw(int("commission_bps")?),
                min_fee: Money::from_raw(int("commission_min")?),
            },
            slippage: Bps::from_raw(int("slippage_bps")?),
        },
        exec: ExecutionConfig {
            delay_bars: delay as usize,
            max_participation: Qty::from_raw(int("max_participation")?),
        },
        seed,
        periods_per_year: j.at("periods_per_year").and_then(Json::as_double).map_err(wrap)?,
        risk_free_rate: j.at("risk_free_rate").and_then(Json::as_double).map_err(wrap)?,
    })
}

/// Parameters as a JSON object (insertion ordered).
///
/// # Errors
/// If a value is not finite.
pub fn params_to_json(p: &ParamSet) -> Result<Json> {
    let mut j = Json::object();
    for (k, v) in &p.values {
        j.set(k.clone(), Json::number(*v)?)?;
    }
    Ok(j)
}

/// Parses [`params_to_json`] output.
///
/// # Errors
/// If not an object of numbers.
pub fn params_from_json(j: &Json) -> Result<ParamSet> {
    let mut p = ParamSet::default();
    for (k, v) in j.entries()? {
        p.values.push((k.clone(), v.as_double()?));
    }
    Ok(p)
}

fn finite_metric(v: f64, name: &str) -> Result<Json> {
    if !v.is_finite() {
        return Err(reg_err(format!("non-finite metric cannot be recorded: {name}")));
    }
    Json::number(v)
}

/// Metrics as JSON.
///
/// # Errors
/// [`Error::Registry`] if any metric is not finite.
pub fn metrics_to_json(m: &Metrics) -> Result<Json> {
    Ok(Json::object()
        .with("valid", Json::boolean(m.valid))
        .with("periods", Json::integer(i64::from(m.periods)))
        .with("total_return", finite_metric(m.total_return, "total_return")?)
        .with("annualized_return", finite_metric(m.annualized_return, "annualized_return")?)
        .with("annualized_volatility", finite_metric(m.annualized_volatility, "annualized_volatility")?)
        .with("sharpe", finite_metric(m.sharpe, "sharpe")?)
        .with("max_drawdown", finite_metric(m.max_drawdown, "max_drawdown")?))
}

/// Parses [`metrics_to_json`] output.
///
/// # Errors
/// For missing or malformed fields.
pub fn metrics_from_json(j: &Json) -> Result<Metrics> {
    let d = |k: &str| j.at(k).and_then(Json::as_double);
    Ok(Metrics {
        valid: j.at("valid")?.as_bool()?,
        periods: i32::try_from(j.at("periods")?.as_int()?).map_err(|_| reg_err("periods out of range"))?,
        total_return: d("total_return")?,
        annualized_return: d("annualized_return")?,
        annualized_volatility: d("annualized_volatility")?,
        sharpe: d("sharpe")?,
        max_drawdown: d("max_drawdown")?,
    })
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// One logged event.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Position in the log, from 0.
    pub seq: u64,
    /// Event type, e.g. "run.recorded".
    pub kind: String,
    /// When it was appended.
    pub ts_ns: i64,
    /// Event body.
    pub payload: Json,
    /// Hash of the previous event.
    pub prev: String,
    /// This event's hash.
    pub hash: String,
}

fn body_dump(seq: u64, kind: &str, ts: i64, payload: &Json) -> String {
    Json::object()
        .with("seq", Json::integer(seq as i64))
        .with("type", Json::string(kind))
        .with("ts_ns", Json::integer(ts))
        .with("payload", payload.clone())
        .dump()
}

fn event_hash(prev: &str, seq: u64, kind: &str, ts: i64, payload: &Json) -> String {
    sha256_hex(format!("{prev}\n{}", body_dump(seq, kind, ts, payload)).as_bytes())
}

fn parse_event(line: &str) -> Result<Event> {
    let inner = || -> Result<Event> {
        let j = Json::parse(line)?;
        let seq = j.at("seq")?.as_int()?;
        if seq < 0 {
            return Err(reg_err("negative sequence number"));
        }
        Ok(Event {
            seq: seq as u64,
            kind: j.at("type")?.as_str()?.to_string(),
            ts_ns: j.at("ts_ns")?.as_int()?,
            payload: j.at("payload")?.clone(),
            prev: j.at("prev")?.as_str()?.to_string(),
            hash: j.at("hash")?.as_str()?.to_string(),
        })
    };
    inner().map_err(|e| match e {
        Error::Registry(_) => e,
        other => reg_err(format!("malformed event: {other}")),
    })
}

fn make_id(prefix: &str, seed: &str) -> String {
    format!("{prefix}-{}", &sha256_hex(seed.as_bytes())[..16])
}

/// An object with "id" first, followed by the members of `base`.
fn with_id(prefix: &str, prev: &str, base: &Json) -> Result<Json> {
    let mut out =
        Json::object().with("id", Json::string(make_id(prefix, &format!("{prev}|{prefix}|{}", base.dump()))));
    for (k, v) in base.entries()? {
        out.set(k.clone(), v.clone())?;
    }
    Ok(out)
}

fn str_or_empty(j: &Json, key: &str) -> Result<String> {
    match j.find(key) {
        Some(v) if !v.is_null() => Ok(v.as_str()?.to_string()),
        _ => Ok(String::new()),
    }
}

// ---------------------------------------------------------------------------
// Projections
// ---------------------------------------------------------------------------

/// An experiment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperimentInfo {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Hypothesis.
    pub hypothesis: String,
    /// Tags.
    pub tags: Vec<String>,
    /// Parent experiment (empty for a root).
    pub parent_id: String,
    /// Creation time.
    pub created_ts: i64,
    /// Notes.
    pub notes: Vec<String>,
}

/// What to record about a run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunInput {
    /// Experiment.
    pub experiment_id: String,
    /// "backtest", "sweep_trial", "walk_forward_oos", ...
    pub kind: String,
    /// `DataStore` id (empty if the data is not in a store).
    pub dataset_id: String,
    /// Strategy name.
    pub strategy: String,
    /// Hash of the strategy source, supplied by the caller.
    pub strategy_sha256: String,
    /// Parameters.
    pub params: ParamSet,
    /// Configuration.
    pub config: BacktestConfig,
    /// Run record.
    pub run: RunRecord,
    /// Metrics.
    pub metrics: Metrics,
    /// Returns (for the per-period Sharpe and observation count).
    pub returns: Vec<f64>,
    /// Counts toward the project-wide trial total used by DSR.
    pub counts_as_trial: bool,
    /// Free text.
    pub note: String,
}

impl Default for RunInput {
    fn default() -> Self {
        Self {
            experiment_id: String::new(),
            kind: "backtest".into(),
            dataset_id: String::new(),
            strategy: String::new(),
            strategy_sha256: String::new(),
            params: ParamSet::default(),
            config: BacktestConfig::default(),
            run: RunRecord::default(),
            metrics: Metrics::default(),
            returns: Vec::new(),
            counts_as_trial: true,
            note: String::new(),
        }
    }
}

/// A recorded run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunInfo {
    /// Id.
    pub id: String,
    /// Experiment.
    pub experiment_id: String,
    /// Kind.
    pub kind: String,
    /// Dataset id.
    pub dataset_id: String,
    /// Bars fingerprint.
    pub dataset_bars_sha256: String,
    /// Strategy name.
    pub strategy: String,
    /// Strategy source hash.
    pub strategy_sha256: String,
    /// Parameters.
    pub params: ParamSet,
    /// Configuration JSON.
    pub config: Json,
    /// Config fingerprint.
    pub config_sha256: String,
    /// Result fingerprint.
    pub result_sha256: String,
    /// Engine version.
    pub engine_version: String,
    /// Seed.
    pub seed: u64,
    /// Bars.
    pub bar_count: usize,
    /// Metrics.
    pub metrics: Metrics,
    /// Per-period Sharpe.
    pub sharpe_per_period: f64,
    /// Return observations.
    pub n_obs: usize,
    /// Counts as a trial.
    pub counts_as_trial: bool,
    /// When recorded.
    pub recorded_ts: i64,
    /// Notes.
    pub notes: Vec<String>,
}

/// A recorded artifact (the bytes live elsewhere).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArtifactInfo {
    /// Id.
    pub id: String,
    /// Experiment.
    pub experiment_id: String,
    /// Run (may be empty).
    pub run_id: String,
    /// E.g. "survival_report".
    pub kind: String,
    /// Hash of the bytes.
    pub sha256: String,
    /// Where the caller stored it.
    pub uri: String,
    /// Summary.
    pub summary: Json,
}

/// Run query.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunFilter {
    /// Experiment.
    pub experiment_id: Option<String>,
    /// Dataset.
    pub dataset_id: Option<String>,
    /// Strategy.
    pub strategy: Option<String>,
    /// Kind.
    pub kind: Option<String>,
    /// Minimum annualized Sharpe.
    pub min_sharpe: Option<f64>,
    /// Only runs that count as trials.
    pub only_trials: bool,
}

/// Differences between two runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunComparison {
    /// Same bars.
    pub same_dataset: bool,
    /// Same strategy name and source.
    pub same_strategy_source: bool,
    /// Same parameters.
    pub same_params: bool,
    /// Same configuration.
    pub same_config: bool,
    /// Same engine.
    pub same_engine: bool,
    /// Same result.
    pub same_result: bool,
    /// b - a.
    pub sharpe_delta: f64,
    /// b - a.
    pub total_return_delta: f64,
    /// Human-readable differences.
    pub differences: Vec<String>,
}

/// Outcome of verifying the log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerifyReport {
    /// Whether the chain is intact.
    pub ok: bool,
    /// Events verified.
    pub events: usize,
    /// The chain head; anchor this somewhere you trust.
    pub last_hash: String,
    /// First problem found, empty when ok.
    pub detail: String,
}

fn parse_experiment(e: &Event) -> Result<ExperimentInfo> {
    let p = &e.payload;
    Ok(ExperimentInfo {
        id: p.at("id")?.as_str()?.into(),
        name: p.at("name")?.as_str()?.into(),
        hypothesis: str_or_empty(p, "hypothesis")?,
        tags: p.at("tags")?.items()?.iter().map(|t| t.as_str().map(str::to_string)).collect::<Result<_>>()?,
        parent_id: str_or_empty(p, "parent_id")?,
        created_ts: e.ts_ns,
        notes: Vec::new(),
    })
}

fn parse_run(e: &Event) -> Result<RunInfo> {
    let p = &e.payload;
    let s = |k: &str| -> Result<String> { Ok(p.at(k)?.as_str()?.to_string()) };
    let note = str_or_empty(p, "note")?;
    Ok(RunInfo {
        id: s("id")?,
        experiment_id: s("experiment_id")?,
        kind: s("kind")?,
        dataset_id: str_or_empty(p, "dataset_id")?,
        dataset_bars_sha256: s("dataset_bars_sha256")?,
        strategy: s("strategy")?,
        strategy_sha256: str_or_empty(p, "strategy_sha256")?,
        params: params_from_json(p.at("params")?)?,
        config: p.at("config")?.clone(),
        config_sha256: s("config_sha256")?,
        result_sha256: s("result_sha256")?,
        engine_version: s("engine_version")?,
        seed: s("seed")?.parse().unwrap_or(0),
        bar_count: usize::try_from(p.at("bar_count")?.as_int()?).unwrap_or(0),
        metrics: metrics_from_json(p.at("metrics")?)?,
        sharpe_per_period: p.at("sharpe_per_period")?.as_double()?,
        n_obs: usize::try_from(p.at("n_obs")?.as_int()?).unwrap_or(0),
        counts_as_trial: p.at("counts_as_trial")?.as_bool()?,
        recorded_ts: e.ts_ns,
        notes: if note.is_empty() { Vec::new() } else { vec![note] },
    })
}

fn parse_artifact(e: &Event) -> Result<ArtifactInfo> {
    let p = &e.payload;
    Ok(ArtifactInfo {
        id: p.at("id")?.as_str()?.into(),
        experiment_id: p.at("experiment_id")?.as_str()?.into(),
        run_id: str_or_empty(p, "run_id")?,
        kind: p.at("kind")?.as_str()?.into(),
        sha256: p.at("sha256")?.as_str()?.into(),
        uri: str_or_empty(p, "uri")?,
        summary: p.at("summary")?.clone(),
    })
}

/// Event builder for [`Registry::append_event_if`]: sees the whole log and the
/// previous hash; returns the payload to append, or `None` to append nothing.
pub type CheckedBuilder<'a> = dyn FnMut(&[Event], &str) -> Result<Option<Json>> + 'a;

/// The registry. See the module documentation.
pub struct Registry {
    root: PathBuf,
    log: PathBuf,
    clock: Clock,
    events: Vec<Event>,
    offset: u64,
    last_hash: String,
    indexed: usize,
    experiments: Vec<ExperimentInfo>,
    runs: Vec<RunInfo>,
    artifacts: Vec<ArtifactInfo>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("root", &self.root)
            .field("events", &self.events.len())
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// Opens (creating if needed) and verifies the whole chain, using the
    /// system clock.
    ///
    /// # Errors
    /// [`Error::Registry`] if the chain is broken.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        Self::with_clock(root, system_clock_ns())
    }

    /// Opens with a custom clock (tests use a deterministic one).
    ///
    /// # Errors
    /// [`Error::Registry`] if the chain is broken.
    pub fn with_clock(root: impl Into<PathBuf>, clock: Clock) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|e| reg_err(format!("cannot create registry directory {}: {e}", root.display())))?;
        let mut r = Self {
            log: root.join("registry.jsonl"),
            root,
            clock,
            events: Vec::new(),
            offset: 0,
            last_hash: GENESIS.into(),
            indexed: 0,
            experiments: Vec::new(),
            runs: Vec::new(),
            artifacts: Vec::new(),
        };
        r.refresh()?;
        r.index_new_events()?;
        Ok(r)
    }

    /// The registry directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("registry.lock")
    }

    fn refresh(&mut self) -> Result<()> {
        if !self.log.exists() {
            return Ok(());
        }
        let _lock = FileLock::acquire(&self.lock_path(), false)?;
        self.read_tail()
    }

    /// Consumes and verifies events appended since `offset` (caller holds a lock).
    fn read_tail(&mut self) -> Result<()> {
        if !self.log.exists() {
            return Ok(());
        }
        let size =
            fs::metadata(&self.log).map_err(|e| reg_err(format!("cannot stat registry log: {e}")))?.len();
        if size < self.offset {
            return Err(reg_err("registry log shrank: events were removed after it was read"));
        }
        if size == self.offset {
            return Ok(());
        }
        let mut f = File::open(&self.log).map_err(|_| reg_err("cannot open registry log"))?;
        f.seek(SeekFrom::Start(self.offset)).map_err(|_| reg_err("cannot open registry log"))?;
        let mut chunk = Vec::with_capacity((size - self.offset) as usize);
        f.take(size - self.offset)
            .read_to_end(&mut chunk)
            .map_err(|_| reg_err("short read of registry log"))?;
        if chunk.len() as u64 != size - self.offset {
            return Err(reg_err("short read of registry log"));
        }
        let mut pos = 0;
        while pos < chunk.len() {
            let Some(nl) = chunk[pos..].iter().position(|&c| c == b'\n').map(|i| pos + i) else {
                return Err(reg_err(format!(
                    "incomplete final event after sequence {} (a writer may have crashed mid-append; run \
                     `spectra-registry --root DIR repair` to remove it)",
                    self.events.len()
                )));
            };
            let line = std::str::from_utf8(&chunk[pos..nl])
                .map_err(|_| reg_err("malformed event: invalid UTF-8"))?;
            let e = parse_event(line)?;
            let where_ = format!(" at sequence {}", self.events.len());
            if e.seq != self.events.len() as u64 {
                return Err(reg_err(format!("hash chain broken: unexpected sequence number{where_}")));
            }
            if e.prev != self.last_hash {
                return Err(reg_err(format!("hash chain broken: previous-hash mismatch{where_}")));
            }
            if e.hash != event_hash(&e.prev, e.seq, &e.kind, e.ts_ns, &e.payload) {
                return Err(reg_err(format!(
                    "hash chain broken: event content does not match its hash{where_}"
                )));
            }
            self.last_hash.clone_from(&e.hash);
            self.events.push(e);
            self.offset += (nl + 1 - pos) as u64;
            pos = nl + 1;
        }
        Ok(())
    }

    /// Atomic check-and-append. While holding the exclusive lock, and after
    /// picking up other writers' events, calls `build` with the whole log; if
    /// it returns a payload that event is appended and its hash returned.
    ///
    /// # Errors
    /// Builder errors, a broken chain, or write failures.
    pub fn append_event_if(&mut self, kind: &str, build: &mut CheckedBuilder<'_>) -> Result<Option<String>> {
        let _lock = FileLock::acquire(&self.lock_path(), true)?;
        self.read_tail()?; // pick up anything another writer appended while we waited
        let Some(payload) = build(&self.events, &self.last_hash)? else {
            return Ok(None);
        };
        let seq = self.events.len() as u64;
        let ts = (self.clock)();
        let hash = event_hash(&self.last_hash, seq, kind, ts, &payload);
        let line = Json::object()
            .with("seq", Json::integer(seq as i64))
            .with("type", Json::string(kind))
            .with("ts_ns", Json::integer(ts))
            .with("payload", payload.clone())
            .with("prev", Json::string(self.last_hash.clone()))
            .with("hash", Json::string(hash.clone()));
        let text = line.dump() + "\n";
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)
            .map_err(|_| reg_err("cannot open registry log for append"))?;
        f.write_all(text.as_bytes()).map_err(|_| reg_err("write to registry log failed"))?;
        f.sync_all().map_err(|_| reg_err("write to registry log failed"))?;
        self.offset += text.len() as u64;
        self.events.push(Event {
            seq,
            kind: kind.into(),
            ts_ns: ts,
            payload,
            prev: self.last_hash.clone(),
            hash: hash.clone(),
        });
        self.last_hash.clone_from(&hash);
        Ok(Some(hash))
    }

    /// Appends one event; the builder receives the previous hash (stable while
    /// the append lock is held) so ids can be derived from it.
    ///
    /// # Errors
    /// As [`Registry::append_event_if`].
    pub fn append_event_with(
        &mut self,
        kind: &str,
        mut build: impl FnMut(&str) -> Result<Json>,
    ) -> Result<String> {
        let h = self.append_event_if(kind, &mut |_, prev| build(prev).map(Some))?;
        h.ok_or_else(|| Error::Internal("unconditional append wrote nothing".into()))
    }

    /// Appends one event with a fixed payload.
    ///
    /// # Errors
    /// As [`Registry::append_event_if`].
    pub fn append_event(&mut self, kind: &str, payload: Json) -> Result<String> {
        let mut slot = Some(payload);
        self.append_event_with(kind, |_| {
            slot.take().ok_or_else(|| Error::Internal("payload already used".into()))
        })
    }

    /// Every event, after picking up other writers' appends.
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn events(&mut self) -> Result<&[Event]> {
        self.refresh()?;
        Ok(&self.events)
    }

    /// Re-reads and re-verifies the whole file from disk.
    pub fn verify(&self) -> VerifyReport {
        let mut rep = VerifyReport { ok: true, ..VerifyReport::default() };
        let mut prev = GENESIS.to_string();
        let mut n = 0usize;
        if self.log.exists() {
            match fs::read(&self.log) {
                Err(e) => {
                    rep.ok = false;
                    rep.detail = format!("cannot read registry log: {e}");
                }
                Ok(all) => {
                    let mut pos = 0;
                    while pos < all.len() {
                        let Some(nl) = all[pos..].iter().position(|&c| c == b'\n').map(|i| pos + i) else {
                            rep.ok = false;
                            rep.detail = format!("incomplete final event after sequence {n}");
                            break;
                        };
                        let parsed = std::str::from_utf8(&all[pos..nl])
                            .map_err(|_| reg_err("malformed event: invalid UTF-8"))
                            .and_then(parse_event);
                        let e = match parsed {
                            Ok(e) => e,
                            Err(err) => {
                                rep.ok = false;
                                rep.detail = err.to_string();
                                break;
                            }
                        };
                        let where_ = format!(" at sequence {n}");
                        let problem = if e.seq != n as u64 {
                            Some(format!("unexpected sequence number{where_} (an event was removed, reordered or inserted)"))
                        } else if e.prev != prev {
                            Some(format!("previous-hash mismatch{where_}"))
                        } else if e.hash != event_hash(&e.prev, e.seq, &e.kind, e.ts_ns, &e.payload) {
                            Some(format!("event content does not match its hash{where_}"))
                        } else if n < self.events.len() && self.events[n].hash != e.hash {
                            Some(format!("the log was rewritten after this registry read it{where_}"))
                        } else {
                            None
                        };
                        if let Some(p) = problem {
                            rep.ok = false;
                            rep.detail = p;
                            break;
                        }
                        prev = e.hash;
                        n += 1;
                        pos = nl + 1;
                    }
                }
            }
        }
        rep.events = n;
        rep.last_hash = prev;
        rep
    }

    fn index_new_events(&mut self) -> Result<()> {
        while self.indexed < self.events.len() {
            let e = &self.events[self.indexed];
            let wrap =
                |err: Error| reg_err(format!("malformed '{}' event at sequence {}: {err}", e.kind, e.seq));
            match e.kind.as_str() {
                "experiment.created" => {
                    let x = parse_experiment(e).map_err(wrap)?;
                    self.experiments.push(x);
                }
                "run.recorded" => {
                    let r = parse_run(e).map_err(wrap)?;
                    self.runs.push(r);
                }
                "artifact.recorded" => {
                    let a = parse_artifact(e).map_err(wrap)?;
                    self.artifacts.push(a);
                }
                "note.added" => {
                    let target = e.payload.at("target_id").and_then(Json::as_str).map_err(wrap)?.to_string();
                    let text = e.payload.at("text").and_then(Json::as_str).map_err(wrap)?.to_string();
                    for x in self.experiments.iter_mut().filter(|x| x.id == target) {
                        x.notes.push(text.clone());
                    }
                    for r in self.runs.iter_mut().filter(|r| r.id == target) {
                        r.notes.push(text.clone());
                    }
                }
                _ => {} // other event types (the holdout vault's) are not part of this projection
            }
            self.indexed += 1;
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        self.refresh()?;
        self.index_new_events()
    }

    // -- writing ---------------------------------------------------------------

    /// Creates an experiment, optionally linked to a parent.
    ///
    /// # Errors
    /// Empty name or unknown parent.
    pub fn create_experiment(
        &mut self,
        name: &str,
        hypothesis: &str,
        tags: &[&str],
        parent_id: &str,
    ) -> Result<String> {
        if name.is_empty() {
            return Err(reg_err("experiment name must not be empty"));
        }
        self.sync()?;
        if !parent_id.is_empty() && !self.experiments.iter().any(|x| x.id == parent_id) {
            return Err(reg_err(format!("unknown parent experiment: {parent_id}")));
        }
        let base = Json::object()
            .with("name", Json::string(name))
            .with("hypothesis", Json::string(hypothesis))
            .with("tags", Json::Array(tags.iter().map(|t| Json::string(*t)).collect()))
            .with("parent_id", Json::string(parent_id));
        let mut id = String::new();
        self.append_event_with("experiment.created", |prev| {
            let p = with_id("exp", prev, &base)?;
            id = p.at("id")?.as_str()?.to_string();
            Ok(p)
        })?;
        self.index_new_events()?;
        Ok(id)
    }

    /// A new experiment linked to `parent_id`.
    ///
    /// # Errors
    /// As [`Registry::create_experiment`].
    pub fn fork_experiment(
        &mut self,
        parent_id: &str,
        name: &str,
        hypothesis: &str,
        tags: &[&str],
    ) -> Result<String> {
        if parent_id.is_empty() {
            return Err(reg_err("fork_experiment requires a parent experiment id"));
        }
        self.create_experiment(name, hypothesis, tags, parent_id)
    }

    /// Records a run.
    ///
    /// # Errors
    /// Unknown experiment, empty kind or non-finite metrics.
    pub fn record_run(&mut self, input: &RunInput) -> Result<String> {
        self.sync()?;
        if !self.experiments.iter().any(|x| x.id == input.experiment_id) {
            return Err(reg_err(format!("unknown experiment: {}", input.experiment_id)));
        }
        if input.kind.is_empty() {
            return Err(reg_err("run kind must not be empty"));
        }
        let base = Json::object()
            .with("experiment_id", Json::string(input.experiment_id.clone()))
            .with("kind", Json::string(input.kind.clone()))
            .with("dataset_id", Json::string(input.dataset_id.clone()))
            .with("dataset_bars_sha256", Json::string(input.run.dataset_sha256.clone()))
            .with("strategy", Json::string(input.strategy.clone()))
            .with("strategy_sha256", Json::string(input.strategy_sha256.clone()))
            .with("params", params_to_json(&input.params)?)
            .with("config", config_to_json(&input.config)?)
            .with("config_sha256", Json::string(input.run.config_sha256.clone()))
            .with("result_sha256", Json::string(input.run.result_sha256.clone()))
            .with("engine_version", Json::string(input.run.engine_version.clone()))
            .with("seed", Json::string(input.run.seed.to_string()))
            .with("bar_count", Json::integer(input.run.bar_count as i64))
            .with("metrics", metrics_to_json(&input.metrics)?)
            .with("sharpe_per_period", finite_metric(sharpe_per_period(&input.returns), "sharpe_per_period")?)
            .with("n_obs", Json::integer(input.returns.len() as i64))
            .with("counts_as_trial", Json::boolean(input.counts_as_trial))
            .with("note", Json::string(input.note.clone()));
        let mut id = String::new();
        self.append_event_with("run.recorded", |prev| {
            let p = with_id("run", prev, &base)?;
            id = p.at("id")?.as_str()?.to_string();
            Ok(p)
        })?;
        self.index_new_events()?;
        Ok(id)
    }

    /// Records a single backtest.
    ///
    /// # Errors
    /// As [`Registry::record_run`].
    #[allow(clippy::too_many_arguments)]
    pub fn record_backtest(
        &mut self,
        experiment_id: &str,
        dataset_id: &str,
        strategy: &str,
        strategy_sha256: &str,
        params: &ParamSet,
        config: &BacktestConfig,
        result: &BacktestResult,
        note: &str,
    ) -> Result<String> {
        self.record_run(&RunInput {
            experiment_id: experiment_id.into(),
            kind: "backtest".into(),
            dataset_id: dataset_id.into(),
            strategy: strategy.into(),
            strategy_sha256: strategy_sha256.into(),
            params: params.clone(),
            config: *config,
            run: result.run.clone(),
            metrics: result.metrics,
            returns: returns_from_curve(&result.equity_curve),
            counts_as_trial: true,
            note: note.into(),
        })
    }

    /// One run event per trial (kind "sweep_trial"). Returns run ids in trial order.
    ///
    /// # Errors
    /// As [`Registry::record_run`].
    pub fn record_sweep(
        &mut self,
        experiment_id: &str,
        dataset_id: &str,
        strategy: &str,
        strategy_sha256: &str,
        config: &BacktestConfig,
        sweep: &SweepResult,
    ) -> Result<Vec<String>> {
        sweep
            .trials
            .iter()
            .map(|t| {
                self.record_run(&RunInput {
                    experiment_id: experiment_id.into(),
                    kind: "sweep_trial".into(),
                    dataset_id: dataset_id.into(),
                    strategy: strategy.into(),
                    strategy_sha256: strategy_sha256.into(),
                    params: t.params.clone(),
                    config: *config,
                    run: t.run.clone(),
                    metrics: t.metrics,
                    returns: t.returns.clone(),
                    counts_as_trial: true,
                    note: String::new(),
                })
            })
            .collect()
    }

    /// Records an artifact's hash and location.
    ///
    /// # Errors
    /// Unknown experiment or run.
    pub fn record_artifact(
        &mut self,
        experiment_id: &str,
        run_id: &str,
        kind: &str,
        bytes: &[u8],
        uri: &str,
        summary: Json,
    ) -> Result<String> {
        self.sync()?;
        if !self.experiments.iter().any(|x| x.id == experiment_id) {
            return Err(reg_err(format!("unknown experiment: {experiment_id}")));
        }
        if !run_id.is_empty() && !self.runs.iter().any(|r| r.id == run_id) {
            return Err(reg_err(format!("unknown run: {run_id}")));
        }
        let base = Json::object()
            .with("experiment_id", Json::string(experiment_id))
            .with("run_id", Json::string(run_id))
            .with("kind", Json::string(kind))
            .with("sha256", Json::string(sha256_hex(bytes)))
            .with("uri", Json::string(uri))
            .with("summary", summary);
        let mut id = String::new();
        self.append_event_with("artifact.recorded", |prev| {
            let p = with_id("art", prev, &base)?;
            id = p.at("id")?.as_str()?.to_string();
            Ok(p)
        })?;
        self.index_new_events()?;
        Ok(id)
    }

    /// Adds a note to an experiment or run.
    ///
    /// # Errors
    /// Unknown target.
    pub fn add_note(&mut self, target_id: &str, text: &str) -> Result<()> {
        self.sync()?;
        let known =
            self.experiments.iter().any(|x| x.id == target_id) || self.runs.iter().any(|r| r.id == target_id);
        if !known {
            return Err(reg_err(format!("unknown note target: {target_id}")));
        }
        self.append_event(
            "note.added",
            Json::object().with("target_id", Json::string(target_id)).with("text", Json::string(text)),
        )?;
        self.index_new_events()
    }

    // -- reading ---------------------------------------------------------------

    /// All experiments.
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn experiments(&mut self) -> Result<Vec<ExperimentInfo>> {
        self.sync()?;
        Ok(self.experiments.clone())
    }

    /// One experiment.
    ///
    /// # Errors
    /// Unknown id.
    pub fn experiment(&mut self, id: &str) -> Result<ExperimentInfo> {
        self.sync()?;
        self.experiments
            .iter()
            .find(|x| x.id == id)
            .cloned()
            .ok_or_else(|| reg_err(format!("unknown experiment: {id}")))
    }

    /// Root first, ending at `id`.
    ///
    /// # Errors
    /// Unknown id.
    pub fn lineage(&mut self, id: &str) -> Result<Vec<String>> {
        let mut chain = Vec::new();
        let mut cur = id.to_string();
        let mut guard = 0;
        while !cur.is_empty() && guard < 10_000 {
            let x = self.experiment(&cur)?;
            chain.push(x.id);
            cur = x.parent_id;
            guard += 1;
        }
        chain.reverse();
        Ok(chain)
    }

    /// Runs matching a filter.
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn runs(&mut self, f: &RunFilter) -> Result<Vec<RunInfo>> {
        self.sync()?;
        Ok(self
            .runs
            .iter()
            .filter(|r| {
                f.experiment_id.as_ref().is_none_or(|v| *v == r.experiment_id)
                    && f.dataset_id.as_ref().is_none_or(|v| *v == r.dataset_id)
                    && f.strategy.as_ref().is_none_or(|v| *v == r.strategy)
                    && f.kind.as_ref().is_none_or(|v| *v == r.kind)
                    && f.min_sharpe.is_none_or(|m| r.metrics.sharpe >= m)
                    && (!f.only_trials || r.counts_as_trial)
            })
            .cloned()
            .collect())
    }

    /// One run.
    ///
    /// # Errors
    /// Unknown id.
    pub fn run(&mut self, id: &str) -> Result<RunInfo> {
        self.sync()?;
        self.runs.iter().find(|r| r.id == id).cloned().ok_or_else(|| reg_err(format!("unknown run: {id}")))
    }

    /// Artifacts of an experiment.
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn artifacts(&mut self, experiment_id: &str) -> Result<Vec<ArtifactInfo>> {
        self.sync()?;
        Ok(self.artifacts.iter().filter(|a| a.experiment_id == experiment_id).cloned().collect())
    }

    /// Runs that count as trials (optionally for one experiment).
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn trial_count(&mut self, experiment_id: Option<&str>) -> Result<usize> {
        Ok(self
            .runs(&RunFilter {
                only_trials: true,
                experiment_id: experiment_id.map(str::to_string),
                ..RunFilter::default()
            })?
            .len())
    }

    /// A [`TrialLedger`] of every trial in the project, so DSR deflates by the
    /// true number of attempts.
    ///
    /// # Errors
    /// If the chain is broken.
    pub fn trial_ledger(&mut self) -> Result<TrialLedger> {
        let mut ledger = TrialLedger::default();
        for r in self.runs(&RunFilter { only_trials: true, ..RunFilter::default() })? {
            ledger.add_summary(TrialSummary {
                id: ledger.next_trial_id(),
                label: r.experiment_id,
                params: r.params.canonical(),
                result_sha256: r.result_sha256,
                sharpe_per_period: r.sharpe_per_period,
                n_obs: r.n_obs,
                objective: r.metrics.sharpe,
            });
        }
        Ok(ledger)
    }

    /// What differs between two runs.
    ///
    /// # Errors
    /// Unknown ids.
    pub fn compare(&mut self, a_id: &str, b_id: &str) -> Result<RunComparison> {
        let a = self.run(a_id)?;
        let b = self.run(b_id)?;
        let mut c = RunComparison {
            same_dataset: a.dataset_bars_sha256 == b.dataset_bars_sha256,
            same_strategy_source: a.strategy == b.strategy && a.strategy_sha256 == b.strategy_sha256,
            same_params: a.params == b.params,
            same_config: a.config_sha256 == b.config_sha256,
            same_engine: a.engine_version == b.engine_version,
            same_result: a.result_sha256 == b.result_sha256,
            sharpe_delta: b.metrics.sharpe - a.metrics.sharpe,
            total_return_delta: b.metrics.total_return - a.metrics.total_return,
            differences: Vec::new(),
        };
        let short = |s: &str| s.chars().take(12).collect::<String>();
        if !c.same_dataset {
            c.differences.push(format!(
                "dataset: {} vs {}",
                short(&a.dataset_bars_sha256),
                short(&b.dataset_bars_sha256)
            ));
        }
        if a.strategy != b.strategy {
            c.differences.push(format!("strategy: {} vs {}", a.strategy, b.strategy));
        }
        if a.strategy == b.strategy && a.strategy_sha256 != b.strategy_sha256 {
            c.differences.push("strategy source changed".into());
        }
        if !c.same_params {
            c.differences.push(format!("parameters: {} vs {}", a.params.canonical(), b.params.canonical()));
        }
        if !c.same_config {
            c.differences.push("configuration differs".into());
        }
        if !c.same_engine {
            c.differences.push(format!("engine: {} vs {}", a.engine_version, b.engine_version));
        }
        if a.seed != b.seed {
            c.differences.push(format!("seed: {} vs {}", a.seed, b.seed));
        }
        Ok(c)
    }
}

/// What [`repair_incomplete_tail`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairReport {
    /// Complete events kept.
    pub events_kept: usize,
    /// Bytes removed from the end of the log (0 if nothing was wrong).
    pub bytes_removed: u64,
    /// SHA-256 of the removed bytes, recorded in the repair event.
    pub removed_sha256: String,
}

/// Removes an incomplete final line left by a crashed writer, after checking
/// that every complete event before it verifies, and records what was removed
/// as a `registry.repaired` event. Refuses to touch a log whose complete
/// events are themselves broken (that is tampering or corruption, not a
/// crash). New in the Rust port; the C++ engine could only report the problem.
///
/// # Errors
/// [`Error::Registry`] if the complete part of the log is broken or the file
/// cannot be rewritten.
pub fn repair_incomplete_tail(root: impl AsRef<Path>, clock: Clock) -> Result<RepairReport> {
    let root = root.as_ref();
    let log = root.join("registry.jsonl");
    let lock = FileLock::acquire(&root.join("registry.lock"), true)?;
    let all = fs::read(&log).map_err(|e| reg_err(format!("cannot read registry log: {e}")))?;
    let keep = all.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    if keep == all.len() {
        drop(lock);
        let reg = Registry::with_clock(root, clock)?;
        return Ok(RepairReport {
            events_kept: reg.events.len(),
            bytes_removed: 0,
            removed_sha256: String::new(),
        });
    }
    // Verify the complete part before changing anything.
    let mut prev = GENESIS.to_string();
    let mut n = 0u64;
    for line in all[..keep].split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
        let e =
            parse_event(std::str::from_utf8(line).map_err(|_| reg_err("malformed event: invalid UTF-8"))?)?;
        if e.seq != n || e.prev != prev || e.hash != event_hash(&e.prev, e.seq, &e.kind, e.ts_ns, &e.payload)
        {
            return Err(reg_err(format!(
                "refusing to repair: the log is broken at sequence {n}, not just at its end"
            )));
        }
        prev = e.hash;
        n += 1;
    }
    let removed = &all[keep..];
    let report = RepairReport {
        events_kept: n as usize,
        bytes_removed: removed.len() as u64,
        removed_sha256: sha256_hex(removed),
    };
    let f = OpenOptions::new()
        .write(true)
        .open(&log)
        .map_err(|_| reg_err("cannot open registry log for repair"))?;
    f.set_len(keep as u64).map_err(|_| reg_err("cannot truncate registry log"))?;
    f.sync_all().map_err(|_| reg_err("cannot truncate registry log"))?;
    drop(lock);
    let mut reg = Registry::with_clock(root, clock)?;
    reg.append_event(
        "registry.repaired",
        Json::object()
            .with("bytes_removed", Json::integer(report.bytes_removed as i64))
            .with("removed_sha256", Json::string(report.removed_sha256.clone())),
    )?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// Reproduction
// ---------------------------------------------------------------------------

/// Outcome of re-running a recorded run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReproductionReport {
    /// The re-run's result fingerprint equals the recorded one.
    pub reproduced: bool,
    /// Recorded fingerprint.
    pub expected_result_sha256: String,
    /// Re-run fingerprint.
    pub actual_result_sha256: String,
    /// Anything that explains or qualifies the outcome.
    pub notes: Vec<String>,
}

/// Re-runs a recorded run from its dataset id, parameters and configuration
/// and compares fingerprints. Mismatches are reported, never hidden.
///
/// # Errors
/// Unknown run, a dataset that fails verification, or a stored configuration
/// that cannot be parsed.
pub fn reproduce_run(
    registry: &mut Registry,
    store: &DataStore,
    run_id: &str,
    factory: &StrategyFactory<'_>,
    current_strategy_sha256: &str,
) -> Result<ReproductionReport> {
    let rec = registry.run(run_id)?;
    let mut rep = ReproductionReport {
        expected_result_sha256: rec.result_sha256.clone(),
        ..ReproductionReport::default()
    };
    if rec.dataset_id.is_empty() {
        rep.notes.push(
            "The run did not record a dataset id, so its data cannot be retrieved from the store.".into(),
        );
        return Ok(rep);
    }
    if !store.contains(&rec.dataset_id) {
        rep.notes.push(format!("Dataset {} is not in the store.", rec.dataset_id));
        return Ok(rep);
    }
    let data = store.get(&rec.dataset_id)?;
    if data.info.bars_sha256 != rec.dataset_bars_sha256 {
        rep.notes.push("The stored dataset's bars fingerprint differs from the one the run recorded.".into());
        return Ok(rep);
    }
    let cfg = config_from_json(&rec.config)?;
    if config_fingerprint(&cfg) != rec.config_sha256 {
        rep.notes.push("The stored configuration does not reproduce the recorded config fingerprint.".into());
        return Ok(rep);
    }
    if rec.engine_version != spectra_core::ENGINE_VERSION {
        rep.notes.push(format!(
            "Recorded with engine {}; running {}. Results can legitimately differ across engine versions.",
            rec.engine_version,
            spectra_core::ENGINE_VERSION
        ));
    }
    if !current_strategy_sha256.is_empty() && current_strategy_sha256 != rec.strategy_sha256 {
        rep.notes.push("The strategy source has changed since the run was recorded.".into());
    }
    let mut strategy = match factory(&rec.params) {
        Ok(s) => s,
        Err(e) => {
            rep.notes.push(format!("The strategy factory returned no strategy: {e}"));
            return Ok(rep);
        }
    };
    let r = run_backtest(&data.bars, strategy.as_mut(), &cfg)?;
    rep.actual_result_sha256 = r.run.result_sha256;
    rep.reproduced = rep.actual_result_sha256 == rep.expected_result_sha256;
    if !rep.reproduced {
        rep.notes.push("The re-run's result fingerprint differs from the recorded one.".into());
    }
    Ok(rep)
}
