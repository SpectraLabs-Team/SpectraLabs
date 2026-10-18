//! Performance metrics computed from an equity curve.
//!
//! Metrics are analytics, not accounting, so they use `f64`.
//!
//! Definitions (n = number of return periods, ppy = periods per year):
//! ```text
//!   period return      r_t  = E_t / E_{t-1} - 1                 (simple returns)
//!   total return            = E_n / E_0 - 1
//!   annualized return       = (E_n / E_0)^(ppy / n) - 1          (geometric)
//!   annualized volatility   = stdev(r) * sqrt(ppy)               (sample stdev, n-1)
//!   Sharpe ratio            = mean(r - rf/ppy) / stdev(r) * sqrt(ppy)
//!   max drawdown            = max_t (peak_t - E_t) / peak_t      (positive fraction)
//! ```
//! Sharpe and volatility are 0 with fewer than 2 returns or zero standard
//! deviation. Periods with non-positive prior equity are skipped when forming
//! returns. Ported operation-for-operation from the C++ reference so results
//! are bit-identical.

use spectra_core::Money;

/// One point of the equity curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EquityPoint {
    /// Timestamp.
    pub ts_ns: i64,
    /// Cash plus marked positions.
    pub equity: Money,
    /// Cash.
    pub cash: Money,
}

/// Headline metrics of a run.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Metrics {
    /// False when the curve has fewer than 2 points.
    pub valid: bool,
    /// Number of return periods.
    pub periods: i32,
    /// Total return.
    pub total_return: f64,
    /// Geometric annualized return.
    pub annualized_return: f64,
    /// Annualized volatility.
    pub annualized_volatility: f64,
    /// Annualized Sharpe ratio.
    pub sharpe: f64,
    /// Maximum drawdown (positive fraction).
    pub max_drawdown: f64,
}

/// Simple period returns of an equity series (periods whose prior equity is
/// not positive are skipped).
pub fn returns_from_equity(equity: &[f64]) -> Vec<f64> {
    if equity.len() < 2 {
        return Vec::new();
    }
    equity.windows(2).filter(|w| w[0] > 0.0).map(|w| w[1] / w[0] - 1.0).collect()
}

/// Simple period returns of an equity curve.
pub fn returns_from_curve(curve: &[EquityPoint]) -> Vec<f64> {
    let eq: Vec<f64> = curve.iter().map(|p| p.equity.to_f64()).collect();
    returns_from_equity(&eq)
}

/// Metrics of a plain equity series.
pub fn compute_metrics_from_equity(eq: &[f64], periods_per_year: f64, risk_free_rate: f64) -> Metrics {
    let mut m = Metrics::default();
    if eq.len() < 2 || periods_per_year <= 0.0 {
        return m;
    }
    m.valid = true;

    let returns = returns_from_equity(eq);
    let mut peak = eq[0];
    let mut max_dd = 0.0f64;
    for &e in &eq[1..] {
        if e > peak {
            peak = e;
        }
        if peak > 0.0 {
            max_dd = max_dd.max((peak - e) / peak);
        }
    }
    m.max_drawdown = max_dd;
    m.periods = (eq.len() - 1) as i32;

    let first = eq[0];
    let last = eq[eq.len() - 1];
    if first > 0.0 {
        m.total_return = last / first - 1.0;
        let base = 1.0 + m.total_return;
        m.annualized_return =
            if base > 0.0 { base.powf(periods_per_year / f64::from(m.periods)) - 1.0 } else { -1.0 };
    }

    if returns.len() >= 2 {
        let n = returns.len() as f64;
        let mut mean = 0.0;
        for &r in &returns {
            mean += r;
        }
        mean /= n;
        let mut ss = 0.0;
        for &r in &returns {
            ss += (r - mean) * (r - mean);
        }
        let sd = (ss / (n - 1.0)).sqrt();
        m.annualized_volatility = sd * periods_per_year.sqrt();
        if sd > 0.0 {
            let excess = mean - risk_free_rate / periods_per_year;
            m.sharpe = excess / sd * periods_per_year.sqrt();
        }
    }
    m
}

/// Metrics of an equity curve.
pub fn compute_metrics(curve: &[EquityPoint], periods_per_year: f64, risk_free_rate: f64) -> Metrics {
    let eq: Vec<f64> = curve.iter().map(|p| p.equity.to_f64()).collect();
    compute_metrics_from_equity(&eq, periods_per_year, risk_free_rate)
}

/// Drawdown profile of an equity series.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DrawdownStats {
    /// False with fewer than 2 points.
    pub valid: bool,
    /// Largest peak-to-trough fall (positive fraction).
    pub max_drawdown: f64,
    /// Fall from the running peak at the end.
    pub current_drawdown: f64,
    /// Longest run of consecutive periods below the running peak.
    pub longest_underwater: usize,
    /// Root-mean-square of drawdown fractions.
    pub ulcer_index: f64,
}

/// Drawdown statistics. Needs at least 2 equity points.
pub fn compute_drawdown_stats(equity: &[f64]) -> DrawdownStats {
    let mut d = DrawdownStats::default();
    if equity.len() < 2 {
        return d;
    }
    d.valid = true;
    let mut peak = equity[0];
    let mut run = 0usize;
    let mut sq = 0.0;
    for &e in equity {
        if e > peak {
            peak = e;
        }
        let dd = if peak > 0.0 { (peak - e) / peak } else { 0.0 };
        d.max_drawdown = d.max_drawdown.max(dd);
        sq += dd * dd;
        if e < peak {
            run += 1;
            d.longest_underwater = d.longest_underwater.max(run);
        } else {
            run = 0;
        }
        d.current_drawdown = dd;
    }
    d.ulcer_index = (sq / equity.len() as f64).sqrt();
    d
}

/// Historical value-at-risk and expected shortfall of a return series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailStats {
    /// False with fewer than 20 returns.
    pub valid: bool,
    /// Confidence level, e.g. 0.95.
    pub level: f64,
    /// Historical VaR as a positive loss fraction.
    pub var: f64,
    /// Expected shortfall: mean loss at or beyond VaR (positive).
    pub es: f64,
    /// Worst single-period return.
    pub worst: f64,
}

impl Default for TailStats {
    fn default() -> Self {
        Self { valid: false, level: 0.95, var: 0.0, es: 0.0, worst: 0.0 }
    }
}

/// Index into an ASCENDING-sorted sample of size `n` for the nearest-rank
/// `p`-quantile: the smallest k with (k + 1) / n >= p, clamped to [0, n - 1].
/// A small tolerance stops products like 0.05 * 100 = 5.000000000000001 from
/// rounding up a whole rank.
pub fn nearest_rank_index(n: usize, p: f64) -> usize {
    if n == 0 {
        return 0;
    }
    let rank = (p * n as f64 - 1e-9).ceil();
    let idx = (rank - 1.0).max(0.0).min((n - 1) as f64);
    idx as usize
}

/// Historical (empirical) VaR/ES at `level` (e.g. 0.95). Non-parametric, so it
/// never exceeds the worst loss in the sample.
pub fn compute_tail_stats(returns: &[f64], level: f64) -> TailStats {
    let mut t = TailStats { level, ..TailStats::default() };
    if returns.len() < 20 || !(level > 0.0 && level < 1.0) {
        return t;
    }
    let mut r = returns.to_vec();
    r.sort_by(f64::total_cmp);
    let idx = nearest_rank_index(r.len(), 1.0 - level);
    t.valid = true;
    t.var = (-r[idx]).max(0.0);
    let mut sum = 0.0;
    for &x in &r[..=idx] {
        sum += x;
    }
    t.es = (-(sum / (idx + 1) as f64)).max(0.0);
    t.worst = r[0];
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(values: &[f64]) -> Vec<EquityPoint> {
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| EquityPoint {
                ts_ns: i as i64 + 1,
                equity: Money::from_f64(v).unwrap(),
                cash: Money::ZERO,
            })
            .collect()
    }

    fn near(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} vs {b}");
    }

    #[test]
    fn known_curve() {
        let m = compute_metrics(&curve(&[100.0, 110.0, 99.0, 121.0]), 3.0, 0.0);
        assert!(m.valid);
        assert_eq!(m.periods, 3);
        near(m.total_return, 0.21, 1e-12);
        near(m.annualized_return, 0.21, 1e-12);
        near(m.max_drawdown, 0.1, 1e-12);
        let r = [0.1, 99.0 / 110.0 - 1.0, 121.0 / 99.0 - 1.0];
        let mean = r.iter().sum::<f64>() / 3.0;
        let sd = (r.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / 2.0).sqrt();
        near(m.sharpe, mean / sd * 3f64.sqrt(), 1e-12);
        near(m.annualized_volatility, sd * 3f64.sqrt(), 1e-12);
    }

    #[test]
    fn flat_and_rising_curves() {
        let m = compute_metrics(&curve(&[100.0, 100.0, 100.0, 100.0]), 252.0, 0.0);
        assert!(m.valid);
        near(m.total_return, 0.0, 1e-15);
        near(m.sharpe, 0.0, 1e-15);
        near(m.annualized_volatility, 0.0, 1e-15);
        let up = compute_metrics(&curve(&[100.0, 101.0, 102.0, 103.0]), 252.0, 0.0);
        near(up.max_drawdown, 0.0, 1e-15);
        assert!(up.total_return > 0.0);
    }

    #[test]
    fn annualizes_geometrically_and_handles_degenerate_input() {
        let m = compute_metrics(&curve(&[100.0, 110.0]), 126.0, 0.0);
        near(m.annualized_return, 1.1f64.powf(126.0) - 1.0, 1e-6);
        assert!(!compute_metrics(&curve(&[100.0]), 252.0, 0.0).valid);
        assert!(!compute_metrics(&curve(&[]), 252.0, 0.0).valid);
        assert!(!compute_metrics(&curve(&[100.0, 101.0]), 0.0, 0.0).valid);
        let wiped = compute_metrics(&curve(&[100.0, 0.0]), 252.0, 0.0);
        near(wiped.total_return, -1.0, 1e-12);
        near(wiped.max_drawdown, 1.0, 1e-12);
    }

    #[test]
    fn drawdown_and_tail_stats() {
        let d = compute_drawdown_stats(&[100.0, 120.0, 90.0, 95.0, 130.0, 117.0]);
        near(d.max_drawdown, 0.25, 1e-12);
        assert_eq!(d.longest_underwater, 2);
        near(d.current_drawdown, 0.1, 1e-12);
        let returns: Vec<f64> = (0..100).map(|i| f64::from(i) / 100.0 - 0.5).collect();
        let t = compute_tail_stats(&returns, 0.95);
        assert!(t.valid);
        near(t.var, 0.46, 1e-12); // 5th smallest of -0.50..=0.49
        near(t.es, 0.48, 1e-12);
        assert!(!compute_tail_stats(&returns[..19], 0.95).valid);
        assert_eq!(nearest_rank_index(100, 0.05), 4);
        assert_eq!(nearest_rank_index(0, 0.5), 0);
    }
}
