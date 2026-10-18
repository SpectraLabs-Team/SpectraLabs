//! Statistics for judging whether a backtested Sharpe ratio can be trusted.
//!
//! References: Bailey & López de Prado, "The Sharpe Ratio Efficient Frontier"
//! (2012) and "The Deflated Sharpe Ratio" (2014).
//!
//! All Sharpe ratios here are PER-PERIOD (not annualized): mean / sample stdev
//! of period returns with a zero risk-free rate.
//!
//! Assumptions (also surfaced in validation reports):
//!  * PSR/DSR assume serially independent returns well described by their
//!    first four moments.
//!  * The expected-maximum-Sharpe benchmark assumes the N trials are
//!    independent draws; correlated trials (typical for parameter grids) make
//!    the effective number of trials smaller than the raw count.

pub use spectra_engine::metrics::nearest_rank_index;

const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;

// --- erfc ---------------------------------------------------------------------
// `f64::erfc` is not stable in Rust, so this is a port of the fdlibm `erfc`
// that glibc ships (sysdeps/ieee754/dbl-64/s_erf.c), including glibc's
// evaluation order. It is bit-identical to glibc's `erfc` (checked on 200,000
// points), which keeps PSR/DSR identical to the C++ reference engine.

const ERX: f64 = 8.450_629_115_104_675_292_97e-01;
const PP0: f64 = 1.283_791_670_955_125_585_61e-01;
const PP1: f64 = -3.250_421_072_470_014_993_70e-01;
const PP2: f64 = -2.848_174_957_559_851_047_66e-02;
const PP3: f64 = -5.770_270_296_489_441_591_57e-03;
const PP4: f64 = -2.376_301_665_665_016_260_84e-05;
const QQ1: f64 = 3.979_172_239_591_553_528_19e-01;
const QQ2: f64 = 6.502_224_998_876_729_444_85e-02;
const QQ3: f64 = 5.081_306_281_875_765_627_76e-03;
const QQ4: f64 = 1.324_947_380_043_216_445_26e-04;
const QQ5: f64 = -3.960_228_278_775_368_123_20e-06;
const PA0: f64 = -2.362_118_560_752_659_440_77e-03;
const PA1: f64 = 4.148_561_186_837_483_316_66e-01;
const PA2: f64 = -3.722_078_760_357_013_238_47e-01;
const PA3: f64 = 3.183_466_199_011_617_536_74e-01;
const PA4: f64 = -1.108_946_942_823_966_774_76e-01;
const PA5: f64 = 3.547_830_432_561_823_593_71e-02;
const PA6: f64 = -2.166_375_594_868_790_843_00e-03;
const QA1: f64 = 1.064_208_804_008_442_282_86e-01;
const QA2: f64 = 5.403_979_177_021_710_489_37e-01;
const QA3: f64 = 7.182_865_441_419_626_628_68e-02;
const QA4: f64 = 1.261_712_198_087_616_421_12e-01;
const QA5: f64 = 1.363_708_391_202_905_073_62e-02;
const QA6: f64 = 1.198_449_984_679_910_741_70e-02;
const RA0: f64 = -9.864_944_034_847_148_227_05e-03;
const RA1: f64 = -6.938_585_727_071_817_643_72e-01;
const RA2: f64 = -1.055_862_622_532_329_098_14e+01;
const RA3: f64 = -6.237_533_245_032_600_603_96e+01;
const RA4: f64 = -1.623_966_694_625_734_703_55e+02;
const RA5: f64 = -1.846_050_929_067_110_359_94e+02;
const RA6: f64 = -8.128_743_550_630_659_342_46e+01;
const RA7: f64 = -9.814_329_344_169_145_485_92e+00;
const SA1: f64 = 1.965_127_166_743_925_712_92e+01;
const SA2: f64 = 1.376_577_541_435_190_426_00e+02;
const SA3: f64 = 4.345_658_774_752_292_288_21e+02;
const SA4: f64 = 6.453_872_717_332_678_803_36e+02;
const SA5: f64 = 4.290_081_400_275_678_333_86e+02;
const SA6: f64 = 1.086_350_055_417_794_351_34e+02;
const SA7: f64 = 6.570_249_770_319_281_701_35e+00;
const SA8: f64 = -6.042_441_521_485_809_874_38e-02;
const RB0: f64 = -9.864_942_924_700_099_285_97e-03;
const RB1: f64 = -7.992_832_376_805_230_065_74e-01;
const RB2: f64 = -1.775_795_491_775_475_198_89e+01;
const RB3: f64 = -1.606_363_848_558_219_160_62e+02;
const RB4: f64 = -6.375_664_433_683_896_277_22e+02;
const RB5: f64 = -1.025_095_131_611_077_249_54e+03;
const RB6: f64 = -4.835_191_916_086_513_970_19e+02;
const SB1: f64 = 3.033_806_074_348_245_829_24e+01;
const SB2: f64 = 3.257_925_129_965_739_188_26e+02;
const SB3: f64 = 1.536_729_586_084_436_959_94e+03;
const SB4: f64 = 3.199_858_219_508_595_539_08e+03;
const SB5: f64 = 2.553_050_406_433_164_425_83e+03;
const SB6: f64 = 4.745_285_412_069_553_672_15e+02;
const SB7: f64 = -2.244_095_244_658_581_833_62e+01;
const TINY: f64 = 1e-300;

/// Complementary error function (glibc-exact; see above).
#[allow(clippy::many_single_char_names, clippy::unreadable_literal, clippy::excessive_precision)]
pub fn erfc(x: f64) -> f64 {
    let hx = (x.to_bits() >> 32) as u32 as i32;
    let ix = hx & 0x7fff_ffff;
    if ix >= 0x7ff0_0000 {
        return f64::from(((hx as u32) >> 31) << 1) + 1.0 / x;
    }
    if ix < 0x3feb_0000 {
        if ix < 0x3c70_0000 {
            return 1.0 - x;
        }
        let z = x * x;
        let r1 = PP0 + z * PP1;
        let z2 = z * z;
        let r2 = PP2 + z * PP3;
        let z4 = z2 * z2;
        let s1 = 1.0 + z * QQ1;
        let s2 = QQ2 + z * QQ3;
        let s3 = QQ4 + z * QQ5;
        let r = r1 + z2 * r2 + z4 * PP4;
        let s = s1 + z2 * s2 + z4 * s3;
        let y = r / s;
        if hx < 0x3fd0_0000 {
            return 1.0 - (x + x * y);
        }
        let mut r = x * y;
        r += x - 0.5;
        return 0.5 - r;
    }
    if ix < 0x3ff4_0000 {
        let s = x.abs() - 1.0;
        let p1 = PA0 + s * PA1;
        let s2 = s * s;
        let q1 = 1.0 + s * QA1;
        let s4 = s2 * s2;
        let p2 = PA2 + s * PA3;
        let s6 = s4 * s2;
        let q2 = QA2 + s * QA3;
        let p3 = PA4 + s * PA5;
        let q3 = QA4 + s * QA5;
        let p = p1 + s2 * p2 + s4 * p3 + s6 * PA6;
        let q = q1 + s2 * q2 + s4 * q3 + s6 * QA6;
        return if hx >= 0 { (1.0 - ERX) - p / q } else { 1.0 + (ERX + p / q) };
    }
    if ix < 0x403c_0000 {
        let ax = x.abs();
        let s = 1.0 / (ax * ax);
        let (rr, ss);
        if ix < 0x4006_DB6D {
            let r1 = RA0 + s * RA1;
            let s2 = s * s;
            let t1 = 1.0 + s * SA1;
            let s4 = s2 * s2;
            let r2 = RA2 + s * RA3;
            let s6 = s4 * s2;
            let t2 = SA2 + s * SA3;
            let s8 = s4 * s4;
            let r3 = RA4 + s * RA5;
            let t3 = SA4 + s * SA5;
            let r4 = RA6 + s * RA7;
            let t4 = SA6 + s * SA7;
            rr = r1 + s2 * r2 + s4 * r3 + s6 * r4;
            ss = t1 + s2 * t2 + s4 * t3 + s6 * t4 + s8 * SA8;
        } else {
            if hx < 0 && ix >= 0x4018_0000 {
                return 2.0 - TINY;
            }
            let r1 = RB0 + s * RB1;
            let s2 = s * s;
            let t1 = 1.0 + s * SB1;
            let s4 = s2 * s2;
            let r2 = RB2 + s * RB3;
            let s6 = s4 * s2;
            let t2 = SB2 + s * SB3;
            let r3 = RB4 + s * RB5;
            let t3 = SB4 + s * SB5;
            let t4 = SB6 + s * SB7;
            rr = r1 + s2 * r2 + s4 * r3 + s6 * RB6;
            ss = t1 + s2 * t2 + s4 * t3 + s6 * t4;
        }
        let z = f64::from_bits(ax.to_bits() & 0xffff_ffff_0000_0000);
        let r = (-z * z - 0.5625).exp() * ((z - ax) * (z + ax) + rr / ss).exp();
        return if hx > 0 { r / ax } else { 2.0 - r / ax };
    }
    if hx > 0 {
        TINY * TINY
    } else {
        2.0 - TINY
    }
}

/// Standard normal CDF.
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

/// Inverse standard normal CDF: Acklam's rational approximation plus one
/// Halley refinement step. -inf at 0, +inf at 1, NaN outside [0, 1].
#[allow(clippy::unreadable_literal)]
pub fn normal_inv_cdf(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] =
        [7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00, 3.754408661907416e+00];
    const P_LOW: f64 = 0.02425;

    if !(0.0..=1.0).contains(&p) {
        return f64::NAN;
    }
    if p == 0.0 {
        return f64::NEG_INFINITY;
    }
    if p == 1.0 {
        return f64::INFINITY;
    }
    let x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    let e = normal_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// Sample moments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moments {
    /// Count.
    pub n: usize,
    /// Mean.
    pub mean: f64,
    /// Sample standard deviation (n - 1).
    pub stdev: f64,
    /// m3 / m2^1.5 (population moments).
    pub skew: f64,
    /// m4 / m2^2 (NOT excess; a normal has 3).
    pub kurt: f64,
}

impl Default for Moments {
    fn default() -> Self {
        Self { n: 0, mean: 0.0, stdev: 0.0, skew: 0.0, kurt: 3.0 }
    }
}

/// Moments of `x`. With fewer than 2 points or zero variance: stdev 0, skew
/// 0, kurt 3.
pub fn moments(x: &[f64]) -> Moments {
    let mut m = Moments { n: x.len(), ..Moments::default() };
    if x.len() < 2 {
        return m;
    }
    let n = x.len() as f64;
    let mut mean = 0.0;
    for &v in x {
        mean += v;
    }
    mean /= n;
    let (mut m2, mut m3, mut m4) = (0.0, 0.0, 0.0);
    for &v in x {
        let dv = v - mean;
        m2 += dv * dv;
        m3 += dv * dv * dv;
        m4 += dv * dv * dv * dv;
    }
    m.mean = mean;
    m.stdev = (m2 / (n - 1.0)).sqrt();
    m2 /= n;
    m3 /= n;
    m4 /= n;
    if m2 > 0.0 {
        m.skew = m3 / m2.powf(1.5);
        m.kurt = m4 / (m2 * m2);
    }
    m
}

/// Per-period Sharpe ratio; 0 when the standard deviation is 0 or n < 2.
pub fn sharpe_per_period(returns: &[f64]) -> f64 {
    let m = moments(returns);
    if m.stdev > 0.0 {
        m.mean / m.stdev
    } else {
        0.0
    }
}

/// Probability that the true Sharpe exceeds `sr_benchmark`, given an observed
/// per-period Sharpe `sr` over `n_obs` returns with the given skew/kurtosis.
/// NaN when `n_obs < 2` or the variance term is not positive.
pub fn probabilistic_sharpe(sr: f64, sr_benchmark: f64, n_obs: usize, skew: f64, kurt: f64) -> f64 {
    if n_obs < 2 {
        return f64::NAN;
    }
    let var_term = 1.0 - skew * sr + (kurt - 1.0) / 4.0 * sr * sr;
    if !(var_term > 0.0) {
        return f64::NAN;
    }
    let z = (sr - sr_benchmark) * ((n_obs - 1) as f64).sqrt() / var_term.sqrt();
    normal_cdf(z)
}

/// Expected maximum per-period Sharpe among `n_trials` independent trials with
/// zero true Sharpe. 0 when `n_trials <= 1` or the variance is not positive.
pub fn expected_max_sharpe(sr_variance: f64, n_trials: usize) -> f64 {
    if n_trials <= 1 || !(sr_variance > 0.0) {
        return 0.0;
    }
    let n = n_trials as f64;
    let a = normal_inv_cdf(1.0 - 1.0 / n);
    let b = normal_inv_cdf(1.0 - 1.0 / (n * std::f64::consts::E));
    sr_variance.sqrt() * ((1.0 - EULER_GAMMA) * a + EULER_GAMMA * b)
}

/// Deflated Sharpe Ratio: PSR evaluated against [`expected_max_sharpe`].
pub fn deflated_sharpe(returns: &[f64], n_trials: usize, sr_variance: f64) -> f64 {
    let m = moments(returns);
    let sr = if m.stdev > 0.0 { m.mean / m.stdev } else { 0.0 };
    probabilistic_sharpe(sr, expected_max_sharpe(sr_variance, n_trials), m.n, m.skew, m.kurt)
}

/// Observations needed for PSR(sr vs benchmark) to reach `confidence`.
/// +inf when `sr <= benchmark`; NaN when the variance term is not positive.
pub fn min_track_record_length(sr: f64, sr_benchmark: f64, skew: f64, kurt: f64, confidence: f64) -> f64 {
    if sr <= sr_benchmark {
        return f64::INFINITY;
    }
    let var_term = 1.0 - skew * sr + (kurt - 1.0) / 4.0 * sr * sr;
    if !(var_term > 0.0) {
        return f64::NAN;
    }
    let z = normal_inv_cdf(confidence);
    let ratio = z / (sr - sr_benchmark);
    1.0 + var_term * ratio * ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_functions() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-16);
        assert!((normal_cdf(1.959_963_984_540_054) - 0.975).abs() < 1e-12);
        for p in [1e-10, 0.001, 0.02425, 0.3, 0.5, 0.9, 0.99, 1.0 - 1e-9] {
            assert!((normal_cdf(normal_inv_cdf(p)) - p).abs() < 1e-13 * p.max(1e-3) * 10.0, "p = {p}");
        }
        assert_eq!(normal_inv_cdf(0.0), f64::NEG_INFINITY);
        assert!(normal_inv_cdf(1.5).is_nan());
        assert_eq!(erfc(f64::INFINITY), 0.0);
        assert_eq!(erfc(f64::NEG_INFINITY), 2.0);
    }

    #[test]
    fn psr_dsr_and_track_record() {
        assert!((probabilistic_sharpe(0.1, 0.1, 100, 0.0, 3.0) - 0.5).abs() < 1e-15);
        assert!(probabilistic_sharpe(0.1, 0.0, 1, 0.0, 3.0).is_nan());
        assert_eq!(expected_max_sharpe(0.01, 1), 0.0);
        assert!(expected_max_sharpe(0.01, 100) > expected_max_sharpe(0.01, 10));
        assert_eq!(min_track_record_length(0.0, 0.1, 0.0, 3.0, 0.95), f64::INFINITY);
        let m = moments(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(m.mean, 2.5);
        assert!((m.kurt - 1.64).abs() < 1e-12);
        assert_eq!(moments(&[1.0]).kurt, 3.0);
    }
}
