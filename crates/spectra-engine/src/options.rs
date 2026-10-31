//! European option model (M24.4): Black-Scholes-Merton with a continuous
//! dividend yield, the Greeks, and implied volatility.
//!
//! The normal distribution function is Hart's double-precision algorithm
//! (as given by West, "Better approximations to cumulative normal
//! functions", 2005): accurate to about 1e-14 and computed the same way on
//! every platform, so prices reproduce exactly.
//!
//! Model review: the formulas here are listed for the M26 correctness audit
//! (task 5a). Options trade in backtests and on paper; the audit checks this
//! model against reference values.

use spectra_core::{Error, OptionRight, Result};

/// Standard normal density.
pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// Standard normal distribution function (Hart 1968, double precision).
pub fn norm_cdf(x: f64) -> f64 {
    let z = x.abs();
    let c = if z > 37.0 {
        0.0
    } else {
        let e = (-z * z / 2.0).exp();
        if z < 7.071_067_811_865_475 {
            let mut n = 0.035_262_496_599_891_1 * z + 0.700_383_064_443_688;
            n = n * z + 6.373_962_203_531_65;
            n = n * z + 33.912_866_078_383;
            n = n * z + 112.079_291_497_871;
            n = n * z + 221.213_596_169_931;
            n = n * z + 220.206_867_912_376;
            let mut d = 0.088_388_347_648_318_4 * z + 1.755_667_163_182_64;
            d = d * z + 16.064_177_579_207;
            d = d * z + 86.780_732_202_946_1;
            d = d * z + 296.564_248_779_674;
            d = d * z + 637.333_633_378_831;
            d = d * z + 793.826_512_519_948;
            d = d * z + 440.413_735_824_752;
            e * n / d
        } else {
            let mut b = z + 0.65;
            b = z + 4.0 / b;
            b = z + 3.0 / b;
            b = z + 2.0 / b;
            b = z + 1.0 / b;
            e / b / 2.506_628_274_631
        }
    };
    if x > 0.0 {
        1.0 - c
    } else {
        c
    }
}

/// Inputs to the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionInputs {
    /// Call or put.
    pub right: OptionRight,
    /// Underlying price.
    pub spot: f64,
    /// Strike.
    pub strike: f64,
    /// Time to expiry in years.
    pub years: f64,
    /// Continuously compounded risk-free rate.
    pub rate: f64,
    /// Continuous dividend (or carry) yield.
    pub dividend_yield: f64,
    /// Annual volatility.
    pub volatility: f64,
}

/// Price and Greeks of one option.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OptionValue {
    /// Theoretical price.
    pub price: f64,
    /// dPrice/dSpot.
    pub delta: f64,
    /// d²Price/dSpot².
    pub gamma: f64,
    /// dPrice/dVolatility, per 1.00 of volatility (divide by 100 for a vol point).
    pub vega: f64,
    /// dPrice/dTime, per year (negative as time passes; divide by 365 for a day).
    pub theta: f64,
    /// dPrice/dRate, per 1.00 of rate.
    pub rho: f64,
}

impl OptionInputs {
    /// Checks the inputs.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] naming the problem.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::InvalidArgument(m.into()));
        if !(self.spot.is_finite() && self.spot > 0.0) {
            return bad("spot must be positive");
        }
        if !(self.strike.is_finite() && self.strike > 0.0) {
            return bad("strike must be positive");
        }
        if !(self.years.is_finite() && self.years >= 0.0) {
            return bad("time to expiry must not be negative");
        }
        if !(self.volatility.is_finite() && self.volatility >= 0.0 && self.volatility <= 10.0) {
            return bad("volatility must be between 0 and 10 (1000%)");
        }
        if !self.rate.is_finite() || !self.dividend_yield.is_finite() {
            return bad("rates must be finite");
        }
        Ok(())
    }

    /// Black-Scholes-Merton price and Greeks.
    ///
    /// # Errors
    /// As [`Self::validate`].
    pub fn value(&self) -> Result<OptionValue> {
        self.validate()?;
        let (s, k, t, r, q, v) =
            (self.spot, self.strike, self.years, self.rate, self.dividend_yield, self.volatility);
        let dq = (-q * t).exp();
        let dr = (-r * t).exp();
        // At expiry, or with no volatility: the discounted forward intrinsic value.
        if t == 0.0 || v == 0.0 {
            let fwd = s * dq;
            let pv_k = k * dr;
            let itm = match self.right {
                OptionRight::Call => fwd > pv_k,
                OptionRight::Put => fwd < pv_k,
            };
            let price = match self.right {
                OptionRight::Call => (fwd - pv_k).max(0.0),
                OptionRight::Put => (pv_k - fwd).max(0.0),
            };
            let delta = match (self.right, itm) {
                (OptionRight::Call, true) => dq,
                (OptionRight::Put, true) => -dq,
                _ => 0.0,
            };
            return Ok(OptionValue { price, delta, ..OptionValue::default() });
        }
        let sq = t.sqrt();
        let d1 = ((s / k).ln() + (r - q + 0.5 * v * v) * t) / (v * sq);
        let d2 = d1 - v * sq;
        let pdf1 = norm_pdf(d1);
        let gamma = dq * pdf1 / (s * v * sq);
        let vega = s * dq * pdf1 * sq;
        Ok(match self.right {
            OptionRight::Call => OptionValue {
                price: s * dq * norm_cdf(d1) - k * dr * norm_cdf(d2),
                delta: dq * norm_cdf(d1),
                gamma,
                vega,
                theta: -s * dq * pdf1 * v / (2.0 * sq) - r * k * dr * norm_cdf(d2)
                    + q * s * dq * norm_cdf(d1),
                rho: k * t * dr * norm_cdf(d2),
            },
            OptionRight::Put => OptionValue {
                price: k * dr * norm_cdf(-d2) - s * dq * norm_cdf(-d1),
                delta: -dq * norm_cdf(-d1),
                gamma,
                vega,
                theta: -s * dq * pdf1 * v / (2.0 * sq) + r * k * dr * norm_cdf(-d2)
                    - q * s * dq * norm_cdf(-d1),
                rho: -k * t * dr * norm_cdf(-d2),
            },
        })
    }

    /// The volatility at which the model price equals `price`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when `price` is outside the no-arbitrage
    /// bounds (below the discounted intrinsic value or above its maximum), or
    /// at expiry.
    pub fn implied_volatility(&self, price: f64) -> Result<f64> {
        let at = |v: f64| OptionInputs { volatility: v, ..*self }.value().map(|x| x.price);
        if self.years <= 0.0 {
            return Err(Error::InvalidArgument("no implied volatility at expiry".into()));
        }
        let (mut lo, mut hi) = (1e-9, 10.0);
        let (plo, phi) = (at(lo)?, at(hi)?);
        if !(price.is_finite() && price >= plo - 1e-12 && price <= phi) {
            return Err(Error::InvalidArgument(format!(
                "price {price} is outside the model's range [{plo}, {phi}] for these inputs"
            )));
        }
        let mut v = 0.2;
        for _ in 0..200 {
            let val = OptionInputs { volatility: v, ..*self }.value()?;
            let diff = val.price - price;
            if diff.abs() < 1e-12 {
                return Ok(v);
            }
            if diff > 0.0 {
                hi = v;
            } else {
                lo = v;
            }
            let next = v - diff / val.vega;
            v = if val.vega > 1e-12 && next > lo && next < hi { next } else { 0.5 * (lo + hi) };
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(right: OptionRight) -> OptionInputs {
        OptionInputs {
            right,
            spot: 100.0,
            strike: 100.0,
            years: 1.0,
            rate: 0.05,
            dividend_yield: 0.0,
            volatility: 0.2,
        }
    }

    #[test]
    fn cdf_matches_known_values() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-15);
        assert!((norm_cdf(1.0) - 0.841_344_746_068_542_9).abs() < 1e-14);
        assert!((norm_cdf(-1.96) - 0.024_997_895_148_220_43).abs() < 1e-14);
        assert!((norm_cdf(8.0) - 1.0).abs() < 1e-14);
        assert_eq!(norm_cdf(-40.0), 0.0);
    }

    #[test]
    fn textbook_price() {
        // Hull: S=K=100, T=1, r=5%, sigma=20%: call 10.4506, put 5.5735.
        let c = inputs(OptionRight::Call).value().unwrap();
        let p = inputs(OptionRight::Put).value().unwrap();
        assert!((c.price - 10.450_583_572_185_565).abs() < 1e-9, "{}", c.price);
        assert!((p.price - 5.573_526_022_256_971).abs() < 1e-9, "{}", p.price);
        // Put-call parity: C - P = S e^{-qT} - K e^{-rT}.
        assert!((c.price - p.price - (100.0 - 100.0 * (-0.05f64).exp())).abs() < 1e-12);
        assert!((c.delta - 0.636_830_651_175_619).abs() < 1e-9);
        assert!((c.gamma - p.gamma).abs() < 1e-15 && (c.vega - p.vega).abs() < 1e-12);
    }

    #[test]
    fn greeks_agree_with_finite_differences() {
        let base = OptionInputs { dividend_yield: 0.02, strike: 105.0, ..inputs(OptionRight::Put) };
        let v = base.value().unwrap();
        let h = 1e-4;
        let f = |i: OptionInputs| i.value().unwrap().price;
        let ds = (f(OptionInputs { spot: 100.0 + h, ..base }) - f(OptionInputs { spot: 100.0 - h, ..base }))
            / (2.0 * h);
        let dv = (f(OptionInputs { volatility: 0.2 + h, ..base })
            - f(OptionInputs { volatility: 0.2 - h, ..base }))
            / (2.0 * h);
        let dr = (f(OptionInputs { rate: 0.05 + h, ..base }) - f(OptionInputs { rate: 0.05 - h, ..base }))
            / (2.0 * h);
        let dt = (f(OptionInputs { years: 1.0 - h, ..base }) - f(OptionInputs { years: 1.0 + h, ..base }))
            / (2.0 * h);
        assert!((v.delta - ds).abs() < 1e-7);
        assert!((v.vega - dv).abs() < 1e-6);
        assert!((v.rho - dr).abs() < 1e-6);
        assert!((v.theta - dt).abs() < 1e-6);
    }

    #[test]
    fn implied_volatility_round_trips() {
        for right in [OptionRight::Call, OptionRight::Put] {
            for vol in [0.05, 0.2, 0.8, 2.5] {
                let i = OptionInputs { volatility: vol, strike: 90.0, ..inputs(right) };
                let p = i.value().unwrap().price;
                assert!((i.implied_volatility(p).unwrap() - vol).abs() < 1e-8, "{right:?} {vol}");
            }
        }
        assert!(inputs(OptionRight::Call).implied_volatility(0.01).is_err()); // below intrinsic bound
    }

    #[test]
    fn expiry_is_intrinsic() {
        let i = OptionInputs { years: 0.0, spot: 110.0, ..inputs(OptionRight::Call) };
        assert_eq!(i.value().unwrap().price, 10.0);
        assert_eq!(OptionRight::Put.intrinsic(90.0, 100.0), 10.0); // defined with the record, in spectra-core
        assert!(OptionInputs { spot: -1.0, ..inputs(OptionRight::Call) }.value().is_err());
    }
}
