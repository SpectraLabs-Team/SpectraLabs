//! Discount curves and interest-rate swaps (M24.4).
//!
//! A [`Curve`] holds discount factors at increasing times (years) and
//! interpolates log-linearly between them: forward rates are flat between
//! nodes, the standard robust choice. [`Curve::bootstrap`] builds one from
//! money-market deposits (simple rates) and par swap rates, so that every
//! input instrument reprices exactly.
//!
//! Swaps are single-curve fixed-for-floating: the floating leg is worth
//! `DF(start) - DF(end)` per unit notional. Times are year fractions from
//! the valuation date; schedules are regular (`1 / frequency` years).

use spectra_core::{Error, Result};

/// A discount curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// `(time in years, discount factor)`, strictly increasing in time, starting at `(0, 1)`.
    nodes: Vec<(f64, f64)>,
}

/// One input to a bootstrap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CurveQuote {
    /// A deposit to `years` at a simple annual `rate`: `DF = 1 / (1 + rate * years)`.
    Deposit {
        /// Maturity, years.
        years: f64,
        /// Simple annual rate.
        rate: f64,
    },
    /// A par swap to `years` paying a fixed `rate` `frequency` times a year.
    Swap {
        /// Maturity, years.
        years: f64,
        /// Par fixed rate.
        rate: f64,
        /// Fixed payments a year.
        frequency: u32,
    },
}

impl CurveQuote {
    fn years(&self) -> f64 {
        match *self {
            Self::Deposit { years, .. } | Self::Swap { years, .. } => years,
        }
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::InvalidArgument(m.into()))
}

/// Payment times of a regular schedule to `years`, `frequency` a year (the last at `years`).
fn schedule(years: f64, frequency: u32) -> Vec<f64> {
    let f = f64::from(frequency);
    let n = (years * f).round().max(1.0) as usize;
    (1..=n).map(|i| i as f64 / f).map(|t| if (t - years).abs() < 1e-9 { years } else { t }).collect()
}

impl Curve {
    /// A curve from `(years, discount factor)` nodes (the `(0, 1)` node is added).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] unless times increase and factors are positive.
    pub fn from_discount_factors(points: &[(f64, f64)]) -> Result<Self> {
        let mut nodes = vec![(0.0, 1.0)];
        for &(t, df) in points {
            if !(t.is_finite() && df.is_finite() && df > 0.0) {
                return bad("curve times must be finite and discount factors positive");
            }
            if t <= nodes.last().map_or(0.0, |n| n.0) {
                return bad("curve times must be positive and strictly increasing");
            }
            nodes.push((t, df));
        }
        if nodes.len() < 2 {
            return bad("a curve needs at least one point");
        }
        Ok(Self { nodes })
    }

    /// A flat curve at a continuously compounded `rate`, with a node a year to `years`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a non-finite rate or non-positive length.
    pub fn flat(rate: f64, years: f64) -> Result<Self> {
        let usable = rate.is_finite() && years.is_finite() && years > 0.0;
        if !usable {
            return bad("flat curve needs a finite rate and a positive length");
        }
        let n = years.ceil() as usize;
        let pts: Vec<(f64, f64)> = (1..=n).map(|i| i as f64).map(|t| (t, (-rate * t).exp())).collect();
        Self::from_discount_factors(&pts)
    }

    /// The discount factor at `t` years (flat forward beyond the last node).
    pub fn df(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        let k = self.nodes.partition_point(|n| n.0 < t);
        let (t1, d1, t2, d2) = if k >= self.nodes.len() {
            let n = self.nodes.len();
            let (a, b) = (self.nodes[n - 2], self.nodes[n - 1]);
            (a.0, a.1, b.0, b.1)
        } else {
            let (a, b) = (self.nodes[k - 1], self.nodes[k]);
            (a.0, a.1, b.0, b.1)
        };
        let w = (t - t1) / (t2 - t1);
        (d1.ln() * (1.0 - w) + d2.ln() * w).exp()
    }

    /// Continuously compounded zero rate to `t`.
    pub fn zero_rate(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return self.zero_rate(1e-6);
        }
        -self.df(t).ln() / t
    }

    /// The curve with every zero rate moved by `shift` (0.0001 is one basis point).
    #[must_use]
    pub fn shifted(&self, shift: f64) -> Self {
        Self { nodes: self.nodes.iter().map(|&(t, d)| (t, d * (-shift * t).exp())).collect() }
    }

    /// Nodes, `(years, discount factor)`.
    pub fn nodes(&self) -> &[(f64, f64)] {
        &self.nodes
    }

    /// Builds a curve on which every quote reprices exactly.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for unsorted or duplicate maturities, bad
    /// rates or frequencies, or quotes no positive discount factor can match.
    pub fn bootstrap(quotes: &[CurveQuote]) -> Result<Self> {
        if quotes.is_empty() {
            return bad("a bootstrap needs at least one quote");
        }
        let mut curve = Self { nodes: vec![(0.0, 1.0)] };
        let mut last = 0.0;
        for q in quotes {
            let t = q.years();
            if !(t.is_finite() && t > last) {
                return bad("quote maturities must be positive and strictly increasing");
            }
            last = t;
            let df = match *q {
                CurveQuote::Deposit { years, rate } => {
                    if !(rate.is_finite() && rate > -1.0 / years) {
                        return bad("deposit rate out of range");
                    }
                    1.0 / (1.0 + rate * years)
                }
                CurveQuote::Swap { years, rate, frequency } => {
                    if ![1, 2, 4, 12].contains(&frequency) || !rate.is_finite() {
                        return bad("swap frequency must be 1, 2, 4 or 12 and the rate finite");
                    }
                    curve.solve_swap_node(years, rate, frequency)?
                }
            };
            if !(df.is_finite() && df > 0.0) {
                return bad(format!("no positive discount factor matches the quote at {t} years"));
            }
            curve.nodes.push((t, df));
        }
        Ok(curve)
    }

    /// The discount factor at `years` that makes a par swap price to zero,
    /// with intermediate payments interpolated against the new node.
    fn solve_swap_node(&self, years: f64, rate: f64, frequency: u32) -> Result<f64> {
        let tau = 1.0 / f64::from(frequency);
        let times = schedule(years, frequency);
        let err = |d: f64| {
            let mut trial = self.clone();
            trial.nodes.push((years, d));
            let annuity: f64 = times.iter().map(|&t| tau * trial.df(t)).sum();
            rate * annuity - (1.0 - d)
        };
        // err is increasing in d: bisect on (0, 2], then polish.
        let (mut lo, mut hi) = (1e-12, 2.0);
        if err(lo) > 0.0 || err(hi) < 0.0 {
            return bad(format!("the swap quote at {years} years cannot be matched"));
        }
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if err(mid) > 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
            if hi - lo < 1e-15 {
                break;
            }
        }
        Ok(0.5 * (lo + hi))
    }
}

/// A fixed-for-floating interest-rate swap from the valuation date.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swap {
    /// Notional.
    pub notional: f64,
    /// Fixed rate paid (by the payer) or received (by the receiver).
    pub fixed_rate: f64,
    /// Maturity, years.
    pub years: f64,
    /// Fixed payments a year.
    pub frequency: u32,
    /// True when the holder pays fixed and receives floating.
    pub pay_fixed: bool,
}

/// Value and risk of a swap.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SwapValue {
    /// Present value to the holder.
    pub pv: f64,
    /// The fixed rate that makes the swap worth zero.
    pub par_rate: f64,
    /// PV change for a one basis point parallel rise in zero rates.
    pub dv01: f64,
    /// Present value of 1 a year paid on the fixed schedule (the annuity).
    pub annuity: f64,
}

impl Swap {
    /// Value on `curve`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad frequency, maturity or notional.
    pub fn value(&self, curve: &Curve) -> Result<SwapValue> {
        if ![1, 2, 4, 12].contains(&self.frequency) {
            return bad("swap frequency must be 1, 2, 4 or 12");
        }
        if !(self.years.is_finite()
            && self.years > 0.0
            && self.notional.is_finite()
            && self.fixed_rate.is_finite())
        {
            return bad("swap needs a positive maturity and finite notional and rate");
        }
        let pv_on = |c: &Curve| {
            let tau = 1.0 / f64::from(self.frequency);
            let annuity: f64 = schedule(self.years, self.frequency).iter().map(|&t| tau * c.df(t)).sum();
            let float = 1.0 - c.df(self.years);
            let payer = self.notional * (float - self.fixed_rate * annuity);
            (if self.pay_fixed { payer } else { -payer }, annuity, float / annuity)
        };
        let (pv, annuity, par_rate) = pv_on(curve);
        let up = pv_on(&curve.shifted(0.0001)).0;
        let down = pv_on(&curve.shifted(-0.0001)).0;
        Ok(SwapValue { pv, par_rate, dv01: (up - down) / 2.0, annuity })
    }
}

/// Present value per 100 face of cash flows `(years, amount)` on `curve`,
/// with every zero rate moved by `spread`.
pub fn pv_flows(flows: &[(f64, f64)], curve: &Curve, spread: f64) -> f64 {
    flows.iter().map(|&(t, cf)| cf * curve.df(t) * (-spread * t).exp()).sum()
}

/// The spread over `curve` (continuously compounded) at which `flows` are worth `price`.
///
/// # Errors
/// [`Error::InvalidArgument`] when no spread between -50% and +100% matches.
pub fn z_spread(flows: &[(f64, f64)], curve: &Curve, price: f64) -> Result<f64> {
    let g = |s: f64| pv_flows(flows, curve, s) - price;
    let (mut lo, mut hi) = (-0.5, 1.0);
    if flows.is_empty() || g(lo) < 0.0 || g(hi) > 0.0 {
        return bad("no spread reprices these cash flows to the price");
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if g(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-14 {
            break;
        }
    }
    Ok(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quotes() -> Vec<CurveQuote> {
        vec![
            CurveQuote::Deposit { years: 0.25, rate: 0.040 },
            CurveQuote::Deposit { years: 0.5, rate: 0.041 },
            CurveQuote::Swap { years: 1.0, rate: 0.042, frequency: 2 },
            CurveQuote::Swap { years: 2.0, rate: 0.043, frequency: 2 },
            CurveQuote::Swap { years: 5.0, rate: 0.045, frequency: 2 },
            CurveQuote::Swap { years: 10.0, rate: 0.047, frequency: 2 },
        ]
    }

    #[test]
    fn bootstrap_reprices_every_quote() {
        let c = Curve::bootstrap(&quotes()).unwrap();
        assert!((c.df(0.25) - 1.0 / (1.0 + 0.04 * 0.25)).abs() < 1e-14);
        for q in quotes() {
            if let CurveQuote::Swap { years, rate, frequency } = q {
                let s = Swap { notional: 1e6, fixed_rate: rate, years, frequency, pay_fixed: true };
                let v = s.value(&c).unwrap();
                assert!(v.pv.abs() < 1e-6, "{years}y swap pv {}", v.pv);
                assert!((v.par_rate - rate).abs() < 1e-12);
            }
        }
        // Discount factors fall and zero rates are sensible.
        assert!(c.nodes().windows(2).all(|w| w[1].1 < w[0].1));
        assert!((c.zero_rate(10.0) - 0.046).abs() < 0.003);
    }

    #[test]
    fn flat_curve_swap_and_dv01() {
        let c = Curve::flat(0.05, 30.0).unwrap();
        assert!((c.df(7.3) - (-0.05f64 * 7.3).exp()).abs() < 1e-14);
        let payer = Swap { notional: 1e7, fixed_rate: 0.04, years: 5.0, frequency: 1, pay_fixed: true };
        let v = payer.value(&c).unwrap();
        assert!(v.pv > 0.0, "paying 4% when par is higher is worth something");
        assert!(v.dv01 > 0.0, "a payer gains when rates rise");
        let recv = Swap { pay_fixed: false, ..payer }.value(&c).unwrap();
        assert!((v.pv + recv.pv).abs() < 1e-6);
        // Annual par rate on a flat 5% continuous curve: e^0.05 - 1.
        assert!((v.par_rate - (0.05f64.exp() - 1.0)).abs() < 1e-12);
    }

    #[test]
    fn z_spread_round_trips() {
        let c = Curve::flat(0.03, 10.0).unwrap();
        let flows: Vec<(f64, f64)> =
            (1..=5).map(|i| (f64::from(i), if i == 5 { 105.0 } else { 5.0 })).collect();
        let p = pv_flows(&flows, &c, 0.0125);
        assert!((z_spread(&flows, &c, p).unwrap() - 0.0125).abs() < 1e-10);
    }

    #[test]
    fn refuses_bad_input() {
        assert!(Curve::bootstrap(&[]).is_err());
        assert!(Curve::bootstrap(&[
            CurveQuote::Deposit { years: 1.0, rate: 0.04 },
            CurveQuote::Deposit { years: 0.5, rate: 0.04 }
        ])
        .is_err());
        assert!(Curve::bootstrap(&[CurveQuote::Swap { years: 2.0, rate: 0.04, frequency: 3 }]).is_err());
        assert!(Curve::from_discount_factors(&[(1.0, -0.5)]).is_err());
    }
}
