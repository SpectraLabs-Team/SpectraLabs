//! Engine routes: request validation and the calls into the risk engine.
//! Everything here is synchronous; the HTTP layer runs it on the blocking pool.

use spectra_core::{Error, Json};
use spectra_risk::{benchmark, stress_test, Backend, Metric, Portfolio, RiskConfig, RiskEngine, SCENARIOS};

/// Total simulated paths one benchmark request may cost (as in the Python backend).
pub const BENCHMARK_PATH_BUDGET: u64 = 10_000_000;

/// A route's result: JSON, or an HTTP status and a message.
pub type Reply = Result<Json, (u16, String)>;

/// JSON text for the browser: like [`Json::dump`], but doubles use the
/// shortest representation that round-trips (`0.99`, not
/// `0.98999999999999999`). Same values, friendlier display.
pub fn to_text(j: &Json) -> String {
    let mut out = String::new();
    write_text(j, &mut out);
    out
}

fn write_text(j: &Json, out: &mut String) {
    match j {
        Json::Double(d) if d.is_finite() => {
            let s = format!("{d}");
            out.push_str(&s);
        }
        Json::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_text(v, out);
            }
            out.push(']');
        }
        Json::Object(members) => {
            out.push('{');
            for (i, (k, v)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Json::string(k.clone()).dump());
                out.push(':');
                write_text(v, out);
            }
            out.push('}');
        }
        other => out.push_str(&other.dump()),
    }
}

pub(crate) fn unprocessable(msg: impl Into<String>) -> (u16, String) {
    (422, msg.into())
}

fn engine_error(e: &Error) -> (u16, String) {
    match e {
        Error::InvalidArgument(_) | Error::Data(_) | Error::Unsupported(_) => (422, e.to_string()),
        _ => (500, e.to_string()),
    }
}

/// An integer field: a JSON integer, an integral number, or a numeric string.
fn int_field(j: &Json, key: &str, default: i64, msg: &str) -> Result<i64, (u16, String)> {
    match j.find(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Int(v)) => Ok(*v),
        Some(Json::Double(d)) if d.fract() == 0.0 && d.abs() < 9e15 => Ok(*d as i64),
        Some(Json::String(s)) => s.trim().parse().map_err(|_| unprocessable(msg)),
        _ => Err(unprocessable(msg)),
    }
}

fn float_field(j: &Json, key: &str, default: f64, msg: &str) -> Result<f64, (u16, String)> {
    match j.find(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Int(v)) => Ok(*v as f64),
        Some(Json::Double(d)) => Ok(*d),
        Some(Json::String(s)) => {
            s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| unprocessable(msg))
        }
        _ => Err(unprocessable(msg)),
    }
}

fn str_field(j: &Json, key: &str, default: &str) -> Result<String, (u16, String)> {
    match j.find(key) {
        None | Some(Json::Null) => Ok(default.to_string()),
        Some(Json::String(s)) => Ok(s.clone()),
        _ => Err(unprocessable(format!("{key} must be a string."))),
    }
}

fn simulations(j: &Json, default: i64) -> Result<u64, (u16, String)> {
    let n = int_field(j, "simulations", default, "Simulation count must be an integer.")?;
    if !(1_000..=100_000_000).contains(&n) {
        return Err(unprocessable("Simulations must be between 1,000 and 100,000,000."));
    }
    Ok(n as u64)
}

fn portfolio(j: &Json) -> Result<Portfolio, (u16, String)> {
    let csv = match j.find("portfolio_csv") {
        Some(Json::String(s)) if !s.trim().is_empty() => s,
        _ => return Err(unprocessable("Portfolio CSV is required.")),
    };
    if csv.len() > 10_000_000 {
        return Err((413, "Portfolio CSV exceeds the 10 MB limit.".into()));
    }
    Portfolio::from_csv_str(csv).map_err(|e| engine_error(&e))
}

fn run_options(j: &Json, base: RiskConfig) -> Result<RiskConfig, (u16, String)> {
    let mut cfg = base;
    let seed = int_field(j, "seed", cfg.seed as i64, "Seed must be a non-negative integer.")?;
    cfg.seed = u64::try_from(seed).map_err(|_| unprocessable("Seed must be a non-negative integer."))?;
    cfg.backend = Backend::parse(&str_field(j, "backend", "auto")?).map_err(|e| engine_error(&e))?;
    let days = float_field(j, "horizon_days", cfg.horizon_years * 252.0, "horizon_days must be numeric.")?;
    if !(days > 0.0 && days <= 252.0 * 30.0) {
        return Err(unprocessable("horizon_days must be between 0 and 7560 (30 years)."));
    }
    cfg.horizon_years = days / 252.0;
    Ok(cfg)
}

/// The validated arguments of a risk request: metric name, portfolio, settings.
fn risk_args(j: &Json) -> Result<(String, Portfolio, RiskConfig), (u16, String)> {
    let metric = str_field(j, "metric", "var")?.trim().to_ascii_lowercase();
    if metric.is_empty() || !metric.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(unprocessable("Metric name is invalid."));
    }
    let confidence = float_field(j, "confidence", 0.99, "Confidence must be numeric.")?;
    let sims = simulations(j, 1_000_000)?;
    if !(confidence > 0.5 && confidence < 1.0) {
        return Err(unprocessable("Confidence must be between 0.5 and 1.0."));
    }
    let p = portfolio(j)?;
    let cfg = run_options(j, RiskConfig { confidence, simulations: sims, ..RiskConfig::default() })?;
    Ok((metric, p, cfg))
}

/// Calculate the requested portfolio risk metric from a JSON request.
///
/// # Errors
///
/// Returns an HTTP-style error when request fields are invalid, the portfolio
/// cannot be parsed, or the risk engine cannot calculate the requested metric.
pub fn risk(j: &Json) -> Reply {
    let (metric, p, cfg) = risk_args(j)?;
    let report = RiskEngine::new().calculate_named(&metric, &p, &cfg).map_err(|e| engine_error(&e))?;
    Ok(report.to_json_value())
}

/// The validated arguments of a benchmark request: portfolio, simulations,
/// repetitions, warmups.
fn benchmark_args(j: &Json) -> Result<(Portfolio, u64, usize, usize), (u16, String)> {
    let sims = simulations(j, 100_000)?;
    let reps = int_field(j, "repetitions", 5, "Repetitions and warmups must be integers.")?;
    let warm = int_field(j, "warmups", 1, "Repetitions and warmups must be integers.")?;
    if !(1..=30).contains(&reps) || !(0..=5).contains(&warm) {
        return Err(unprocessable("Use 1–30 repetitions and 0–5 warmups."));
    }
    if sims.saturating_mul((reps + warm) as u64).saturating_mul(3) > BENCHMARK_PATH_BUDGET {
        return Err(unprocessable(
            "This benchmark exceeds the 10 million total-path limit. Reduce simulations or repetitions.",
        ));
    }
    let p = portfolio(j)?;
    Ok((p, sims, reps as usize, warm as usize))
}

/// Benchmark risk calculations for the portfolio described by a JSON request.
///
/// # Errors
///
/// Returns an HTTP-style error when benchmark settings are invalid, the
/// requested work exceeds the path budget, the portfolio cannot be parsed,
/// or the benchmark fails.
pub fn benchmark_route(j: &Json) -> Reply {
    let (p, sims, reps, warm) = benchmark_args(j)?;
    benchmark(&p, sims, reps, warm).map_err(|e| engine_error(&e))
}

/// The validated arguments of a stress request: scenario, portfolio, settings.
fn stress_args(j: &Json) -> Result<(String, Portfolio, RiskConfig), (u16, String)> {
    let scenario = str_field(j, "scenario", "market-crash")?;
    if !SCENARIOS.iter().any(|s| s.name == scenario) {
        return Err(unprocessable("Unknown stress scenario."));
    }
    let p = portfolio(j)?;
    let sims = simulations(j, 1_000_000)?;
    let cfg = run_options(j, RiskConfig { simulations: sims, ..RiskConfig::default() })?;
    Ok((scenario, p, cfg))
}

/// Run a named stress scenario for the portfolio described by a JSON request.
///
/// # Errors
///
/// Returns an HTTP-style error when the scenario or request fields are invalid,
/// the portfolio cannot be parsed, or the stress calculation fails.
pub fn stress(j: &Json) -> Reply {
    let (scenario, p, cfg) = stress_args(j)?;
    Ok(stress_test(&RiskEngine::new(), &p, &scenario, &cfg).map_err(|e| engine_error(&e))?.to_json_value())
}

/// Checks a job's kind and payload the way the matching synchronous route
/// would, without running the computation. Used when a job is queued, so a bad
/// request fails at once instead of on the worker.
///
/// # Errors
/// A 422 (or the route's own status) and a message, as the synchronous route
/// would return.
pub fn validate_job(kind: &str, payload: &Json) -> Result<(), (u16, String)> {
    if payload.entries().is_err() {
        return Err(unprocessable("payload must be a JSON object."));
    }
    match kind {
        "backtest" => validate_backtest(payload),
        "benchmark" => benchmark_args(payload).map(|_| ()),
        "risk" => risk_args(payload).map(|_| ()),
        "stress" => stress_args(payload).map(|_| ()),
        _ => Err(unprocessable(format!("Unknown job kind {kind:?}; use benchmark, risk or stress."))),
    }
}

/// Job kinds supported by the background worker.
pub const JOB_KINDS: [&str; 4] = ["backtest", "benchmark", "risk", "stress"];

/// Execute the computation associated with a queued job.
///
/// # Errors
/// Returns the same HTTP-style error as the corresponding synchronous route.
pub fn run_job(kind: &str, payload: &Json) -> Reply {
    match kind {
        "backtest" => backtest_job(payload),
        "benchmark" => benchmark_route(payload),
        "risk" => risk(payload),
        "stress" => stress(payload),
        _ => Err(unprocessable(format!("Unknown job kind {kind:?}."))),
    }
}

/// Execute queued work and report benchmark progress after each repetition.
///
/// # Errors
/// Returns the matching engine route's validation or computation error.
pub fn run_job_with_progress(kind: &str, payload: &Json, mut on_progress: impl FnMut(f32)) -> Reply {
    if kind == "benchmark" {
        let (portfolio, simulations, repetitions, warmups) = benchmark_args(payload)?;
        return spectra_risk::benchmark_with_progress(
            &portfolio,
            simulations,
            repetitions,
            warmups,
            on_progress,
        )
        .map_err(|e| engine_error(&e));
    }
    if kind == "backtest" {
        on_progress(0.0);
        let result = backtest_job(payload);
        if result.is_ok() {
            on_progress(1.0);
        }
        return result;
    }
    let result = run_job(kind, payload);
    if result.is_ok() {
        on_progress(1.0);
    }
    result
}

fn backtest_inputs(payload: &Json) -> Result<(String, usize, usize, String), (u16, String)> {
    let csv = str_field(payload, "csv", "")?;
    if csv.trim().is_empty() {
        return Err(unprocessable("csv must not be empty."));
    }
    if csv.len() > 10_000_000 {
        return Err((413, "Dataset CSV exceeds the 10 MB limit.".into()));
    }
    let fast = int_field(payload, "fast", 5, "fast must be a positive integer.")?;
    let slow = int_field(payload, "slow", 20, "slow must be a positive integer.")?;
    if !(1..1000).contains(&fast) || !(2..=1000).contains(&slow) || fast >= slow {
        return Err(unprocessable("SMA periods must satisfy 1 <= fast < slow <= 1000."));
    }
    let experiment_id = str_field(payload, "experiment_id", "")?;
    if experiment_id.trim().is_empty() || experiment_id.len() > 128 {
        return Err(unprocessable("experiment_id is required and must be at most 128 characters."));
    }
    Ok((csv, usize::try_from(fast).unwrap_or(5), usize::try_from(slow).unwrap_or(20), experiment_id))
}

fn validate_backtest(payload: &Json) -> Result<(), (u16, String)> {
    if payload.entries().is_err() {
        return Err(unprocessable("payload must be a JSON object."));
    }
    backtest_inputs(payload).map(|_| ())
}

struct SmaCross(usize, usize);

impl spectra_engine::Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut spectra_engine::Context<'_>, bars: &[spectra_core::Bar]) {
        for bar in bars {
            let id = bar.instrument;
            let history = ctx.history(id);
            if history.len() < self.1 {
                continue;
            }
            let sma = |period: usize| -> i128 {
                history[history.len() - period..]
                    .iter()
                    .map(|item| i128::from(item.close.raw()))
                    .sum::<i128>()
                    / i128::try_from(period).unwrap_or(1)
            };
            let position = ctx.position(id);
            if sma(self.0) > sma(self.1) && position.is_zero() {
                if let Ok(quantity) = spectra_core::Qty::from_f64(1.0) {
                    ctx.market(id, spectra_core::Side::Buy, quantity);
                }
            } else if sma(self.0) <= sma(self.1) && position.raw() > 0 {
                ctx.market(id, spectra_core::Side::Sell, position);
            }
        }
    }
}

/// Key of the backtest output holding the files the worker stores as run
/// artifacts. The worker removes it before storing the report, so the report
/// keeps only summary numbers (plan decision 0.6).
pub const BACKTEST_ARTIFACTS_KEY: &str = "artifacts";

/// The run's series as CSV files: `equity_curve`, `trades` and `returns`.
fn backtest_artifacts(result: &spectra_engine::BacktestResult, returns: &[f64]) -> Json {
    use std::fmt::Write as _;
    let mut equity = String::from("ts_ns,equity,cash\n");
    for point in &result.equity_curve {
        let _ = writeln!(equity, "{},{},{}", point.ts_ns, point.equity, point.cash);
    }
    let mut trades = String::from("seq,ts_ns,instrument,side,qty,price,fee,realized,cash_after\n");
    for entry in result.ledger.journal().iter().filter(|e| e.kind == spectra_core::EntryType::Fill) {
        let fill = &entry.fill;
        let side = match fill.side {
            spectra_core::Side::Buy => "buy",
            spectra_core::Side::Sell => "sell",
        };
        let _ = writeln!(
            trades,
            "{},{},{},{side},{},{},{},{},{}",
            entry.seq,
            fill.ts_ns,
            fill.instrument,
            fill.qty,
            fill.price,
            fill.fee,
            entry.realized,
            entry.cash_after
        );
    }
    // Returns start at the second equity point (one per period).
    let mut series = String::from("ts_ns,return\n");
    for (point, value) in result.equity_curve.iter().skip(1).zip(returns) {
        let _ = writeln!(series, "{},{value:?}", point.ts_ns);
    }
    Json::object()
        .with("equity_curve", Json::string(equity))
        .with("trades", Json::string(trades))
        .with("returns", Json::string(series))
}

fn backtest_job(payload: &Json) -> Reply {
    let (csv, fast, slow, experiment_id) = backtest_inputs(payload)?;
    let note = str_field(payload, "note", "")?;
    if note.len() > 2_000 {
        return Err(unprocessable("note must be at most 2,000 characters."));
    }
    let dataset = spectra_data::load_csv(csv.as_bytes(), &spectra_data::CsvOptions::default())
        .map_err(|error| unprocessable(format!("Dataset CSV is invalid: {error}")))?;
    let config = spectra_engine::BacktestConfig::default();
    let mut strategy = SmaCross(fast, slow);
    let result = spectra_engine::run_backtest(&dataset.bars, &mut strategy, &config)
        .map_err(|error| engine_error(&error))?;
    let returns = spectra_engine::returns_from_curve(&result.equity_curve);
    let artifacts = backtest_artifacts(&result, &returns);
    let params = spectra_research::ParamSet::from_pairs([("fast", fast as f64), ("slow", slow as f64)]);
    let metrics = spectra_registry::registry::metrics_to_json(&result.metrics)
        .map_err(|error| unprocessable(error.to_string()))?;
    let config_json = spectra_registry::registry::config_to_json(&config)
        .map_err(|error| unprocessable(error.to_string()))?;
    let params_json = spectra_registry::registry::params_to_json(&params)
        .map_err(|error| unprocessable(error.to_string()))?;
    let returns = returns
        .into_iter()
        .map(|value| Json::number(value).map_err(|error| unprocessable(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let strategy_sha256 = spectra_core::sha256_hex(b"spectra:builtin:sma-cross:v1");
    Ok(Json::object()
        .with("experiment_id", Json::string(experiment_id))
        .with("dataset_id", Json::string(dataset.info.id))
        .with("strategy", Json::string("sma_cross"))
        .with("strategy_sha256", Json::string(strategy_sha256))
        .with("params", params_json)
        .with("config", config_json)
        .with("metrics", metrics)
        .with("returns", Json::Array(returns))
        .with("note", Json::string(note))
        .with(BACKTEST_ARTIFACTS_KEY, artifacts)
        .with(
            "run",
            Json::object()
                .with("engine_version", Json::string(result.run.engine_version))
                .with("dataset_sha256", Json::string(result.run.dataset_sha256))
                .with("config_sha256", Json::string(result.run.config_sha256))
                .with("result_sha256", Json::string(result.run.result_sha256))
                .with("seed", Json::string(result.run.seed.to_string()))
                .with("bar_count", Json::integer(i64::try_from(result.run.bar_count).unwrap_or(i64::MAX))),
        ))
}

/// Maximum duration allowed for a job before its result is discarded.
pub fn job_timeout(kind: &str) -> std::time::Duration {
    std::time::Duration::from_secs(if kind == "benchmark" { 600 } else { 300 })
}

/// Seed recorded for a job run, when the payload explicitly provides one.
pub fn job_seed(kind: &str, payload: &Json) -> Option<i64> {
    if !matches!(kind, "risk" | "stress") {
        return None;
    }
    int_field(payload, "seed", RiskConfig::default().seed as i64, "Seed must be a non-negative integer.").ok()
}

/// Run parameters with potentially large CSV content replaced by its digest.
pub fn run_params(payload: &Json) -> Json {
    let mut params = Json::object();
    if let Ok(entries) = payload.entries() {
        for (key, value) in entries {
            if key == "portfolio_csv" {
                if let Json::String(csv) = value {
                    let _ = params
                        .set("portfolio_sha256", Json::string(spectra_core::sha256_hex(csv.as_bytes())));
                }
            } else {
                let _ = params.set(key.clone(), value.clone());
            }
        }
    }
    params
}

pub fn metrics_route() -> Json {
    Json::Array(
        Metric::all()
            .map(|m| {
                Json::object()
                    .with("name", Json::string(m.name()))
                    .with("supported", Json::boolean(m.is_supported()))
            })
            .collect(),
    )
}

#[cfg(test)]
mod backtest_tests {
    use super::*;

    fn sample_csv() -> String {
        let mut csv = String::from("timestamp,symbol,open,high,low,close,volume\n");
        let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid date");
        for day in 0_u32..32 {
            let date = start + chrono::Duration::days(i64::from(day));
            let close = 100.0 + f64::from(day);
            csv.push_str(&format!("{},TEST,{close},{},{},{close},100000\n", date, close + 1.0, close - 1.0));
        }
        csv
    }

    #[test]
    fn backtest_job_validates_and_returns_reproducibility_fingerprints() {
        let payload = Json::object()
            .with("experiment_id", Json::string("019a0000-0000-7000-8000-000000000001"))
            .with("csv", Json::string(sample_csv()))
            .with("fast", Json::integer(2))
            .with("slow", Json::integer(5));
        validate_job("backtest", &payload).expect("valid SMA backtest input");
        let output = run_job("backtest", &payload).expect("backtest completes");
        assert_eq!(output.at("strategy").and_then(Json::as_str).unwrap(), "sma_cross");
        for key in ["dataset_id", "strategy_sha256"] {
            assert_eq!(output.at(key).and_then(Json::as_str).unwrap().len(), 64);
        }
        for key in ["dataset_sha256", "config_sha256", "result_sha256"] {
            assert_eq!(
                output.at("run").and_then(|run| run.at(key)).and_then(Json::as_str).unwrap().len(),
                64
            );
        }
        assert!(!output.at("returns").and_then(Json::items).unwrap().is_empty());
        let files = output.at(BACKTEST_ARTIFACTS_KEY).unwrap();
        let equity = files.at("equity_curve").and_then(Json::as_str).unwrap();
        assert!(equity.starts_with("ts_ns,equity,cash\n"));
        assert_eq!(equity.lines().count(), 33, "header plus one line per bar");
        let trades = files.at("trades").and_then(Json::as_str).unwrap();
        assert!(trades.starts_with("seq,ts_ns,instrument,side,qty,price,fee,realized,cash_after\n"));
        assert!(trades.lines().count() > 1, "a rising series crosses at least once");
        let returns = files.at("returns").and_then(Json::as_str).unwrap();
        assert_eq!(
            returns.lines().count() - 1,
            output.at("returns").and_then(Json::items).unwrap().len(),
            "one CSV row per return"
        );
    }

    #[test]
    fn backtest_job_rejects_invalid_sma_parameters() {
        let payload = Json::object()
            .with("experiment_id", Json::string("experiment"))
            .with("csv", Json::string(sample_csv()))
            .with("fast", Json::integer(20))
            .with("slow", Json::integer(5));
        assert!(validate_job("backtest", &payload).is_err());
    }
}
