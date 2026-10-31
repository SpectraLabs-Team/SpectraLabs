//! Interest-rate swaps in backtests (M24.4).
//!
//! A swap is marked every bar from the discount curve known at that time
//! (never a later one), so a run needs a dated history of curve quotes:
//! [`CurveSnapshot`]s, each bootstrapped once into a [`Curve`].
//!
//! Conventions.
//! - Fixed-for-floating, single curve: the same curve discounts and projects.
//!   Both legs pay on one regular schedule running backward from maturity in
//!   steps of `12 / frequency` months, accruing `1 / frequency` a period.
//! - The floating rate of a period is fixed at its start from the curve known
//!   then: `L = (1 / DF(t) - 1) / tau`, `t` the period's length in years.
//! - The quantity traded is the receiver's side: buying receives fixed and
//!   pays floating; selling pays fixed. A position's price is
//!   `100 + 100 * NPV` per 1 of notional, as swap futures are quoted, so a
//!   swap at its par rate trades at 100. Its `multiplier` is notional per
//!   unit divided by 100 (as for bonds), so `price * qty * multiplier` is cash.
//! - A swap is marked to market like a future: price moves settle to cash
//!   every bar, and the net coupon of each period (`(fixed - L) * tau` per 1
//!   of notional, received or paid) is paid on its end date, when the mark
//!   drops by the same amount. At maturity the position closes at 100.
//! - Times on the curve are calendar time over 365.25 days.
//!
//! Bars for a swap are generated, not read: one per dataset timestamp at
//! which the swap is alive, its current period's fixing is known and a
//! curve is known. They carry no volume (there is no traded volume to model),
//! so volume-based slippage and impact do not apply to swaps and the
//! participation cap is not applied to them.

use std::collections::BTreeMap;

use crate::bonds::BondTerms;
use crate::curves::{Curve, CurveQuote};
use crate::instruments::InstrumentSetup;
use spectra_core::{
    Bar, DayCount, Error, Instrument, InstrumentId, InstrumentType, Price, Qty, Ratio, Result,
    COUPON_FREQUENCIES,
};

const DAY_NS: i64 = 86_400_000_000_000;
/// Nanoseconds in a year of 365.25 days: how calendar time maps to curve time.
pub const YEAR_NS: f64 = 365.25 * 86_400e9;

/// What a curve quote is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    /// A money-market deposit at a simple rate.
    Deposit,
    /// A par swap paying fixed `frequency` times a year.
    Swap {
        /// Fixed payments a year: 1, 2, 4 or 12.
        frequency: u32,
    },
}

impl QuoteKind {
    /// Name used in requests and records.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deposit => "deposit",
            Self::Swap { .. } => "swap",
        }
    }

    /// The fixed-leg frequency of a swap quote.
    pub const fn frequency(self) -> Option<u32> {
        match self {
            Self::Deposit => None,
            Self::Swap { frequency } => Some(frequency),
        }
    }
}

/// One quote of a snapshot, exactly (fixed-point years and rate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurvePoint {
    /// Deposit or par swap.
    pub kind: QuoteKind,
    /// Maturity in years.
    pub years: Ratio,
    /// Rate as a fraction (0.04 is 4%).
    pub rate: Ratio,
}

/// The quotes of one curve as known from `as_of_ns` on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveSnapshot {
    /// When these quotes became known, nanoseconds since the epoch.
    pub as_of_ns: i64,
    /// Quotes, by increasing maturity.
    pub quotes: Vec<CurvePoint>,
}

impl CurveSnapshot {
    /// The quotes as bootstrap inputs.
    pub fn to_quotes(&self) -> Vec<CurveQuote> {
        self.quotes
            .iter()
            .map(|q| match q.kind {
                QuoteKind::Deposit => CurveQuote::Deposit { years: q.years.to_f64(), rate: q.rate.to_f64() },
                QuoteKind::Swap { frequency } => {
                    CurveQuote::Swap { years: q.years.to_f64(), rate: q.rate.to_f64(), frequency }
                }
            })
            .collect()
    }

    /// The curve these quotes reprice exactly.
    ///
    /// # Errors
    /// As [`Curve::bootstrap`].
    pub fn build(&self) -> Result<Curve> {
        Curve::bootstrap(&self.to_quotes())
    }
}

/// Checks a curve's history: non-empty, strictly increasing in time, every
/// snapshot bootstrappable.
///
/// # Errors
/// [`Error::InvalidArgument`] naming the curve and the problem.
pub fn validate_history(name: &str, history: &[CurveSnapshot]) -> Result<()> {
    let bad = |m: String| Err(Error::InvalidArgument(m));
    if name.trim().is_empty() {
        return bad("a curve needs a name".into());
    }
    if history.is_empty() {
        return bad(format!("curve {name} has no quotes"));
    }
    if history.windows(2).any(|w| w[1].as_of_ns <= w[0].as_of_ns) {
        return bad(format!("curve {name}: snapshots must be strictly increasing in time"));
    }
    for s in history {
        if let Err(e) = s.build() {
            return bad(format!("curve {name} at {}: {e}", s.as_of_ns));
        }
    }
    Ok(())
}

/// Every curve of a run, each snapshot bootstrapped once.
#[derive(Debug, Clone, Default)]
pub struct CurveHistory {
    by_name: BTreeMap<String, Vec<(i64, Curve)>>,
}

impl CurveHistory {
    /// Bootstraps every snapshot.
    ///
    /// # Errors
    /// As [`validate_history`].
    pub fn build(curves: &BTreeMap<String, Vec<CurveSnapshot>>) -> Result<Self> {
        let mut by_name = BTreeMap::new();
        for (name, history) in curves {
            validate_history(name, history)?;
            let built = history.iter().map(|s| Ok((s.as_of_ns, s.build()?))).collect::<Result<Vec<_>>>()?;
            by_name.insert(name.clone(), built);
        }
        Ok(Self { by_name })
    }

    /// The curve `name` as known at `ts_ns`: its latest snapshot at or before then.
    pub fn at(&self, name: &str, ts_ns: i64) -> Option<&Curve> {
        let h = self.by_name.get(name)?;
        let k = h.partition_point(|(t, _)| *t <= ts_ns);
        (k > 0).then(|| &h[k - 1].1)
    }

    /// True when there are no curves.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// A swap's mark at one time.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SwapMark {
    /// `100 + 100 * NPV` per 1 of notional, to the receiver of fixed.
    pub price: f64,
    /// NPV per 1 of notional, to the receiver of fixed.
    pub npv: f64,
    /// The fixed rate that would make the remaining swap worth zero.
    pub par_rate: f64,
    /// The current period's floating rate.
    pub fixing: f64,
    /// Price change (per 100 notional) for a one basis point parallel rise in zero rates.
    pub dv01: f64,
}

/// The terms the swap model needs.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapTerms {
    /// Maturity, nanoseconds since the epoch (the last payment date).
    pub maturity_ns: i64,
    /// Fixed rate as a fraction (may be negative).
    pub fixed_rate: f64,
    /// Payments a year on both legs: 1, 2, 4 or 12.
    pub frequency: u32,
    /// The curve the swap is marked on.
    pub curve: String,
}

impl SwapTerms {
    /// The terms of a swap record (`None` for anything else).
    pub fn from_instrument(inst: &Instrument) -> Option<Self> {
        if inst.kind != InstrumentType::Swap {
            return None;
        }
        let frequency = inst.coupon_frequency?;
        if !COUPON_FREQUENCIES.contains(&frequency) {
            return None;
        }
        Some(Self {
            maturity_ns: inst.maturity_ns?,
            fixed_rate: inst.coupon_rate?.to_f64(),
            frequency,
            curve: inst.underlying.clone()?,
        })
    }

    fn tau(&self) -> f64 {
        1.0 / f64::from(self.frequency)
    }

    /// The payment schedule (dates only; the coupon rate here is not used).
    fn schedule(&self) -> BondTerms {
        BondTerms {
            maturity_ns: self.maturity_ns,
            coupon_rate: 0.0,
            frequency: self.frequency,
            day_count: DayCount::ActActIcma,
        }
    }

    fn maturity_day(&self) -> i64 {
        self.maturity_ns.div_euclid(DAY_NS)
    }

    /// The start day of the period ending on `end_day`.
    fn start_of(&self, end_day: i64) -> Option<i64> {
        self.schedule().period_around(end_day * DAY_NS).map(|(s, _)| s)
    }

    /// The floating rate of the period `(start_day, end_day)`, fixed at its
    /// start from the curve known then; `None` when no curve was known.
    pub fn fixing(&self, curves: &CurveHistory, start_day: i64, end_day: i64) -> Option<f64> {
        let c = curves.at(&self.curve, start_day * DAY_NS)?;
        let t = (end_day - start_day) as f64 * DAY_NS as f64 / YEAR_NS;
        Some((1.0 / c.df(t) - 1.0) / self.tau())
    }

    /// Payment days strictly after `from_ns` and on or before `to_ns`.
    pub fn payment_days_between(&self, from_ns: i64, to_ns: i64) -> Vec<i64> {
        self.schedule().coupon_days_between(from_ns, to_ns)
    }

    /// The receiver's net coupon per 100 of notional for the period ending
    /// on `end_day`: `100 * (fixed - L) * tau`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when the period's fixing is not known.
    pub fn net_coupon(&self, curves: &CurveHistory, end_day: i64) -> Result<f64> {
        let fixing = self
            .start_of(end_day)
            .and_then(|start| self.fixing(curves, start, end_day))
            .ok_or_else(|| {
                Error::InvalidArgument(format!(
                    "swap on curve {}: no curve known at the start of the period ending on day {end_day}",
                    self.curve
                ))
            })?;
        Ok(100.0 * (self.fixed_rate - fixing) * self.tau())
    }

    /// The mark at `ts_ns`, or `None` before the current period's fixing or
    /// any curve is known, and after maturity. On the maturity date itself
    /// every payment has been made and the mark is 100.
    pub fn mark(&self, curves: &CurveHistory, ts_ns: i64) -> Option<SwapMark> {
        let day = ts_ns.div_euclid(DAY_NS);
        let mat = self.maturity_day();
        if day > mat {
            return None;
        }
        if day == mat {
            return Some(SwapMark { price: 100.0, ..SwapMark::default() });
        }
        let days = self.payment_days_between(ts_ns, self.maturity_ns);
        let &next = days.first()?;
        let start = self.start_of(next)?;
        let fixing = self.fixing(curves, start, next)?;
        let curve = curves.at(&self.curve, ts_ns)?;
        let tau = self.tau();
        let years = |d: i64| (d * DAY_NS - ts_ns) as f64 / YEAR_NS;
        let value = |c: &Curve| {
            let annuity: f64 = days.iter().map(|&d| tau * c.df(years(d))).sum();
            let float = (1.0 + fixing * tau) * c.df(years(next)) - c.df(years(mat));
            (self.fixed_rate * annuity - float, annuity, float)
        };
        let (npv, annuity, float) = value(curve);
        let up = value(&curve.shifted(0.0001)).0;
        let down = value(&curve.shifted(-0.0001)).0;
        Some(SwapMark {
            price: 100.0 + 100.0 * npv,
            npv,
            par_rate: if annuity > 0.0 { float / annuity } else { 0.0 },
            fixing,
            dv01: 100.0 * (up - down) / 2.0,
        })
    }
}

/// Bars for every swap in `setup`, one per distinct timestamp of `bars` at
/// which the swap has a mark (see [`SwapTerms::mark`]). Sorted by time.
///
/// # Errors
/// A curve history that does not validate; a mark that is not a positive price.
pub fn swap_bars(setup: &InstrumentSetup, bars: &[Bar]) -> Result<Vec<Bar>> {
    let swaps: Vec<(InstrumentId, SwapTerms)> = setup
        .instruments
        .iter()
        .filter_map(|(&id, i)| SwapTerms::from_instrument(i).map(|t| (id, t)))
        .collect();
    if swaps.is_empty() {
        return Ok(Vec::new());
    }
    let curves = CurveHistory::build(&setup.curves)?;
    let mut times: Vec<i64> = bars.iter().map(|b| b.ts_ns).collect();
    times.dedup();
    let mut out = Vec::new();
    for &ts in &times {
        for (id, terms) in &swaps {
            let Some(m) = terms.mark(&curves, ts) else { continue };
            let px = Price::from_f64(m.price)?;
            if px.raw() <= 0 {
                return Err(Error::InvalidArgument(format!(
                    "swap {}: its mark {:.6} at {ts} is not a positive price",
                    setup.instrument(*id).map_or("?", |i| i.symbol.as_str()),
                    m.price
                )));
            }
            out.push(Bar::new(ts, *id, px, px, px, px, Qty::ZERO));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: i64 = DAY_NS;

    fn snap(as_of_day: i64, rate: f64) -> CurveSnapshot {
        let r = Ratio::from_f64(rate).unwrap();
        CurveSnapshot {
            as_of_ns: as_of_day * D,
            quotes: vec![
                CurvePoint { kind: QuoteKind::Deposit, years: Ratio::from_f64(0.5).unwrap(), rate: r },
                CurvePoint {
                    kind: QuoteKind::Swap { frequency: 2 },
                    years: Ratio::from_units(2).unwrap(),
                    rate: r,
                },
                CurvePoint {
                    kind: QuoteKind::Swap { frequency: 2 },
                    years: Ratio::from_units(5).unwrap(),
                    rate: r,
                },
            ],
        }
    }

    fn history(snaps: Vec<CurveSnapshot>) -> CurveHistory {
        CurveHistory::build(&BTreeMap::from([("USD".to_string(), snaps)])).unwrap()
    }

    /// 2-year semiannual swap maturing on 1972-01-01 (day 730).
    fn terms(fixed: f64) -> SwapTerms {
        SwapTerms { maturity_ns: 730 * D, fixed_rate: fixed, frequency: 2, curve: "USD".into() }
    }

    #[test]
    fn point_in_time_curves_only() {
        let h = history(vec![snap(0, 0.04), snap(100, 0.05)]);
        assert!(h.at("USD", -1).is_none());
        let at = |t| h.at("USD", t).unwrap().zero_rate(1.0);
        assert!((at(99 * D) - at(0)).abs() < 1e-15);
        assert!(at(100 * D) > at(99 * D)); // the new snapshot only from its time
        assert!(h.at("EUR", 0).is_none());
    }

    #[test]
    fn a_swap_at_inception_near_its_par_rate_is_near_100() {
        let h = history(vec![snap(0, 0.04)]);
        let m = terms(0.04).mark(&h, 0).unwrap();
        // Calendar periods differ slightly from the curve's regular 0.5-year
        // grid, so par is close to, not exactly, the quoted rate.
        assert!((m.par_rate - 0.04).abs() < 2e-4, "{}", m.par_rate);
        assert!((m.price - 100.0).abs() < 0.05, "{}", m.price);
        let at_par = terms(m.par_rate).mark(&h, 0).unwrap();
        assert!((at_par.price - 100.0).abs() < 1e-9, "{}", at_par.price);
        // A receiver gains when rates fall: dv01 (per 100) is negative.
        assert!(m.dv01 < 0.0 && m.dv01 > -0.03, "{}", m.dv01);
    }

    #[test]
    fn a_receiver_gains_when_rates_fall() {
        let h = history(vec![snap(0, 0.04), snap(30, 0.03)]);
        let s = terms(0.04);
        let before = s.mark(&h, 29 * D).unwrap();
        let after = s.mark(&h, 30 * D).unwrap();
        assert!(after.price > before.price + 1.0, "{} -> {}", before.price, after.price);
        // The first period's fixing stays what it was at its start.
        assert!((after.fixing - before.fixing).abs() < 1e-15);
    }

    #[test]
    fn net_coupons_and_the_mark_drop_on_payment_dates() {
        let h = history(vec![snap(0, 0.04)]);
        let s = terms(0.05);
        let days = s.payment_days_between(0, s.maturity_ns);
        assert_eq!(days, vec![181, 365, 546, 730]); // Jul 1, Jan 1, Jul 1, Jan 1
        let c = s.net_coupon(&h, 181).unwrap();
        let fixing = s.fixing(&h, 0, 181).unwrap();
        assert!((c - 100.0 * (0.05 - fixing) * 0.5).abs() < 1e-12);
        // Across a payment the mark drops by about the coupon paid.
        let before = s.mark(&h, 180 * D).unwrap().price;
        let on = s.mark(&h, 181 * D).unwrap().price;
        assert!((before - on - c).abs() < 0.01, "{before} {on} {c}");
        assert_eq!(s.mark(&h, 730 * D).unwrap().price, 100.0);
        assert!(s.mark(&h, 731 * D).is_none());
    }

    #[test]
    fn no_mark_before_the_fixing_is_known() {
        // The period running Jul 1 1969 - Jan 1 1970 fixed before the first curve.
        let s = SwapTerms { maturity_ns: 365 * D, fixed_rate: 0.04, frequency: 1, curve: "USD".into() };
        let h = history(vec![snap(10, 0.04)]);
        assert!(s.mark(&h, 20 * D).is_none()); // its period started on day 0, before day 10
        assert!(s.net_coupon(&h, 365).is_err());
    }

    #[test]
    fn histories_are_checked() {
        let one = snap(5, 0.04);
        assert!(validate_history("USD", &[]).is_err());
        assert!(validate_history("USD", &[one.clone(), one.clone()]).is_err());
        assert!(validate_history(" ", &[one]).is_err());
    }
}
