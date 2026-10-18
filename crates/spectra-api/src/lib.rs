//! Spectra Labs HTTP API: the Rust replacement for the FastAPI backend. It
//! calls the risk engine directly (the Python backend shelled out to the C++
//! CLI and parsed its stdout) and serves the same routes and JSON shapes the
//! Next.js dashboard already uses:
//!
//! | route | |
//! |---|---|
//! | `GET /`, `GET /health` | service info |
//! | `POST /api/risk` | `{portfolio_csv, metric, confidence, simulations[, seed, backend, horizon_days]}` |
//! | `POST /api/benchmark` | `{portfolio_csv, simulations, repetitions, warmups}` |
//! | `POST /api/stress` | `{portfolio_csv, scenario[, simulations, seed]}` |
//! | `GET /api/metrics` | which metric names are computable |
//! | `GET /api/sample-portfolio` | `{csv}` |
//! | `GET /api/telemetry/{cpu,workers,gpu}` | host readings |
//! | `/api/alpaca/*` | 501: not ported (see below) |
//!
//! The Alpaca brokerage proxy is not ported: it needs an HTTPS client, and
//! this workspace builds with the standard library only (no crates.io
//! access). Those routes answer 501 with an explanation; the dashboard does
//! not call them.
//!
//! Configuration (environment): `SPECTRA_API_ADDR` (default
//! `127.0.0.1:8000`) and `SPECTRA_CORS_ORIGINS` (comma-separated, default
//! `http://localhost:3000`).

pub mod http;
pub mod telemetry;

use spectra_core::{Error, Json, ENGINE_VERSION};
use spectra_risk::{benchmark, stress_test, Backend, Metric, Portfolio, RiskConfig, RiskEngine, SCENARIOS};

pub use http::{Request, Response};

/// The sample portfolio served at `/api/sample-portfolio`.
pub const SAMPLE_PORTFOLIO: &str = include_str!("../../../examples/sample_portfolio.csv");

/// Total simulated paths one benchmark request may cost (as in the Python backend).
pub const BENCHMARK_PATH_BUDGET: u64 = 10_000_000;

/// Server settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiConfig {
    /// Origins allowed by CORS.
    pub cors_origins: Vec<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self { cors_origins: vec!["http://localhost:3000".into()] }
    }
}

impl ApiConfig {
    /// Reads `SPECTRA_CORS_ORIGINS`.
    pub fn from_env() -> Self {
        match std::env::var("SPECTRA_CORS_ORIGINS") {
            Ok(v) => Self {
                cors_origins: v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            },
            Err(_) => Self::default(),
        }
    }
}

type Reply = Result<Json, (u16, String)>;

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

fn unprocessable(msg: impl Into<String>) -> (u16, String) {
    (422, msg.into())
}

fn engine_error(e: &Error) -> (u16, String) {
    match e {
        Error::InvalidArgument(_) | Error::Data(_) | Error::Unsupported(_) => (422, e.to_string()),
        _ => (500, e.to_string()),
    }
}

fn body_json(req: &Request) -> Result<Json, (u16, String)> {
    let text =
        std::str::from_utf8(&req.body).map_err(|_| unprocessable("Request body must be UTF-8 JSON."))?;
    let j = Json::parse(text).map_err(|e| unprocessable(format!("Request body is not valid JSON: {e}")))?;
    if j.entries().is_err() {
        return Err(unprocessable("Request body must be a JSON object."));
    }
    Ok(j)
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

fn risk(req: &Request) -> Reply {
    let j = body_json(req)?;
    let metric = str_field(&j, "metric", "var")?.trim().to_ascii_lowercase();
    if metric.is_empty() || !metric.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(unprocessable("Metric name is invalid."));
    }
    let confidence = float_field(&j, "confidence", 0.99, "Confidence must be numeric.")?;
    let sims = simulations(&j, 1_000_000)?;
    if !(confidence > 0.5 && confidence < 1.0) {
        return Err(unprocessable("Confidence must be between 0.5 and 1.0."));
    }
    let p = portfolio(&j)?;
    let cfg = run_options(&j, RiskConfig { confidence, simulations: sims, ..RiskConfig::default() })?;
    let report = RiskEngine::new().calculate_named(&metric, &p, &cfg).map_err(|e| engine_error(&e))?;
    Ok(report.to_json_value())
}

fn benchmark_route(req: &Request) -> Reply {
    let j = body_json(req)?;
    let sims = simulations(&j, 100_000)?;
    let reps = int_field(&j, "repetitions", 5, "Repetitions and warmups must be integers.")?;
    let warm = int_field(&j, "warmups", 1, "Repetitions and warmups must be integers.")?;
    if !(1..=30).contains(&reps) || !(0..=5).contains(&warm) {
        return Err(unprocessable("Use 1–30 repetitions and 0–5 warmups."));
    }
    if sims.saturating_mul((reps + warm) as u64).saturating_mul(3) > BENCHMARK_PATH_BUDGET {
        return Err(unprocessable(
            "This benchmark exceeds the 10 million total-path limit. Reduce simulations or repetitions.",
        ));
    }
    let p = portfolio(&j)?;
    benchmark(&p, sims, reps as usize, warm as usize).map_err(|e| engine_error(&e))
}

fn stress(req: &Request) -> Reply {
    let j = body_json(req)?;
    let scenario = str_field(&j, "scenario", "market-crash")?;
    if !SCENARIOS.iter().any(|s| s.name == scenario) {
        return Err(unprocessable("Unknown stress scenario."));
    }
    let p = portfolio(&j)?;
    let sims = simulations(&j, 1_000_000)?;
    let cfg = run_options(&j, RiskConfig { simulations: sims, ..RiskConfig::default() })?;
    Ok(stress_test(&RiskEngine::new(), &p, &scenario, &cfg).map_err(|e| engine_error(&e))?.to_json_value())
}

fn metrics_route() -> Json {
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

fn cors_headers(cfg: &ApiConfig, req: &Request) -> Vec<(String, String)> {
    let Some(origin) = req.header("origin") else { return Vec::new() };
    if !cfg.cors_origins.iter().any(|o| o == origin || o == "*") {
        return Vec::new();
    }
    vec![
        ("Access-Control-Allow-Origin".into(), origin.to_string()),
        ("Vary".into(), "Origin".into()),
        ("Access-Control-Allow-Methods".into(), "GET, POST, OPTIONS".into()),
        ("Access-Control-Allow-Headers".into(), "Content-Type".into()),
        ("Access-Control-Max-Age".into(), "600".into()),
    ]
}

/// Routes one request.
pub fn handle(cfg: &ApiConfig, req: &Request) -> Response {
    let cors = cors_headers(cfg, req);
    let mut resp = if req.method == "OPTIONS" {
        if cors.is_empty() && req.header("origin").is_some() {
            Response::error(403, "Origin not allowed.")
        } else {
            Response { status: 204, headers: Vec::new(), body: Vec::new() }
        }
    } else {
        let reply: Reply = match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") => Ok(Json::object()
                .with("message", Json::string("Spectra Labs API"))
                .with("version", Json::string(ENGINE_VERSION))),
            ("GET", "/health") => Ok(Json::object()
                .with("status", Json::string("healthy"))
                .with("engine", Json::string("spectra-risk"))
                .with("engine_version", Json::string(ENGINE_VERSION))
                .with("cli_exists", Json::boolean(true))),
            ("POST", "/api/risk") => risk(req),
            ("POST", "/api/benchmark") => benchmark_route(req),
            ("POST", "/api/stress") => stress(req),
            ("GET", "/api/metrics") => Ok(metrics_route()),
            ("GET", "/api/sample-portfolio") => Ok(Json::object().with("csv", Json::string(SAMPLE_PORTFOLIO))),
            ("GET", "/api/telemetry/cpu") => Ok(telemetry::cpu()),
            ("GET", "/api/telemetry/workers") => Ok(telemetry::workers()),
            ("GET", "/api/telemetry/gpu") => Ok(telemetry::gpu()),
            (_, p) if p.starts_with("/api/alpaca") => Err((
                501,
                "The Alpaca brokerage proxy is not available in the Rust API: it needs an HTTPS client, and this \
                 build uses the standard library only."
                    .into(),
            )),
            (
                _,
                "/" | "/health" | "/api/risk" | "/api/benchmark" | "/api/stress" | "/api/metrics" | "/api/sample-portfolio"
                | "/api/telemetry/cpu" | "/api/telemetry/workers" | "/api/telemetry/gpu",
            ) => Err((405, "Method not allowed.".into())),
            _ => Err((404, "Not Found".into())),
        };
        match reply {
            Ok(j) => Response::json(200, to_text(&j)),
            Err((code, msg)) => Response::error(code, &msg),
        }
    };
    resp.headers.extend(cors);
    resp
}
