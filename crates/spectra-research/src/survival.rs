//! Strategy Survival Lab.
//!
//! Takes ONE strategy configuration and tries to break it. Each test reports
//! its own numbers and a [`Verdict`] against documented, configurable
//! thresholds. There is deliberately NO aggregate score: a single number
//! would let a strong result in one area hide a fatal weakness in another.
//! The report's `worst` field is only the most severe component verdict.
//!
//! Verdicts, in increasing severity: Pass < Warn < Insufficient < Fail.
//! `Insufficient` means the data cannot support ANY conclusion; it is more
//! serious than a warning and never a pass.
//!
//! The lab stresses ONE chosen configuration for fragility; it cannot tell
//! whether that configuration was merely the luckiest of many tried. That is
//! the job of the Deflated Sharpe Ratio, PBO and walk-forward validation.
//!
//! Components: 1 cost sensitivity, 2 execution delay, 3 parameter stability,
//! 4 time-period robustness, 5 trade bootstrap, 6 sample size and
//! concentration, 7 regime sensitivity, 8 benchmark-relative, 9 drawdown and
//! tail risk. Thresholds are conservative rules of thumb, not statistical
//! tests. Output text is identical to the C++ reference.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use spectra_core::cfmt::{fmt_f, pad_left, pad_right};
use spectra_core::{timestamp_offsets, Bar, Bps, EntryType, Error, Json, Ledger, Result};
use spectra_engine::{
    compute_drawdown_stats, compute_tail_stats, BacktestConfig, BacktestResult, DrawdownStats, Metrics,
    RunRecord, TailStats,
};

use crate::benchmark::{aligned_returns, benchmark_returns, compute_benchmark_metrics, BenchmarkMetrics};
use crate::stats::{moments, nearest_rank_index};
use crate::sweep::{run_one, ParamSet, StrategyFactory};
use crate::trades::{compute_trade_stats, extract_trades, resample_trades, TradeResampling, TradeStats};

/// Component verdict, in increasing severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Verdict {
    /// No weakness found.
    Pass,
    /// A weakness worth a human look.
    Warn,
    /// The data cannot support any conclusion.
    #[default]
    Insufficient,
    /// Demonstrated fragility.
    Fail,
}

/// Progress through a survival test's backtest re-runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurvivalProgress {
    /// Test number, from 1 (cost sensitivity) to 9 (drawdown and tail risk).
    pub test_number: u8,
    /// One-based completed step within this test.
    pub step: usize,
    /// Planned number of steps in this test.
    pub total_steps: usize,
}

struct SurvivalExecution<'a> {
    cancel: Option<&'a AtomicBool>,
    progress: Option<&'a mut dyn FnMut(SurvivalProgress)>,
    test_number: u8,
    step: usize,
    total_steps: usize,
}

impl SurvivalExecution<'_> {
    fn begin_test(&mut self, test_number: u8, total_steps: usize) {
        self.test_number = test_number;
        self.step = 0;
        self.total_steps = total_steps;
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    fn complete_test(&mut self, test_number: u8) {
        self.begin_test(test_number, 1);
        self.step = 1;
        if let Some(callback) = &mut self.progress {
            callback(SurvivalProgress { test_number, step: 1, total_steps: 1 });
        }
    }

    fn run_one(
        &mut self,
        bars: &[Bar],
        factory: &StrategyFactory<'_>,
        config: &BacktestConfig,
        params: &ParamSet,
    ) -> Result<BacktestResult> {
        self.check_cancelled()?;
        self.step += 1;
        if let Some(callback) = &mut self.progress {
            callback(SurvivalProgress {
                test_number: self.test_number,
                step: self.step,
                total_steps: self.total_steps,
            });
        }
        // A callback can set the cancel flag; honor it before starting work.
        self.check_cancelled()?;
        run_one(bars, factory, config, params)
    }
}

impl Verdict {
    /// Display name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Insufficient => "INSUFFICIENT",
            Self::Fail => "FAIL",
        }
    }
}

fn worse(a: Verdict, b: Verdict) -> Verdict {
    a.max(b)
}

/// Thresholds and grids.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalConfig {
    /// 1. Additional slippage levels (bps) on top of the baseline.
    pub extra_slippage_bps: Vec<f64>,
    /// The level the cost verdict is judged at.
    pub cost_stress_bps: f64,
    /// 2. Delays (bars beyond the baseline). Judged at 1 bar.
    pub delay_bars: Vec<usize>,
    /// 3. Relative changes applied to one parameter at a time.
    pub perturbation_fractions: Vec<f64>,
    /// Parameters rounded to the nearest integer after perturbing.
    pub integer_params: BTreeSet<String>,
    /// Warn below this share of neighbours with Sharpe > 0.
    pub perturbation_positive_warn: f64,
    /// Fail below this share.
    pub perturbation_positive_fail: f64,
    /// 4. Number of sub-periods.
    pub n_subperiods: usize,
    /// Minimum timestamps per sub-period.
    pub min_timestamps_per_subperiod: usize,
    /// Warn below this share of sub-periods with Sharpe > 0.
    pub subperiod_positive_warn: f64,
    /// Fail below this share.
    pub subperiod_positive_fail: f64,
    /// 5-6. Bootstrap resamples.
    pub bootstrap_resamples: usize,
    /// Resampling seed.
    pub seed: u64,
    /// Minimum trades to resample.
    pub min_trades_for_resampling: usize,
    /// Fewer trades than this is Insufficient.
    pub trades_insufficient_below: usize,
    /// Fewer trades than this is a warning.
    pub trades_warn_below: usize,
    /// Warn when P(resampled total P&L < 0) exceeds this.
    pub bootstrap_loss_warn: f64,
    /// Fail when it exceeds this.
    pub bootstrap_loss_fail: f64,
    /// Warn when this share of reshuffles has a deeper drawdown than observed.
    pub lucky_ordering_warn: f64,
    /// Warn when the best trade is this share of winning P&L.
    pub top_trade_warn: f64,
    /// Fail at this share.
    pub top_trade_fail: f64,
    /// Warn when a stressed Sharpe falls below this fraction of baseline.
    pub degradation_warn_ratio: f64,
    /// 7. Trailing volatility window.
    pub regime_vol_window: usize,
    /// Minimum labelled periods.
    pub regime_min_periods: usize,
    /// Minimum periods per regime bucket.
    pub regime_min_per_bucket: usize,
    /// 9. Warn at this max drawdown.
    pub max_drawdown_warn: f64,
    /// Fail at this max drawdown.
    pub max_drawdown_fail: f64,
    /// Tail level for VaR/ES.
    pub tail_level: f64,
}

impl Default for SurvivalConfig {
    fn default() -> Self {
        Self {
            extra_slippage_bps: vec![0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0],
            cost_stress_bps: 5.0,
            delay_bars: vec![0, 1, 2, 5],
            perturbation_fractions: vec![-0.25, -0.10, 0.10, 0.25],
            integer_params: BTreeSet::new(),
            perturbation_positive_warn: 0.80,
            perturbation_positive_fail: 0.50,
            n_subperiods: 4,
            min_timestamps_per_subperiod: 20,
            subperiod_positive_warn: 0.75,
            subperiod_positive_fail: 0.50,
            bootstrap_resamples: 2000,
            seed: 1,
            min_trades_for_resampling: 10,
            trades_insufficient_below: 10,
            trades_warn_below: 30,
            bootstrap_loss_warn: 0.10,
            bootstrap_loss_fail: 0.25,
            lucky_ordering_warn: 0.80,
            top_trade_warn: 0.50,
            top_trade_fail: 0.80,
            degradation_warn_ratio: 0.50,
            regime_vol_window: 20,
            regime_min_periods: 60,
            regime_min_per_bucket: 10,
            max_drawdown_warn: 0.20,
            max_drawdown_fail: 0.40,
            tail_level: 0.95,
        }
    }
}

impl SurvivalConfig {
    /// Default thresholds, named for workspace preset storage.
    pub fn default_preset() -> Self {
        Self::default()
    }

    /// A stricter preset: more cost/delay stress and tighter risk limits.
    pub fn strict_preset() -> Self {
        Self {
            extra_slippage_bps: vec![0.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0],
            cost_stress_bps: 10.0,
            delay_bars: vec![0, 1, 2, 5, 10],
            perturbation_positive_warn: 0.90,
            perturbation_positive_fail: 0.70,
            subperiod_positive_warn: 1.0,
            subperiod_positive_fail: 0.75,
            bootstrap_resamples: 5000,
            bootstrap_loss_warn: 0.05,
            bootstrap_loss_fail: 0.15,
            max_drawdown_warn: 0.15,
            max_drawdown_fail: 0.30,
            ..Self::default()
        }
    }

    /// A more permissive preset for exploratory research.
    pub fn lenient_preset() -> Self {
        Self {
            extra_slippage_bps: vec![0.0, 1.0, 2.0, 5.0, 10.0],
            cost_stress_bps: 2.0,
            delay_bars: vec![0, 1, 2],
            perturbation_positive_warn: 0.60,
            perturbation_positive_fail: 0.30,
            subperiod_positive_warn: 0.50,
            subperiod_positive_fail: 0.25,
            bootstrap_resamples: 1000,
            bootstrap_loss_warn: 0.20,
            bootstrap_loss_fail: 0.40,
            max_drawdown_warn: 0.30,
            max_drawdown_fail: 0.60,
            ..Self::default()
        }
    }

    /// Validates threshold ordering, probability ranges, and non-empty grids.
    ///
    /// # Errors
    /// Returns [`Error::InvalidArgument`] when a threshold is out of range or
    /// a required grid/count is empty or inconsistent.
    pub fn validate(&self) -> Result<()> {
        let invalid = |message: &str| Error::InvalidArgument(format!("invalid survival config: {message}"));
        let ordered = |warn: f64, fail: f64| {
            warn.is_finite()
                && fail.is_finite()
                && (0.0..=1.0).contains(&fail)
                && (0.0..=1.0).contains(&warn)
                && fail <= warn
        };
        if self.extra_slippage_bps.is_empty()
            || self.extra_slippage_bps.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(invalid("slippage grid must be non-empty, finite, and non-negative"));
        }
        if !self.cost_stress_bps.is_finite() || self.cost_stress_bps < 0.0 {
            return Err(invalid("cost stress must be finite and non-negative"));
        }
        if self.delay_bars.is_empty() {
            return Err(invalid("delay grid must be non-empty"));
        }
        if self.perturbation_fractions.is_empty()
            || self.perturbation_fractions.iter().any(|v| !v.is_finite() || *v <= -1.0)
        {
            return Err(invalid("perturbation grid must be non-empty, finite, and greater than -1"));
        }
        if !ordered(self.perturbation_positive_warn, self.perturbation_positive_fail)
            || !ordered(self.subperiod_positive_warn, self.subperiod_positive_fail)
            || !ordered(self.bootstrap_loss_fail, self.bootstrap_loss_warn)
            || !ordered(self.top_trade_fail, self.top_trade_warn)
        {
            return Err(invalid("warn/fail thresholds must be in [0, 1] with fail no greater than warn"));
        }
        if !self.degradation_warn_ratio.is_finite() || !(0.0..=1.0).contains(&self.degradation_warn_ratio) {
            return Err(invalid("degradation ratio must be in [0, 1]"));
        }
        if !self.lucky_ordering_warn.is_finite() || !(0.0..=1.0).contains(&self.lucky_ordering_warn) {
            return Err(invalid("lucky-ordering threshold must be in [0, 1]"));
        }
        if !self.max_drawdown_warn.is_finite()
            || !self.max_drawdown_fail.is_finite()
            || self.max_drawdown_warn < 0.0
            || self.max_drawdown_fail < self.max_drawdown_warn
        {
            return Err(invalid("drawdown fail threshold must be at least the non-negative warn threshold"));
        }
        if !self.tail_level.is_finite() || !(0.0..1.0).contains(&self.tail_level) {
            return Err(invalid("tail level must be in (0, 1)"));
        }
        if self.n_subperiods < 2
            || self.min_timestamps_per_subperiod == 0
            || self.bootstrap_resamples == 0
            || self.min_trades_for_resampling == 0
            || self.trades_insufficient_below == 0
            || self.trades_warn_below < self.trades_insufficient_below
            || self.regime_vol_window == 0
            || self.regime_min_periods == 0
            || self.regime_min_per_bucket == 0
        {
            return Err(invalid(
                "counts must be positive and trade warning count must be at least the insufficient count",
            ));
        }
        Ok(())
    }

    /// Serializes thresholds in a stable object shape.
    ///
    /// # Errors
    /// Returns an error if the configuration is invalid or contains a value
    /// that cannot be represented by the JSON format.
    pub fn to_json(&self) -> Result<Json> {
        self.validate()?;
        let nums = |values: &[f64]| -> Result<Json> {
            Ok(Json::Array(values.iter().map(|v| Json::number(*v)).collect::<Result<Vec<_>>>()?))
        };
        let ints = |values: &[usize]| -> Result<Json> {
            Ok(Json::Array(values.iter().map(|v| Json::integer(*v as i64)).collect()))
        };
        let mut integer_params = Json::array();
        for name in &self.integer_params {
            integer_params.push(Json::string(name))?;
        }
        Ok(Json::object()
            .with("extra_slippage_bps", nums(&self.extra_slippage_bps)?)
            .with("cost_stress_bps", Json::number(self.cost_stress_bps)?)
            .with("delay_bars", ints(&self.delay_bars)?)
            .with("perturbation_fractions", nums(&self.perturbation_fractions)?)
            .with("integer_params", integer_params)
            .with("perturbation_positive_warn", Json::number(self.perturbation_positive_warn)?)
            .with("perturbation_positive_fail", Json::number(self.perturbation_positive_fail)?)
            .with("n_subperiods", Json::integer(self.n_subperiods as i64))
            .with("min_timestamps_per_subperiod", Json::integer(self.min_timestamps_per_subperiod as i64))
            .with("subperiod_positive_warn", Json::number(self.subperiod_positive_warn)?)
            .with("subperiod_positive_fail", Json::number(self.subperiod_positive_fail)?)
            .with("bootstrap_resamples", Json::integer(self.bootstrap_resamples as i64))
            .with("seed", Json::integer(self.seed as i64))
            .with("min_trades_for_resampling", Json::integer(self.min_trades_for_resampling as i64))
            .with("trades_insufficient_below", Json::integer(self.trades_insufficient_below as i64))
            .with("trades_warn_below", Json::integer(self.trades_warn_below as i64))
            .with("bootstrap_loss_warn", Json::number(self.bootstrap_loss_warn)?)
            .with("bootstrap_loss_fail", Json::number(self.bootstrap_loss_fail)?)
            .with("lucky_ordering_warn", Json::number(self.lucky_ordering_warn)?)
            .with("top_trade_warn", Json::number(self.top_trade_warn)?)
            .with("top_trade_fail", Json::number(self.top_trade_fail)?)
            .with("degradation_warn_ratio", Json::number(self.degradation_warn_ratio)?)
            .with("regime_vol_window", Json::integer(self.regime_vol_window as i64))
            .with("regime_min_periods", Json::integer(self.regime_min_periods as i64))
            .with("regime_min_per_bucket", Json::integer(self.regime_min_per_bucket as i64))
            .with("max_drawdown_warn", Json::number(self.max_drawdown_warn)?)
            .with("max_drawdown_fail", Json::number(self.max_drawdown_fail)?)
            .with("tail_level", Json::number(self.tail_level)?))
    }

    /// Parses a complete config object and rejects invalid thresholds.
    ///
    /// # Errors
    /// Returns [`Error::Json`] if a required field is absent, has the wrong
    /// type, or fails configuration validation.
    pub fn from_json(value: &Json) -> Result<Self> {
        let int = |key: &str| -> Result<usize> {
            usize::try_from(value.at(key)?.as_int()?)
                .map_err(|_| Error::Json(format!("{key} must be non-negative")))
        };
        let num = |key: &str| -> Result<f64> { value.at(key)?.as_double() };
        let nums =
            |key: &str| -> Result<Vec<f64>> { value.at(key)?.items()?.iter().map(Json::as_double).collect() };
        let delays = value
            .at("delay_bars")?
            .items()?
            .iter()
            .map(|v| {
                usize::try_from(v.as_int()?)
                    .map_err(|_| Error::Json("delay_bars must be non-negative".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let integer_params = value
            .at("integer_params")?
            .items()?
            .iter()
            .map(|v| Ok(v.as_str()?.to_owned()))
            .collect::<Result<BTreeSet<_>>>()?;
        let config = Self {
            extra_slippage_bps: nums("extra_slippage_bps")?,
            cost_stress_bps: num("cost_stress_bps")?,
            delay_bars: delays,
            perturbation_fractions: nums("perturbation_fractions")?,
            integer_params,
            perturbation_positive_warn: num("perturbation_positive_warn")?,
            perturbation_positive_fail: num("perturbation_positive_fail")?,
            n_subperiods: int("n_subperiods")?,
            min_timestamps_per_subperiod: int("min_timestamps_per_subperiod")?,
            subperiod_positive_warn: num("subperiod_positive_warn")?,
            subperiod_positive_fail: num("subperiod_positive_fail")?,
            bootstrap_resamples: int("bootstrap_resamples")?,
            seed: u64::try_from(value.at("seed")?.as_int()?)
                .map_err(|_| Error::Json("seed must be non-negative".into()))?,
            min_trades_for_resampling: int("min_trades_for_resampling")?,
            trades_insufficient_below: int("trades_insufficient_below")?,
            trades_warn_below: int("trades_warn_below")?,
            bootstrap_loss_warn: num("bootstrap_loss_warn")?,
            bootstrap_loss_fail: num("bootstrap_loss_fail")?,
            lucky_ordering_warn: num("lucky_ordering_warn")?,
            top_trade_warn: num("top_trade_warn")?,
            top_trade_fail: num("top_trade_fail")?,
            degradation_warn_ratio: num("degradation_warn_ratio")?,
            regime_vol_window: int("regime_vol_window")?,
            regime_min_periods: int("regime_min_periods")?,
            regime_min_per_bucket: int("regime_min_per_bucket")?,
            max_drawdown_warn: num("max_drawdown_warn")?,
            max_drawdown_fail: num("max_drawdown_fail")?,
            tail_level: num("tail_level")?,
        };
        config.validate().map_err(|e| Error::Json(e.to_string()))?;
        Ok(config)
    }
}

/// One cost level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CostLevel {
    /// Extra slippage (bps).
    pub extra_bps: f64,
    /// Annualized Sharpe.
    pub sharpe: f64,
    /// Total return.
    pub total_return: f64,
}

/// Component 1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CostSensitivity {
    /// Tested levels.
    pub levels: Vec<CostLevel>,
    /// Sharpe at `cost_stress_bps`.
    pub stressed_sharpe: f64,
    /// Sharpe falls to <= 0 within the tested range.
    pub breakeven_found: bool,
    /// Interpolated breakeven extra slippage.
    pub breakeven_extra_bps: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// One delay level.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DelayLevel {
    /// Extra bars.
    pub delay_bars: usize,
    /// Sharpe.
    pub sharpe: f64,
    /// Total return.
    pub total_return: f64,
}

/// Component 2.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DelaySensitivity {
    /// Tested levels.
    pub levels: Vec<DelayLevel>,
    /// Sharpe with a one-bar delay.
    pub one_bar_sharpe: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// One perturbed parameter set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Neighbour {
    /// Parameters.
    pub params: ParamSet,
    /// Sharpe.
    pub sharpe: f64,
    /// Total return.
    pub total_return: f64,
}

/// Component 3.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParameterStability {
    /// Baseline Sharpe.
    pub baseline_sharpe: f64,
    /// Neighbours that ran.
    pub neighbours: Vec<Neighbour>,
    /// Neighbours the strategy factory rejected.
    pub skipped: usize,
    /// Share with Sharpe > 0.
    pub share_positive: f64,
    /// Median Sharpe.
    pub median_sharpe: f64,
    /// Minimum Sharpe.
    pub min_sharpe: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// One sub-period.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Subperiod {
    /// First timestamp.
    pub first_ts: i64,
    /// Last timestamp.
    pub last_ts: i64,
    /// Sharpe.
    pub sharpe: f64,
    /// Total return.
    pub total_return: f64,
    /// Max drawdown.
    pub max_drawdown: f64,
}

/// Component 4.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeRobustness {
    /// Sub-periods.
    pub subperiods: Vec<Subperiod>,
    /// Share with Sharpe > 0.
    pub share_positive: f64,
    /// First-half Sharpe.
    pub first_half_sharpe: f64,
    /// Second-half Sharpe.
    pub second_half_sharpe: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// Component 5.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TradeBootstrap {
    /// Trade statistics.
    pub stats: TradeStats,
    /// Resampling results.
    pub resampling: TradeResampling,
    /// Open positions at the end.
    pub open_positions_at_end: usize,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// Component 6.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SampleAdequacy {
    /// Closed trades.
    pub trades: usize,
    /// Best trade share of winning P&L.
    pub top_trade_share: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// One volatility regime.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RegimeBucket {
    /// Name.
    pub name: String,
    /// Periods in the bucket.
    pub periods: usize,
    /// Share of labelled periods.
    pub share: f64,
    /// Mean return per period.
    pub mean_return: f64,
    /// Annualized Sharpe (0 when the bucket is too small).
    pub sharpe: f64,
}

/// Component 7.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RegimeSensitivity {
    /// Low, mid, high trailing volatility.
    pub buckets: Vec<RegimeBucket>,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// Component 8.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BenchmarkRelative {
    /// Metrics.
    pub metrics: BenchmarkMetrics,
    /// Mean of |equity - cash| / equity: roughly how much was invested (net).
    pub average_net_exposure: f64,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// Component 9.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RiskProfile {
    /// Drawdown statistics.
    pub drawdown: DrawdownStats,
    /// Tail statistics.
    pub tail: TailStats,
    /// Verdict.
    pub verdict: Verdict,
    /// Finding.
    pub finding: String,
}

/// The full battery.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurvivalReport {
    /// Schema version for consumers that persist report JSON.
    pub schema_version: u32,
    /// Exact thresholds used for this report.
    pub config: SurvivalConfig,
    /// Parameters tested.
    pub params: ParamSet,
    /// Baseline metrics.
    pub baseline: Metrics,
    /// Baseline run record.
    pub baseline_run: RunRecord,
    /// Baseline fill count.
    pub baseline_fills: usize,
    /// 1.
    pub costs: CostSensitivity,
    /// 2.
    pub delay: DelaySensitivity,
    /// 3.
    pub stability: ParameterStability,
    /// 4.
    pub time: TimeRobustness,
    /// 5.
    pub trades: TradeBootstrap,
    /// 6.
    pub sample: SampleAdequacy,
    /// 7.
    pub regimes: RegimeSensitivity,
    /// 8.
    pub benchmark: BenchmarkRelative,
    /// 9.
    pub risk: RiskProfile,
    /// Most severe component verdict (NOT a score).
    pub worst: Verdict,
    /// What the tests do and ignore.
    pub assumptions: Vec<String>,
}

fn json_f(v: f64) -> Json {
    if v.is_finite() {
        Json::number(v).unwrap_or(Json::Null)
    } else {
        Json::Null
    }
}

fn json_usize(v: usize) -> Json {
    i64::try_from(v).map_or(Json::Null, Json::integer)
}

fn json_strings(values: &[String]) -> Json {
    Json::Array(values.iter().map(Json::string).collect())
}

fn json_metrics(m: &Metrics) -> Json {
    Json::object()
        .with("valid", Json::boolean(m.valid))
        .with("periods", Json::integer(i64::from(m.periods)))
        .with("total_return", json_f(m.total_return))
        .with("annualized_return", json_f(m.annualized_return))
        .with("annualized_volatility", json_f(m.annualized_volatility))
        .with("sharpe", json_f(m.sharpe))
        .with("max_drawdown", json_f(m.max_drawdown))
}

fn json_verdict(v: Verdict) -> Json {
    Json::string(v.as_str())
}

impl SurvivalReport {
    /// Converts every report component to deterministic JSON. Non-finite
    /// analytics are represented as null because RFC 8259 has no infinities.
    ///
    /// # Errors
    /// Returns an error if the captured configuration is invalid or a
    /// parameter name cannot be inserted into the JSON object.
    pub fn to_json(&self) -> Result<Json> {
        let levels = |values: Vec<Json>| Json::Array(values);
        let costs = Json::object()
            .with(
                "levels",
                levels(
                    self.costs
                        .levels
                        .iter()
                        .map(|v| {
                            Json::object()
                                .with("extra_bps", json_f(v.extra_bps))
                                .with("sharpe", json_f(v.sharpe))
                                .with("total_return", json_f(v.total_return))
                        })
                        .collect(),
                ),
            )
            .with("stressed_sharpe", json_f(self.costs.stressed_sharpe))
            .with("breakeven_found", Json::boolean(self.costs.breakeven_found))
            .with("breakeven_extra_bps", json_f(self.costs.breakeven_extra_bps))
            .with("verdict", json_verdict(self.costs.verdict))
            .with("finding", Json::string(&self.costs.finding));
        let delay = Json::object()
            .with(
                "levels",
                levels(
                    self.delay
                        .levels
                        .iter()
                        .map(|v| {
                            Json::object()
                                .with("delay_bars", json_usize(v.delay_bars))
                                .with("sharpe", json_f(v.sharpe))
                                .with("total_return", json_f(v.total_return))
                        })
                        .collect(),
                ),
            )
            .with("one_bar_sharpe", json_f(self.delay.one_bar_sharpe))
            .with("verdict", json_verdict(self.delay.verdict))
            .with("finding", Json::string(&self.delay.finding));
        let stability = Json::object()
            .with("baseline_sharpe", json_f(self.stability.baseline_sharpe))
            .with(
                "neighbours",
                levels(
                    self.stability
                        .neighbours
                        .iter()
                        .map(|v| {
                            Json::object()
                                .with("params", Json::string(v.params.canonical()))
                                .with("sharpe", json_f(v.sharpe))
                                .with("total_return", json_f(v.total_return))
                        })
                        .collect(),
                ),
            )
            .with("skipped", json_usize(self.stability.skipped))
            .with("share_positive", json_f(self.stability.share_positive))
            .with("median_sharpe", json_f(self.stability.median_sharpe))
            .with("min_sharpe", json_f(self.stability.min_sharpe))
            .with("verdict", json_verdict(self.stability.verdict))
            .with("finding", Json::string(&self.stability.finding));
        let time = Json::object()
            .with(
                "subperiods",
                levels(
                    self.time
                        .subperiods
                        .iter()
                        .map(|v| {
                            Json::object()
                                .with("first_ts", Json::integer(v.first_ts))
                                .with("last_ts", Json::integer(v.last_ts))
                                .with("sharpe", json_f(v.sharpe))
                                .with("total_return", json_f(v.total_return))
                                .with("max_drawdown", json_f(v.max_drawdown))
                        })
                        .collect(),
                ),
            )
            .with("share_positive", json_f(self.time.share_positive))
            .with("first_half_sharpe", json_f(self.time.first_half_sharpe))
            .with("second_half_sharpe", json_f(self.time.second_half_sharpe))
            .with("verdict", json_verdict(self.time.verdict))
            .with("finding", Json::string(&self.time.finding));
        let band = |b: &crate::trades::PercentileBand| {
            Json::object().with("p05", json_f(b.p05)).with("p50", json_f(b.p50)).with("p95", json_f(b.p95))
        };
        let st = &self.trades.stats;
        let res = &self.trades.resampling;
        let trades = Json::object()
            .with(
                "stats",
                Json::object()
                    .with("n", json_usize(st.n))
                    .with("wins", json_usize(st.wins))
                    .with("losses", json_usize(st.losses))
                    .with("win_rate", json_f(st.win_rate))
                    .with("avg_win", json_f(st.avg_win))
                    .with("avg_loss", json_f(st.avg_loss))
                    .with("payoff_ratio", json_f(st.payoff_ratio))
                    .with("expectancy", json_f(st.expectancy))
                    .with("profit_factor", json_f(st.profit_factor))
                    .with("total_net", json_f(st.total_net))
                    .with("best", json_f(st.best))
                    .with("worst", json_f(st.worst))
                    .with("top_trade_share", json_f(st.top_trade_share))
                    .with("avg_holding_ns", json_f(st.avg_holding_ns)),
            )
            .with(
                "resampling",
                Json::object()
                    .with("valid", Json::boolean(res.valid))
                    .with("resamples", json_usize(res.resamples))
                    .with("observed_max_drawdown", json_f(res.observed_max_drawdown))
                    .with("bootstrap_terminal_pnl", band(&res.bootstrap_terminal_pnl))
                    .with(
                        "bootstrap_terminal_histogram",
                        levels(
                            res.bootstrap_terminal_histogram
                                .iter()
                                .map(|bin| {
                                    Json::object()
                                        .with("lower", json_f(bin.lower))
                                        .with("upper", json_f(bin.upper))
                                        .with("count", json_usize(bin.count))
                                })
                                .collect(),
                        ),
                    )
                    .with("bootstrap_max_drawdown", band(&res.bootstrap_max_drawdown))
                    .with("bootstrap_prob_loss", json_f(res.bootstrap_prob_loss))
                    .with("reshuffle_max_drawdown", band(&res.reshuffle_max_drawdown))
                    .with("reshuffle_prob_worse_drawdown", json_f(res.reshuffle_prob_worse_drawdown)),
            )
            .with("open_positions_at_end", json_usize(self.trades.open_positions_at_end))
            .with("verdict", json_verdict(self.trades.verdict))
            .with("finding", Json::string(&self.trades.finding));
        let sample = Json::object()
            .with("trades", json_usize(self.sample.trades))
            .with("top_trade_share", json_f(self.sample.top_trade_share))
            .with("verdict", json_verdict(self.sample.verdict))
            .with("finding", Json::string(&self.sample.finding));
        let regimes = Json::object()
            .with(
                "buckets",
                levels(
                    self.regimes
                        .buckets
                        .iter()
                        .map(|v| {
                            Json::object()
                                .with("name", Json::string(&v.name))
                                .with("periods", json_usize(v.periods))
                                .with("share", json_f(v.share))
                                .with("mean_return", json_f(v.mean_return))
                                .with("sharpe", json_f(v.sharpe))
                        })
                        .collect(),
                ),
            )
            .with("verdict", json_verdict(self.regimes.verdict))
            .with("finding", Json::string(&self.regimes.finding));
        let bm = &self.benchmark.metrics;
        let benchmark = Json::object()
            .with(
                "metrics",
                Json::object()
                    .with("valid", Json::boolean(bm.valid))
                    .with("beta", json_f(bm.beta))
                    .with("alpha_annualized", json_f(bm.alpha_annualized))
                    .with("correlation", json_f(bm.correlation))
                    .with("tracking_error", json_f(bm.tracking_error))
                    .with("information_ratio", json_f(bm.information_ratio))
                    .with("up_capture", json_f(bm.up_capture))
                    .with("down_capture", json_f(bm.down_capture)),
            )
            .with("average_net_exposure", json_f(self.benchmark.average_net_exposure))
            .with("verdict", json_verdict(self.benchmark.verdict))
            .with("finding", Json::string(&self.benchmark.finding));
        let d = &self.risk.drawdown;
        let t = &self.risk.tail;
        let risk = Json::object()
            .with(
                "drawdown",
                Json::object()
                    .with("valid", Json::boolean(d.valid))
                    .with("max_drawdown", json_f(d.max_drawdown))
                    .with("current_drawdown", json_f(d.current_drawdown))
                    .with("longest_underwater", json_usize(d.longest_underwater))
                    .with("ulcer_index", json_f(d.ulcer_index)),
            )
            .with(
                "tail",
                Json::object()
                    .with("valid", Json::boolean(t.valid))
                    .with("level", json_f(t.level))
                    .with("var", json_f(t.var))
                    .with("es", json_f(t.es))
                    .with("worst", json_f(t.worst)),
            )
            .with("verdict", json_verdict(self.risk.verdict))
            .with("finding", Json::string(&self.risk.finding));
        let mut params = Json::object();
        for (name, value) in &self.params.values {
            params.set(name, json_f(*value))?;
        }
        Ok(Json::object()
            .with("schema_version", Json::integer(i64::from(self.schema_version)))
            .with("config", self.config.to_json()?)
            .with("params", params)
            .with("baseline", json_metrics(&self.baseline))
            .with(
                "baseline_run",
                Json::object()
                    .with("engine_version", Json::string(&self.baseline_run.engine_version))
                    .with("dataset_sha256", Json::string(&self.baseline_run.dataset_sha256))
                    .with("config_sha256", Json::string(&self.baseline_run.config_sha256))
                    .with("result_sha256", Json::string(&self.baseline_run.result_sha256))
                    .with("seed", Json::integer(self.baseline_run.seed as i64))
                    .with("bar_count", json_usize(self.baseline_run.bar_count)),
            )
            .with("baseline_fills", json_usize(self.baseline_fills))
            .with("costs", costs)
            .with("delay", delay)
            .with("stability", stability)
            .with("time", time)
            .with("trades", trades)
            .with("sample", sample)
            .with("regimes", regimes)
            .with("benchmark", benchmark)
            .with("risk", risk)
            .with("worst", json_verdict(self.worst))
            .with("assumptions", json_strings(&self.assumptions)))
    }
}

#[derive(Clone, Copy)]
struct Summary {
    valid: bool,
    sharpe: f64,
    total_return: f64,
    max_drawdown: f64,
}

fn summarize(r: &BacktestResult) -> Summary {
    Summary {
        valid: r.metrics.valid,
        sharpe: r.metrics.sharpe,
        total_return: r.metrics.total_return,
        max_drawdown: r.metrics.max_drawdown,
    }
}

fn count_fills(l: &Ledger) -> usize {
    l.journal().iter().filter(|e| e.kind == EntryType::Fill).count()
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// "Stressed Sharpe vs baseline" rule shared by the cost and delay tests.
fn degradation_verdict(base: f64, stressed: f64, warn_ratio: f64) -> Verdict {
    if stressed <= 0.0 {
        Verdict::Fail
    } else if stressed < warn_ratio * base {
        Verdict::Warn
    } else {
        Verdict::Pass
    }
}

const NO_TRADES: &str =
    "The baseline run closed no trades, so there is nothing to stress; a strategy that never \
                         trades cannot be validated.";

fn f2(v: f64) -> String {
    fmt_f(v, 2)
}

/// Runs the full battery on `params`. Ordinary problems with the strategy or
/// data become Insufficient/Fail findings.
///
/// # Errors
/// Invalid bars or configuration, or a baseline strategy the factory rejects.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn run_survival_lab(
    bars: &[Bar],
    factory: &StrategyFactory<'_>,
    config: &BacktestConfig,
    params: &ParamSet,
    sc: &SurvivalConfig,
) -> Result<SurvivalReport> {
    run_survival_lab_controlled(bars, factory, config, params, sc, None, None)
}

/// Runs the battery with between-run cancellation and per-test progress.
/// Cancellation returns [`Error::Cancelled`].
///
/// # Errors
/// Returns an error for invalid input, rejected strategy parameters, failed
/// backtests, or a requested cancellation.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn run_survival_lab_controlled<'a>(
    bars: &[Bar],
    factory: &StrategyFactory<'_>,
    config: &BacktestConfig,
    params: &ParamSet,
    sc: &SurvivalConfig,
    cancel: Option<&'a AtomicBool>,
    progress: Option<&'a mut dyn FnMut(SurvivalProgress)>,
) -> Result<SurvivalReport> {
    sc.validate()?;
    let mut execution = SurvivalExecution { cancel, progress, test_number: 1, step: 0, total_steps: 1 };
    execution.check_cancelled()?;
    let mut rep = SurvivalReport {
        schema_version: 1,
        config: sc.clone(),
        params: params.clone(),
        ..SurvivalReport::default()
    };
    let base = run_one(bars, factory, config, params)?;
    let base_sum = summarize(&base);
    rep.baseline = base.metrics;
    rep.baseline_run = base.run.clone();
    rep.baseline_fills = count_fills(&base.ledger);
    let extraction = extract_trades(&base.ledger)?;
    let traded = rep.baseline_fills > 0 && base_sum.valid;

    rep.assumptions = [
        "Every test re-runs the full event-driven backtest on the same bars; only the named input changes.",
        "Cost test adds slippage on top of the baseline cost model; the Sharpe verdict is judged at the configured stress level.",
        "Delay test postpones fills by whole bars; cancels still take effect immediately.",
        "Parameter test perturbs one parameter at a time, never several together, so interactions are not explored.",
        "Sub-periods and halves are run from a cold start with fresh capital; long look-backs lose their warm-up.",
        "Trade resampling treats trades as exchangeable fixed dollar amounts and measures drawdown on closed trades only.",
        "Regimes are terciles of trailing realized volatility of the equal-weight benchmark, labelled with full-sample thresholds (descriptive, not predictive). HMM regimes belong to the risk engine.",
        "The benchmark is an equal-weight buy-and-hold of the dataset's instruments, ignoring costs. A strategy that deploys only a small share of capital will look weak against a fully invested benchmark; weigh alpha and information ratio against the reported exposure.",
        "These tests stress ONE chosen configuration for fragility. They cannot detect that it was the luckiest of many tried; use the Deflated Sharpe Ratio, PBO and walk-forward validation for that.",
        "Verdict thresholds are conservative rules of thumb, not statistical tests, and no aggregate score is produced.",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    // ---- 1. Cost sensitivity -------------------------------------------------
    {
        let c = &mut rep.costs;
        if traded {
            let mut grid = sc.extra_slippage_bps.clone();
            if !grid.contains(&0.0) {
                grid.push(0.0);
            }
            grid.sort_by(f64::total_cmp);
            let total_steps =
                grid.iter().filter(|&&v| v != 0.0).count() + usize::from(sc.cost_stress_bps != 0.0);
            execution.begin_test(1, total_steps);
            let mut level_at = |extra: f64| -> Result<CostLevel> {
                if extra == 0.0 {
                    return Ok(CostLevel {
                        extra_bps: 0.0,
                        sharpe: base_sum.sharpe,
                        total_return: base_sum.total_return,
                    });
                }
                let mut cfg = *config;
                cfg.costs.slippage = config.costs.slippage.checked_add(Bps::from_f64(extra)?)?;
                let s = summarize(&execution.run_one(bars, factory, &cfg, params)?);
                Ok(CostLevel { extra_bps: extra, sharpe: s.sharpe, total_return: s.total_return })
            };
            for &extra in &grid {
                c.levels.push(level_at(extra)?);
            }
            c.stressed_sharpe = level_at(sc.cost_stress_bps)?.sharpe;
            if base_sum.sharpe <= 0.0 {
                c.breakeven_found = true;
                c.breakeven_extra_bps = 0.0;
            } else {
                for i in 1..c.levels.len() {
                    if c.levels[i].sharpe <= 0.0 {
                        let (a, b) = (c.levels[i - 1], c.levels[i]);
                        c.breakeven_found = true;
                        c.breakeven_extra_bps =
                            a.extra_bps + a.sharpe / (a.sharpe - b.sharpe) * (b.extra_bps - a.extra_bps);
                        break;
                    }
                }
            }
            c.verdict = degradation_verdict(base_sum.sharpe, c.stressed_sharpe, sc.degradation_warn_ratio);
            c.finding = format!(
                "Sharpe {} at baseline costs, {} with +{} bps extra slippage; ",
                f2(base_sum.sharpe),
                f2(c.stressed_sharpe),
                fmt_f(sc.cost_stress_bps, 1)
            );
            c.finding += &if c.breakeven_found {
                format!("Sharpe reaches zero at about +{} bps.", fmt_f(c.breakeven_extra_bps, 1))
            } else {
                format!(
                    "Sharpe stays positive up to +{} bps.",
                    fmt_f(c.levels.last().map_or(0.0, |l| l.extra_bps), 0)
                )
            };
        } else {
            c.finding = NO_TRADES.into();
        }
        if execution.test_number == 1 && execution.step == 0 {
            execution.complete_test(1);
        }
    }

    // ---- 2. Execution delay --------------------------------------------------
    {
        let d = &mut rep.delay;
        if traded {
            let mut grid = sc.delay_bars.clone();
            if !grid.contains(&0) {
                grid.push(0);
            }
            grid.sort_unstable();
            let total_steps = grid.iter().filter(|&&v| v != 0).count() + 1;
            execution.begin_test(2, total_steps);
            let mut level_at = |extra: usize| -> Result<DelayLevel> {
                if extra == 0 {
                    return Ok(DelayLevel {
                        delay_bars: 0,
                        sharpe: base_sum.sharpe,
                        total_return: base_sum.total_return,
                    });
                }
                let mut cfg = *config;
                cfg.exec.delay_bars = config.exec.delay_bars + extra;
                let s = summarize(&execution.run_one(bars, factory, &cfg, params)?);
                Ok(DelayLevel { delay_bars: extra, sharpe: s.sharpe, total_return: s.total_return })
            };
            for &extra in &grid {
                d.levels.push(level_at(extra)?);
            }
            d.one_bar_sharpe = level_at(1)?.sharpe;
            d.verdict = degradation_verdict(base_sum.sharpe, d.one_bar_sharpe, sc.degradation_warn_ratio);
            d.finding = format!(
                "Sharpe {} on time, {} when every fill is one bar late.",
                f2(base_sum.sharpe),
                f2(d.one_bar_sharpe)
            );
        } else {
            d.finding = NO_TRADES.into();
        }
        if execution.test_number < 2 || execution.step == 0 {
            execution.complete_test(2);
        }
    }

    // ---- 3. Parameter stability ----------------------------------------------
    {
        let p = &mut rep.stability;
        p.baseline_sharpe = base_sum.sharpe;
        if traded {
            let mut candidates = Vec::new();
            let mut seen: BTreeSet<String> = BTreeSet::from([params.canonical()]);
            for k in 0..params.values.len() {
                for &frac in &sc.perturbation_fractions {
                    let mut n = params.clone();
                    let mut v = params.values[k].1 * (1.0 + frac);
                    if sc.integer_params.contains(&params.values[k].0) {
                        v = v.round();
                    }
                    n.values[k].1 = v;
                    if seen.insert(n.canonical()) {
                        candidates.push(n);
                    }
                }
            }
            let mut sharpes = Vec::new();
            execution.begin_test(3, candidates.len());
            for n in candidates {
                match execution.run_one(bars, factory, config, &n) {
                    Ok(r) => {
                        let s = summarize(&r);
                        sharpes.push(s.sharpe);
                        p.neighbours.push(Neighbour {
                            params: n,
                            sharpe: s.sharpe,
                            total_return: s.total_return,
                        });
                    }
                    Err(e) if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) => return Err(e),
                    Err(_) => p.skipped += 1, // the strategy rejected these parameters (e.g. fast >= slow)
                }
            }
            if sharpes.len() < 4 {
                p.finding = format!(
                    "Only {} valid neighbouring parameter sets could be run ({} rejected by the strategy); too few \
                     to judge stability.",
                    sharpes.len(),
                    p.skipped
                );
            } else {
                let positive = sharpes.iter().filter(|&&s| s > 0.0).count();
                p.share_positive = positive as f64 / sharpes.len() as f64;
                p.median_sharpe = median(sharpes.clone());
                p.min_sharpe = sharpes.iter().copied().fold(f64::INFINITY, f64::min);
                p.verdict = if p.share_positive < sc.perturbation_positive_fail {
                    Verdict::Fail
                } else if p.share_positive < sc.perturbation_positive_warn
                    || p.median_sharpe < sc.degradation_warn_ratio * base_sum.sharpe
                {
                    Verdict::Warn
                } else {
                    Verdict::Pass
                };
                p.finding = format!(
                    "{} neighbours: {}% have positive Sharpe; median {} (baseline {}), worst {}.",
                    sharpes.len(),
                    fmt_f(p.share_positive * 100.0, 0),
                    f2(p.median_sharpe),
                    f2(base_sum.sharpe),
                    f2(p.min_sharpe)
                );
            }
        } else {
            p.finding = NO_TRADES.into();
        }
        if execution.test_number < 3 || execution.step == 0 {
            execution.complete_test(3);
        }
    }

    // ---- 4. Time-period robustness -------------------------------------------
    {
        let t = &mut rep.time;
        let offsets = timestamp_offsets(bars);
        let n_ts = offsets.len() - 1;
        let k = sc.n_subperiods.max(2);
        if !traded {
            t.finding = NO_TRADES.into();
        } else if n_ts < k * sc.min_timestamps_per_subperiod {
            t.finding = format!(
                "{n_ts} timestamps cannot be split into {k} sub-periods of at least {}.",
                sc.min_timestamps_per_subperiod
            );
        } else {
            execution.begin_test(4, k + 2);
            let mut run_range = |from: usize, to: usize| -> Result<(Summary, i64, i64)> {
                let slice = &bars[offsets[from]..offsets[to]];
                let r = execution.run_one(slice, factory, config, params)?;
                Ok((summarize(&r), slice[0].ts_ns, slice[slice.len() - 1].ts_ns))
            };
            let seg = n_ts / k;
            let mut positive = 0usize;
            for i in 0..k {
                let from = i * seg;
                let to = if i + 1 == k { n_ts } else { (i + 1) * seg };
                let (s, first, last) = run_range(from, to)?;
                t.subperiods.push(Subperiod {
                    first_ts: first,
                    last_ts: last,
                    sharpe: s.sharpe,
                    total_return: s.total_return,
                    max_drawdown: s.max_drawdown,
                });
                if s.sharpe > 0.0 {
                    positive += 1;
                }
            }
            t.share_positive = positive as f64 / k as f64;
            t.first_half_sharpe = run_range(0, n_ts / 2)?.0.sharpe;
            t.second_half_sharpe = run_range(n_ts / 2, n_ts)?.0.sharpe;
            let mut v = if t.share_positive < sc.subperiod_positive_fail {
                Verdict::Fail
            } else if t.share_positive < sc.subperiod_positive_warn {
                Verdict::Warn
            } else {
                Verdict::Pass
            };
            if t.first_half_sharpe > 0.0 && t.second_half_sharpe <= 0.0 {
                v = worse(v, Verdict::Fail);
            } else if t.second_half_sharpe < sc.degradation_warn_ratio * t.first_half_sharpe {
                v = worse(v, Verdict::Warn);
            }
            t.verdict = v;
            t.finding = format!(
                "{}% of {k} sub-periods have positive Sharpe; first half {}, second half {}.",
                fmt_f(t.share_positive * 100.0, 0),
                f2(t.first_half_sharpe),
                f2(t.second_half_sharpe)
            );
        }
        if execution.test_number < 4 || execution.step == 0 {
            execution.complete_test(4);
        }
    }

    // ---- 5-6. Trades: sample adequacy and resampling -------------------------
    {
        let tb = &mut rep.trades;
        let sa = &mut rep.sample;
        tb.stats = compute_trade_stats(&extraction.trades);
        tb.open_positions_at_end = extraction.open_positions;
        sa.trades = tb.stats.n;
        sa.top_trade_share = tb.stats.top_trade_share;
        if sa.trades == 0 {
            sa.finding = NO_TRADES.into();
            tb.finding = NO_TRADES.into();
        } else {
            let mut v = if sa.trades < sc.trades_insufficient_below {
                Verdict::Insufficient
            } else if sa.trades < sc.trades_warn_below {
                Verdict::Warn
            } else {
                Verdict::Pass
            };
            // Concentration is only meaningful once there are enough trades.
            if sa.trades >= sc.trades_insufficient_below {
                if sa.top_trade_share > sc.top_trade_fail {
                    v = worse(v, Verdict::Fail);
                } else if sa.top_trade_share > sc.top_trade_warn {
                    v = worse(v, Verdict::Warn);
                }
            }
            sa.verdict = v;
            sa.finding = format!(
                "{} closed trades ({} needed to conclude anything, {} for a clean pass); the best trade is {}% of \
                 all winning P&L.",
                sa.trades,
                sc.trades_insufficient_below,
                sc.trades_warn_below,
                fmt_f(sa.top_trade_share * 100.0, 0)
            );
            if sa.trades < sc.min_trades_for_resampling {
                tb.finding = format!(
                    "{} trades is too few to resample meaningfully (minimum {}).",
                    sa.trades, sc.min_trades_for_resampling
                );
            } else {
                let pnl: Vec<f64> = extraction.trades.iter().map(|t| t.net.to_f64()).collect();
                tb.resampling =
                    resample_trades(&pnl, config.initial_cash.to_f64(), sc.bootstrap_resamples, sc.seed);
                let r = &tb.resampling;
                let mut bv = if r.bootstrap_prob_loss > sc.bootstrap_loss_fail {
                    Verdict::Fail
                } else if r.bootstrap_prob_loss > sc.bootstrap_loss_warn {
                    Verdict::Warn
                } else {
                    Verdict::Pass
                };
                if r.reshuffle_prob_worse_drawdown > sc.lucky_ordering_warn {
                    bv = worse(bv, Verdict::Warn);
                }
                tb.verdict = bv;
                tb.finding = format!(
                    "{}% of bootstrap resamples lose money (5th-95th percentile P&L {} to {}); observed max \
                     drawdown {}% is deeper than {}% of reshuffled orderings.",
                    fmt_f(r.bootstrap_prob_loss * 100.0, 0),
                    fmt_f(r.bootstrap_terminal_pnl.p05, 0),
                    fmt_f(r.bootstrap_terminal_pnl.p95, 0),
                    fmt_f(r.observed_max_drawdown * 100.0, 1),
                    fmt_f((1.0 - r.reshuffle_prob_worse_drawdown) * 100.0, 0)
                );
            }
        }
    }
    execution.complete_test(5);
    execution.complete_test(6);

    // ---- 7. Regime sensitivity & 8. Benchmark-relative -----------------------
    {
        let rs = aligned_returns(&base.equity_curve);
        let rb = benchmark_returns(bars);
        let rg = &mut rep.regimes;
        let bm = &mut rep.benchmark;
        if !traded || rs.len() != rb.len() {
            rg.finding = if traded {
                "Strategy and benchmark return series are not aligned.".into()
            } else {
                NO_TRADES.into()
            };
            bm.finding.clone_from(&rg.finding);
        } else {
            bm.metrics = compute_benchmark_metrics(&rs, &rb, config.periods_per_year)?;
            let (mut sum, mut cnt) = (0.0, 0usize);
            for p in &base.equity_curve {
                let e = p.equity.to_f64();
                if e > 0.0 {
                    sum += (e - p.cash.to_f64()).abs() / e;
                    cnt += 1;
                }
            }
            bm.average_net_exposure = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
            if bm.metrics.valid {
                let m = &bm.metrics;
                bm.verdict = if m.alpha_annualized <= 0.0 || m.information_ratio <= 0.0 {
                    Verdict::Warn
                } else {
                    Verdict::Pass
                };
                bm.finding = format!(
                    "Beta {}, annualized alpha {}%, information ratio {}, correlation {}; average net exposure {}% \
                     of equity (the benchmark is 100% invested, so a low exposure depresses alpha).",
                    f2(m.beta),
                    f2(m.alpha_annualized * 100.0),
                    f2(m.information_ratio),
                    f2(m.correlation),
                    fmt_f(bm.average_net_exposure * 100.0, 1)
                );
            } else {
                bm.finding = "The benchmark has no variance or too few periods to compare against.".into();
            }

            // Regimes: trailing-volatility terciles of the benchmark.
            let w = sc.regime_vol_window;
            let mut vol = Vec::new();
            if w >= 2 {
                for t in w..rb.len() {
                    let window = &rb[t - w..t];
                    let mut mean = 0.0;
                    for &x in window {
                        mean += x;
                    }
                    mean /= w as f64;
                    let mut ss = 0.0;
                    for &x in window {
                        ss += (x - mean) * (x - mean);
                    }
                    vol.push((ss / (w - 1) as f64).sqrt());
                }
            }
            if w < 2 || vol.len() < sc.regime_min_periods {
                // (With a window below 2 the C++ engine still counted one label per period.)
                let labelled = if w < 2 { rb.len().saturating_sub(w) } else { vol.len() };
                rg.finding = format!(
                    "{labelled} labelled periods is fewer than the {} needed to compare regimes.",
                    sc.regime_min_periods
                );
            } else {
                let mut sorted = vol.clone();
                sorted.sort_by(f64::total_cmp);
                let t1 = sorted[nearest_rank_index(sorted.len(), 1.0 / 3.0)];
                let t2 = sorted[nearest_rank_index(sorted.len(), 2.0 / 3.0)];
                let mut by: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
                for (i, &v) in vol.iter().enumerate() {
                    let bucket = if v <= t1 {
                        0
                    } else if v <= t2 {
                        1
                    } else {
                        2
                    };
                    by[bucket].push(rs[w + i]);
                }
                const NAMES: [&str; 3] = ["low volatility", "medium volatility", "high volatility"];
                let mut negative = 0usize;
                let mut too_small = false;
                for (b, values) in by.iter().enumerate() {
                    let mut out = RegimeBucket {
                        name: NAMES[b].into(),
                        periods: values.len(),
                        share: values.len() as f64 / vol.len() as f64,
                        ..RegimeBucket::default()
                    };
                    if !values.is_empty() {
                        let mut mean = 0.0;
                        for &x in values {
                            mean += x;
                        }
                        mean /= values.len() as f64;
                        out.mean_return = mean;
                        if values.len() >= sc.regime_min_per_bucket {
                            let mo = moments(values);
                            out.sharpe = if mo.stdev > 0.0 {
                                mean / mo.stdev * config.periods_per_year.sqrt()
                            } else {
                                0.0
                            };
                        }
                    }
                    if out.periods < sc.regime_min_per_bucket {
                        too_small = true;
                    }
                    if out.mean_return < 0.0 {
                        negative += 1;
                    }
                    rg.buckets.push(out);
                }
                if too_small {
                    rg.finding = "At least one volatility regime has too few periods to evaluate.".into();
                } else {
                    rg.verdict = match negative {
                        0 => Verdict::Pass,
                        1 => Verdict::Warn,
                        _ => Verdict::Fail,
                    };
                    rg.finding = format!(
                        "Mean return per period: {}% low, {}% medium, {}% high volatility; {negative} of 3 regimes \
                         lose money.",
                        fmt_f(rg.buckets[0].mean_return * 100.0, 4),
                        fmt_f(rg.buckets[1].mean_return * 100.0, 4),
                        fmt_f(rg.buckets[2].mean_return * 100.0, 4)
                    );
                }
            }
        }
    }
    execution.complete_test(7);
    execution.complete_test(8);

    // ---- 9. Drawdown & tail risk -----------------------------------------------
    {
        let rk = &mut rep.risk;
        let eq: Vec<f64> = base.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
        rk.drawdown = compute_drawdown_stats(&eq);
        rk.tail = compute_tail_stats(&aligned_returns(&base.equity_curve), sc.tail_level);
        if rk.drawdown.valid {
            rk.verdict = if rk.drawdown.max_drawdown >= sc.max_drawdown_fail {
                Verdict::Fail
            } else if rk.drawdown.max_drawdown >= sc.max_drawdown_warn {
                Verdict::Warn
            } else {
                Verdict::Pass
            };
            rk.finding = format!(
                "Max drawdown {}%, longest time under water {} periods",
                fmt_f(rk.drawdown.max_drawdown * 100.0, 1),
                rk.drawdown.longest_underwater
            );
            rk.finding += &if rk.tail.valid {
                format!(
                    "; historical {}% VaR {}% and ES {}% per period.",
                    fmt_f(sc.tail_level * 100.0, 0),
                    f2(rk.tail.var * 100.0),
                    f2(rk.tail.es * 100.0)
                )
            } else {
                "; too few returns for tail estimates.".into()
            };
        } else {
            rk.finding = "Too few equity points to measure drawdown.".into();
        }
    }
    execution.complete_test(9);

    rep.worst = [
        rep.costs.verdict,
        rep.delay.verdict,
        rep.stability.verdict,
        rep.time.verdict,
        rep.trades.verdict,
        rep.sample.verdict,
        rep.regimes.verdict,
        rep.benchmark.verdict,
        rep.risk.verdict,
    ]
    .into_iter()
    .fold(Verdict::Pass, worse);
    Ok(rep)
}

/// Plain-text report (identical to the C++ reference's).
pub fn format_survival_report(r: &SurvivalReport) -> String {
    let mut out = String::from("Strategy Survival Report\n========================\n");
    out += &format!("parameters: {}\n", r.params.canonical());
    out += &format!(
        "baseline: Sharpe {}, total return {}%, max drawdown {}%, {} fills\n",
        f2(r.baseline.sharpe),
        f2(r.baseline.total_return * 100.0),
        f2(r.baseline.max_drawdown * 100.0),
        r.baseline_fills
    );
    out += &format!("baseline run: {}\n\n", r.baseline_run.result_sha256);
    let section = |out: &mut String, title: &str, v: Verdict, finding: &str| {
        *out += &format!("[{}] {title}\n    {finding}\n", pad_right(v.as_str(), 12));
    };
    section(&mut out, "Cost sensitivity", r.costs.verdict, &r.costs.finding);
    for l in &r.costs.levels {
        out += &format!(
            "      +{} bps   Sharpe {}   return {}%\n",
            pad_left(&fmt_f(l.extra_bps, 1), 6),
            pad_left(&f2(l.sharpe), 6),
            pad_left(&f2(l.total_return * 100.0), 7)
        );
    }
    section(&mut out, "Execution delay", r.delay.verdict, &r.delay.finding);
    for l in &r.delay.levels {
        out += &format!(
            "      +{} bar(s)     Sharpe {}   return {}%\n",
            l.delay_bars,
            pad_left(&f2(l.sharpe), 6),
            pad_left(&f2(l.total_return * 100.0), 7)
        );
    }
    section(&mut out, "Parameter stability", r.stability.verdict, &r.stability.finding);
    section(&mut out, "Time-period robustness", r.time.verdict, &r.time.finding);
    for s in &r.time.subperiods {
        out += &format!(
            "      {} .. {}   Sharpe {}   return {}%   max dd {}%\n",
            if s.first_ts == 0 { "start" } else { "-" },
            if s.last_ts == 0 { "end" } else { "-" },
            pad_left(&f2(s.sharpe), 6),
            pad_left(&f2(s.total_return * 100.0), 7),
            pad_left(&fmt_f(s.max_drawdown * 100.0, 1), 5)
        );
    }
    section(&mut out, "Trade bootstrap", r.trades.verdict, &r.trades.finding);
    if r.trades.stats.n > 0 {
        let s = &r.trades.stats;
        out += &format!(
            "      win rate {}%, payoff {}, profit factor {}, expectancy {} per trade, {} position(s) still open\n",
            fmt_f(s.win_rate * 100.0, 0),
            f2(s.payoff_ratio),
            f2(s.profit_factor),
            f2(s.expectancy),
            r.trades.open_positions_at_end
        );
    }
    section(&mut out, "Sample size & concentration", r.sample.verdict, &r.sample.finding);
    section(&mut out, "Regime sensitivity", r.regimes.verdict, &r.regimes.finding);
    section(&mut out, "Benchmark-relative", r.benchmark.verdict, &r.benchmark.finding);
    section(&mut out, "Drawdown & tail risk", r.risk.verdict, &r.risk.finding);
    out += &format!(
        "\nMost severe component verdict: {}  (this is NOT a score; read each component above)\n",
        r.worst.as_str()
    );
    out += "\nAssumptions\n";
    for a in &r.assumptions {
        out += &format!("  - {a}\n");
    }
    out
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn config_json_round_trips_and_presets_validate() {
        for config in [
            SurvivalConfig::default_preset(),
            SurvivalConfig::strict_preset(),
            SurvivalConfig::lenient_preset(),
        ] {
            config.validate().unwrap();
            let encoded = config.to_json().unwrap();
            assert_eq!(SurvivalConfig::from_json(&encoded).unwrap(), config);
        }
    }

    #[test]
    fn config_rejects_bad_threshold_order_and_empty_grid() {
        let config = SurvivalConfig { perturbation_positive_fail: 0.9, ..SurvivalConfig::default() };
        assert!(config.validate().is_err());
        let config = SurvivalConfig { delay_bars: Vec::new(), ..SurvivalConfig::default() };
        assert!(config.validate().is_err());
    }
}
