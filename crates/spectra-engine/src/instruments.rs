//! What a run knows about its instruments (M24): their records, the run's
//! base currency, and point-in-time FX rates.
//!
//! The ledger books everything in the base currency. An instrument quoted in
//! another currency is converted when it fills and when it is marked, at the
//! latest rate known at that timestamp (never a later one). Foreign proceeds
//! are treated as converted at once, so there are no foreign cash balances;
//! FX moves therefore show up in the P&L of the positions that carry them.
//! An instrument whose currency has no rate series is refused, never
//! converted 1:1 (decision 2).
//!
//! A setup with nothing in it, or only plain equities in the base currency
//! (see [`Instrument::is_plain_equity`]), changes nothing: the run, its
//! journal and its fingerprints are exactly those of a run without one.

use std::collections::BTreeMap;

use crate::bonds::BondTerms;
use crate::futures::RollRule;
use spectra_core::fixed::{div_round_half_away, narrow};
use spectra_core::{
    is_currency_code, sha256_hex, Error, Instrument, InstrumentId, InstrumentType, Price, Result,
    DEFAULT_BASE_CURRENCY,
};

/// Fixed-point scale of FX rates: 12 decimals, so a rate such as JPY in USD
/// (about 0.0066) keeps its full precision.
pub const FX_SCALE: i128 = 1_000_000_000_000;

/// One FX observation: from `as_of_ns` on, one unit of the series' currency
/// is worth `rate / FX_SCALE` units of the base currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FxRate {
    /// When this rate became known, nanoseconds since the Unix epoch.
    pub as_of_ns: i64,
    /// Base-currency units per unit of the foreign currency, scaled by [`FX_SCALE`].
    pub rate: i128,
}

impl FxRate {
    /// A rate from a decimal string such as `"1.0845"`, exactly.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for anything but a positive decimal with at most 12 fractional digits.
    pub fn parse_rate(text: &str) -> Result<i128> {
        let bad = || Error::InvalidArgument(format!("FX rate \"{text}\" must be a positive decimal number"));
        let t = text.trim();
        let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
        if whole.is_empty() && frac.is_empty()
            || !whole.chars().all(|c| c.is_ascii_digit())
            || !frac.chars().all(|c| c.is_ascii_digit())
            || frac.len() > 12
        {
            return Err(bad());
        }
        let w: i128 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| bad())? };
        let mut f: i128 = if frac.is_empty() { 0 } else { frac.parse().map_err(|_| bad())? };
        for _ in frac.len()..12 {
            f *= 10;
        }
        let rate = w.checked_mul(FX_SCALE).and_then(|v| v.checked_add(f)).ok_or_else(bad)?;
        if rate <= 0 {
            return Err(bad());
        }
        Ok(rate)
    }

    /// A rate from a JSON number, rounded to 12 decimals.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] unless finite and positive.
    pub fn rate_from_f64(v: f64) -> Result<i128> {
        if !v.is_finite() || v <= 0.0 || v > 1e12 {
            return Err(Error::InvalidArgument(format!("FX rate {v} must be a positive finite number")));
        }
        let r = (v * FX_SCALE as f64).round() as i128;
        if r <= 0 {
            return Err(Error::InvalidArgument(format!("FX rate {v} is too small")));
        }
        Ok(r)
    }

    /// The rate as a decimal string with 12 fractional digits.
    pub fn rate_text(rate: i128) -> String {
        format!("{}.{:012}", rate / FX_SCALE, rate % FX_SCALE)
    }
}

/// Instruments, base currency and FX rates for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentSetup {
    /// The currency the books are kept in.
    pub base_currency: String,
    /// Records by instrument id. Ids without a record are plain equities in the base currency.
    pub instruments: BTreeMap<InstrumentId, Instrument>,
    /// Rate series by currency code, each strictly increasing in time.
    pub fx: BTreeMap<String, Vec<FxRate>>,
    /// When futures roll and how continuous series are adjusted (decision 3).
    pub roll: RollRule,
    /// Whether bond bars are clean or dirty prices (M24.3 task 12).
    pub bond_quote: BondQuote,
}

/// How bond prices in the bars are quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BondQuote {
    /// Without accrued interest, the market convention: the engine adds the
    /// accrued interest to every fill and mark.
    #[default]
    Clean,
    /// Accrued interest included: fills and marks use the price as it is.
    Dirty,
}

impl BondQuote {
    /// Name used in requests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
        }
    }

    /// The convention called `name`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "clean" => Some(Self::Clean),
            "dirty" => Some(Self::Dirty),
            _ => None,
        }
    }
}

impl Default for InstrumentSetup {
    fn default() -> Self {
        Self {
            base_currency: DEFAULT_BASE_CURRENCY.into(),
            roll: RollRule::default(),
            bond_quote: BondQuote::default(),
            instruments: BTreeMap::new(),
            fx: BTreeMap::new(),
        }
    }
}

impl InstrumentSetup {
    /// The record of `id`, if the run has one.
    pub fn instrument(&self, id: InstrumentId) -> Option<&Instrument> {
        self.instruments.get(&id)
    }

    /// The coupon terms of `id`, when it is a bond.
    pub fn bond_terms(&self, id: InstrumentId) -> Option<BondTerms> {
        self.instrument(id).and_then(BondTerms::from_instrument)
    }

    /// The type of `id` (equity when it has no record).
    pub fn kind(&self, id: InstrumentId) -> InstrumentType {
        self.instrument(id).map_or(InstrumentType::Equity, |i| i.kind)
    }

    /// Instruments whose record changes how the run behaves.
    pub fn effective(&self) -> impl Iterator<Item = (&InstrumentId, &Instrument)> {
        self.instruments.iter().filter(|(_, i)| !i.is_plain_equity(&self.base_currency))
    }

    /// True when the setup changes nothing about a run.
    pub fn is_trivial(&self) -> bool {
        self.effective().next().is_none()
    }

    /// Currencies other than the base that some instrument is quoted in.
    pub fn foreign_currencies(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .instruments
            .values()
            .map(|i| i.currency.as_str())
            .filter(|c| *c != self.base_currency)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Checks every record and every rate series, and that every foreign
    /// currency in use has a series.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] naming the first problem.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Error::InvalidArgument(m);
        self.roll.validate()?;
        if !is_currency_code(&self.base_currency) {
            return Err(bad(format!(
                "base currency \"{}\" must be a three-letter ISO code",
                self.base_currency
            )));
        }
        for (id, inst) in &self.instruments {
            inst.validate().map_err(|e| bad(format!("instrument {} ({id}): {e}", inst.symbol)))?;
        }
        for (ccy, series) in &self.fx {
            if !is_currency_code(ccy) {
                return Err(bad(format!("FX currency \"{ccy}\" must be a three-letter ISO code")));
            }
            if *ccy == self.base_currency {
                return Err(bad(format!("FX rates for the base currency {ccy} are not needed")));
            }
            if series.is_empty() {
                return Err(bad(format!("FX series {ccy} has no rates")));
            }
            for w in series.windows(2) {
                if w[1].as_of_ns <= w[0].as_of_ns {
                    return Err(bad(format!("FX series {ccy} must be strictly increasing in time")));
                }
            }
            if series.iter().any(|r| r.rate <= 0) {
                return Err(bad(format!("FX series {ccy} has a non-positive rate")));
            }
        }
        for ccy in self.foreign_currencies() {
            if !self.fx.contains_key(ccy) {
                return Err(bad(format!(
                    "no FX rates from {ccy} to {}: instruments in {ccy} cannot be converted",
                    self.base_currency
                )));
            }
        }
        Ok(())
    }

    /// The rate in force for `id`'s currency at `ts_ns`, or `None` when it is
    /// quoted in the base currency (no conversion).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] when no rate was known yet at `ts_ns`.
    pub fn fx_rate(&self, id: InstrumentId, ts_ns: i64) -> Result<Option<i128>> {
        let Some(inst) = self.instrument(id) else { return Ok(None) };
        if inst.currency == self.base_currency {
            return Ok(None);
        }
        let series = self.fx.get(&inst.currency).ok_or_else(|| {
            Error::InvalidArgument(format!("no FX rates from {} to {}", inst.currency, self.base_currency))
        })?;
        let k = series.partition_point(|r| r.as_of_ns <= ts_ns);
        if k == 0 {
            return Err(Error::InvalidArgument(format!(
                "no {}→{} rate known at or before {ts_ns} (instrument {})",
                inst.currency, self.base_currency, inst.symbol
            )));
        }
        Ok(Some(series[k - 1].rate))
    }

    /// The price the books use for quoted `price` of `id` at `ts_ns`: a bond
    /// quoted clean gets its accrued interest added (the dirty price, which is
    /// what changes hands), then anything not in the base currency is converted.
    /// Unchanged for an instrument in the base currency that is not a bond.
    ///
    /// # Errors
    /// As [`Self::fx_rate`]; overflow; a conversion that rounds to zero.
    pub fn to_base(&self, id: InstrumentId, price: Price, ts_ns: i64) -> Result<Price> {
        let price = match (self.bond_quote, self.bond_terms(id)) {
            (BondQuote::Clean, Some(terms)) => price.checked_add(Price::from_f64(terms.accrued(ts_ns))?)?,
            _ => price,
        };
        self.fx_convert(id, price, ts_ns)
    }

    /// `price` of `id` converted to the base currency at the rate known at
    /// `ts_ns`, rounded half away from zero; unchanged in the base currency.
    ///
    /// # Errors
    /// As [`Self::fx_rate`]; overflow; a conversion that rounds to zero.
    pub fn fx_convert(&self, id: InstrumentId, price: Price, ts_ns: i64) -> Result<Price> {
        let Some(rate) = self.fx_rate(id, ts_ns)? else { return Ok(price) };
        let raw = narrow(div_round_half_away(i128::from(price.raw()) * rate, FX_SCALE))?;
        if raw <= 0 && price.raw() > 0 {
            return Err(Error::InvalidArgument(format!(
                "price {price} of instrument {id} rounds to zero in {}",
                self.base_currency
            )));
        }
        Ok(Price::from_raw(raw))
    }

    /// Canonical text of everything that can change a run: the base currency,
    /// every effective record, and the rate series of the currencies in use.
    pub fn canonical(&self) -> String {
        let mut s = format!("instruments-v1;base={};", self.base_currency);
        for (id, i) in self.effective() {
            s.push_str(&format!(
                "inst={id},{},{},{},{},{},{},{},{},{},{};",
                i.symbol,
                i.kind.as_str(),
                i.currency,
                i.multiplier.raw(),
                i.tick_size.raw(),
                i.lot_size.raw(),
                opt_i64(i.expiry_ns),
                opt_i64(i.strike.map(Price::raw)),
                opt_i64(i.maturity_ns),
                opt_i64(i.coupon_rate.map(spectra_core::Ratio::raw)),
            ));
            if let Some(u) = &i.underlying {
                s.push_str(&format!("underlying={u};"));
            }
            if let (Some(f), Some(d)) = (i.coupon_frequency, i.day_count) {
                s.push_str(&format!("coupons={f},{};", d.as_str()));
            }
        }
        // The roll rule changes only runs that have futures.
        if self.effective().any(|(_, i)| i.kind == InstrumentType::Future) {
            s.push_str(&self.roll.canonical());
        }
        if self.effective().any(|(_, i)| i.kind == InstrumentType::Bond) {
            s.push_str(&format!("bond_quote={};", self.bond_quote.as_str()));
        }
        for ccy in self.foreign_currencies() {
            if let Some(series) = self.fx.get(ccy) {
                s.push_str(&format!("fx={ccy},{};", series.len()));
                for r in series {
                    s.push_str(&format!("{}:{};", r.as_of_ns, r.rate));
                }
            }
        }
        s
    }

    /// SHA-256 of [`Self::canonical`].
    pub fn fingerprint(&self) -> String {
        sha256_hex(self.canonical().as_bytes())
    }
}

fn opt_i64(v: Option<i64>) -> String {
    v.map_or_else(|| "-".to_string(), |x| x.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::{Qty, Ratio, SCALE};

    fn eur_stock() -> Instrument {
        Instrument::equity("SAP", "EUR", Price::from_raw(SCALE / 100), Qty::ONE)
    }

    #[test]
    fn parses_rates_exactly() {
        assert_eq!(FxRate::parse_rate("1.0845").unwrap(), 1_084_500_000_000);
        assert_eq!(FxRate::parse_rate("0.006634127891").unwrap(), 6_634_127_891);
        assert_eq!(FxRate::parse_rate("2").unwrap(), 2 * FX_SCALE);
        assert!(FxRate::parse_rate("0").is_err());
        assert!(FxRate::parse_rate("-1.2").is_err());
        assert!(FxRate::parse_rate("1.0000000000001").is_err());
        assert!(FxRate::parse_rate("abc").is_err());
        assert_eq!(FxRate::rate_text(1_084_500_000_000), "1.084500000000");
    }

    #[test]
    fn plain_equities_are_trivial() {
        let mut s = InstrumentSetup::default();
        s.instruments.insert(0, Instrument::equity("AAPL", "USD", Price::from_raw(1), Qty::ONE));
        assert!(s.is_trivial());
        assert_eq!(s.validate(), Ok(()));
        s.instruments.insert(1, eur_stock());
        assert!(!s.is_trivial());
    }

    #[test]
    fn refuses_foreign_currency_without_rates() {
        let mut s = InstrumentSetup::default();
        s.instruments.insert(1, eur_stock());
        assert!(s.validate().unwrap_err().to_string().contains("no FX rates from EUR"));
    }

    #[test]
    fn converts_point_in_time_only() {
        let mut s = InstrumentSetup::default();
        s.instruments.insert(1, eur_stock());
        s.fx.insert(
            "EUR".into(),
            vec![
                FxRate { as_of_ns: 100, rate: 1_100_000_000_000 },
                FxRate { as_of_ns: 200, rate: 1_200_000_000_000 },
            ],
        );
        s.validate().unwrap();
        let px = Price::from_raw(100 * SCALE);
        assert!(s.to_base(1, px, 99).is_err()); // nothing known yet: refused, not guessed
        assert_eq!(s.to_base(1, px, 100).unwrap(), Price::from_raw(110 * SCALE));
        assert_eq!(s.to_base(1, px, 199).unwrap(), Price::from_raw(110 * SCALE)); // no look-ahead
        assert_eq!(s.to_base(1, px, 200).unwrap(), Price::from_raw(120 * SCALE));
        assert_eq!(s.to_base(7, px, 0).unwrap(), px); // no record: base currency
    }

    #[test]
    fn fingerprint_ignores_plain_equities_and_unused_fx() {
        let base = InstrumentSetup::default();
        let mut with_plain = base.clone();
        with_plain.instruments.insert(0, Instrument::equity("AAPL", "USD", Price::from_raw(1), Qty::ONE));
        assert_eq!(base.fingerprint(), with_plain.fingerprint());
        let mut fut = with_plain.clone();
        let mut f = Instrument::equity("ESZ6", "USD", Price::from_raw(SCALE / 4), Qty::ONE);
        f.kind = InstrumentType::Future;
        f.multiplier = Ratio::from_units(50).unwrap();
        f.expiry_ns = Some(1);
        fut.instruments.insert(1, f);
        assert_ne!(base.fingerprint(), fut.fingerprint());
    }
}
