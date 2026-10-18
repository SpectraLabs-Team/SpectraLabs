//! Risk metrics computed from a simulated loss distribution.
//!
//! The Monte Carlo engine produces one terminal loss per path: independent
//! draws of "how much could the portfolio lose over the horizon". Metrics
//! that describe that distribution are computed here exactly as the C++
//! engine computed them (same formulas, same summation order, so the same
//! bits):
//!
//! | metric | definition |
//! |---|---|
//! | `VaR` | the `confidence` quantile of losses, linearly interpolated between order statistics |
//! | `CVaR` | mean of losses at or beyond VaR (expected shortfall) |
//! | `Volatility` | sample standard deviation of losses |
//! | `DownsideDeviation`, `SemiDeviation` | RMS of the losses above their mean (both names, one formula, as in C++) |
//! | `Skewness` | third standardized moment (divides by n, uses the sample stdev) |
//! | `Kurtosis` | fourth standardized moment minus 3 |
//! | `HHI` | Herfindahl index of the portfolio's gross weights |
//!
//! The C++ engine also accepted names it could not honestly compute from
//! this model and returned made-up numbers for them: drawdown and
//! risk-adjusted ratios (Sharpe, Sortino, Calmar, ...) computed over the
//! *order of simulation paths*, which has no time meaning, with hard-coded
//! risk-free and benchmark rates; and Beta, Marginal/Component/Incremental
//! VaR, Tracking Error and Diversification Ratio returned VaR, 0.7 or 1.0.
//! Here those names are recognized but return [`Error::Unsupported`] with
//! an explanation. Time-series versions of the ratios live in
//! `spectra_engine::performance`, computed on a backtest's returns.

use spectra_core::{Error, Result};

use crate::portfolio::Portfolio;

/// Every metric name the engine recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Metric {
    VaR,
    CVaR,
    Volatility,
    DownsideDeviation,
    SemiDeviation,
    Skewness,
    Kurtosis,
    Hhi,
    // Recognized, not computable from an aggregate loss distribution:
    MaxDrawdown,
    AverageDrawdown,
    DrawdownDuration,
    UlcerIndex,
    SharpeRatio,
    SortinoRatio,
    CalmarRatio,
    InformationRatio,
    OmegaRatio,
    TreynorRatio,
    SterlingRatio,
    BurkeRatio,
    MarginalVaR,
    ComponentVaR,
    IncrementalVaR,
    Beta,
    TrackingError,
    DiversificationRatio,
}

const ALL: [(Metric, &str); 26] = [
    (Metric::VaR, "VaR"),
    (Metric::CVaR, "CVaR"),
    (Metric::Volatility, "Volatility"),
    (Metric::DownsideDeviation, "DownsideDeviation"),
    (Metric::SemiDeviation, "SemiDeviation"),
    (Metric::Skewness, "Skewness"),
    (Metric::Kurtosis, "Kurtosis"),
    (Metric::Hhi, "HHI"),
    (Metric::MaxDrawdown, "MaxDrawdown"),
    (Metric::AverageDrawdown, "AverageDrawdown"),
    (Metric::DrawdownDuration, "DrawdownDuration"),
    (Metric::UlcerIndex, "UlcerIndex"),
    (Metric::SharpeRatio, "SharpeRatio"),
    (Metric::SortinoRatio, "SortinoRatio"),
    (Metric::CalmarRatio, "CalmarRatio"),
    (Metric::InformationRatio, "InformationRatio"),
    (Metric::OmegaRatio, "OmegaRatio"),
    (Metric::TreynorRatio, "TreynorRatio"),
    (Metric::SterlingRatio, "SterlingRatio"),
    (Metric::BurkeRatio, "BurkeRatio"),
    (Metric::MarginalVaR, "MarginalVaR"),
    (Metric::ComponentVaR, "ComponentVaR"),
    (Metric::IncrementalVaR, "IncrementalVaR"),
    (Metric::Beta, "Beta"),
    (Metric::TrackingError, "TrackingError"),
    (Metric::DiversificationRatio, "DiversificationRatio"),
];

impl Metric {
    /// Canonical name (as reported in [`crate::RiskReport`]).
    pub fn name(self) -> &'static str {
        ALL.iter().find(|(m, _)| *m == self).map_or("?", |(_, n)| n)
    }

    /// Parses a name case-insensitively, ignoring `_` and `-` ("var",
    /// "CVaR", "max_drawdown").
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for an unknown name.
    pub fn parse(name: &str) -> Result<Self> {
        let key: String =
            name.chars().filter(|c| *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase();
        ALL.iter()
            .find(|(_, n)| n.to_ascii_lowercase() == key)
            .map(|(m, _)| *m)
            .ok_or_else(|| Error::InvalidArgument(format!("Unknown metric: {name}")))
    }

    /// Every metric.
    pub fn all() -> impl Iterator<Item = Self> {
        ALL.iter().map(|(m, _)| *m)
    }

    /// Whether [`compute`] returns a value for this metric.
    pub fn is_supported(self) -> bool {
        matches!(
            self,
            Self::VaR
                | Self::CVaR
                | Self::Volatility
                | Self::DownsideDeviation
                | Self::SemiDeviation
                | Self::Skewness
                | Self::Kurtosis
                | Self::Hhi
        )
    }
}

fn empty(name: &str) -> Error {
    Error::InvalidArgument(format!("compute({name}): empty loss vector."))
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().fold(0.0, |a, x| a + x) / v.len() as f64
}

fn std_dev(v: &[f64], m: f64) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let ss = v.iter().fold(0.0, |a, x| {
        let d = x - m;
        a + d * d
    });
    (ss / (v.len() - 1) as f64).sqrt()
}

/// Value at Risk: the `confidence` quantile of `losses`.
///
/// # Errors
/// [`Error::InvalidArgument`] for an empty vector.
pub fn value_at_risk(losses: &[f64], confidence: f64) -> Result<f64> {
    if losses.is_empty() {
        return Err(empty("VaR"));
    }
    let mut sorted = losses.to_vec();
    sorted.sort_by(f64::total_cmp);
    Ok(quantile_sorted(&sorted, confidence))
}

/// The interpolated quantile of an already sorted vector (non-empty).
pub fn quantile_sorted(sorted: &[f64], confidence: f64) -> f64 {
    let rank = confidence * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = (rank.ceil() as usize).min(sorted.len() - 1);
    let frac = rank - lower as f64;
    sorted[lower] + frac * (sorted[upper] - sorted[lower])
}

/// Conditional VaR (expected shortfall): mean of losses `>=` VaR.
///
/// # Errors
/// [`Error::InvalidArgument`] for an empty vector.
pub fn conditional_var(losses: &[f64], confidence: f64) -> Result<f64> {
    if losses.is_empty() {
        return Err(empty("CVaR"));
    }
    let threshold = value_at_risk(losses, confidence)?;
    let (mut sum, mut count) = (0.0, 0usize);
    for &l in losses {
        if l >= threshold {
            sum += l;
            count += 1;
        }
    }
    Ok(if count == 0 { threshold } else { sum / count as f64 })
}

/// Sample standard deviation of losses.
///
/// # Errors
/// [`Error::InvalidArgument`] for an empty vector.
pub fn volatility(losses: &[f64]) -> Result<f64> {
    if losses.is_empty() {
        return Err(empty("Volatility"));
    }
    Ok(std_dev(losses, mean(losses)))
}

/// RMS of the losses above their mean.
///
/// # Errors
/// [`Error::InvalidArgument`] for an empty vector.
pub fn downside_deviation(losses: &[f64]) -> Result<f64> {
    if losses.is_empty() {
        return Err(empty("DownsideDeviation"));
    }
    let m = mean(losses);
    let (mut ss, mut n) = (0.0, 0usize);
    for &l in losses {
        if l > m {
            let d = l - m;
            ss += d * d;
            n += 1;
        }
    }
    Ok(if n == 0 { 0.0 } else { (ss / n as f64).sqrt() })
}

/// Skewness of losses.
///
/// # Errors
/// [`Error::InvalidArgument`] with fewer than 3 samples.
pub fn skewness(losses: &[f64]) -> Result<f64> {
    if losses.len() < 3 {
        return Err(Error::InvalidArgument("compute(Skewness): need at least 3 samples.".into()));
    }
    let m = mean(losses);
    let s = std_dev(losses, m);
    if s == 0.0 {
        return Ok(0.0);
    }
    let sum = losses.iter().fold(0.0, |a, l| {
        let d = (l - m) / s;
        a + d * d * d
    });
    Ok(sum / losses.len() as f64)
}

/// Excess kurtosis of losses.
///
/// # Errors
/// [`Error::InvalidArgument`] with fewer than 4 samples.
pub fn excess_kurtosis(losses: &[f64]) -> Result<f64> {
    if losses.len() < 4 {
        return Err(Error::InvalidArgument("compute(Kurtosis): need at least 4 samples.".into()));
    }
    let m = mean(losses);
    let s = std_dev(losses, m);
    if s == 0.0 {
        return Ok(0.0);
    }
    let sum = losses.iter().fold(0.0, |a, l| {
        let d = (l - m) / s;
        a + d * d * d * d
    });
    Ok(sum / losses.len() as f64 - 3.0)
}

fn unsupported(metric: Metric) -> Error {
    let why = match metric {
        Metric::MaxDrawdown
        | Metric::AverageDrawdown
        | Metric::DrawdownDuration
        | Metric::UlcerIndex
        | Metric::SharpeRatio
        | Metric::SortinoRatio
        | Metric::CalmarRatio
        | Metric::InformationRatio
        | Metric::OmegaRatio
        | Metric::TreynorRatio
        | Metric::SterlingRatio
        | Metric::BurkeRatio => {
            "is a time-series metric, but Monte Carlo terminal losses are independent draws with no time \
             order, so any value computed from them would be meaningless. Compute it from a return series \
             instead (spectra_engine::performance on a backtest's equity curve)."
        }
        _ => {
            "needs per-asset simulated paths or benchmark returns, which the aggregate single-factor model \
             does not produce. It is not computed rather than approximated by a placeholder."
        }
    };
    Error::Unsupported(format!("{} {why}", metric.name()))
}

/// Computes `metric` from simulated `losses` (and, for HHI, the portfolio).
///
/// # Errors
/// [`Error::InvalidArgument`] for too few losses and [`Error::Unsupported`]
/// for metrics this model cannot compute (see the module documentation).
pub fn compute(metric: Metric, losses: &[f64], confidence: f64, portfolio: &Portfolio) -> Result<f64> {
    match metric {
        Metric::VaR => value_at_risk(losses, confidence),
        Metric::CVaR => conditional_var(losses, confidence),
        Metric::Volatility => volatility(losses),
        Metric::DownsideDeviation | Metric::SemiDeviation => {
            if losses.is_empty() {
                return Err(empty(metric.name()));
            }
            downside_deviation(losses)
        }
        Metric::Skewness => skewness(losses),
        Metric::Kurtosis => excess_kurtosis(losses),
        Metric::Hhi => portfolio.hhi(),
        other => Err(unsupported(other)),
    }
}
