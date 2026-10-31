//! The instrument record: one shape for every tradable type (M24.1).
//!
//! Equities are the default and behave exactly as they did before this model
//! existed: multiplier 1, no expiry, no strike, no coupon. Other types carry
//! their own nullable fields, and each type validates its own (decision 1).
//!
//! An instrument may be quoted in any currency. A run converts it to the
//! run's base currency with point-in-time FX rates, and refuses it when no
//! rate series exists (decision 2): never a silent 1:1 conversion.

use std::fmt;

use crate::fixed::{Price, Qty, Ratio};

/// What kind of contract an instrument is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InstrumentType {
    /// A cash equity.
    #[default]
    Equity,
    /// A futures contract (needs an expiry).
    Future,
    /// A bond (needs a maturity and a coupon rate).
    Bond,
    /// An option (needs an expiry and a strike). Deferred past stage 2.
    Option,
    /// A fixed-for-floating interest-rate swap, marked from a dated curve history.
    Swap,
}

impl InstrumentType {
    /// Name used in storage and the API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equity => "equity",
            Self::Future => "future",
            Self::Bond => "bond",
            Self::Option => "option",
            Self::Swap => "swap",
        }
    }

    /// The type called `name`, if any.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Equity, Self::Future, Self::Bond, Self::Option, Self::Swap]
            .into_iter()
            .find(|t| t.as_str() == name)
    }

    /// The broker asset class for this type, where the broker has one.
    /// Alpaca's name for cash equities is `us_equity`.
    pub const fn broker_asset_class(self) -> Option<&'static str> {
        match self {
            Self::Equity => Some("us_equity"),
            Self::Future => Some("us_futures"),
            Self::Option => Some("us_option"),
            Self::Bond | Self::Swap => None,
        }
    }
}

impl fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One tradable instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    /// Symbol, unique within a workspace.
    pub symbol: String,
    /// Contract type.
    pub kind: InstrumentType,
    /// ISO 4217 code of the currency the price is quoted in.
    pub currency: String,
    /// Currency units per price unit per contract (1 for equities; 50 for an E-mini style future).
    pub multiplier: Ratio,
    /// Smallest price increment; must be positive.
    pub tick_size: Price,
    /// Smallest tradable quantity; must be positive.
    pub lot_size: Qty,
    /// Expiry (futures and options), nanoseconds since the Unix epoch.
    pub expiry_ns: Option<i64>,
    /// Strike (options).
    pub strike: Option<Price>,
    /// Maturity (bonds), nanoseconds since the Unix epoch.
    pub maturity_ns: Option<i64>,
    /// Annual coupon rate as a fraction (bonds): 0.05 is 5%.
    pub coupon_rate: Option<Ratio>,
    /// What the contract is on: a future's chain (contracts with the same
    /// underlying roll into each other, e.g. `ES`), or an option's underlying symbol.
    pub underlying: Option<String>,
    /// Coupons a year (bonds): 1, 2, 4 or 12.
    pub coupon_frequency: Option<u32>,
    /// How accrued interest counts days (bonds).
    pub day_count: Option<DayCount>,
    /// Call or put (options). Options are European and cash-settled at expiry.
    pub option_right: Option<OptionRight>,
}

/// Call or put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionRight {
    /// Right to buy the underlying at the strike.
    Call,
    /// Right to sell the underlying at the strike.
    Put,
}

impl OptionRight {
    /// Name used in requests and storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Put => "put",
        }
    }

    /// The right called `name`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "call" => Some(Self::Call),
            "put" => Some(Self::Put),
            _ => None,
        }
    }

    /// Value at expiry for an underlying price `spot` and strike `strike`.
    pub fn intrinsic(self, spot: f64, strike: f64) -> f64 {
        match self {
            Self::Call => (spot - strike).max(0.0),
            Self::Put => (strike - spot).max(0.0),
        }
    }
}

/// How a fraction of a year is counted between two dates (bond accrued interest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DayCount {
    /// Actual/Actual (ICMA): every full period pays exactly `rate / frequency`. Government bonds.
    ActActIcma,
    /// 30/360 (US bond basis). Corporate and agency bonds.
    Thirty360,
    /// Actual/360. Money-market instruments.
    Act360,
    /// Actual/365 fixed.
    Act365F,
}

impl DayCount {
    /// Every convention.
    pub const ALL: [Self; 4] = [Self::ActActIcma, Self::Thirty360, Self::Act360, Self::Act365F];

    /// Name used in requests and storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ActActIcma => "act_act_icma",
            Self::Thirty360 => "30_360",
            Self::Act360 => "act_360",
            Self::Act365F => "act_365f",
        }
    }

    /// The convention called `name`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == name)
    }
}

/// Coupon frequencies a bond may have.
pub const COUPON_FREQUENCIES: [u32; 4] = [1, 2, 4, 12];

/// The base currency of a workspace that has not chosen one.
pub const DEFAULT_BASE_CURRENCY: &str = "USD";

/// True for a three-letter upper-case ISO 4217 style code.
pub fn is_currency_code(s: &str) -> bool {
    s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase())
}

impl Instrument {
    /// A cash equity in `currency`: multiplier 1, tick and lot size as given.
    pub fn equity(
        symbol: impl Into<String>,
        currency: impl Into<String>,
        tick_size: Price,
        lot_size: Qty,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            kind: InstrumentType::Equity,
            currency: currency.into(),
            multiplier: Ratio::ONE,
            tick_size,
            lot_size,
            expiry_ns: None,
            strike: None,
            maturity_ns: None,
            coupon_rate: None,
            underlying: None,
            coupon_frequency: None,
            day_count: None,
            option_right: None,
        }
    }

    /// Checks the record's own fields: symbol, currency code, positive
    /// multiplier, tick and lot size, and the fields its type needs.
    ///
    /// # Errors
    /// The first problem found, in words that name the field.
    pub fn validate(&self) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol is required".into());
        }
        if !is_currency_code(&self.currency) {
            return Err(format!("currency \"{}\" must be a three-letter ISO code", self.currency));
        }
        if self.multiplier.raw() <= 0 {
            return Err("multiplier must be positive".into());
        }
        if self.tick_size.raw() <= 0 {
            return Err("tick size must be positive".into());
        }
        if self.lot_size.raw() <= 0 {
            return Err("lot size must be positive".into());
        }
        if self.underlying.as_deref().is_some_and(|u| u.trim().is_empty()) {
            return Err("underlying must not be blank".into());
        }
        self.validate_type_fields()
    }

    /// True for a record that changes nothing about a run: an equity quoted in
    /// `base_currency` with multiplier 1 and lot size 1. Runs leave these out
    /// of their fingerprint, so storing such a record never moves a result.
    pub fn is_plain_equity(&self, base_currency: &str) -> bool {
        self.kind == InstrumentType::Equity
            && self.currency == base_currency
            && self.multiplier == Ratio::ONE
            && self.lot_size == Qty::ONE
    }

    fn validate_type_fields(&self) -> Result<(), String> {
        let need = |present: bool, name: &str| {
            if present {
                Ok(())
            } else {
                Err(format!("a {} needs {name}", self.kind))
            }
        };
        let forbid = |present: bool, name: &str| {
            if present {
                Err(format!("a {} cannot have {name}", self.kind))
            } else {
                Ok(())
            }
        };
        if !matches!(self.kind, InstrumentType::Bond | InstrumentType::Swap) {
            forbid(self.coupon_frequency.is_some(), "a coupon frequency")?;
        }
        if self.kind != InstrumentType::Bond {
            forbid(self.day_count.is_some(), "a day count")?;
        }
        if self.kind != InstrumentType::Option {
            forbid(self.option_right.is_some(), "a call or put right")?;
        }
        match self.kind {
            InstrumentType::Equity => {
                forbid(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
                forbid(self.maturity_ns.is_some(), "a maturity")?;
                forbid(self.coupon_rate.is_some(), "a coupon rate")?;
                forbid(self.underlying.is_some(), "an underlying")?;
                if self.multiplier != Ratio::ONE {
                    return Err("an equity's multiplier must be 1".into());
                }
            }
            InstrumentType::Future => {
                need(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
                forbid(self.maturity_ns.is_some(), "a maturity")?;
                forbid(self.coupon_rate.is_some(), "a coupon rate")?;
            }
            InstrumentType::Option => {
                need(self.expiry_ns.is_some(), "an expiry")?;
                need(self.strike.is_some(), "a strike")?;
                need(self.option_right.is_some(), "a right (call or put)")?;
                need(self.underlying.is_some(), "an underlying")?;
                forbid(self.maturity_ns.is_some(), "a maturity")?;
                forbid(self.coupon_rate.is_some(), "a coupon rate")?;
            }
            InstrumentType::Bond => {
                need(self.maturity_ns.is_some(), "a maturity")?;
                need(self.coupon_rate.is_some(), "a coupon rate")?;
                forbid(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
                forbid(self.underlying.is_some(), "an underlying")?;
                need(self.coupon_frequency.is_some(), "a coupon frequency")?;
                need(self.day_count.is_some(), "a day count")?;
                if self.coupon_frequency.is_some_and(|f| !COUPON_FREQUENCIES.contains(&f)) {
                    return Err("a bond's coupon frequency must be 1, 2, 4 or 12 a year".into());
                }
                if self.coupon_rate.is_some_and(|c| c.raw() < 0 || c > Ratio::ONE) {
                    return Err("a bond's coupon rate must be between 0 and 1".into());
                }
            }
            InstrumentType::Swap => {
                // A fixed-for-floating swap: maturity, fixed rate, payment
                // frequency, and the discount curve it is marked on.
                need(self.maturity_ns.is_some(), "a maturity")?;
                need(self.coupon_rate.is_some(), "a fixed rate (coupon_rate)")?;
                need(self.coupon_frequency.is_some(), "a fixed-leg frequency (coupon_frequency)")?;
                forbid(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
                need(self.underlying.is_some(), "a discount curve (underlying)")?;
                if self.coupon_frequency.is_some_and(|f| !COUPON_FREQUENCIES.contains(&f)) {
                    return Err("a swap's fixed-leg frequency must be 1, 2, 4 or 12 a year".into());
                }
                if self.coupon_rate.is_some_and(|c| c.raw() < -Ratio::ONE.raw() || c > Ratio::ONE) {
                    return Err("a swap's fixed rate must be between -1 and 1".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::SCALE;

    fn px(v: i64) -> Price {
        Price::from_raw(v * SCALE)
    }
    fn lot() -> Qty {
        Qty::from_raw(SCALE)
    }

    #[test]
    fn equity_validates() {
        let e = Instrument::equity("AAPL", "USD", Price::from_raw(1), lot());
        assert_eq!(e.validate(), Ok(()));
        assert!(e.is_plain_equity("USD"));
        assert!(!e.is_plain_equity("EUR"));
    }

    #[test]
    fn refuses_bad_currency_code() {
        let e = Instrument::equity("SAP", "eur", Price::from_raw(1), lot());
        assert!(e.validate().unwrap_err().contains("ISO"));
    }

    #[test]
    fn refuses_bad_multiplier_and_tick() {
        let mut e = Instrument::equity("AAPL", "USD", Price::from_raw(1), lot());
        e.multiplier = Ratio::ZERO;
        assert!(e.validate().unwrap_err().contains("multiplier"));
        let mut e = Instrument::equity("AAPL", "USD", Price::ZERO, lot());
        assert!(e.validate().unwrap_err().contains("tick"));
        e.tick_size = Price::from_raw(1);
        e.lot_size = Qty::ZERO;
        assert!(e.validate().unwrap_err().contains("lot"));
    }

    #[test]
    fn each_type_validates_its_own_fields() {
        let mut f = Instrument::equity("ESZ6", "USD", Price::from_raw(1), lot());
        f.kind = InstrumentType::Future;
        f.multiplier = Ratio::from_units(50).unwrap();
        assert!(f.validate().unwrap_err().contains("expiry"));
        f.expiry_ns = Some(1);
        assert_eq!(f.validate(), Ok(()));

        let mut o = f.clone();
        o.kind = InstrumentType::Option;
        assert!(o.validate().unwrap_err().contains("strike"));
        o.strike = Some(px(100));
        assert!(o.validate().unwrap_err().contains("right"));
        o.option_right = Some(OptionRight::Call);
        assert!(o.validate().unwrap_err().contains("underlying"));
        o.underlying = Some("SPX".into());
        assert_eq!(o.validate(), Ok(()));

        let mut b = Instrument::equity("UST10", "USD", Price::from_raw(1), lot());
        b.kind = InstrumentType::Bond;
        assert!(b.validate().unwrap_err().contains("maturity"));

        let mut e = Instrument::equity("AAPL", "USD", Price::from_raw(1), lot());
        e.expiry_ns = Some(1);
        assert!(e.validate().unwrap_err().contains("cannot have an expiry"));
        e.expiry_ns = None;
        e.multiplier = Ratio::from_units(2).unwrap();
        assert!(e.validate().unwrap_err().contains("must be 1"));
    }

    #[test]
    fn type_names_round_trip() {
        for t in [
            InstrumentType::Equity,
            InstrumentType::Future,
            InstrumentType::Bond,
            InstrumentType::Option,
            InstrumentType::Swap,
        ] {
            assert_eq!(InstrumentType::parse(t.as_str()), Some(t));
        }
        assert_eq!(InstrumentType::Equity.broker_asset_class(), Some("us_equity"));
        assert_eq!(InstrumentType::Bond.broker_asset_class(), None);
    }

    #[test]
    fn swaps_need_their_terms() {
        let mut s = Instrument::equity("USD5Y", "USD", Price::from_raw(1), lot());
        s.kind = InstrumentType::Swap;
        assert!(s.validate().unwrap_err().contains("a maturity"));
        s.maturity_ns = Some(1);
        s.coupon_rate = Some(Ratio::from_f64(-0.002).unwrap()); // negative rates are allowed
        assert!(s.validate().unwrap_err().contains("frequency"));
        s.coupon_frequency = Some(2);
        assert!(s.validate().unwrap_err().contains("curve"));
        s.underlying = Some("USD-SOFR".into());
        s.validate().unwrap();
        s.coupon_frequency = Some(3);
        assert!(s.validate().unwrap_err().contains("1, 2, 4 or 12"));
        s.coupon_frequency = Some(2);
        s.day_count = Some(DayCount::Act360);
        assert!(s.validate().unwrap_err().contains("a day count"));
        s.day_count = None;
        s.strike = Some(Price::from_raw(1));
        assert!(s.validate().unwrap_err().contains("a strike"));
    }
}
