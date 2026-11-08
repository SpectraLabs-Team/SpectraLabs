//! Alpha Workbench (M12): Signal testing and alpha evaluation.
//!
//! Provides point-in-time signal testing without lookahead leakage:
//! - Signal inputs tied to dataset versions with point-in-time forward return calculation.
//! - Per-date Spearman rank Information Coefficient (IC) with mean, standard deviation, and t-statistic.
//! - Signal decay modeling across multiple horizons (1, 5, 10, 20 days) and half-life estimation.
//! - Quantile spread (top-minus-bottom) and top-quantile turnover analysis.
//! - Gate threshold evaluation for signal promotion and consumer display (e.g. M42 sentiment score).
//! - Built standard-library only to ensure byte-exact determinism.

use spectra_core::error::{Error, Result};
use spectra_core::Json;
use std::collections::{BTreeMap, BTreeSet};

/// Trading-day horizons supported by default.
pub const DEFAULT_HORIZONS: [usize; 4] = [1, 5, 10, 20];

/// Default number of quantiles (quintiles).
pub const DEFAULT_QUANTILES: usize = 5;

/// Single daily price observation for a symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceBar {
    /// Symbol ticker (e.g. "AAPL").
    pub symbol: String,
    /// Trading date in YYYY-MM-DD or monotonic index representation.
    pub date: String,
    /// Close price.
    pub close: f64,
    /// Bar timestamp in nanoseconds since Unix epoch (UTC).
    pub ts_ns: i64,
    /// Point-in-time timestamp when this bar became known.
    pub known_at_ns: i64,
}

impl PriceBar {
    pub fn new(symbol: impl Into<String>, date: impl Into<String>, close: f64, ts_ns: i64) -> Self {
        Self { symbol: symbol.into(), date: date.into(), close, ts_ns, known_at_ns: ts_ns }
    }

    pub fn with_known_at(
        symbol: impl Into<String>,
        date: impl Into<String>,
        close: f64,
        ts_ns: i64,
        known_at_ns: i64,
    ) -> Self {
        Self { symbol: symbol.into(), date: date.into(), close, ts_ns, known_at_ns }
    }
}

/// A signal observation for a symbol on a specific date.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalValue {
    pub symbol: String,
    pub date: String,
    pub value: f64,
}

impl SignalValue {
    pub fn new(symbol: impl Into<String>, date: impl Into<String>, value: f64) -> Self {
        Self { symbol: symbol.into(), date: date.into(), value }
    }
}

/// Built-in signal generators supported in Phase 1 (M12 Task 0 Decision 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinSignal {
    /// Past price momentum: (Close_t - Close_{t-lookback}) / Close_{t-lookback}.
    Momentum { lookback_days: usize },
    /// Mean reversion: -(Close_t - RollingMean(Close)) / RollingStd(Close).
    MeanReversion { lookback_days: usize },
    /// Simple moving average ratio: (SMA_fast / SMA_slow) - 1.0.
    SmaCrossover { fast: usize, slow: usize },
    /// Externally supplied signal values (e.g. M42 sentiment score).
    Custom { name: String },
}

/// Computes built-in signal series strictly point-in-time.
///
/// Guaranteed leakage-free: bars at date > t are strictly excluded when computing the signal at date t.
///
/// # Errors
/// Returns [`Error::InvalidArgument`] if parameter bounds are violated or if a custom signal is requested without series.
pub fn generate_builtin_signal(signal: &BuiltinSignal, bars: &[PriceBar]) -> Result<Vec<SignalValue>> {
    match signal {
        BuiltinSignal::Custom { name: _ } => {
            Err(Error::InvalidArgument("Custom signals must be supplied directly".into()))
        }
        BuiltinSignal::Momentum { lookback_days } => {
            if *lookback_days == 0 {
                return Err(Error::InvalidArgument("lookback_days must be >= 1".into()));
            }
            Ok(compute_momentum_signal(bars, *lookback_days))
        }
        BuiltinSignal::MeanReversion { lookback_days } => {
            if *lookback_days < 2 {
                return Err(Error::InvalidArgument("lookback_days must be >= 2 for mean reversion".into()));
            }
            Ok(compute_mean_reversion_signal(bars, *lookback_days))
        }
        BuiltinSignal::SmaCrossover { fast, slow } => {
            if *fast == 0 || *slow <= *fast {
                return Err(Error::InvalidArgument("fast must be >= 1 and slow > fast".into()));
            }
            Ok(compute_sma_crossover_signal(bars, *fast, *slow))
        }
    }
}

fn compute_momentum_signal(bars: &[PriceBar], lookback: usize) -> Vec<SignalValue> {
    let mut by_symbol: BTreeMap<String, Vec<&PriceBar>> = BTreeMap::new();
    for b in bars {
        by_symbol.entry(b.symbol.clone()).or_default().push(b);
    }

    let mut signals = Vec::new();
    for (sym, mut sym_bars) in by_symbol {
        sym_bars.sort_by_key(|b| b.ts_ns);
        for i in lookback..sym_bars.len() {
            let curr = sym_bars[i];
            let prev = sym_bars[i - lookback];
            if prev.close > 0.0 && curr.close.is_finite() && prev.close.is_finite() {
                let mom = (curr.close - prev.close) / prev.close;
                signals.push(SignalValue::new(&sym, &curr.date, mom));
            }
        }
    }
    signals.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));
    signals
}

fn compute_mean_reversion_signal(bars: &[PriceBar], lookback: usize) -> Vec<SignalValue> {
    let mut by_symbol: BTreeMap<String, Vec<&PriceBar>> = BTreeMap::new();
    for b in bars {
        by_symbol.entry(b.symbol.clone()).or_default().push(b);
    }

    let mut signals = Vec::new();
    for (sym, mut sym_bars) in by_symbol {
        sym_bars.sort_by_key(|b| b.ts_ns);
        for i in (lookback - 1)..sym_bars.len() {
            let window = &sym_bars[(i + 1 - lookback)..=i];
            let n = window.len() as f64;
            let mean = window.iter().map(|b| b.close).sum::<f64>() / n;
            let variance = window.iter().map(|b| (b.close - mean).powi(2)).sum::<f64>() / (n - 1.0);
            let stdev = variance.sqrt();
            let curr = sym_bars[i];
            let z = if stdev > 1e-12 { -(curr.close - mean) / stdev } else { 0.0 };
            signals.push(SignalValue::new(&sym, &curr.date, z));
        }
    }
    signals.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));
    signals
}

fn compute_sma_crossover_signal(bars: &[PriceBar], fast: usize, slow: usize) -> Vec<SignalValue> {
    let mut by_symbol: BTreeMap<String, Vec<&PriceBar>> = BTreeMap::new();
    for b in bars {
        by_symbol.entry(b.symbol.clone()).or_default().push(b);
    }

    let mut signals = Vec::new();
    for (sym, mut sym_bars) in by_symbol {
        sym_bars.sort_by_key(|b| b.ts_ns);
        for i in (slow - 1)..sym_bars.len() {
            let fast_win = &sym_bars[(i + 1 - fast)..=i];
            let slow_win = &sym_bars[(i + 1 - slow)..=i];
            let fast_sma = fast_win.iter().map(|b| b.close).sum::<f64>() / (fast as f64);
            let slow_sma = slow_win.iter().map(|b| b.close).sum::<f64>() / (slow as f64);
            let val = if slow_sma > 0.0 { (fast_sma / slow_sma) - 1.0 } else { 0.0 };
            let curr = sym_bars[i];
            signals.push(SignalValue::new(&sym, &curr.date, val));
        }
    }
    signals.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));
    signals
}

/// Point-in-time forward return calculation for given horizons.
///
/// For each symbol and trading date $t_i$, computes forward return:
/// $R_{i, H} = (Close_{t_{i+H}} - Close_{t_i}) / Close_{t_i}$.
///
/// Returns a map: (symbol, date) -> BTreeMap<horizon, forward_return>.
pub fn compute_forward_returns(
    bars: &[PriceBar],
    horizons: &[usize],
) -> BTreeMap<(String, String), BTreeMap<usize, f64>> {
    let mut by_symbol: BTreeMap<String, Vec<&PriceBar>> = BTreeMap::new();
    for b in bars {
        by_symbol.entry(b.symbol.clone()).or_default().push(b);
    }

    let mut fwd_returns = BTreeMap::new();
    for (sym, mut sym_bars) in by_symbol {
        sym_bars.sort_by_key(|b| b.ts_ns);
        let n = sym_bars.len();
        for i in 0..n {
            let curr = sym_bars[i];
            let mut horizon_map = BTreeMap::new();
            for &h in horizons {
                if i + h < n {
                    let future = sym_bars[i + h];
                    if curr.close > 0.0 && curr.close.is_finite() && future.close.is_finite() {
                        let ret = (future.close - curr.close) / curr.close;
                        horizon_map.insert(h, ret);
                    }
                }
            }
            if !horizon_map.is_empty() {
                fwd_returns.insert((sym.clone(), curr.date.clone()), horizon_map);
            }
        }
    }
    fwd_returns
}

/// Fractional ranks for tied values: ties receive the average of their ranks.
pub fn fractional_ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    let mut indexed: Vec<(usize, f64)> = values.iter().copied().enumerate().collect();
    // Sort stably by value. For non-finite numbers, sort to end.
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && (indexed[j + 1].1 - indexed[i].1).abs() <= 1e-12 {
            j += 1;
        }
        // Average rank for indices from i to j (1-based ranks i+1 .. j+1).
        let avg_rank = (i + 1 + j + 1) as f64 / 2.0;
        for k in i..=j {
            ranks[indexed[k].0] = avg_rank;
        }
        i = j + 1;
    }
    ranks
}

/// Spearman rank correlation coefficient between two numeric slices of equal length.
///
/// # Errors
/// Returns [`Error::InvalidArgument`] if slice lengths differ or if fewer than 2 elements are provided.
pub fn spearman_correlation(x: &[f64], y: &[f64]) -> Result<f64> {
    if x.len() != y.len() {
        return Err(Error::InvalidArgument(format!(
            "slices must have equal length, got {} and {}",
            x.len(),
            y.len()
        )));
    }
    if x.len() < 2 {
        return Err(Error::InvalidArgument("correlation requires at least 2 points".into()));
    }

    let rx = fractional_ranks(x);
    let ry = fractional_ranks(y);

    let n = rx.len() as f64;
    let mean_rank_x = rx.iter().sum::<f64>() / n;
    let mean_rank_y = ry.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;

    for i in 0..rx.len() {
        let dx = rx[i] - mean_rank_x;
        let dy = ry[i] - mean_rank_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    if var_x <= 1e-15 || var_y <= 1e-15 {
        return Ok(0.0);
    }

    let corr = cov / (var_x.sqrt() * var_y.sqrt());
    // Clamp to [-1.0, 1.0] against precision fuzz
    Ok(corr.clamp(-1.0, 1.0))
}

/// Information Coefficient summary metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct IcSummary {
    /// Horizon in trading days (e.g. 1, 5, 10, 20).
    pub horizon: usize,
    /// Mean per-date Spearman rank correlation.
    pub mean_ic: f64,
    /// Sample standard deviation of per-date ICs.
    pub ic_stdev: f64,
    /// Student's t-statistic: mean / (stdev / sqrt(N)).
    pub t_stat: f64,
    /// Number of dates evaluated.
    pub date_count: usize,
    /// Daily IC series (date, ic).
    pub daily_ics: Vec<(String, f64)>,
}

/// Decay analysis result across multiple horizons.
#[derive(Debug, Clone, PartialEq)]
pub struct DecayResult {
    /// IC metrics for each evaluated horizon.
    pub horizon_metrics: Vec<IcSummary>,
    /// Estimated signal half-life in trading days (from exponential fit).
    pub half_life_days: f64,
}

/// Fits exponential half-life from horizon metrics: IC(h) = IC_0 * 2^(-h / t_half).
///
/// ln(IC(h)) = ln(IC_0) - lambda * h, where lambda = ln(2) / t_half.
pub fn fit_signal_half_life(metrics: &[IcSummary]) -> f64 {
    let positive_points: Vec<(f64, f64)> =
        metrics.iter().filter(|m| m.mean_ic > 1e-6).map(|m| (m.horizon as f64, m.mean_ic.ln())).collect();

    if positive_points.len() < 2 {
        return 0.0;
    }

    let n = positive_points.len() as f64;
    let mean_x = positive_points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = positive_points.iter().map(|p| p.1).sum::<f64>() / n;

    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in positive_points {
        num += (x - mean_x) * (y - mean_y);
        den += (x - mean_x).powi(2);
    }

    if den <= 1e-12 {
        return 0.0;
    }

    let slope = num / den;
    let lambda = -slope;
    if lambda > 1e-6 {
        (2.0f64).ln() / lambda
    } else {
        0.0
    }
}

/// Quantile performance and top-minus-bottom spread analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct QuantileAnalysis {
    pub quantile_count: usize,
    /// Average return for each quantile (from Q1 lowest signal to Q_top highest signal).
    pub quantile_mean_returns: Vec<f64>,
    /// Mean top-minus-bottom forward return spread per period.
    pub top_bottom_spread: f64,
    /// Annualized spread (assuming 252 trading days).
    pub annualized_spread: f64,
    /// Mean turnover of the top quantile across consecutive dates.
    pub top_quantile_turnover: f64,
}

/// Owner configurable gate threshold for signal validation (M12 Task 7).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SignalGateThreshold {
    /// Minimum acceptable mean Information Coefficient (e.g. 0.05).
    pub min_mean_ic: Option<f64>,
    /// Minimum acceptable t-statistic (e.g. 2.0).
    pub min_t_stat: Option<f64>,
    /// Maximum acceptable top-quantile turnover (e.g. 0.70).
    pub max_turnover: Option<f64>,
    /// Minimum signal half-life in days (e.g. 3.0).
    pub min_half_life_days: Option<f64>,
    /// Maximum acceptable signal half-life in days (e.g. 20.0).
    pub max_half_life_days: Option<f64>,
}

impl SignalGateThreshold {
    /// Default gate thresholds approved by the owner:
    /// Mean IC >= 0.05, t-statistic >= 2.0, Half-life <= 20.0 days.
    pub fn owner_default() -> Self {
        Self {
            min_mean_ic: Some(0.05),
            min_t_stat: Some(2.0),
            max_turnover: None,
            min_half_life_days: None,
            max_half_life_days: Some(20.0),
        }
    }

    /// Evaluates if the signal test passes the threshold criteria.
    pub fn evaluate(&self, mean_ic: f64, t_stat: f64, turnover: f64, half_life: f64) -> (bool, Vec<String>) {
        let mut passed = true;
        let mut reasons = Vec::new();

        if let Some(min_ic) = self.min_mean_ic {
            if mean_ic < min_ic {
                passed = false;
                reasons.push(format!("Mean IC {mean_ic:.4} below required threshold {min_ic:.4}"));
            }
        }
        if let Some(min_t) = self.min_t_stat {
            if t_stat < min_t {
                passed = false;
                reasons.push(format!("t-statistic {t_stat:.2} below required threshold {min_t:.2}"));
            }
        }
        if let Some(max_turn) = self.max_turnover {
            if turnover > max_turn {
                passed = false;
                let turn_pct = turnover * 100.0;
                let max_pct = max_turn * 100.0;
                reasons.push(format!("Top-quantile turnover {turn_pct:.2}% above limit {max_pct:.2}%"));
            }
        }
        if let Some(min_hl) = self.min_half_life_days {
            if half_life < min_hl {
                passed = false;
                reasons.push(format!("Half-life {half_life:.1} days below required {min_hl:.1} days"));
            }
        }
        if let Some(max_hl) = self.max_half_life_days {
            if half_life > max_hl {
                passed = false;
                reasons.push(format!(
                    "Half-life {half_life:.1} days exceeds maximum threshold {max_hl:.1} days"
                ));
            }
        }

        if passed && reasons.is_empty() {
            reasons.push("Signal passed all owner gate thresholds".to_string());
        }

        (passed, reasons)
    }
}

/// Comprehensive signal test report (M12 Tasks 1-7).
#[derive(Debug, Clone, PartialEq)]
pub struct SignalTestReport {
    /// Signal identifier or name.
    pub signal_name: String,
    /// Symbols in test universe.
    pub universe: Vec<String>,
    /// Start date of test.
    pub start_date: String,
    /// End date of test.
    pub end_date: String,
    /// Dataset version ID (point-in-time reference).
    pub dataset_version_id: Option<String>,
    /// Evaluated trading horizons.
    pub horizons: Vec<usize>,
    /// Primary holding horizon (default 5 trading days).
    pub primary_horizon: usize,
    /// Primary IC summary.
    pub primary_ic: IcSummary,
    /// Multi-horizon decay analysis.
    pub decay: DecayResult,
    /// Quantile and turnover analysis.
    pub quantiles: QuantileAnalysis,
    /// Configured gate threshold.
    pub threshold: Option<SignalGateThreshold>,
    /// Whether signal passed gate threshold.
    pub passed_gate: bool,
    /// Explanatory gate reasons.
    pub gate_reasons: Vec<String>,
    /// Number of tests run on this dataset (overfitting transparency).
    pub tests_run_count: usize,
    /// Mandatory research disclaimer.
    pub disclaimer: &'static str,
}

impl SignalTestReport {
    /// Formats the report as a deterministic `spectra_core::Json` object.
    pub fn to_json(&self) -> Json {
        let mut obj = vec![
            ("signal_name".into(), Json::String(self.signal_name.clone())),
            ("universe".into(), Json::Array(self.universe.iter().map(|s| Json::String(s.clone())).collect())),
            ("start_date".into(), Json::String(self.start_date.clone())),
            ("end_date".into(), Json::String(self.end_date.clone())),
            (
                "dataset_version_id".into(),
                self.dataset_version_id.as_ref().map_or(Json::Null, |id| Json::String(id.clone())),
            ),
            ("horizons".into(), Json::Array(self.horizons.iter().map(|&h| Json::Int(h as i64)).collect())),
            ("primary_horizon".into(), Json::Int(self.primary_horizon as i64)),
        ];

        // Primary IC
        let ic_obj = vec![
            ("mean_ic".into(), Json::number(self.primary_ic.mean_ic).unwrap_or(Json::Double(0.0))),
            ("ic_stdev".into(), Json::number(self.primary_ic.ic_stdev).unwrap_or(Json::Double(0.0))),
            ("t_stat".into(), Json::number(self.primary_ic.t_stat).unwrap_or(Json::Double(0.0))),
            ("date_count".into(), Json::Int(self.primary_ic.date_count as i64)),
        ];
        obj.push(("primary_ic".into(), Json::Object(ic_obj)));

        // Decay metrics
        let mut decay_arr = Vec::new();
        for m in &self.decay.horizon_metrics {
            decay_arr.push(Json::Object(vec![
                ("horizon".into(), Json::Int(m.horizon as i64)),
                ("mean_ic".into(), Json::number(m.mean_ic).unwrap_or(Json::Double(0.0))),
                ("ic_stdev".into(), Json::number(m.ic_stdev).unwrap_or(Json::Double(0.0))),
                ("t_stat".into(), Json::number(m.t_stat).unwrap_or(Json::Double(0.0))),
                ("date_count".into(), Json::Int(m.date_count as i64)),
            ]));
        }
        let decay_obj = vec![
            ("horizons".into(), Json::Array(decay_arr)),
            ("half_life_days".into(), Json::number(self.decay.half_life_days).unwrap_or(Json::Double(0.0))),
        ];
        obj.push(("decay".into(), Json::Object(decay_obj)));

        // Quantiles & Turnover
        let quantiles_arr = self
            .quantiles
            .quantile_mean_returns
            .iter()
            .map(|&r| Json::number(r).unwrap_or(Json::Double(0.0)))
            .collect();
        let quantiles_obj = vec![
            ("quantile_count".into(), Json::Int(self.quantiles.quantile_count as i64)),
            ("quantile_mean_returns".into(), Json::Array(quantiles_arr)),
            (
                "top_bottom_spread".into(),
                Json::number(self.quantiles.top_bottom_spread).unwrap_or(Json::Double(0.0)),
            ),
            (
                "annualized_spread".into(),
                Json::number(self.quantiles.annualized_spread).unwrap_or(Json::Double(0.0)),
            ),
            (
                "top_quantile_turnover".into(),
                Json::number(self.quantiles.top_quantile_turnover).unwrap_or(Json::Double(0.0)),
            ),
        ];
        obj.push(("quantiles".into(), Json::Object(quantiles_obj)));

        // Gate & Risk
        obj.push(("passed_gate".into(), Json::Bool(self.passed_gate)));
        obj.push((
            "gate_reasons".into(),
            Json::Array(self.gate_reasons.iter().map(|r| Json::String(r.clone())).collect()),
        ));
        obj.push(("tests_run_count".into(), Json::Int(self.tests_run_count as i64)));
        obj.push(("disclaimer".into(), Json::String(self.disclaimer.into())));

        Json::Object(obj)
    }

    /// Formats the report as CSV.
    pub fn to_csv(&self) -> String {
        let mut csv = String::new();
        csv.push_str("# SpectraLabs M12 Alpha Workbench: Signal Test Report\n");
        csv.push_str(&format!("# Signal: {}\n", self.signal_name));
        csv.push_str(&format!("# Universe: {}\n", self.universe.join(",")));
        csv.push_str(&format!("# Start Date: {}\n", self.start_date));
        csv.push_str(&format!("# End Date: {}\n", self.end_date));
        csv.push_str(&format!("# Primary Horizon: {} days\n", self.primary_horizon));
        csv.push_str(&format!("# Mean IC: {:.4}\n", self.primary_ic.mean_ic));
        csv.push_str(&format!("# IC Stdev: {:.4}\n", self.primary_ic.ic_stdev));
        csv.push_str(&format!("# t-statistic: {:.2}\n", self.primary_ic.t_stat));
        csv.push_str(&format!("# Half-life (days): {:.2}\n", self.decay.half_life_days));
        csv.push_str(&format!("# Top-Bottom Spread: {:.4}\n", self.quantiles.top_bottom_spread));
        csv.push_str(&format!("# Top-Quantile Turnover: {:.4}\n", self.quantiles.top_quantile_turnover));
        csv.push_str(&format!("# Passed Gate: {}\n", self.passed_gate));
        csv.push_str(&format!("# Tests Run Count: {}\n", self.tests_run_count));
        csv.push_str(&format!("# Disclaimer: {}\n", self.disclaimer));
        csv.push_str("date,horizon,mean_ic\n");
        for (d, ic) in &self.primary_ic.daily_ics {
            csv.push_str(&format!("{},{},{:.6}\n", d, self.primary_horizon, ic));
        }
        csv
    }
}

/// Runs the complete signal test suite on input signals and price bars.
///
/// # Errors
/// Returns [`Error::InvalidArgument`] if signals or bars are empty.
pub fn run_signal_test(
    signal_name: &str,
    signals: &[SignalValue],
    bars: &[PriceBar],
    horizons: &[usize],
    primary_horizon: usize,
    quantile_count: usize,
    threshold: Option<&SignalGateThreshold>,
    dataset_version_id: Option<String>,
    tests_run_count: usize,
) -> Result<SignalTestReport> {
    if signals.is_empty() {
        return Err(Error::InvalidArgument("signal values cannot be empty".into()));
    }
    if bars.is_empty() {
        return Err(Error::InvalidArgument("price bars cannot be empty".into()));
    }

    let effective_horizons = if horizons.is_empty() { &DEFAULT_HORIZONS[..] } else { horizons };

    let fwd_returns = compute_forward_returns(bars, effective_horizons);

    // Group signals by date: date -> Vec<(symbol, signal_value)>
    let mut signals_by_date: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut universe_set = BTreeSet::new();
    for s in signals {
        signals_by_date.entry(s.date.clone()).or_default().push((s.symbol.clone(), s.value));
        universe_set.insert(s.symbol.clone());
    }

    let universe: Vec<String> = universe_set.into_iter().collect();
    let dates: Vec<String> = signals_by_date.keys().cloned().collect();
    let start_date = dates.first().cloned().unwrap_or_else(String::new);
    let end_date = dates.last().cloned().unwrap_or_else(String::new);

    // Compute IC per horizon
    let mut horizon_metrics = Vec::new();
    for &h in effective_horizons {
        let mut daily_ics = Vec::new();
        for (d, sigs) in &signals_by_date {
            let mut sig_vals = Vec::new();
            let mut ret_vals = Vec::new();
            for (sym, s_val) in sigs {
                if let Some(h_map) = fwd_returns.get(&(sym.clone(), d.clone())) {
                    if let Some(&ret) = h_map.get(&h) {
                        sig_vals.push(*s_val);
                        ret_vals.push(ret);
                    }
                }
            }
            if sig_vals.len() >= 2 {
                if let Ok(ic) = spearman_correlation(&sig_vals, &ret_vals) {
                    daily_ics.push((d.clone(), ic));
                }
            }
        }

        let n = daily_ics.len();
        let (mean_ic, ic_stdev, t_stat) = if n >= 2 {
            let mean = daily_ics.iter().map(|(_, ic)| *ic).sum::<f64>() / (n as f64);
            let var = daily_ics.iter().map(|(_, ic)| (ic - mean).powi(2)).sum::<f64>() / ((n - 1) as f64);
            let stdev = var.sqrt();
            let t = if stdev > 1e-12 { mean / (stdev / (n as f64).sqrt()) } else { 0.0 };
            (mean, stdev, t)
        } else if n == 1 {
            (daily_ics[0].1, 0.0, 0.0)
        } else {
            (0.0, 0.0, 0.0)
        };

        horizon_metrics.push(IcSummary { horizon: h, mean_ic, ic_stdev, t_stat, date_count: n, daily_ics });
    }

    let primary_ic =
        horizon_metrics.iter().find(|m| m.horizon == primary_horizon).cloned().unwrap_or_else(|| {
            horizon_metrics.first().cloned().unwrap_or(IcSummary {
                horizon: primary_horizon,
                mean_ic: 0.0,
                ic_stdev: 0.0,
                t_stat: 0.0,
                date_count: 0,
                daily_ics: Vec::new(),
            })
        });

    let half_life_days = fit_signal_half_life(&horizon_metrics);
    let decay = DecayResult { horizon_metrics, half_life_days };

    // Quantile analysis for primary horizon
    let q_count = if quantile_count < 2 { DEFAULT_QUANTILES } else { quantile_count };
    let mut quantile_sums = vec![0.0; q_count];
    let mut quantile_counts = vec![0usize; q_count];
    let mut daily_spreads = Vec::new();
    let mut daily_top_symbols: Vec<BTreeSet<String>> = Vec::new();

    for (d, sigs) in &signals_by_date {
        let mut paired: Vec<(String, f64, f64)> = Vec::new();
        for (sym, s_val) in sigs {
            if let Some(h_map) = fwd_returns.get(&(sym.clone(), d.clone())) {
                if let Some(&ret) = h_map.get(&primary_horizon) {
                    paired.push((sym.clone(), *s_val, ret));
                }
            }
        }
        if paired.len() < q_count {
            continue;
        }

        // Sort ascending by signal value
        paired.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let m = paired.len();
        let mut top_set = BTreeSet::new();

        let mut current_q_rets = vec![Vec::new(); q_count];
        for (idx, (sym, _, ret)) in paired.into_iter().enumerate() {
            let q = (idx * q_count) / m;
            let q_clamped = q.min(q_count - 1);
            current_q_rets[q_clamped].push(ret);
            if q_clamped == q_count - 1 {
                top_set.insert(sym);
            }
        }

        daily_top_symbols.push(top_set);

        for q in 0..q_count {
            if !current_q_rets[q].is_empty() {
                let mean_q = current_q_rets[q].iter().sum::<f64>() / (current_q_rets[q].len() as f64);
                quantile_sums[q] += mean_q;
                quantile_counts[q] += 1;
            }
        }

        if !current_q_rets[0].is_empty() && !current_q_rets[q_count - 1].is_empty() {
            let bottom = current_q_rets[0].iter().sum::<f64>() / (current_q_rets[0].len() as f64);
            let top =
                current_q_rets[q_count - 1].iter().sum::<f64>() / (current_q_rets[q_count - 1].len() as f64);
            daily_spreads.push(top - bottom);
        }
    }

    let mut quantile_mean_returns = vec![0.0; q_count];
    for q in 0..q_count {
        if quantile_counts[q] > 0 {
            quantile_mean_returns[q] = quantile_sums[q] / (quantile_counts[q] as f64);
        }
    }

    let top_bottom_spread = if daily_spreads.is_empty() {
        0.0
    } else {
        daily_spreads.iter().sum::<f64>() / (daily_spreads.len() as f64)
    };
    let annualized_spread = top_bottom_spread * (252.0 / (primary_horizon as f64)).sqrt();

    // Top quantile turnover
    let mut turnover_sum = 0.0;
    let mut turnover_count = 0usize;
    for i in 1..daily_top_symbols.len() {
        let prev = &daily_top_symbols[i - 1];
        let curr = &daily_top_symbols[i];
        if !curr.is_empty() {
            let intersection = curr.intersection(prev).count();
            let turnover = 1.0 - (intersection as f64 / curr.len() as f64);
            turnover_sum += turnover;
            turnover_count += 1;
        }
    }
    let top_quantile_turnover = if turnover_count > 0 { turnover_sum / (turnover_count as f64) } else { 0.0 };

    let quantiles = QuantileAnalysis {
        quantile_count: q_count,
        quantile_mean_returns,
        top_bottom_spread,
        annualized_spread,
        top_quantile_turnover,
    };

    // Evaluate gate threshold
    let (passed_gate, gate_reasons) = if let Some(t) = threshold {
        t.evaluate(primary_ic.mean_ic, primary_ic.t_stat, top_quantile_turnover, decay.half_life_days)
    } else {
        (true, vec!["No owner threshold specified; test evaluated for research".to_string()])
    };

    Ok(SignalTestReport {
        signal_name: signal_name.to_string(),
        universe,
        start_date,
        end_date,
        dataset_version_id,
        horizons: effective_horizons.to_vec(),
        primary_horizon,
        primary_ic,
        decay,
        quantiles,
        threshold: threshold.cloned(),
        passed_gate,
        gate_reasons,
        tests_run_count: tests_run_count.max(1),
        disclaimer: "research only, not a prediction",
    })
}
