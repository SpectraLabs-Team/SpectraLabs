//! Additional performance and risk measures on a return series.
//!
//! These replace the ratio, distribution and drawdown "metrics" of the C++
//! risk project (`src/risk/metrics/*.cpp`), which were evaluated on Monte
//! Carlo terminal losses as if they were a time series, and used hard-coded
//! placeholders (a 2% risk-free rate per *period*, a 5% benchmark, beta 0.7).
//! Here every measure takes an actual per-period return series (for example a
//! backtest's equity-curve returns), states its formula, and returns `None`
//! when it is undefined instead of inventing a number.
//!
//! Notation: r = per-period simple returns, n = their count, ppy = periods per
//! year, rf = annual risk-free rate, mar = minimum acceptable return per period.

/// Mean of a slice (0 for an empty slice).
fn mean(x: &[f64]) -> f64 {
    if x.is_empty() {
        0.0
    } else {
        x.iter().sum::<f64>() / x.len() as f64
    }
}

/// Sample standard deviation (n - 1); `None` with fewer than 2 points.
pub fn stdev(x: &[f64]) -> Option<f64> {
    if x.len() < 2 {
        return None;
    }
    let m = mean(x);
    Some((x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (x.len() - 1) as f64).sqrt())
}

/// Compound annual growth rate of the series: `prod(1 + r)^(ppy / n) - 1`.
pub fn cagr(r: &[f64], ppy: f64) -> Option<f64> {
    if r.is_empty() || ppy <= 0.0 {
        return None;
    }
    let growth: f64 = r.iter().map(|x| 1.0 + x).product();
    (growth > 0.0).then(|| growth.powf(ppy / r.len() as f64) - 1.0)
}

/// Downside deviation below `mar`: `sqrt(sum(min(r - mar, 0)^2) / n)`.
pub fn downside_deviation(r: &[f64], mar: f64) -> Option<f64> {
    if r.is_empty() {
        return None;
    }
    let ss: f64 = r.iter().map(|x| (x - mar).min(0.0).powi(2)).sum();
    Some((ss / r.len() as f64).sqrt())
}

/// Semi-deviation: root mean square deviation of the observations below the
/// mean, `sqrt(mean((r_i - mean)^2 for r_i < mean))`. 0 if none is below.
pub fn semi_deviation(r: &[f64]) -> Option<f64> {
    if r.is_empty() {
        return None;
    }
    let m = mean(r);
    let below: Vec<f64> = r.iter().filter(|&&x| x < m).map(|x| (x - m).powi(2)).collect();
    Some(if below.is_empty() { 0.0 } else { mean(&below).sqrt() })
}

/// Skewness with population moments: `m3 / m2^1.5`.
pub fn skewness(r: &[f64]) -> Option<f64> {
    if r.len() < 3 {
        return None;
    }
    let m = mean(r);
    let m2 = mean(&r.iter().map(|x| (x - m).powi(2)).collect::<Vec<_>>());
    let m3 = mean(&r.iter().map(|x| (x - m).powi(3)).collect::<Vec<_>>());
    (m2 > 0.0).then(|| m3 / m2.powf(1.5))
}

/// Excess kurtosis with population moments: `m4 / m2^2 - 3` (0 for a normal).
pub fn excess_kurtosis(r: &[f64]) -> Option<f64> {
    if r.len() < 4 {
        return None;
    }
    let m = mean(r);
    let m2 = mean(&r.iter().map(|x| (x - m).powi(2)).collect::<Vec<_>>());
    let m4 = mean(&r.iter().map(|x| (x - m).powi(4)).collect::<Vec<_>>());
    (m2 > 0.0).then(|| m4 / (m2 * m2) - 3.0)
}

/// Annualized Sortino ratio: `(mean(r) - rf/ppy) / downside_deviation(r, rf/ppy) * sqrt(ppy)`.
/// `None` when the downside deviation is zero (no returns below the target).
pub fn sortino(r: &[f64], ppy: f64, rf: f64) -> Option<f64> {
    if ppy <= 0.0 {
        return None;
    }
    let target = rf / ppy;
    let dd = downside_deviation(r, target)?;
    (dd > 0.0).then(|| (mean(r) - target) / dd * ppy.sqrt())
}

/// Omega ratio at `threshold`: `sum(max(r - t, 0)) / sum(max(t - r, 0))`.
/// `None` when nothing falls below the threshold.
pub fn omega(r: &[f64], threshold: f64) -> Option<f64> {
    let gains: f64 = r.iter().map(|x| (x - threshold).max(0.0)).sum();
    let losses: f64 = r.iter().map(|x| (threshold - x).max(0.0)).sum();
    (losses > 0.0).then(|| gains / losses)
}

/// Drawdown episodes of the compounded return series: the maximum depth
/// (positive fraction) of each separate stretch below the running peak.
pub fn drawdown_episodes(r: &[f64]) -> Vec<f64> {
    let mut eq = 1.0f64;
    let mut peak = 1.0f64;
    let mut depth = 0.0f64;
    let mut out = Vec::new();
    for x in r {
        eq *= 1.0 + x;
        if eq >= peak {
            if depth > 0.0 {
                out.push(depth);
            }
            peak = eq;
            depth = 0.0;
        } else if peak > 0.0 {
            depth = depth.max((peak - eq) / peak);
        }
    }
    if depth > 0.0 {
        out.push(depth);
    }
    out
}

/// Calmar ratio: `CAGR / max drawdown`. `None` without a drawdown.
pub fn calmar(r: &[f64], ppy: f64) -> Option<f64> {
    let max_dd = drawdown_episodes(r).into_iter().fold(0.0, f64::max);
    let g = cagr(r, ppy)?;
    (max_dd > 0.0).then(|| g / max_dd)
}

/// Sterling ratio (episode form): `CAGR / mean episode depth`.
pub fn sterling(r: &[f64], ppy: f64) -> Option<f64> {
    let eps = drawdown_episodes(r);
    let g = cagr(r, ppy)?;
    (!eps.is_empty()).then(|| g / mean(&eps))
}

/// Burke ratio: `CAGR / sqrt(sum(episode depth^2))`.
pub fn burke(r: &[f64], ppy: f64) -> Option<f64> {
    let eps = drawdown_episodes(r);
    let g = cagr(r, ppy)?;
    let root = eps.iter().map(|d| d * d).sum::<f64>().sqrt();
    (root > 0.0).then(|| g / root)
}

/// Rolling annualized Sharpe ratio over `window` periods (entry k covers
/// returns `k..k+window`). Windows with zero volatility give 0.
pub fn rolling_sharpe(r: &[f64], window: usize, ppy: f64) -> Vec<f64> {
    if window < 2 || r.len() < window {
        return Vec::new();
    }
    r.windows(window)
        .map(|w| match stdev(w) {
            Some(sd) if sd > 0.0 => mean(w) / sd * ppy.sqrt(),
            _ => 0.0,
        })
        .collect()
}

/// Rolling annualized volatility over `window` periods.
pub fn rolling_volatility(r: &[f64], window: usize, ppy: f64) -> Vec<f64> {
    if window < 2 || r.len() < window {
        return Vec::new();
    }
    r.windows(window).map(|w| stdev(w).unwrap_or(0.0) * ppy.sqrt()).collect()
}

/// Every measure above for one return series.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerformanceSummary {
    /// Compound annual growth rate.
    pub cagr: Option<f64>,
    /// Annualized Sortino ratio.
    pub sortino: Option<f64>,
    /// Calmar ratio.
    pub calmar: Option<f64>,
    /// Sterling ratio.
    pub sterling: Option<f64>,
    /// Burke ratio.
    pub burke: Option<f64>,
    /// Omega ratio at a zero threshold.
    pub omega: Option<f64>,
    /// Downside deviation below zero, annualized.
    pub downside_deviation: Option<f64>,
    /// Semi-deviation, annualized.
    pub semi_deviation: Option<f64>,
    /// Skewness.
    pub skewness: Option<f64>,
    /// Excess kurtosis.
    pub excess_kurtosis: Option<f64>,
    /// Number of drawdown episodes.
    pub drawdown_episodes: usize,
    /// Mean depth of the drawdown episodes.
    pub average_drawdown: Option<f64>,
}

/// Computes [`PerformanceSummary`] for per-period returns `r`.
pub fn summarize(r: &[f64], ppy: f64, rf: f64) -> PerformanceSummary {
    let eps = drawdown_episodes(r);
    PerformanceSummary {
        cagr: cagr(r, ppy),
        sortino: sortino(r, ppy, rf),
        calmar: calmar(r, ppy),
        sterling: sterling(r, ppy),
        burke: burke(r, ppy),
        omega: omega(r, 0.0),
        downside_deviation: downside_deviation(r, 0.0).map(|d| d * ppy.max(0.0).sqrt()),
        semi_deviation: semi_deviation(r).map(|d| d * ppy.max(0.0).sqrt()),
        skewness: skewness(r),
        excess_kurtosis: excess_kurtosis(r),
        drawdown_episodes: eps.len(),
        average_drawdown: (!eps.is_empty()).then(|| mean(&eps)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }

    #[test]
    fn hand_computed_values() {
        let r = [0.10, -0.05, 0.02, -0.10, 0.08];
        // growth = 1.1 * 0.95 * 1.02 * 0.9 * 1.08
        let growth = 1.1 * 0.95 * 1.02 * 0.9 * 1.08;
        near(cagr(&r, 5.0).unwrap(), growth - 1.0);
        near(downside_deviation(&r, 0.0).unwrap(), ((0.05f64.powi(2) + 0.10f64.powi(2)) / 5.0).sqrt());
        near(omega(&r, 0.0).unwrap(), 0.20 / 0.15);
        // Episodes: peak 1.1 -> 1.045 (5%), recovers to 1.0659? no: 1.045*1.02=1.0659 < 1.1, then *0.9
        let eps = drawdown_episodes(&r);
        assert_eq!(eps.len(), 1);
        let trough = 1.1 * 0.95 * 1.02 * 0.9;
        near(eps[0], (1.1 - trough) / 1.1);
        near(calmar(&r, 5.0).unwrap(), (growth - 1.0) / eps[0]);
    }

    #[test]
    fn undefined_measures_are_none() {
        let up = [0.01, 0.02, 0.03];
        assert_eq!(sortino(&up, 252.0, 0.0), None);
        assert_eq!(omega(&up, 0.0), None);
        assert_eq!(calmar(&up, 252.0), None);
        assert_eq!(skewness(&[0.5, 0.5, 0.5]), None);
        assert_eq!(excess_kurtosis(&[0.1]), None);
        assert_eq!(cagr(&[], 252.0), None);
        assert!(rolling_sharpe(&up, 5, 252.0).is_empty());
    }

    #[test]
    fn moments_of_a_symmetric_sample() {
        let r = [-2.0, -1.0, 0.0, 1.0, 2.0];
        near(skewness(&r).unwrap(), 0.0);
        // m2 = 2, m4 = (16+1+0+1+16)/5 = 6.8 -> 6.8/4 - 3 = -1.3
        near(excess_kurtosis(&r).unwrap(), -1.3);
        near(semi_deviation(&r).unwrap(), 2.5f64.sqrt());
        assert_eq!(rolling_volatility(&r, 2, 1.0).len(), 4);
    }
}
