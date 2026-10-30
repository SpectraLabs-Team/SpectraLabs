//! Futures chains, the roll rule and continuous series (M24.2, decision 3).
//!
//! Contracts that share an `underlying` form a chain, ordered by expiry. A
//! contract stops being the chain's *front* on its roll date: the start of
//! the day `business_days_before_expiry` business days before its expiry
//! (trading days of the run's exchange when it has one, else Monday to
//! Friday). The run records the rule.
//!
//! P&L is always booked on the real contracts. At the first timestamp on or
//! after the roll date, the engine closes any position in the contract at the
//! bar's open and opens the same position in the next contract at its open,
//! both through the run's cost model; each roll is recorded on the run. A
//! position still open at expiry (no price to roll at) is settled at its last
//! mark.
//!
//! Strategies compute signals on a *continuous* series per chain: the front
//! contract's bars, back-adjusted at each roll that has already happened
//! (never a later one), so a roll does not look like a price jump.

use std::collections::BTreeMap;

use spectra_core::fixed::{div_round_half_away, narrow};
use spectra_core::{Bar, Error, InstrumentId, InstrumentType, Price, Result};
use spectra_data::calendar::Exchange;

use crate::instruments::InstrumentSetup;

/// Default roll point: five business days before expiry. A run may set its own.
pub const DEFAULT_ROLL_BUSINESS_DAYS: u32 = 5;

/// Largest roll offset a run may ask for.
pub const MAX_ROLL_BUSINESS_DAYS: u32 = 60;

const DAY_NS: i64 = 86_400_000_000_000;

/// How earlier bars of a continuous series are adjusted at a roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RollAdjust {
    /// Multiply earlier prices by `new / old` at the roll: keeps percentage
    /// returns and keeps every price positive.
    #[default]
    Ratio,
    /// Add `new - old` to earlier prices: keeps point moves. Refused when it
    /// would make a price zero or negative.
    Difference,
}

impl RollAdjust {
    /// Name used in requests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ratio => "ratio",
            Self::Difference => "difference",
        }
    }

    /// The method called `name`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ratio" => Some(Self::Ratio),
            "difference" => Some(Self::Difference),
            _ => None,
        }
    }
}

/// When positions roll, and how the continuous series is adjusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollRule {
    /// Business days before expiry at which a contract rolls.
    pub business_days_before_expiry: u32,
    /// Continuous-series adjustment.
    pub adjust: RollAdjust,
}

impl Default for RollRule {
    fn default() -> Self {
        Self { business_days_before_expiry: DEFAULT_ROLL_BUSINESS_DAYS, adjust: RollAdjust::Ratio }
    }
}

impl RollRule {
    /// Canonical text, for the run's fingerprint.
    pub fn canonical(&self) -> String {
        format!("roll={},{};", self.business_days_before_expiry, self.adjust.as_str())
    }

    /// Checks the offset.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when it is above [`MAX_ROLL_BUSINESS_DAYS`].
    pub fn validate(&self) -> Result<()> {
        if self.business_days_before_expiry > MAX_ROLL_BUSINESS_DAYS {
            return Err(Error::InvalidArgument(format!(
                "roll business_days_before_expiry must be at most {MAX_ROLL_BUSINESS_DAYS}"
            )));
        }
        Ok(())
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

fn is_business_day(day: i64, exchange: Option<Exchange>) -> bool {
    match exchange {
        Some(ex) => {
            let (y, m, d) = civil_from_days(day);
            ex.is_trading_day(y, m, d)
        }
        // 1970-01-01 was a Thursday: day 0 -> weekday index 3 (Mon = 0).
        None => (day + 3).rem_euclid(7) < 5,
    }
}

/// Start (00:00 UTC) of the day `n` business days before the day of `expiry_ns`.
pub fn roll_ts(expiry_ns: i64, n: u32, exchange: Option<Exchange>) -> i64 {
    let mut day = expiry_ns.div_euclid(DAY_NS);
    let mut left = n;
    // Bounded: at most a few weeks of holidays between business days.
    let mut guard = 0;
    while left > 0 && guard < 400 {
        day -= 1;
        guard += 1;
        if is_business_day(day, exchange) {
            left -= 1;
        }
    }
    day * DAY_NS
}

/// The futures chains of a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chains {
    /// Contracts of each chain, by expiry.
    by_chain: BTreeMap<String, Vec<(i64, InstrumentId)>>,
    /// Chain of each contract.
    chain_of: BTreeMap<InstrumentId, String>,
    /// Roll timestamp of each contract.
    roll_at: BTreeMap<InstrumentId, i64>,
    /// Expiry of each contract.
    expiry: BTreeMap<InstrumentId, i64>,
}

impl Chains {
    /// The chains of `setup`'s futures. A future without an underlying is a
    /// chain of its own (named by its symbol).
    pub fn new(setup: &InstrumentSetup, rule: &RollRule, exchange: Option<Exchange>) -> Self {
        let mut c = Self::default();
        for (&id, inst) in &setup.instruments {
            if inst.kind != InstrumentType::Future {
                continue;
            }
            let Some(expiry) = inst.expiry_ns else { continue };
            let chain = inst.underlying.clone().unwrap_or_else(|| inst.symbol.clone());
            c.by_chain.entry(chain.clone()).or_default().push((expiry, id));
            c.chain_of.insert(id, chain);
            c.roll_at.insert(id, roll_ts(expiry, rule.business_days_before_expiry, exchange));
            c.expiry.insert(id, expiry);
        }
        for list in c.by_chain.values_mut() {
            list.sort_unstable();
        }
        c
    }

    /// True when the run has no futures.
    pub fn is_empty(&self) -> bool {
        self.by_chain.is_empty()
    }

    /// Chain names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_chain.keys().map(String::as_str)
    }

    /// The chain `id` belongs to, if it is a future.
    pub fn chain(&self, id: InstrumentId) -> Option<&str> {
        self.chain_of.get(&id).map(String::as_str)
    }

    /// When `id` rolls, if it is a future.
    pub fn roll_ts(&self, id: InstrumentId) -> Option<i64> {
        self.roll_at.get(&id).copied()
    }

    /// When `id` expires, if it is a future.
    pub fn expiry(&self, id: InstrumentId) -> Option<i64> {
        self.expiry.get(&id).copied()
    }

    /// Every contract with its roll timestamp.
    pub fn contracts(&self) -> impl Iterator<Item = (InstrumentId, i64)> + '_ {
        self.roll_at.iter().map(|(&id, &t)| (id, t))
    }

    /// The front contract of `chain` at `ts`: the earliest one not yet rolled.
    pub fn front(&self, chain: &str, ts: i64) -> Option<InstrumentId> {
        self.by_chain
            .get(chain)?
            .iter()
            .map(|&(_, id)| id)
            .find(|id| self.roll_at.get(id).is_some_and(|&r| r > ts))
    }

    /// The contract after `id` in its chain.
    pub fn next_after(&self, id: InstrumentId) -> Option<InstrumentId> {
        let list = self.by_chain.get(self.chain_of.get(&id)?)?;
        let k = list.iter().position(|&(_, x)| x == id)?;
        list.get(k + 1).map(|&(_, x)| x)
    }

    /// True unless `id` is a future that is not its chain's front at `ts`.
    pub fn is_front(&self, id: InstrumentId, ts: i64) -> bool {
        match self.chain_of.get(&id) {
            None => true,
            Some(chain) => self.front(chain, ts) == Some(id),
        }
    }
}

/// Continuous, back-adjusted series per chain, built as the run goes.
#[derive(Debug, Clone, Default)]
pub struct Continuous {
    series: BTreeMap<String, Vec<Bar>>,
    front: BTreeMap<String, InstrumentId>,
    rolls: BTreeMap<String, Vec<(i64, InstrumentId, InstrumentId)>>,
}

fn adjust_price(px: Price, adjust: RollAdjust, new_ref: Price, old_ref: Price) -> Result<Price> {
    let raw = match adjust {
        RollAdjust::Ratio => narrow(div_round_half_away(
            i128::from(px.raw()) * i128::from(new_ref.raw()),
            i128::from(old_ref.raw()),
        ))?,
        RollAdjust::Difference => {
            px.raw().checked_add(new_ref.raw() - old_ref.raw()).ok_or(Error::Overflow)?
        }
    };
    if raw <= 0 {
        return Err(Error::InvalidArgument(
            "difference roll adjustment made a continuous price zero or negative; use ratio adjustment"
                .into(),
        ));
    }
    Ok(Price::from_raw(raw))
}

impl Continuous {
    /// Appends the front contract's bar at `ts` to each chain, adjusting the
    /// earlier bars when the front changed. `history` already holds the bars
    /// of `ts`.
    ///
    /// # Errors
    /// Overflow, or a difference adjustment that makes a price non-positive.
    pub fn update(
        &mut self,
        chains: &Chains,
        ts: i64,
        group: &[Bar],
        history: &BTreeMap<InstrumentId, Vec<Bar>>,
        adjust: RollAdjust,
    ) -> Result<()> {
        for chain in chains.names() {
            let Some(front) = chains.front(chain, ts) else { continue };
            let Some(bar) = group.iter().find(|b| b.instrument == front) else { continue };
            if let Some(&old) = self.front.get(chain) {
                if old != front {
                    // Same-time reference prices where the old contract has a bar now, else its last close.
                    let old_ref = history.get(&old).and_then(|h| h.last()).map(|b| b.close);
                    if let Some(old_ref) = old_ref.filter(|p| p.raw() > 0) {
                        let new_ref = bar.close;
                        for b in self.series.entry(chain.to_string()).or_default().iter_mut() {
                            b.open = adjust_price(b.open, adjust, new_ref, old_ref)?;
                            b.high = adjust_price(b.high, adjust, new_ref, old_ref)?;
                            b.low = adjust_price(b.low, adjust, new_ref, old_ref)?;
                            b.close = adjust_price(b.close, adjust, new_ref, old_ref)?;
                        }
                    }
                    self.rolls.entry(chain.to_string()).or_default().push((ts, old, front));
                }
            }
            self.front.insert(chain.to_string(), front);
            self.series.entry(chain.to_string()).or_default().push(*bar);
        }
        Ok(())
    }

    /// The continuous series of `chain` so far (empty if unknown).
    pub fn series(&self, chain: &str) -> &[Bar] {
        self.series.get(chain).map_or(&[], Vec::as_slice)
    }

    /// Every chain's series.
    pub fn all(&self) -> &BTreeMap<String, Vec<Bar>> {
        &self.series
    }

    /// Where each chain's front changed: `(ts, from, to)`.
    pub fn switches(&self) -> &BTreeMap<String, Vec<(i64, InstrumentId, InstrumentId)>> {
        &self.rolls
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::{Instrument, Qty, SCALE};

    fn ymd(y: i64, m: u32, d: u32) -> i64 {
        // days_from_civil
        let y2 = if m <= 2 { y - 1 } else { y };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let mp = i64::from((m + 9) % 12);
        let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        (era * 146_097 + doe - 719_468) * DAY_NS
    }

    #[test]
    fn civil_round_trip() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 12, 18), (1969, 12, 31)] {
            assert_eq!(civil_from_days(ymd(y, m, d) / DAY_NS), (y, m, d));
        }
    }

    #[test]
    fn rolls_five_weekdays_before_expiry() {
        // Friday 2026-12-18: five weekdays back is Friday 2026-12-11.
        assert_eq!(roll_ts(ymd(2026, 12, 18) + 3_600_000_000_000, 5, None), ymd(2026, 12, 11));
        // Monday 2026-12-14: back over a weekend to Monday 2026-12-07.
        assert_eq!(roll_ts(ymd(2026, 12, 14), 5, None), ymd(2026, 12, 7));
        assert_eq!(roll_ts(ymd(2026, 12, 14), 0, None), ymd(2026, 12, 14));
    }

    #[test]
    fn exchange_calendar_skips_holidays() {
        // XNYS is closed on Thursday 2026-11-26 (Thanksgiving): two business days before
        // Friday 2026-11-27 is Tuesday 2026-11-24, not Wednesday.
        let ex = Exchange::parse("XNYS").expect("XNYS calendar");
        assert_eq!(roll_ts(ymd(2026, 11, 27), 2, Some(ex)), ymd(2026, 11, 24));
    }

    fn fut(sym: &str, under: &str, expiry: i64) -> Instrument {
        let mut f = Instrument::equity(sym, "USD", Price::from_raw(SCALE / 4), Qty::ONE);
        f.kind = InstrumentType::Future;
        f.multiplier = spectra_core::Ratio::from_units(50).unwrap();
        f.expiry_ns = Some(expiry);
        f.underlying = Some(under.into());
        f
    }

    #[test]
    fn front_and_next() {
        let mut s = InstrumentSetup::default();
        s.instruments.insert(1, fut("ESH7", "ES", ymd(2027, 3, 19)));
        s.instruments.insert(0, fut("ESZ6", "ES", ymd(2026, 12, 18)));
        let c = Chains::new(&s, &RollRule::default(), None);
        assert_eq!(c.front("ES", ymd(2026, 12, 10)), Some(0));
        assert_eq!(c.front("ES", ymd(2026, 12, 11)), Some(1)); // rolled on the 11th
        assert_eq!(c.next_after(0), Some(1));
        assert_eq!(c.next_after(1), None);
        assert!(c.is_front(7, 0)); // not a future
        assert!(!c.is_front(1, ymd(2026, 12, 1)));
    }

    #[test]
    fn ratio_adjustment_keeps_returns() {
        let mut s = InstrumentSetup::default();
        s.instruments.insert(0, fut("A", "X", 3 * DAY_NS + 1));
        s.instruments.insert(1, fut("B", "X", 100 * DAY_NS));
        let rule = RollRule { business_days_before_expiry: 0, adjust: RollAdjust::Ratio };
        let c = Chains::new(&s, &rule, None);
        let px = |v: i64| Price::from_raw(v * SCALE);
        let bar =
            |ts: i64, id, close: i64| Bar::new(ts, id, px(close), px(close), px(close), px(close), Qty::ONE);
        let mut hist: BTreeMap<InstrumentId, Vec<Bar>> = BTreeMap::new();
        let mut cont = Continuous::default();
        for (ts, a, b) in
            [(DAY_NS, 100, 110), (2 * DAY_NS, 102, 112), (3 * DAY_NS, 104, 114), (4 * DAY_NS, 105, 115)]
        {
            let g = vec![bar(ts, 0, a), bar(ts, 1, b)];
            for x in &g {
                hist.entry(x.instrument).or_default().push(*x);
            }
            cont.update(&c, ts, &g, &hist, rule.adjust).unwrap();
        }
        let s = cont.series("X");
        assert_eq!(s.len(), 4);
        // A rolled at day 3 (expiry day, offset 0): the earlier bars are scaled by 114 / 104.
        assert_eq!(s[2].close, px(114));
        assert_eq!(
            s[1].close,
            Price::from_raw(div_round_half_away(102 * i128::from(SCALE) * 114, 104) as i64)
        );
        assert_eq!(cont.switches()["X"], vec![(3 * DAY_NS, 0, 1)]);
    }
}
