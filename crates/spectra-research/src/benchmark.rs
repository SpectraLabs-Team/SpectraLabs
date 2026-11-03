//! Benchmark-relative analysis.
//!
//! The benchmark is an equal-weight buy-and-hold of every instrument in the
//! dataset: at each timestamp it earns the average close-to-close return of the
//! instruments that have a bar there and had a previous close. It ignores costs
//! and rebalancing.
//! ```text
//!   beta              = cov(r_s, r_b) / var(r_b)
//!   alpha (annual)    = (mean(r_s) - beta * mean(r_b)) * ppy
//!   correlation       = corr(r_s, r_b)
//!   tracking error    = stdev(r_s - r_b) * sqrt(ppy)
//!   information ratio = mean(r_s - r_b) / stdev(r_s - r_b) * sqrt(ppy)
//!   up/down capture   = mean(r_s) / mean(r_b) over periods with r_b > 0 / r_b < 0
//! ```

use std::collections::BTreeMap;

use spectra_core::{Bar, Error, InstrumentId, Price, Result};
use spectra_engine::EquityPoint;

/// Period returns of an equity curve, aligned to it (one per consecutive pair
/// of points; returns -1.0 when current equity is non-positive).
///
/// # Errors
/// If any previous equity point is non-positive or non-finite.
pub fn aligned_returns(curve: &[EquityPoint]) -> Result<Vec<f64>> {
    let mut out = Vec::with_capacity(curve.len().saturating_sub(1));
    for w in curve.windows(2) {
        let prev = w[0].equity.to_f64();
        if prev <= 0.0 || !prev.is_finite() {
            return Err(Error::InvalidArgument(format!("aligned_returns: non-positive prior equity {prev}")));
        }
        let curr = w[1].equity.to_f64();
        if curr <= 0.0 {
            out.push(-1.0);
        } else {
            out.push(curr / prev - 1.0);
        }
    }
    Ok(out)
}

/// Equal-weight benchmark returns, one per consecutive pair of unique timestamps.
pub fn benchmark_returns(bars: &[Bar]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut last_close: BTreeMap<InstrumentId, Price> = BTreeMap::new();
    let mut first = true;
    let mut i = 0;
    while i < bars.len() {
        let mut j = i;
        while j < bars.len() && bars[j].ts_ns == bars[i].ts_ns {
            j += 1;
        }
        let mut sum = 0.0;
        let mut count = 0i32;
        for b in &bars[i..j] {
            if let Some(prev) = last_close.get(&b.instrument) {
                sum += b.close.to_f64() / prev.to_f64() - 1.0;
                count += 1;
            }
        }
        if !first {
            out.push(if count > 0 { sum / f64::from(count) } else { 0.0 });
        }
        first = false;
        for b in &bars[i..j] {
            last_close.insert(b.instrument, b.close);
        }
        i = j;
    }
    out
}

/// Strategy-versus-benchmark statistics.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BenchmarkMetrics {
    /// Needs >= 2 aligned periods and non-zero benchmark variance.
    pub valid: bool,
    /// Beta.
    pub beta: f64,
    /// Annualized alpha (simple).
    pub alpha_annualized: f64,
    /// Correlation.
    pub correlation: f64,
    /// Annualized tracking error.
    pub tracking_error: f64,
    /// Annualized information ratio.
    pub information_ratio: f64,
    /// Up capture.
    pub up_capture: f64,
    /// Down capture.
    pub down_capture: f64,
}

/// Computes [`BenchmarkMetrics`] for equal-length series.
///
/// # Errors
/// If the series differ in length, have fewer than 2 observations, `ppy <= 0.0`,
/// or if the benchmark has zero variance or contains NaN.
pub fn compute_benchmark_metrics(s: &[f64], b: &[f64], ppy: f64) -> Result<BenchmarkMetrics> {
    if s.len() != b.len() {
        return Err(Error::InvalidArgument("benchmark metrics: series differ in length".into()));
    }
    let n = s.len();
    if n < 2 {
        return Err(Error::InvalidArgument(format!(
            "benchmark metrics: need at least 2 observations, got {n}"
        )));
    }
    if ppy <= 0.0 || !ppy.is_finite() {
        return Err(Error::InvalidArgument(format!(
            "benchmark metrics: periods_per_year must be positive, got {ppy}"
        )));
    }
    let mut m = BenchmarkMetrics::default();
    let nd = n as f64;
    let (mut ms, mut mb) = (0.0, 0.0);
    for i in 0..n {
        ms += s[i];
        mb += b[i];
    }
    ms /= nd;
    mb /= nd;
    let (mut cov, mut vs, mut vb, mut vd) = (0.0, 0.0, 0.0, 0.0);
    let md = ms - mb;
    for i in 0..n {
        cov += (s[i] - ms) * (b[i] - mb);
        vs += (s[i] - ms) * (s[i] - ms);
        vb += (b[i] - mb) * (b[i] - mb);
        let d = (s[i] - b[i]) - md;
        vd += d * d;
    }
    cov /= nd - 1.0;
    vs /= nd - 1.0;
    vb /= nd - 1.0;
    vd /= nd - 1.0;
    if !(vb > 0.0) {
        return Err(Error::InvalidArgument(
            "benchmark metrics: zero benchmark variance or NaN in benchmark".into(),
        ));
    }
    m.valid = true;
    m.beta = cov / vb;
    m.alpha_annualized = (ms - m.beta * mb) * ppy;
    m.correlation = if vs > 0.0 { cov / (vs * vb).sqrt() } else { 0.0 };
    let te = vd.sqrt();
    m.tracking_error = te * ppy.sqrt();
    m.information_ratio = if te > 1e-15 { md / te * ppy.sqrt() } else { 0.0 };

    let (mut up_s, mut up_b, mut dn_s, mut dn_b) = (0.0, 0.0, 0.0, 0.0);
    let (mut up_n, mut dn_n) = (0i32, 0i32);
    for i in 0..n {
        if b[i] > 0.0 {
            up_s += s[i];
            up_b += b[i];
            up_n += 1;
        } else if b[i] < 0.0 {
            dn_s += s[i];
            dn_b += b[i];
            dn_n += 1;
        }
    }
    if up_n > 0 && up_b != 0.0 {
        m.up_capture = (up_s / f64::from(up_n)) / (up_b / f64::from(up_n));
    }
    if dn_n > 0 && dn_b != 0.0 {
        m.down_capture = (dn_s / f64::from(dn_n)) / (dn_b / f64::from(dn_n));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_series_have_beta_one_and_no_tracking_error() {
        let r = [0.01, -0.02, 0.03, 0.0, -0.01];
        let m = compute_benchmark_metrics(&r, &r, 252.0).unwrap();
        assert!(m.valid);
        assert!((m.beta - 1.0).abs() < 1e-12);
        assert!((m.correlation - 1.0).abs() < 1e-12);
        assert_eq!(m.information_ratio, 0.0);
        assert!(compute_benchmark_metrics(&r, &r[..2], 252.0).is_err());
        let doubled: Vec<f64> = r.iter().map(|x| 2.0 * x).collect();
        assert!((compute_benchmark_metrics(&doubled, &r, 252.0).unwrap().beta - 2.0).abs() < 1e-12);
    }

    #[test]
    fn invalid_inputs_return_err() {
        let r = [0.01, -0.02, 0.03];
        // n < 2
        assert!(compute_benchmark_metrics(&[], &[], 252.0).is_err());
        assert!(compute_benchmark_metrics(&[0.01], &[0.01], 252.0).is_err());
        // ppy <= 0.0
        assert!(compute_benchmark_metrics(&r, &r, 0.0).is_err());
        assert!(compute_benchmark_metrics(&r, &r, -1.0).is_err());
        // zero benchmark variance
        let flat = [0.01, 0.01, 0.01];
        assert!(compute_benchmark_metrics(&r, &flat, 252.0).is_err());
        // NaN in benchmark
        let nan_b = [0.01, f64::NAN, 0.03];
        assert!(compute_benchmark_metrics(&r, &nan_b, 252.0).is_err());
    }

    #[test]
    fn aligned_returns_validates_inputs() {
        use spectra_core::Money;

        let pt = |e: f64| EquityPoint {
            ts_ns: 0,
            equity: Money::from_f64(e).unwrap(),
            cash: Money::from_f64(e).unwrap(),
        };

        // prev <= 0 returns Err
        let curve_zero = [pt(0.0), pt(100.0)];
        assert!(aligned_returns(&curve_zero).is_err());

        // prev > 0 and curr <= 0 returns -1.0
        let curve_loss = [pt(100.0), pt(0.0)];
        assert_eq!(aligned_returns(&curve_loss).unwrap(), vec![-1.0]);

        // normal positive return
        let curve_normal = [pt(100.0), pt(110.0)];
        let ret = aligned_returns(&curve_normal).unwrap();
        assert!((ret[0] - 0.1).abs() < 1e-12);
    }
}
