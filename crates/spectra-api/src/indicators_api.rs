//! Technical indicator series API (M35 Task 2).
//!
//! Provides `GET /api/v1/indicators` to compute aligned series for all 25
//! technical indicators against a dataset version's price bars.

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Query, State};
use axum::response::Response;
use axum::Extension;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use spectra_core::Bar;
use spectra_research::indicators::*;

use crate::datasets_api::load_for_chart;
use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, invalid_fields, AppState, Identity};

/// Median bar interval boundaries from spectra-data.
const DAILY_MIN_NS: i64 = 20 * 3_600 * 1_000_000_000;
const DAILY_MAX_NS: i64 = 4 * 86_400 * 1_000_000_000;

/// Query parameters for `GET /api/v1/indicators`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IndicatorQuery {
    /// Symbol name (e.g. `AAPL`, `SPY`).
    pub symbol: Option<String>,
    /// Dataset version id (UUID).
    pub dataset_version_id: Option<Uuid>,
    /// Timeframe matching dataset native interval (e.g. `1Day`, `1m`, `1h`).
    pub timeframe: Option<String>,
    /// Indicator name (e.g. `SMA`, `EMA`, `BB`, `MACD`, `RSI`).
    pub indicator: Option<String>,
    /// Look-back period for single-period indicators.
    pub period: Option<usize>,
    /// Fast period for MACD (default 12).
    pub fast_period: Option<usize>,
    /// Slow period for MACD (default 26).
    pub slow_period: Option<usize>,
    /// Signal period for MACD (default 9).
    pub signal_period: Option<usize>,
    /// Multiplier for Bollinger Bands (default 2.0) or Keltner (default 1.5).
    pub multiplier: Option<f64>,
    /// %K period for Stochastic (default 14).
    pub k_period: Option<usize>,
    /// %D period for Stochastic (default 3).
    pub d_period: Option<usize>,
    /// Slow %D period for Stochastic (default 3).
    pub slow_period_stoch: Option<usize>,
    /// Tenkan-sen period for Ichimoku (default 9).
    pub tenkan_period: Option<usize>,
    /// Kijun-sen period for Ichimoku (default 26).
    pub kijun_period: Option<usize>,
    /// Senkou Span B period for Ichimoku (default 52).
    pub span_b_period: Option<usize>,
    /// Acceleration factor start for Parabolic SAR (default 0.02).
    pub af_start: Option<f64>,
    /// Acceleration factor step for Parabolic SAR (default 0.02).
    pub af_step: Option<f64>,
    /// Acceleration factor max for Parabolic SAR (default 0.20).
    pub af_max: Option<f64>,
}

/// Aligned indicator series response view.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IndicatorSeriesView {
    /// The symbol charted.
    pub symbol: String,
    /// The indicator name requested.
    pub indicator: String,
    /// The confirmed timeframe.
    pub timeframe: String,
    /// Total number of bars in the aligned series.
    pub total_bars: usize,
    /// Bar start timestamps (nanoseconds UTC) aligned with the output series.
    pub timestamps: Vec<i64>,
    /// Every output series under its own name. Multi-output indicators include
    /// each sub-series; single-output indicators include `"value"` and the indicator name.
    pub series: BTreeMap<String, Vec<Option<f64>>>,
    /// Convenience field for single-output indicators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<Option<f64>>>,
}

fn upper_median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, f64::total_cmp).1
}

/// Checks whether `timeframe` matches the dataset's native bar interval.
fn check_timeframe(tf: &str, median_ns: i64) -> Result<(), String> {
    let s = tf.trim().to_lowercase();
    let is_daily_tf = matches!(s.as_str(), "1d" | "1day" | "day" | "daily" | "d");
    let is_1min_tf = matches!(s.as_str(), "1m" | "1min" | "minute");
    let is_5min_tf = matches!(s.as_str(), "5m" | "5min");
    let is_15min_tf = matches!(s.as_str(), "15m" | "15min");
    let is_1hour_tf = matches!(s.as_str(), "1h" | "1hour" | "hour");
    let is_1sec_tf = matches!(s.as_str(), "1s" | "1sec" | "second");
    let is_1ns_tf = matches!(s.as_str(), "1ns" | "ns");

    if !is_daily_tf && !is_1min_tf && !is_5min_tf && !is_15min_tf && !is_1hour_tf && !is_1sec_tf && !is_1ns_tf
    {
        return Err(format!("unsupported timeframe '{tf}'"));
    }

    if median_ns == 0 {
        // Single bar dataset: cannot infer interval deltas, accept any valid timeframe
        return Ok(());
    }

    let (matches_interval, native_desc) = if (DAILY_MIN_NS..=DAILY_MAX_NS).contains(&median_ns) {
        (is_daily_tf, "1Day")
    } else if (50_000_000_000..=70_000_000_000).contains(&median_ns) {
        (is_1min_tf, "1Min")
    } else if (250_000_000_000..=350_000_000_000).contains(&median_ns) {
        (is_5min_tf, "5Min")
    } else if (800_000_000_000..=1000_000_000_000).contains(&median_ns) {
        (is_15min_tf, "15Min")
    } else if (3000_000_000_000..=4200_000_000_000).contains(&median_ns) {
        (is_1hour_tf, "1Hour")
    } else if (900_000_000..=1_100_000_000).contains(&median_ns) {
        (is_1sec_tf, "1s")
    } else if median_ns <= 10 {
        (is_1ns_tf, "1ns")
    } else {
        (false, "custom")
    };

    if matches_interval {
        Ok(())
    } else {
        Err(format!("timeframe '{tf}' does not match dataset's native bar interval ({native_desc})"))
    }
}

fn parse_usize_param(
    params: &HashMap<String, String>,
    names: &[&str],
    default: usize,
    errors: &mut Vec<(String, String)>,
) -> usize {
    for &name in names {
        if let Some(val) = params.get(name) {
            match val.trim().parse::<usize>() {
                Ok(n) if n > 0 => return n,
                _ => {
                    errors.push((name.to_string(), format!("{name} must be greater than 0")));
                    return default;
                }
            }
        }
    }
    default
}

fn parse_f64_param(
    params: &HashMap<String, String>,
    names: &[&str],
    default: f64,
    errors: &mut Vec<(String, String)>,
) -> f64 {
    for &name in names {
        if let Some(val) = params.get(name) {
            match val.trim().parse::<f64>() {
                Ok(n) if n > 0.0 && n.is_finite() => return n,
                _ => {
                    errors.push((name.to_string(), format!("{name} must be greater than 0")));
                    return default;
                }
            }
        }
    }
    default
}

/// Compute the indicator output series mapping.
fn compute_indicator(
    indicator: &str,
    bars: &[Bar],
    params: &HashMap<String, String>,
    errors: &mut Vec<(String, String)>,
) -> Option<BTreeMap<String, Vec<Option<f64>>>> {
    let ind = indicator.trim().to_uppercase();
    let closes: Vec<f64> = bars.iter().map(|b| b.close.to_f64()).collect();
    let mut out = BTreeMap::new();

    match ind.as_str() {
        "SMA" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = sma(&closes, period);
            out.insert("sma".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "EMA" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = ema(&closes, period);
            out.insert("ema".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "WMA" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = wma(&closes, period);
            out.insert("wma".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "DEMA" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = dema(&closes, period);
            out.insert("dema".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "TEMA" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = tema(&closes, period);
            out.insert("tema".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "BB" | "BOLLINGER" | "BOLLINGER_BANDS" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            let mult = parse_f64_param(params, &["mult", "multiplier", "k"], 2.0, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = bollinger_bands(&closes, period, mult);
            out.insert("middle".to_string(), res.middle);
            out.insert("upper".to_string(), res.upper);
            out.insert("lower".to_string(), res.lower);
            out.insert("percent_b".to_string(), res.percent_b);
            out.insert("bandwidth".to_string(), res.bandwidth);
        }
        "ICHIMOKU" => {
            let tenkan = parse_usize_param(params, &["tenkan", "tenkan_period", "tenkan_p"], 9, errors);
            let kijun = parse_usize_param(params, &["kijun", "kijun_period", "kijun_p"], 26, errors);
            let span_b = parse_usize_param(params, &["span_b", "span_b_period", "span_b_p"], 52, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = ichimoku(bars, tenkan, kijun, span_b);
            out.insert("tenkan_sen".to_string(), res.tenkan_sen);
            out.insert("kijun_sen".to_string(), res.kijun_sen);
            out.insert("senkou_span_a".to_string(), res.senkou_span_a);
            out.insert("senkou_span_b".to_string(), res.senkou_span_b);
            out.insert("chikou_span".to_string(), res.chikou_span);
        }
        "PSAR" | "PARABOLIC_SAR" => {
            let start = parse_f64_param(params, &["start", "af_start"], 0.02, errors);
            let step = parse_f64_param(params, &["step", "af_step", "af_increment"], 0.02, errors);
            let max = parse_f64_param(params, &["max", "af_max"], 0.20, errors);
            if start > max {
                errors.push(("start".to_string(), "start must not be greater than max".to_string()));
            }
            if !errors.is_empty() {
                return None;
            }
            let s = parabolic_sar(bars, start, step, max);
            out.insert("psar".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "DONCHIAN" | "DONCHIAN_CHANNELS" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = donchian_channels(bars, period);
            out.insert("upper".to_string(), res.upper);
            out.insert("middle".to_string(), res.middle);
            out.insert("lower".to_string(), res.lower);
        }
        "KELTNER" | "KELTNER_CHANNELS" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            let mult = parse_f64_param(params, &["mult", "multiplier", "m"], 1.5, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = keltner_channels(bars, period, mult);
            out.insert("upper".to_string(), res.upper);
            out.insert("middle".to_string(), res.middle);
            out.insert("lower".to_string(), res.lower);
        }
        "VWAP" => {
            let s = vwap(bars);
            out.insert("vwap".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "TYPICAL" | "TYPICAL_PRICE" => {
            let s: Vec<Option<f64>> = typical_price(bars).into_iter().map(Some).collect();
            out.insert("typical".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "MEDIAN" | "MEDIAN_PRICE" => {
            let s: Vec<Option<f64>> = median_price(bars).into_iter().map(Some).collect();
            out.insert("median".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "RSI" => {
            let period = parse_usize_param(params, &["period"], 14, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = rsi(&closes, period);
            out.insert("rsi".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "MACD" => {
            let fast = parse_usize_param(params, &["fast", "fast_period"], 12, errors);
            let slow = parse_usize_param(params, &["slow", "slow_period"], 26, errors);
            let signal = parse_usize_param(params, &["signal", "signal_period"], 9, errors);
            if fast >= slow {
                errors.push((
                    "fast_period".to_string(),
                    "fast_period must be less than slow_period".to_string(),
                ));
            }
            if !errors.is_empty() {
                return None;
            }
            let res = macd(&closes, fast, slow, signal);
            out.insert("macd".to_string(), res.macd);
            out.insert("signal".to_string(), res.signal);
            out.insert("histogram".to_string(), res.histogram);
        }
        "STOCH" | "STOCHASTIC" => {
            let k = parse_usize_param(params, &["k", "k_period"], 14, errors);
            let d = parse_usize_param(params, &["d", "d_period"], 3, errors);
            let slow_d = parse_usize_param(params, &["slow_d", "slow_period"], 3, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = stochastic(bars, k, d, slow_d);
            out.insert("percent_k".to_string(), res.percent_k);
            out.insert("percent_d".to_string(), res.percent_d);
            out.insert("slow_d".to_string(), res.slow_d);
        }
        "CCI" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = cci(bars, period);
            out.insert("cci".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "WILLR" | "WILLIAMS_R" => {
            let period = parse_usize_param(params, &["period"], 14, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = williams_r(bars, period);
            out.insert("willr".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "MOM" | "MOMENTUM" => {
            let period = parse_usize_param(params, &["period"], 10, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = momentum(&closes, period);
            out.insert("mom".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "ROC" | "RATE_OF_CHANGE" => {
            let period = parse_usize_param(params, &["period"], 12, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = rate_of_change(&closes, period);
            out.insert("roc".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "ATR" => {
            let period = parse_usize_param(params, &["period"], 14, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = atr(bars, period);
            out.insert("atr".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "ADX" => {
            let period = parse_usize_param(params, &["period"], 14, errors);
            if !errors.is_empty() {
                return None;
            }
            let res = adx(bars, period);
            out.insert("adx".to_string(), res.adx);
            out.insert("plus_di".to_string(), res.plus_di);
            out.insert("minus_di".to_string(), res.minus_di);
        }
        "OBV" => {
            let s = obv(bars);
            out.insert("obv".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "CMF" | "CHAIKIN_MONEY_FLOW" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = chaikin_money_flow(bars, period);
            out.insert("cmf".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "MFI" | "MONEY_FLOW_INDEX" => {
            let period = parse_usize_param(params, &["period"], 14, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = mfi(bars, period);
            out.insert("mfi".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        "STDDEV" | "STD_DEV" => {
            let period = parse_usize_param(params, &["period"], 20, errors);
            if !errors.is_empty() {
                return None;
            }
            let s = std_dev(&closes, period);
            out.insert("stddev".to_string(), s.clone());
            out.insert("value".to_string(), s);
        }
        _ => {
            errors.push(("indicator".to_string(), format!("unknown indicator '{indicator}'")));
            return None;
        }
    }

    Some(out)
}

/// Calculate and retrieve aligned technical indicator series for a symbol.
#[utoipa::path(
    get,
    path = "/api/v1/indicators",
    tag = "indicators",
    params(IndicatorQuery),
    responses(
        (status = 200, description = "Aligned indicator series", body = IndicatorSeriesView),
        (status = 400, description = "No workspace (code `no_workspace`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "Dataset version or symbol not found", body = crate::openapi::ErrorDoc),
        (status = 409, description = "Bars not reachable or sealed holdout", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid request parameters (`details.errors`)", body = crate::openapi::ErrorDoc),
        (status = 503, description = "Database or storage unavailable", body = crate::openapi::ErrorDoc),
    )
)]
pub(crate) async fn indicators_handler(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    query: Result<Query<HashMap<String, String>>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(params)) = query else {
        return invalid_fields(&[("query".to_string(), "malformed query string".to_string())]);
    };

    let mut errors = Vec::new();

    // Required parameter 1: symbol
    let symbol = match params.get("symbol").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            errors.push(("symbol".to_string(), "symbol is required".to_string()));
            String::new()
        }
    };

    // Required parameter 2: dataset_version_id
    let version_id = match params.get("dataset_version_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) => match Uuid::parse_str(s) {
            Ok(u) => Some(u),
            Err(_) => {
                errors.push(("dataset_version_id".to_string(), "must be a valid UUID".to_string()));
                None
            }
        },
        None => {
            errors.push(("dataset_version_id".to_string(), "dataset_version_id is required".to_string()));
            None
        }
    };

    // Required parameter 3: timeframe
    let timeframe = match params.get("timeframe").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            errors.push(("timeframe".to_string(), "timeframe is required".to_string()));
            String::new()
        }
    };

    // Required parameter 4: indicator
    let indicator = match params.get("indicator").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            errors.push(("indicator".to_string(), "indicator is required".to_string()));
            String::new()
        }
    };

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let version_id = version_id.expect("validated above");

    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };

    // Check version resolution & sealed holdouts
    let access = spectra_db::datasets::Access { purpose: "indicators", user: Uuid::parse_str(&id.user).ok() };
    let data = match spectra_db::datasets::resolve_version(&pool, ws, version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(d)) => d,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return crate::optimization_api::sealed_response(&holdout, "reading indicators");
        }
        Ok(spectra_db::datasets::Resolved::Missing) => {
            return error_response(404, "The dataset version no longer exists.");
        }
        Err(e) => return db_error(&e),
    };

    // Load dataset bars via existing chart loader
    let dataset = match load_for_chart(&st, &data.checksum, data.blob_sha256.as_deref()).await {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Look up instrument id
    let Some(instrument) = dataset.symbols.id(&symbol) else {
        return invalid_fields(&[(
            "symbol".to_string(),
            format!(
                "symbol '{symbol}' not in this dataset version (available: {})",
                dataset.symbols.symbols.join(", ")
            ),
        )]);
    };

    let sym_bars: Vec<Bar> = dataset.bars.iter().filter(|b| b.instrument == instrument).copied().collect();

    if sym_bars.is_empty() {
        return invalid_fields(&[(
            "symbol".to_string(),
            format!("no bars found for symbol '{symbol}' in this dataset"),
        )]);
    }

    // Determine dataset's native bar interval
    let interval_deltas: Vec<f64> =
        sym_bars.windows(2).map(|w| (w[1].ts_ns - w[0].ts_ns) as f64).filter(|&d| d > 0.0).collect();

    let median_ns = if !interval_deltas.is_empty() {
        upper_median(interval_deltas) as i64
    } else {
        dataset.quality.symbols.iter().find(|s| s.symbol == symbol).map(|s| s.median_interval_ns).unwrap_or(0)
    };

    if let Err(tf_err) = check_timeframe(&timeframe, median_ns) {
        return invalid_fields(&[("timeframe".to_string(), tf_err)]);
    }

    // Compute indicator series
    let series = match compute_indicator(&indicator, &sym_bars, &params, &mut errors) {
        Some(s) if errors.is_empty() => s,
        _ => return invalid_fields(&errors),
    };

    let timestamps: Vec<i64> = sym_bars.iter().map(|b| b.ts_ns).collect();
    let values = series.get("value").cloned();

    to_json(
        200,
        &IndicatorSeriesView {
            symbol,
            indicator,
            timeframe,
            total_bars: sym_bars.len(),
            timestamps,
            series,
            values,
        },
    )
}
