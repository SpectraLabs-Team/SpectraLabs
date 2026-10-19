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
        "benchmark" => benchmark_args(payload).map(|_| ()),
        "risk" => risk_args(payload).map(|_| ()),
        "stress" => stress_args(payload).map(|_| ()),
        _ => Err(unprocessable(format!("Unknown job kind {kind:?}; use benchmark, risk or stress."))),
    }
}

/// Job kinds supported by the background worker.
pub const JOB_KINDS: [&str; 3] = ["benchmark", "risk", "stress"];

/// Execute the computation associated with a queued job.
///
/// # Errors
/// Returns the same HTTP-style error as the corresponding synchronous route.
pub fn run_job(kind: &str, payload: &Json) -> Reply {
    match kind {
        "benchmark" => benchmark_route(payload),
        "risk" => risk(payload),
        "stress" => stress(payload),
        _ => Err(unprocessable(format!("Unknown job kind {kind:?}."))),
    }
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
