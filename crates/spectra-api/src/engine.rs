//! Engine routes: request validation and the calls into the risk engine.
//! Everything here is synchronous; the HTTP layer runs it on the blocking pool.

use spectra_core::{Error, Json};
use spectra_risk::{
    benchmark, stress_test, Backend, DependenceModel, Metric, Portfolio, RiskConfig, RiskEngine, SCENARIOS,
};

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
        Error::Cancelled => (409, e.to_string()),
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

fn float_array(j: &Json, key: &str) -> Result<Vec<f64>, (u16, String)> {
    let Some(Json::Array(arr)) = j.find(key) else {
        return Err(unprocessable(format!("{key} must be an array of numbers.")));
    };
    if arr.is_empty() {
        return Err(unprocessable(format!("{key} cannot be empty.")));
    }
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        match item {
            Json::Int(v) => out.push(*v as f64),
            Json::Double(d) if d.is_finite() => out.push(*d),
            _ => return Err(unprocessable(format!("Elements of {key} must be finite numbers."))),
        }
    }
    Ok(out)
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

    let dep_str = j.find("dependence").or_else(|| j.find("copula")).and_then(|v| match v {
        Json::String(s) => Some(s.as_str()),
        _ => None,
    });
    if let Some(dep) = dep_str {
        cfg.dependence = match dep {
            "aggregate" => DependenceModel::Aggregate,
            "gaussian_copula" | "gaussian" => DependenceModel::GaussianCopula,
            "t_copula" | "t" | "student_t" => {
                let df = int_field(j, "degrees_of_freedom", 4, "degrees_of_freedom must be an integer.")?;
                if !(3..=100).contains(&df) {
                    return Err(unprocessable("degrees_of_freedom must be between 3 and 100."));
                }
                DependenceModel::TCopula { degrees_of_freedom: df as u32 }
            }
            _ => return Err(unprocessable(format!("Unknown dependence model: {dep}"))),
        };
    }
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

/// Detect market regime from returns using 2-state Gaussian HMM.
///
/// # Errors
///
/// Returns an HTTP error if the returns series is invalid or too short.
pub fn regimes(j: &Json) -> Reply {
    let returns = float_array(j, "returns")?;
    let model = spectra_risk::RegimeModel::fit(&returns).map_err(|e| engine_error(&e))?;
    let portfolio_val = float_field(j, "portfolio_value", 100_000.0, "portfolio_value must be numeric.")?;
    let confidence = float_field(j, "confidence", 0.99, "confidence must be numeric.")?;
    let horizon_days = float_field(j, "horizon_days", 1.0, "horizon_days must be numeric.")?;
    let horizon_years = horizon_days / 252.0;

    let (v0, v1, cur_v) = model.regime_conditional_var(portfolio_val, horizon_years, confidence);

    let state_json = |s: &spectra_risk::RegimeState| {
        Json::object()
            .with("mu_daily", Json::number(s.mu_daily).unwrap_or(Json::Null))
            .with("sigma_daily", Json::number(s.sigma_daily).unwrap_or(Json::Null))
            .with("volatility_annualized", Json::number(s.volatility_annualized).unwrap_or(Json::Null))
    };

    let trans_json = Json::Array(vec![
        Json::Array(vec![
            Json::number(model.transitions[0][0]).unwrap_or(Json::Null),
            Json::number(model.transitions[0][1]).unwrap_or(Json::Null),
        ]),
        Json::Array(vec![
            Json::number(model.transitions[1][0]).unwrap_or(Json::Null),
            Json::number(model.transitions[1][1]).unwrap_or(Json::Null),
        ]),
    ]);

    let decoded = model.decode_viterbi(&returns);
    let decoded_strs: Vec<Json> = decoded.iter().map(|r| Json::string(r.as_str())).collect();

    Ok(Json::object()
        .with("current_regime", Json::string(model.current_regime.as_str()))
        .with("current_high_vol_prob", Json::number(model.current_high_vol_prob).unwrap_or(Json::Null))
        .with("regime_0_var", Json::number(v0).unwrap_or(Json::Null))
        .with("regime_1_var", Json::number(v1).unwrap_or(Json::Null))
        .with("current_regime_var", Json::number(cur_v).unwrap_or(Json::Null))
        .with("state_0", state_json(&model.state_0))
        .with("state_1", state_json(&model.state_1))
        .with("transitions", trans_json)
        .with("viterbi_path", Json::Array(decoded_strs)))
}

/// Fit GARCH(1,1) volatility model and forecast next-step volatility.
///
/// # Errors
///
/// Returns an HTTP error if the returns series is invalid or has negligible variance.
pub fn garch(j: &Json) -> Reply {
    let returns = float_array(j, "returns")?;
    let model = spectra_risk::Garch11::fit(&returns).map_err(|e| engine_error(&e))?;
    let last_r = *returns.last().unwrap_or(&0.0);
    let forecast_1d_var = model.forecast_variance(last_r, None, 1);
    let forecast_1d_vol_ann = (forecast_1d_var.max(0.0) * 252.0).sqrt();

    Ok(Json::object()
        .with("omega", Json::number(model.omega).unwrap_or(Json::Null))
        .with("alpha", Json::number(model.alpha).unwrap_or(Json::Null))
        .with("beta", Json::number(model.beta).unwrap_or(Json::Null))
        .with("persistence", Json::number(model.persistence).unwrap_or(Json::Null))
        .with("unconditional_variance", Json::number(model.unconditional_variance).unwrap_or(Json::Null))
        .with("log_likelihood", Json::number(model.log_likelihood).unwrap_or(Json::Null))
        .with("forecast_1d_variance", Json::number(forecast_1d_var).unwrap_or(Json::Null))
        .with("forecast_1d_volatility_annualized", Json::number(forecast_1d_vol_ann).unwrap_or(Json::Null)))
}

/// Fit EVT Peaks-Over-Threshold model to loss tail and compute EVT VaR / CVaR.
///
/// # Errors
///
/// Returns an HTTP error if the loss series is invalid or has too few observations.
#[allow(clippy::similar_names)]
pub fn evt(j: &Json) -> Reply {
    let losses = float_array(j, "losses")?;
    let confidence = float_field(j, "confidence", 0.99, "confidence must be numeric.")?;
    let threshold_quantile =
        float_field(j, "threshold_quantile", 0.85, "threshold_quantile must be numeric.")?;

    let model = spectra_risk::EvtTailModel::fit(&losses, threshold_quantile).map_err(|e| engine_error(&e))?;
    let evt_var = model.var(confidence);
    let evt_cvar = model.cvar(confidence);

    Ok(Json::object()
        .with("threshold", Json::number(model.threshold).unwrap_or(Json::Null))
        .with("xi", Json::number(model.xi).unwrap_or(Json::Null))
        .with("beta", Json::number(model.beta).unwrap_or(Json::Null))
        .with("total_observations", Json::integer(model.total_observations as i64))
        .with("exceedances_count", Json::integer(model.exceedances_count as i64))
        .with("ks_statistic", Json::number(model.ks_statistic).unwrap_or(Json::Null))
        .with("passes_goodness_of_fit", Json::boolean(model.passes_goodness_of_fit))
        .with("evt_var", Json::number(evt_var).unwrap_or(Json::Null))
        .with("evt_cvar", Json::number(evt_cvar).unwrap_or(Json::Null)))
}

/// Run Kupiec and Christoffersen VaR backtest on realized losses vs forecasts.
///
/// # Errors
///
/// Returns an HTTP error if the loss or forecast series is invalid or sizes mismatch.
pub fn backtest(j: &Json) -> Reply {
    let realized = float_array(j, "realized_losses")?;
    let forecasts = float_array(j, "var_forecasts")?;
    let confidence = float_field(j, "confidence", 0.99, "confidence must be numeric.")?;

    let res = spectra_risk::backtest_var(&realized, &forecasts, confidence).map_err(|e| engine_error(&e))?;

    Ok(Json::object()
        .with("total_observations", Json::integer(res.total_observations as i64))
        .with("exceptions_count", Json::integer(res.exceptions_count as i64))
        .with("expected_exceptions", Json::number(res.expected_exceptions).unwrap_or(Json::Null))
        .with("failure_rate", Json::number(res.failure_rate).unwrap_or(Json::Null))
        .with("kupiec_stat", Json::number(res.kupiec_stat).unwrap_or(Json::Null))
        .with("kupiec_p_value", Json::number(res.kupiec_p_value).unwrap_or(Json::Null))
        .with("christoffersen_stat", Json::number(res.christoffersen_stat).unwrap_or(Json::Null))
        .with("christoffersen_p_value", Json::number(res.christoffersen_p_value).unwrap_or(Json::Null))
        .with("conditional_coverage_stat", Json::number(res.conditional_coverage_stat).unwrap_or(Json::Null))
        .with(
            "conditional_coverage_p_value",
            Json::number(res.conditional_coverage_p_value).unwrap_or(Json::Null),
        )
        .with("basel_zone", Json::string(res.basel_zone.as_str())))
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
        "backtest" => backtest_job(payload).map(|_| ()),
        "benchmark" => benchmark_args(payload).map(|_| ()),
        "risk" => risk_args(payload).map(|_| ()),
        "stress" => stress_args(payload).map(|_| ()),
        "sweep" => {
            if payload.find("dataset_version_id").is_none() {
                return Err(unprocessable("sweep requires dataset_version_id"));
            }
            Ok(())
        }
        "walk_forward" => {
            if payload.find("dataset_version_id").is_none() {
                return Err(unprocessable("walk_forward requires dataset_version_id"));
            }
            Ok(())
        }
        INGEST_JOB_KIND => Err(unprocessable(
            "Ingest jobs are created with POST /api/v1/datasets/ingest, which checks the upload.",
        )),
        _ => Err(unprocessable(format!("Unknown job kind {kind:?}; use {}.", JOB_KINDS.join(", ")))),
    }
}

/// Job kinds `POST /api/v1/jobs` accepts.
pub const JOB_KINDS: [&str; 6] = ["backtest", "benchmark", "risk", "stress", "sweep", "walk_forward"];

/// An upload turned into a dataset version (M1 task 5); queued only by
/// `POST /api/v1/datasets/ingest`.
pub const INGEST_JOB_KIND: &str = "ingest";

/// An automated or manual dataset refresh from a market data provider (M1b task 13).
pub const PROVIDER_REFRESH_JOB_KIND: &str = crate::providers::runner::PROVIDER_REFRESH_JOB_KIND;

/// Job kinds the background worker runs.
pub const WORKER_KINDS: [&str; 9] = [
    "backtest",
    "benchmark",
    "risk",
    "stress",
    INGEST_JOB_KIND,
    "survival",
    PROVIDER_REFRESH_JOB_KIND,
    "sweep",
    "walk_forward",
];

/// A validated `backtest` payload, or a 422 listing every problem.
///
/// # Errors
/// 422 with the field errors joined into one message.
pub fn backtest_job(payload: &Json) -> Result<spectra_backtest::BacktestJob, (u16, String)> {
    spectra_backtest::BacktestJob::from_payload(payload).map_err(|e| unprocessable(e.to_string()))
}

/// Execute the computation associated with a queued job.
///
/// # Errors
/// Returns the same HTTP-style error as the corresponding synchronous route.
pub fn run_job(kind: &str, payload: &Json) -> Reply {
    match kind {
        "benchmark" => benchmark_route(payload),
        "risk" => risk(payload),
        "stress" => stress(payload),
        // Backtests need the database (dataset version) and report progress; the worker runs
        // them on its own path and never calls this for them.
        "backtest" => Err((409, "backtest jobs run on the worker's backtest path".into())),
        PROVIDER_REFRESH_JOB_KIND => {
            Err((409, "provider refresh jobs run on the worker's refresh path".into()))
        }
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
        return Err((409, "backtest jobs run on the worker's backtest path".into()));
    }
    let result = run_job(kind, payload);
    if result.is_ok() {
        on_progress(1.0);
    }
    result
}

/// Key of the backtest output holding files the worker stores as run artifacts.
pub const BACKTEST_ARTIFACTS_KEY: &str = "artifacts";

/// Maximum duration allowed for a job before its result is discarded.
pub fn job_timeout(kind: &str) -> std::time::Duration {
    std::time::Duration::from_secs(match kind {
        "backtest" | INGEST_JOB_KIND | "survival" | "sweep" | "walk_forward" | PROVIDER_REFRESH_JOB_KIND => {
            900
        }
        "benchmark" => 600,
        _ => 300,
    })
}

/// Seed recorded for a job run, when the payload explicitly provides one.
pub fn job_seed(kind: &str, payload: &Json) -> Option<i64> {
    if kind == "backtest" {
        return backtest_job(payload).ok().and_then(|j| i64::try_from(j.request.config.seed).ok());
    }
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
