//! The instrument record: one shape for every tradable type (M24.1).
//!
//! Equities are the default and behave exactly as they did before this model
//! existed: multiplier 1, no expiry, no strike, no coupon. Other types carry
//! their own nullable fields, and each type validates its own (decision 1).
//!
//! Stage 1 accepts one base currency and refuses any instrument in another
//! currency rather than converting it (decision 2).

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
    /// A swap. Deferred until a curve model exists.
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
}

/// The base currency of stage 1 when a workspace does not say otherwise.
pub const DEFAULT_BASE_CURRENCY: &str = "USD";

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
        }
    }

    /// Checks the record against the run's base currency.
    ///
    /// # Errors
    /// The first problem found, in words that name the field.
    pub fn validate(&self, base_currency: &str) -> Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol is required".into());
        }
        if self.currency.len() != 3 || !self.currency.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(format!("currency \"{}\" must be a three-letter ISO code", self.currency));
        }
        if self.currency != base_currency {
            return Err(format!(
                "currency {} differs from the run's base currency {base_currency}; multi-currency is not supported yet",
                self.currency
            ));
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
        self.validate_type_fields()
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
        match self.kind {
            InstrumentType::Equity => {
                forbid(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
                forbid(self.maturity_ns.is_some(), "a maturity")?;
                forbid(self.coupon_rate.is_some(), "a coupon rate")?;
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
                forbid(self.maturity_ns.is_some(), "a maturity")?;
                forbid(self.coupon_rate.is_some(), "a coupon rate")?;
            }
            InstrumentType::Bond => {
                need(self.maturity_ns.is_some(), "a maturity")?;
                need(self.coupon_rate.is_some(), "a coupon rate")?;
                forbid(self.expiry_ns.is_some(), "an expiry")?;
                forbid(self.strike.is_some(), "a strike")?;
            }
            InstrumentType::Swap => return Err("swaps are not supported yet".into()),
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
        assert_eq!(e.validate("USD"), Ok(()));
    }

    #[test]
    fn refuses_other_currency() {
        let e = Instrument::equity("SAP", "EUR", Price::from_raw(1), lot());
        assert!(e.validate("USD").unwrap_err().contains("base currency"));
    }

    #[test]
    fn refuses_bad_multiplier_and_tick() {
        let mut e = Instrument::equity("AAPL", "USD", Price::from_raw(1), lot());
        e.multiplier = Ratio::ZERO;
        assert!(e.validate("USD").unwrap_err().contains("multiplier"));
        let mut e = Instrument::equity("AAPL", "USD", Price::ZERO, lot());
        assert!(e.validate("USD").unwrap_err().contains("tick"));
        e.tick_size = Price::from_raw(1);
        e.lot_size = Qty::ZERO;
        assert!(e.validate("USD").unwrap_err().contains("lot"));
    }

    #[test]
    fn each_type_validates_its_own_fields() {
        let mut f = Instrument::equity("ESZ6", "USD", Price::from_raw(1), lot());
        f.kind = InstrumentType::Future;
        f.multiplier = Ratio::from_units(50).unwrap();
        assert!(f.validate("USD").unwrap_err().contains("expiry"));
        f.expiry_ns = Some(1);
        assert_eq!(f.validate("USD"), Ok(()));

        let mut o = f.clone();
        o.kind = InstrumentType::Option;
        assert!(o.validate("USD").unwrap_err().contains("strike"));
        o.strike = Some(px(100));
        assert_eq!(o.validate("USD"), Ok(()));

        let mut b = Instrument::equity("UST10", "USD", Price::from_raw(1), lot());
        b.kind = InstrumentType::Bond;
        assert!(b.validate("USD").unwrap_err().contains("maturity"));

        let mut e = Instrument::equity("AAPL", "USD", Price::from_raw(1), lot());
        e.expiry_ns = Some(1);
        assert!(e.validate("USD").unwrap_err().contains("cannot have an expiry"));
        e.expiry_ns = None;
        e.multiplier = Ratio::from_units(2).unwrap();
        assert!(e.validate("USD").unwrap_err().contains("must be 1"));
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
}
