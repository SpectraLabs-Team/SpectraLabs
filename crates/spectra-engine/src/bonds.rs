//! Fixed-coupon bond model (M24.3): coupon schedule, day counts, accrued
//! interest, clean and dirty prices, yield to maturity, duration and convexity.
//!
//! Conventions. Prices are per 100 of face value. A bond's `multiplier` is
//! its face value per unit traded divided by 100 (a unit of face 1,000 has
//! multiplier 10), so `price * qty * multiplier` is the cash amount, as for
//! every other instrument. Coupon dates run backward from maturity in steps
//! of `12 / frequency` months; a maturity on the last day of its month keeps
//! later dates on month ends. Ex-coupon periods are not modelled: the holder
//! at the close before a coupon date receives the coupon.

use spectra_core::{DayCount, Error, Instrument, InstrumentType, Result, COUPON_FREQUENCIES};

const DAY_NS: i64 = 86_400_000_000_000;

/// Days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `day` moved by `months` (negative: earlier), clamped to the month's length;
/// with `eom`, the result is the last day of its month.
fn add_months(day: i64, months: i64, eom: bool) -> i64 {
    let (y, m, d) = civil_from_days(day);
    let total = y * 12 + i64::from(m - 1) + months;
    let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let len = month_len(ny, nm);
    days_from_civil(ny, nm, if eom { len } else { d.min(len) })
}

/// The terms the model needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BondTerms {
    /// Maturity, nanoseconds since the epoch (the final coupon and redemption date).
    pub maturity_ns: i64,
    /// Annual coupon rate as a fraction (0.05 is 5%).
    pub coupon_rate: f64,
    /// Coupons per year: 1, 2, 4 or 12.
    pub frequency: u32,
    /// Day count for accrued interest.
    pub day_count: DayCount,
}

impl BondTerms {
    /// The terms of a bond record (`None` for anything else, or a record
    /// missing a term; records are validated before they reach a run).
    pub fn from_instrument(inst: &Instrument) -> Option<Self> {
        if inst.kind != InstrumentType::Bond {
            return None;
        }
        Some(Self {
            maturity_ns: inst.maturity_ns?,
            coupon_rate: inst.coupon_rate?.to_f64(),
            frequency: inst.coupon_frequency?,
            day_count: inst.day_count?,
        })
    }

    /// Checks the terms.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] naming the problem.
    pub fn validate(&self) -> Result<()> {
        if !COUPON_FREQUENCIES.contains(&self.frequency) {
            return Err(Error::InvalidArgument("coupon frequency must be 1, 2, 4 or 12 a year".into()));
        }
        if !self.coupon_rate.is_finite() || !(0.0..=1.0).contains(&self.coupon_rate) {
            return Err(Error::InvalidArgument("coupon rate must be between 0 and 1".into()));
        }
        Ok(())
    }

    fn step_months(&self) -> i64 {
        i64::from(12 / self.frequency)
    }

    /// Coupon per 100 face for one full period.
    pub fn coupon_per_period(&self) -> f64 {
        100.0 * self.coupon_rate / f64::from(self.frequency)
    }

    /// The coupon period containing `ts_ns`: `(start_day, end_day)` with
    /// `start < day(ts) <= end`, or `None` after maturity.
    pub fn period_around(&self, ts_ns: i64) -> Option<(i64, i64)> {
        let mat = self.maturity_ns.div_euclid(DAY_NS);
        let day = ts_ns.div_euclid(DAY_NS);
        if day > mat {
            return None;
        }
        let (my, mm, md) = civil_from_days(mat);
        let eom = md == month_len(my, mm);
        let step = self.step_months();
        // Walk back from maturity; at most a few hundred steps for any real bond.
        let mut end = mat;
        let mut k = 1;
        loop {
            let start = add_months(mat, -step * k, eom);
            if start < day || k > 2_400 {
                return Some((start, end));
            }
            end = start;
            k += 1;
        }
    }

    /// Coupon dates (days since the epoch) strictly after `from_ns` and on or before `to_ns`.
    pub fn coupon_days_between(&self, from_ns: i64, to_ns: i64) -> Vec<i64> {
        let mat = self.maturity_ns.div_euclid(DAY_NS);
        let (lo, hi) = (from_ns.div_euclid(DAY_NS), to_ns.div_euclid(DAY_NS).min(mat));
        let (my, mm, md) = civil_from_days(mat);
        let eom = md == month_len(my, mm);
        let step = self.step_months();
        let mut out = Vec::new();
        let mut k = 0;
        loop {
            let d = add_months(mat, -step * k, eom);
            if d <= lo || k > 2_400 {
                break;
            }
            if d <= hi {
                out.push(d);
            }
            k += 1;
        }
        out.reverse();
        out
    }

    /// Fraction of a year between two days under the day count, for the
    /// coupon period `(ps, pe)` they fall in.
    fn year_fraction(&self, from: i64, to: i64, ps: i64, pe: i64) -> f64 {
        match self.day_count {
            DayCount::ActActIcma => {
                let len = (pe - ps).max(1) as f64;
                (to - from) as f64 / len / f64::from(self.frequency)
            }
            DayCount::Thirty360 => {
                let (y1, m1, d1) = civil_from_days(from);
                let (y2, m2, d2) = civil_from_days(to);
                let d1 = d1.min(30);
                let d2 = if d1 == 30 { d2.min(30) } else { d2 };
                let days =
                    360 * (y2 - y1) + 30 * (i64::from(m2) - i64::from(m1)) + (i64::from(d2) - i64::from(d1));
                days as f64 / 360.0
            }
            DayCount::Act360 => (to - from) as f64 / 360.0,
            DayCount::Act365F => (to - from) as f64 / 365.0,
        }
    }

    /// Coupon paid on the period ending `pe` (per 100 face). Exactly
    /// `rate / frequency` under Act/Act ICMA and 30/360 for regular periods.
    pub fn coupon_amount(&self, ps: i64, pe: i64) -> f64 {
        100.0 * self.coupon_rate * self.year_fraction(ps, pe, ps, pe)
    }

    /// Accrued interest per 100 face at `ts_ns` (zero on a coupon date and after maturity).
    pub fn accrued(&self, ts_ns: i64) -> f64 {
        let Some((ps, pe)) = self.period_around(ts_ns) else { return 0.0 };
        let day = ts_ns.div_euclid(DAY_NS);
        if day >= pe {
            return 0.0;
        }
        100.0 * self.coupon_rate * self.year_fraction(ps, day, ps, pe)
    }

    /// Dirty price from clean at `ts_ns`.
    pub fn dirty(&self, clean: f64, ts_ns: i64) -> f64 {
        clean + self.accrued(ts_ns)
    }

    /// Clean price from dirty at `ts_ns`.
    pub fn clean(&self, dirty: f64, ts_ns: i64) -> f64 {
        dirty - self.accrued(ts_ns)
    }

    /// Remaining cash flows at `ts_ns`: `(time in periods from now, amount per 100 face)`.
    /// The first coupon after today is the fraction of its own period still
    /// to run away (a whole period on a coupon date); each later one is a
    /// period further.
    fn flows(&self, ts_ns: i64) -> Vec<(f64, f64)> {
        let day = ts_ns.div_euclid(DAY_NS);
        let days = self.coupon_days_between(ts_ns, self.maturity_ns);
        let Some(&first) = days.first() else { return Vec::new() };
        let Some((start, end)) = self.period_around(first * DAY_NS) else { return Vec::new() };
        let t0 = (end - day) as f64 / (end - start).max(1) as f64;
        let mut out = Vec::with_capacity(days.len());
        let mut prev = start;
        for (k, &d) in days.iter().enumerate() {
            out.push((t0 + k as f64, self.coupon_amount(prev, d)));
            prev = d;
        }
        if let Some(last) = out.last_mut() {
            last.1 += 100.0;
        }
        out
    }

    /// Remaining cash flows at `ts_ns` as `(years from now, amount per 100
    /// face)`, a period being `1 / frequency` years: for pricing off a curve.
    pub fn cash_flows(&self, ts_ns: i64) -> Vec<(f64, f64)> {
        let f = f64::from(self.frequency);
        self.flows(ts_ns).into_iter().map(|(t, cf)| (t / f, cf)).collect()
    }

    /// Dirty price per 100 face for an annual yield `y` compounded `frequency` times a year.
    pub fn price_from_yield(&self, y: f64, ts_ns: i64) -> f64 {
        let f = f64::from(self.frequency);
        let base = 1.0 + y / f;
        self.flows(ts_ns).iter().map(|&(t, cf)| cf / base.powf(t)).sum()
    }

    /// Yield to maturity (annual, compounded `frequency` times a year) that
    /// reprices the bond to `dirty` at `ts_ns`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] after maturity, for a non-positive price, or
    /// when no yield between -99% and 1000% reprices it.
    pub fn yield_from_price(&self, dirty: f64, ts_ns: i64) -> Result<f64> {
        if !dirty.is_finite() || dirty <= 0.0 {
            return Err(Error::InvalidArgument("price must be positive".into()));
        }
        if self.flows(ts_ns).is_empty() {
            return Err(Error::InvalidArgument("the bond has matured".into()));
        }
        let f = f64::from(self.frequency);
        let (mut lo, mut hi) = (-0.99 * f, 10.0);
        let g = |y: f64| self.price_from_yield(y, ts_ns) - dirty;
        if g(lo) * g(hi) > 0.0 {
            return Err(Error::InvalidArgument("no yield reprices the bond to this price".into()));
        }
        // Newton from the coupon rate, falling back to bisection; price is decreasing in yield.
        let mut y = self.coupon_rate;
        for _ in 0..200 {
            let v = g(y);
            if v.abs() < 1e-12 {
                return Ok(y);
            }
            if v > 0.0 {
                lo = lo.max(y);
            } else {
                hi = hi.min(y);
            }
            let h = 1e-7;
            let d = (g(y + h) - g(y - h)) / (2.0 * h);
            let next = y - v / d;
            y = if d.is_finite() && d < 0.0 && next > lo && next < hi { next } else { 0.5 * (lo + hi) };
        }
        Ok(y)
    }

    /// Macaulay duration in years, modified duration, convexity (years²)
    /// and DV01 (price change per 100 face for a 1bp fall in yield), at yield `y`.
    pub fn risk(&self, y: f64, ts_ns: i64) -> BondRisk {
        let f = f64::from(self.frequency);
        let base = 1.0 + y / f;
        let flows = self.flows(ts_ns);
        let price: f64 = flows.iter().map(|&(t, cf)| cf / base.powf(t)).sum();
        if price <= 0.0 {
            return BondRisk::default();
        }
        let mac_periods: f64 = flows.iter().map(|&(t, cf)| t * cf / base.powf(t)).sum::<f64>() / price;
        let conv_periods: f64 =
            flows.iter().map(|&(t, cf)| t * (t + 1.0) * cf / base.powf(t + 2.0)).sum::<f64>() / price;
        let macaulay = mac_periods / f;
        let modified = macaulay / base;
        BondRisk {
            macaulay,
            modified,
            convexity: conv_periods / (f * f),
            dv01: (self.price_from_yield(y - 0.0001, ts_ns) - self.price_from_yield(y + 0.0001, ts_ns)) / 2.0,
        }
    }
}

/// Interest-rate risk of a bond at a yield.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BondRisk {
    /// Macaulay duration, years.
    pub macaulay: f64,
    /// Modified duration, years.
    pub modified: f64,
    /// Convexity, years squared.
    pub convexity: f64,
    /// Price change per 100 face for a one basis point move in yield.
    pub dv01: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d) * DAY_NS
    }

    fn semi(rate: f64, mat: i64, dc: DayCount) -> BondTerms {
        BondTerms { maturity_ns: mat, coupon_rate: rate, frequency: 2, day_count: dc }
    }

    #[test]
    fn schedule_runs_back_from_maturity_with_month_ends() {
        let b = semi(0.05, ns(2030, 2, 28), DayCount::ActActIcma);
        let days: Vec<(i64, u32, u32)> = b
            .coupon_days_between(ns(2028, 12, 31), ns(2030, 3, 1))
            .into_iter()
            .map(civil_from_days)
            .collect();
        assert_eq!(days, vec![(2029, 2, 28), (2029, 8, 31), (2030, 2, 28)]);
        let b = semi(0.05, ns(2030, 6, 15), DayCount::ActActIcma);
        assert_eq!(
            b.period_around(ns(2027, 3, 1)).map(|(s, e)| (civil_from_days(s), civil_from_days(e))),
            Some(((2026, 12, 15), (2027, 6, 15)))
        );
    }

    #[test]
    fn accrued_interest_by_convention() {
        // 6% semiannual, period 2026-12-15 .. 2027-06-15 (182 days); 76 days in on 2027-03-01.
        let b = semi(0.06, ns(2030, 6, 15), DayCount::ActActIcma);
        assert!((b.accrued(ns(2027, 3, 1)) - 3.0 * 76.0 / 182.0).abs() < 1e-12);
        assert_eq!(b.accrued(ns(2027, 6, 15)), 0.0); // coupon date: reset
        let b = semi(0.06, ns(2030, 6, 15), DayCount::Thirty360);
        // 30/360: Dec 15 -> Mar 1 is 15 + 30 + 30 + 1 = 76 days of 360.
        assert!((b.accrued(ns(2027, 3, 1)) - 6.0 * 76.0 / 360.0).abs() < 1e-12);
        let b = semi(0.06, ns(2030, 6, 15), DayCount::Act365F);
        assert!((b.accrued(ns(2027, 3, 1)) - 6.0 * 76.0 / 365.0).abs() < 1e-12);
        assert!(
            (b.dirty(99.0, ns(2027, 3, 1))
                - b.clean(b.dirty(99.0, ns(2027, 3, 1)), ns(2027, 3, 1))
                - b.accrued(ns(2027, 3, 1)))
            .abs()
                < 1e-12
        );
    }

    #[test]
    fn par_bond_yields_its_coupon_on_a_coupon_date() {
        let b = semi(0.05, ns(2036, 6, 15), DayCount::ActActIcma);
        let t = ns(2026, 6, 15);
        assert!((b.price_from_yield(0.05, t) - 100.0).abs() < 1e-9);
        assert!((b.yield_from_price(100.0, t).unwrap() - 0.05).abs() < 1e-10);
        // Below par, the yield is above the coupon, and it round-trips.
        let y = b.yield_from_price(95.0, t).unwrap();
        assert!(y > 0.05);
        assert!((b.price_from_yield(y, t) - 95.0).abs() < 1e-9);
    }

    #[test]
    fn zero_coupon_duration_is_its_maturity() {
        let b = BondTerms {
            maturity_ns: ns(2036, 6, 15),
            coupon_rate: 0.0,
            frequency: 1,
            day_count: DayCount::ActActIcma,
        };
        let r = b.risk(0.04, ns(2026, 6, 15));
        assert!((r.macaulay - 10.0).abs() < 1e-9);
        assert!((r.modified - 10.0 / 1.04).abs() < 1e-9);
        assert!(r.dv01 > 0.0 && r.convexity > 0.0);
    }

    #[test]
    fn known_textbook_duration() {
        // 10y 8% annual bond at 10% yield: Macaulay duration 7.0439 years (standard textbook value).
        let b = BondTerms {
            maturity_ns: ns(2036, 1, 1),
            coupon_rate: 0.08,
            frequency: 1,
            day_count: DayCount::ActActIcma,
        };
        let r = b.risk(0.10, ns(2026, 1, 1));
        assert!((r.macaulay - 7.0439).abs() < 1e-4, "{}", r.macaulay);
        assert!((b.price_from_yield(0.10, ns(2026, 1, 1)) - 87.7109).abs() < 1e-4);
    }

    #[test]
    fn refuses_bad_terms() {
        let mut b = semi(0.05, ns(2030, 1, 1), DayCount::ActActIcma);
        b.frequency = 3;
        assert!(b.validate().is_err());
        b.frequency = 2;
        b.coupon_rate = -0.01;
        assert!(b.validate().is_err());
        assert!(semi(0.05, ns(2030, 1, 1), DayCount::ActActIcma)
            .yield_from_price(100.0, ns(2031, 1, 1))
            .is_err());
    }
}
