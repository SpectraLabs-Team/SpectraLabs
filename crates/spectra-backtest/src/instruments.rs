//! Instruments, base currency and FX rates in a backtest request (M24).
//!
//! ```json
//! {
//!   "base_currency": "USD",
//!   "instruments": [
//!     { "symbol": "ESZ6", "type": "future", "multiplier": 50, "tick_size": 0.25, "expiry": "2026-12-18" },
//!     { "symbol": "SAP", "currency": "EUR", "tick_size": 0.01 }
//!   ],
//!   "fx_rates": { "EUR": [ { "as_of": "2026-01-02", "rate": "1.0845" } ] },
//!   "curves": { "USD-SOFR": [ { "as_of": "2026-01-02", "quotes": [
//!     { "type": "deposit", "years": 0.25, "rate": "0.043" },
//!     { "type": "swap", "years": 5, "rate": "0.039", "frequency": 2 } ] } ] }
//! }
//! ```
//!
//! A swap is not in the dataset: it names its curve (`underlying`), and the
//! run marks it from that curve's dated quotes (see `spectra_engine::swaps`).
//!
//! Every field is optional. An instrument names its dataset instrument by
//! `symbol` (or by `instrument` id when the dataset has no symbols). Missing
//! fields come from the record's type and the base currency: `type` defaults
//! to `equity`, `currency` to the base currency, `multiplier` and `lot_size`
//! to 1. `tick_size` is required: there is no universal tick.
//!
//! Numbers given as strings are read exactly; JSON numbers are rounded to
//! the engine's precision. The stored form writes exact strings, so a run
//! stored and re-run reproduces its fingerprint.

use std::collections::BTreeMap;

use spectra_core::{
    is_currency_code, Fixed, Instrument, InstrumentId, InstrumentType, Json, Price, Qty, Ratio, Result,
    DEFAULT_BASE_CURRENCY,
};
use spectra_engine::{
    BondQuote, CurvePoint, CurveSnapshot, FxRate, InstrumentSetup, QuoteKind, RollAdjust, RollRule,
    MAX_ROLL_BUSINESS_DAYS,
};

use crate::config::FieldError;

/// How a request names a dataset instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstrumentRef {
    /// By symbol (resolved against the dataset's symbol table).
    Symbol(String),
    /// By instrument id.
    Id(InstrumentId),
}

/// The instrument part of a request, before it is resolved against a dataset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestInstruments {
    /// The run's base currency, if the request set one.
    pub base_currency: Option<String>,
    /// Records, each with how it names its instrument.
    pub instruments: Vec<(InstrumentRef, Instrument)>,
    /// FX series by currency.
    pub fx: BTreeMap<String, Vec<FxRate>>,
    /// The FX timestamps as given, for the stored form.
    fx_text: BTreeMap<String, Vec<String>>,
    /// The futures roll rule, when the request sets one (else the default).
    pub roll: Option<RollRule>,
    /// Whether bond bars are clean or dirty, when the request says (else clean).
    pub bond_quote: Option<BondQuote>,
    /// Dated curve quotes by curve name, for swaps.
    pub curves: BTreeMap<String, Vec<CurveSnapshot>>,
    /// The curve snapshot timestamps as given, for the stored form.
    curves_text: BTreeMap<String, Vec<String>>,
}

/// Request fields this module reads.
pub const FIELDS: [&str; 6] =
    ["base_currency", "instruments", "fx_rates", "futures_roll", "bond_quote", "curves"];

const INSTRUMENT_FIELDS: [&str; 15] = [
    "symbol",
    "instrument",
    "type",
    "currency",
    "multiplier",
    "tick_size",
    "lot_size",
    "expiry",
    "strike",
    "maturity",
    "coupon_rate",
    "underlying",
    "coupon_frequency",
    "day_count",
    "right",
];

fn err(errors: &mut Vec<FieldError>, field: impl Into<String>, message: impl Into<String>) {
    errors.push(FieldError { field: field.into(), message: message.into() });
}

/// A fixed-point value from a JSON number or an exact decimal string.
fn fixed<U: spectra_core::fixed::Unit>(v: &Json) -> Option<Fixed<U>> {
    match v {
        Json::Int(i) => Fixed::from_units(*i).ok(),
        Json::Double(d) => Fixed::from_f64(*d).ok(),
        Json::String(s) => spectra_data::parse_fixed(s).map(Fixed::from_raw),
        _ => None,
    }
}

/// A timestamp from an ISO 8601 string or an integer (nanoseconds since the epoch).
fn timestamp(v: &Json) -> Option<(i64, String)> {
    match v {
        Json::Int(i) => Some((*i, i.to_string())),
        Json::String(s) => spectra_data::parse_timestamp(s, &spectra_data::TimeOptions::default())
            .map(|t| (t.ts_ns, s.trim().to_string())),
        _ => None,
    }
}

/// Reads `futures_roll`: `{"business_days_before_expiry": 5, "adjust": "ratio"}`,
/// both optional (defaults: [`RollRule::default`]).
fn parse_roll(v: &Json, errors: &mut Vec<FieldError>) -> Option<RollRule> {
    let Ok(entries) = v.entries() else {
        err(errors, "futures_roll", "must be an object {business_days_before_expiry, adjust}");
        return None;
    };
    let before = errors.len();
    let mut rule = RollRule::default();
    for (k, val) in entries {
        match (k.as_str(), val) {
            ("business_days_before_expiry", Json::Int(n))
                if (0..=i64::from(MAX_ROLL_BUSINESS_DAYS)).contains(n) =>
            {
                rule.business_days_before_expiry = *n as u32;
            }
            ("business_days_before_expiry", _) => err(
                errors,
                "futures_roll.business_days_before_expiry",
                format!("must be a whole number from 0 to {MAX_ROLL_BUSINESS_DAYS}"),
            ),
            ("adjust", Json::String(a)) => match RollAdjust::parse(a.trim()) {
                Some(m) => rule.adjust = m,
                None => err(errors, "futures_roll.adjust", "must be \"ratio\" or \"difference\""),
            },
            ("adjust", _) => err(errors, "futures_roll.adjust", "must be \"ratio\" or \"difference\""),
            (other, _) => err(errors, format!("futures_roll.{other}"), "unknown field"),
        }
    }
    (errors.len() == before).then_some(rule)
}

/// Reads one curve quote: `{"type": "deposit"|"swap", "years", "rate", "frequency"}`
/// (`frequency` for swaps only: 1, 2, 4 or 12).
fn parse_quote(q: &Json, f: &str, errors: &mut Vec<FieldError>) -> Option<CurvePoint> {
    let before = errors.len();
    if let Ok(es) = q.entries() {
        for (key, _) in es {
            if !["type", "years", "rate", "frequency"].contains(&key.as_str()) {
                err(errors, format!("{f}.{key}"), "unknown field");
            }
        }
    } else {
        err(errors, f, "must be a JSON object");
        return None;
    }
    let years = match q.find("years").and_then(fixed::<spectra_core::fixed::RatioUnit>) {
        Some(y) if y.raw() > 0 => Some(y),
        _ => {
            err(errors, format!("{f}.years"), "must be a positive number of years");
            None
        }
    };
    let rate = match q.find("rate").and_then(fixed::<spectra_core::fixed::RatioUnit>) {
        Some(r) if r.raw().abs() < spectra_core::SCALE => Some(r),
        _ => {
            err(errors, format!("{f}.rate"), "must be a fraction between -1 and 1 (0.04 for 4%)");
            None
        }
    };
    let frequency = match q.find("frequency") {
        None | Some(Json::Null) => None,
        Some(Json::Int(n)) if spectra_core::COUPON_FREQUENCIES.iter().any(|&c| i64::from(c) == *n) => {
            Some(*n as u32)
        }
        Some(_) => {
            err(errors, format!("{f}.frequency"), "must be 1, 2, 4 or 12");
            None
        }
    };
    let kind = match (q.find("type"), frequency) {
        (Some(Json::String(t)), None) if t.trim() == "deposit" => Some(QuoteKind::Deposit),
        (Some(Json::String(t)), Some(frequency)) if t.trim() == "swap" => Some(QuoteKind::Swap { frequency }),
        (Some(Json::String(t)), _) if t.trim() == "swap" => {
            err(errors, format!("{f}.frequency"), "a swap quote needs a frequency: 1, 2, 4 or 12");
            None
        }
        (Some(Json::String(t)), Some(_)) if t.trim() == "deposit" => {
            err(errors, format!("{f}.frequency"), "a deposit has no frequency");
            None
        }
        _ => {
            err(errors, format!("{f}.type"), "must be \"deposit\" or \"swap\"");
            None
        }
    };
    if errors.len() != before {
        return None;
    }
    Some(CurvePoint { kind: kind?, years: years?, rate: rate? })
}

/// Reads one curve's history: a list of `{as_of, quotes}` (see the module
/// documentation), with errors under `field`. Returns the snapshots, sorted
/// by time, with their `as_of` as given. Used by backtest requests and by the
/// curves API, so both apply the same rules.
pub fn parse_curve_history(
    name: &str,
    list: &Json,
    field: &str,
    errors: &mut Vec<FieldError>,
) -> Option<(Vec<String>, Vec<CurveSnapshot>)> {
    if name.trim().is_empty() || name.trim() != name {
        err(errors, field, "a curve name must be non-empty with no surrounding spaces");
        return None;
    }
    let Ok(items) = list.items() else {
        err(errors, field, "must be a list of {as_of, quotes}");
        return None;
    };
    let before = errors.len();
    let mut snaps: Vec<(String, CurveSnapshot)> = Vec::with_capacity(items.len());
    for (k, it) in items.iter().enumerate() {
        let f = format!("{field}[{k}]");
        if let Ok(es) = it.entries() {
            for (key, _) in es {
                if key != "as_of" && key != "quotes" {
                    err(errors, format!("{f}.{key}"), "unknown field");
                }
            }
        }
        let as_of = it.find("as_of").and_then(timestamp);
        if as_of.is_none() {
            err(errors, format!("{f}.as_of"), "must be an ISO 8601 date or time, or nanoseconds");
        }
        let quotes = match it.find("quotes").map(Json::items) {
            Some(Ok(qs)) if !qs.is_empty() => qs
                .iter()
                .enumerate()
                .filter_map(|(j, q)| parse_quote(q, &format!("{f}.quotes[{j}]"), errors))
                .collect(),
            _ => {
                err(
                    errors,
                    format!("{f}.quotes"),
                    "must be a non-empty list of {type, years, rate, frequency}",
                );
                Vec::new()
            }
        };
        if let Some((ts, text)) = as_of {
            snaps.push((text, CurveSnapshot { as_of_ns: ts, quotes }));
        }
    }
    if errors.len() != before {
        return None;
    }
    snaps.sort_by_key(|s| s.1.as_of_ns);
    if snaps.windows(2).any(|w| w[0].1.as_of_ns == w[1].1.as_of_ns) {
        err(errors, field, "has two snapshots for the same time");
        return None;
    }
    let history: Vec<CurveSnapshot> = snaps.iter().map(|s| s.1.clone()).collect();
    if let Err(e) = spectra_engine::swaps::validate_history(name, &history) {
        err(errors, field, e.to_string());
        return None;
    }
    Some((snaps.into_iter().map(|s| s.0).collect(), history))
}

/// Reads one instrument record (the shape of an `instruments` entry), with
/// field names prefixed by `prefix`. Used by backtest requests and by the
/// instruments API, so both apply the same rules.
///
/// # Errors
/// Every problem found.
pub fn parse_instrument(
    item: &Json,
    prefix: &str,
    base: &str,
) -> std::result::Result<(InstrumentRef, Instrument), Vec<FieldError>> {
    let mut errors = Vec::new();
    match parse_one(item, prefix, base, &mut errors) {
        Some(r) if errors.is_empty() => Ok(r),
        _ => Err(errors),
    }
}

fn parse_one(
    item: &Json,
    p: &str,
    base: &str,
    errors: &mut Vec<FieldError>,
) -> Option<(InstrumentRef, Instrument)> {
    let whole = p.trim_end_matches('.').to_string();
    let before = errors.len();
    let Ok(entries) = item.entries() else {
        err(errors, whole.clone(), "must be a JSON object");
        return None;
    };
    for (k, _) in entries {
        if !INSTRUMENT_FIELDS.contains(&k.as_str()) {
            err(errors, format!("{p}{k}"), "unknown field");
        }
    }
    let reference = match (item.find("symbol"), item.find("instrument")) {
        (Some(Json::String(s)), None) if !s.trim().is_empty() => Some(InstrumentRef::Symbol(s.trim().into())),
        (None, Some(Json::Int(id))) if (0..=i64::from(u32::MAX)).contains(id) => {
            Some(InstrumentRef::Id(*id as InstrumentId))
        }
        _ => {
            err(errors, format!("{p}symbol"), "give either symbol (a string) or instrument (an id)");
            None
        }
    };
    let kind = match item.find("type") {
        None | Some(Json::Null) => InstrumentType::Equity,
        Some(Json::String(s)) => InstrumentType::parse(s.trim()).unwrap_or_else(|| {
            err(errors, format!("{p}type"), "must be equity, future, bond, option or swap");
            InstrumentType::Equity
        }),
        Some(_) => {
            err(errors, format!("{p}type"), "must be a string");
            InstrumentType::Equity
        }
    };
    let currency = match item.find("currency") {
        None | Some(Json::Null) => base.to_string(),
        Some(Json::String(s)) if is_currency_code(s.trim()) => s.trim().to_string(),
        Some(_) => {
            err(errors, format!("{p}currency"), "must be a three-letter ISO currency code");
            base.to_string()
        }
    };
    let mut num = |name: &str, default: Option<i64>| -> Option<i64> {
        match item.find(name) {
            None | Some(Json::Null) => {
                if default.is_none() && name == "tick_size" {
                    err(errors, format!("{p}{name}"), "is required");
                }
                default
            }
            Some(v) => match fixed::<spectra_core::fixed::RatioUnit>(v) {
                Some(x) if x.raw() > 0 => Some(x.raw()),
                _ => {
                    err(errors, format!("{p}{name}"), "must be a positive number");
                    None
                }
            },
        }
    };
    let one = Some(Ratio::ONE.raw());
    let multiplier = num("multiplier", one);
    let tick = num("tick_size", None);
    let lot = num("lot_size", one);
    let strike = num("strike", Some(0)).filter(|v| *v > 0);
    let coupon = match item.find("coupon_rate") {
        None | Some(Json::Null) => None,
        Some(v) => match fixed::<spectra_core::fixed::RatioUnit>(v) {
            // A swap's fixed rate may be negative; a bond's coupon may not.
            Some(x) if x.raw() >= 0 || kind == InstrumentType::Swap => Some(x),
            _ => {
                err(errors, format!("{p}coupon_rate"), "must be a non-negative fraction (0.05 for 5%)");
                None
            }
        },
    };
    let underlying = match item.find("underlying") {
        None | Some(Json::Null) => None,
        Some(Json::String(u)) if !u.trim().is_empty() => Some(u.trim().to_string()),
        Some(_) => {
            err(errors, format!("{p}underlying"), "must be a non-empty string");
            None
        }
    };
    let coupon_frequency = match item.find("coupon_frequency") {
        None | Some(Json::Null) => None,
        Some(Json::Int(f))
            if u32::try_from(*f).is_ok_and(|f| spectra_core::COUPON_FREQUENCIES.contains(&f)) =>
        {
            u32::try_from(*f).ok()
        }
        Some(_) => {
            err(errors, format!("{p}coupon_frequency"), "must be 1, 2, 4 or 12 coupons a year");
            None
        }
    };
    let day_count = match item.find("day_count") {
        None | Some(Json::Null) => None,
        Some(Json::String(d)) if spectra_core::DayCount::parse(d.trim()).is_some() => {
            spectra_core::DayCount::parse(d.trim())
        }
        Some(_) => {
            err(errors, format!("{p}day_count"), "must be act_act_icma, 30_360, act_360 or act_365f");
            None
        }
    };
    let option_right = match item.find("right") {
        None | Some(Json::Null) => None,
        Some(Json::String(r)) if spectra_core::OptionRight::parse(r.trim()).is_some() => {
            spectra_core::OptionRight::parse(r.trim())
        }
        Some(_) => {
            err(errors, format!("{p}right"), "must be \"call\" or \"put\"");
            None
        }
    };
    let mut when = |name: &str| match item.find(name) {
        None | Some(Json::Null) => None,
        Some(v) => timestamp(v).map(|t| t.0).or_else(|| {
            err(
                errors,
                format!("{p}{name}"),
                "must be an ISO 8601 date or time, or nanoseconds since the epoch",
            );
            None
        }),
    };
    let expiry = when("expiry");
    let maturity = when("maturity");
    if errors.len() != before {
        return None;
    }
    let symbol = match &reference {
        Some(InstrumentRef::Symbol(s)) => s.clone(),
        Some(InstrumentRef::Id(id)) => format!("#{id}"),
        None => return None,
    };
    let inst = Instrument {
        symbol,
        kind,
        currency,
        multiplier: Ratio::from_raw(multiplier?),
        tick_size: Price::from_raw(tick?),
        lot_size: Qty::from_raw(lot?),
        expiry_ns: expiry,
        strike: strike.map(Price::from_raw),
        maturity_ns: maturity,
        coupon_rate: coupon,
        underlying,
        coupon_frequency,
        day_count,
        option_right,
    };
    if let Err(e) = inst.validate() {
        err(errors, whole, e);
        return None;
    }
    Some((reference?, inst))
}

impl RequestInstruments {
    /// True when the request says nothing about instruments.
    pub fn is_empty(&self) -> bool {
        self.base_currency.is_none()
            && self.instruments.is_empty()
            && self.fx.is_empty()
            && self.roll.is_none()
            && self.bond_quote.is_none()
            && self.curves.is_empty()
    }

    /// The base currency the run books in.
    pub fn base(&self) -> &str {
        self.base_currency.as_deref().unwrap_or(DEFAULT_BASE_CURRENCY)
    }

    /// Reads `base_currency`, `instruments` and `fx_rates` from a request,
    /// recording every problem.
    pub fn parse(doc: &Json, errors: &mut Vec<FieldError>) -> Option<Self> {
        let before = errors.len();
        let mut out = Self::default();
        match doc.find("base_currency") {
            None | Some(Json::Null) => {}
            Some(Json::String(s)) if is_currency_code(s.trim()) => out.base_currency = Some(s.trim().into()),
            Some(_) => {
                err(errors, "base_currency", "must be a three-letter ISO currency code such as \"USD\"");
            }
        }
        if let Some(list) = doc.find("instruments").filter(|v| !v.is_null()) {
            match list.items() {
                Ok(items) => {
                    for (i, item) in items.iter().enumerate() {
                        if let Some(rec) = parse_one(item, &format!("instruments[{i}]."), out.base(), errors)
                        {
                            out.instruments.push(rec);
                        }
                    }
                }
                Err(_) => err(errors, "instruments", "must be a list"),
            }
        }
        if let Some(fx) = doc.find("fx_rates").filter(|v| !v.is_null()) {
            Self::parse_fx(fx, &mut out, errors);
        }
        if let Some(c) = doc.find("curves").filter(|v| !v.is_null()) {
            Self::parse_curves(c, &mut out, errors);
        }
        if let Some(r) = doc.find("futures_roll").filter(|v| !v.is_null()) {
            out.roll = parse_roll(r, errors);
        }
        match doc.find("bond_quote") {
            None | Some(Json::Null) => {}
            Some(Json::String(q)) if BondQuote::parse(q.trim()).is_some() => {
                out.bond_quote = BondQuote::parse(q.trim());
            }
            Some(_) => err(errors, "bond_quote", "must be \"clean\" or \"dirty\""),
        }
        // One record per instrument.
        for (i, (r, _)) in out.instruments.iter().enumerate() {
            if out.instruments[..i].iter().any(|(o, _)| o == r) {
                err(errors, format!("instruments[{i}]"), "names the same instrument as an earlier entry");
            }
        }
        (errors.len() == before).then_some(out)
    }

    fn parse_curves(curves: &Json, out: &mut Self, errors: &mut Vec<FieldError>) {
        let Ok(entries) = curves.entries() else {
            err(errors, "curves", "must be an object of curve name to a list of {as_of, quotes}");
            return;
        };
        for (name, list) in entries {
            if let Some((texts, history)) = parse_curve_history(name, list, &format!("curves.{name}"), errors)
            {
                out.curves_text.insert(name.clone(), texts);
                out.curves.insert(name.clone(), history);
            }
        }
    }

    fn parse_fx(fx: &Json, out: &mut Self, errors: &mut Vec<FieldError>) {
        let Ok(entries) = fx.entries() else {
            err(errors, "fx_rates", "must be an object of currency code to a list of {as_of, rate}");
            return;
        };
        for (ccy, list) in entries {
            let field = format!("fx_rates.{ccy}");
            if !is_currency_code(ccy) {
                err(errors, &field, "must be a three-letter ISO currency code");
                continue;
            }
            let Ok(items) = list.items() else {
                err(errors, &field, "must be a list of {as_of, rate}");
                continue;
            };
            let mut series: Vec<(i64, String, i128)> = Vec::with_capacity(items.len());
            for (k, it) in items.iter().enumerate() {
                let f = format!("{field}[{k}]");
                let as_of = it.find("as_of").and_then(timestamp);
                let rate = match it.find("rate") {
                    Some(Json::String(s)) => FxRate::parse_rate(s).ok(),
                    Some(Json::Int(v)) => FxRate::rate_from_f64(*v as f64).ok(),
                    Some(Json::Double(v)) => FxRate::rate_from_f64(*v).ok(),
                    _ => None,
                };
                match (as_of, rate) {
                    (Some((ts, text)), Some(r)) => series.push((ts, text, r)),
                    (None, _) => {
                        err(errors, format!("{f}.as_of"), "must be an ISO 8601 date or time, or nanoseconds");
                    }
                    (_, None) => err(errors, format!("{f}.rate"), "must be a positive number"),
                }
            }
            series.sort_by_key(|s| s.0);
            if series.windows(2).any(|w| w[0].0 == w[1].0) {
                err(errors, &field, "has two rates for the same time");
                continue;
            }
            out.fx_text.insert(ccy.clone(), series.iter().map(|s| s.1.clone()).collect());
            out.fx.insert(
                ccy.clone(),
                series.into_iter().map(|(ts, _, rate)| FxRate { as_of_ns: ts, rate }).collect(),
            );
        }
    }

    /// `symbols` with every swap the request names appended (a swap has no
    /// bars in a dataset: the run generates them), in request order.
    ///
    /// # Errors
    /// A message when a swap is named but the dataset has no symbol table.
    pub fn symbols_with_swaps(&self, symbols: &[String]) -> std::result::Result<Vec<String>, String> {
        let mut out = symbols.to_vec();
        for (i, (r, inst)) in self.instruments.iter().enumerate() {
            if inst.kind != InstrumentType::Swap {
                continue;
            }
            match r {
                InstrumentRef::Symbol(s) if !out.contains(s) => {
                    if symbols.is_empty() {
                        return Err(format!(
                            "instruments[{i}]: a swap needs a dataset with symbols, to add {s} to"
                        ));
                    }
                    out.push(s.clone());
                }
                InstrumentRef::Symbol(_) => {}
                InstrumentRef::Id(_) => {
                    return Err(format!("instruments[{i}]: name a swap by symbol, not by instrument id"));
                }
            }
        }
        Ok(out)
    }

    /// The setup for a dataset whose instrument ids are positions in `symbols`.
    /// Records naming a symbol the dataset does not have are left out: a
    /// workspace's instrument list covers more symbols than any one dataset.
    ///
    /// # Errors
    /// A message for a record naming an id outside the dataset.
    pub fn resolve(&self, symbols: &[String]) -> std::result::Result<InstrumentSetup, String> {
        let mut setup = InstrumentSetup {
            base_currency: self.base().to_string(),
            roll: self.roll.unwrap_or_default(),
            bond_quote: self.bond_quote.unwrap_or_default(),
            ..InstrumentSetup::default()
        };
        for (i, (r, inst)) in self.instruments.iter().enumerate() {
            let id = match r {
                InstrumentRef::Symbol(s) => {
                    match symbols.iter().position(|x| x == s).and_then(|k| u32::try_from(k).ok()) {
                        Some(id) => id,
                        None => continue,
                    }
                }
                InstrumentRef::Id(id) => {
                    if !symbols.is_empty() && *id as usize >= symbols.len() {
                        return Err(format!("instruments[{i}].instrument: {id} is not in this dataset"));
                    }
                    *id
                }
            };
            setup.instruments.insert(id, inst.clone());
        }
        // Each option's underlying, by symbol (or "#<id>" in a dataset without symbols).
        for (&id, inst) in &setup.instruments {
            if inst.kind != InstrumentType::Option {
                continue;
            }
            let Some(u) = inst.underlying.as_deref() else { continue };
            let found =
                symbols.iter().position(|x| x == u).and_then(|k| u32::try_from(k).ok()).or_else(|| {
                    u.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()).filter(|_| symbols.is_empty())
                });
            match found {
                Some(uid) => {
                    setup.underlying_ids.insert(id, uid);
                }
                None => {
                    return Err(format!("option {}: its underlying {u} is not in this dataset", inst.symbol))
                }
            }
        }
        // Only the curves the resolved swaps are marked on.
        for inst in setup.instruments.values().filter(|i| i.kind == InstrumentType::Swap) {
            let Some(name) = inst.underlying.as_deref() else { continue };
            match self.curves.get(name) {
                Some(h) => {
                    setup.curves.insert(name.to_string(), h.clone());
                }
                None => {
                    return Err(format!(
                    "swap {}: the request has no quotes for its curve {name} (give them under \"curves\")",
                    inst.symbol
                ))
                }
            }
        }
        // Only the series the resolved instruments need.
        for ccy in setup.foreign_currencies().into_iter().map(str::to_string).collect::<Vec<_>>() {
            if let Some(series) = self.fx.get(&ccy) {
                setup.fx.insert(ccy, series.clone());
            }
        }
        Ok(setup)
    }

    /// Adds this part to a stored request document, exactly.
    ///
    /// # Errors
    /// Never for a parsed request (numbers are written as strings).
    pub fn write_into(&self, mut doc: Json) -> Result<Json> {
        if let Some(b) = &self.base_currency {
            doc = doc.with("base_currency", Json::string(b.clone()));
        }
        if let Some(q) = self.bond_quote {
            doc = doc.with("bond_quote", Json::string(q.as_str()));
        }
        if let Some(r) = &self.roll {
            doc = doc.with(
                "futures_roll",
                Json::object()
                    .with(
                        "business_days_before_expiry",
                        Json::integer(i64::from(r.business_days_before_expiry)),
                    )
                    .with("adjust", Json::string(r.adjust.as_str())),
            );
        }
        if !self.instruments.is_empty() {
            let mut list = Json::array();
            for (r, i) in &self.instruments {
                let mut j = match r {
                    InstrumentRef::Symbol(s) => Json::object().with("symbol", Json::string(s.clone())),
                    InstrumentRef::Id(id) => Json::object().with("instrument", Json::integer(i64::from(*id))),
                };
                j = j
                    .with("type", Json::string(i.kind.as_str()))
                    .with("currency", Json::string(i.currency.clone()))
                    .with("multiplier", Json::string(i.multiplier.to_string()))
                    .with("tick_size", Json::string(i.tick_size.to_string()))
                    .with("lot_size", Json::string(i.lot_size.to_string()));
                if let Some(t) = i.expiry_ns {
                    j = j.with("expiry", Json::integer(t));
                }
                if let Some(p) = i.strike {
                    j = j.with("strike", Json::string(p.to_string()));
                }
                if let Some(t) = i.maturity_ns {
                    j = j.with("maturity", Json::integer(t));
                }
                if let Some(c) = i.coupon_rate {
                    j = j.with("coupon_rate", Json::string(c.to_string()));
                }
                if let Some(u) = &i.underlying {
                    j = j.with("underlying", Json::string(u.clone()));
                }
                if let Some(f) = i.coupon_frequency {
                    j = j.with("coupon_frequency", Json::integer(i64::from(f)));
                }
                if let Some(d) = i.day_count {
                    j = j.with("day_count", Json::string(d.as_str()));
                }
                if let Some(r) = i.option_right {
                    j = j.with("right", Json::string(r.as_str()));
                }
                list.push(j)?;
            }
            doc = doc.with("instruments", list);
        }
        if !self.fx.is_empty() {
            let mut fx = Json::object();
            for (ccy, series) in &self.fx {
                let texts = self.fx_text.get(ccy);
                let mut list = Json::array();
                for (k, r) in series.iter().enumerate() {
                    let as_of = texts.and_then(|t| t.get(k)).map_or_else(
                        || Json::integer(r.as_of_ns),
                        |s| s.parse::<i64>().map_or_else(|_| Json::string(s.clone()), Json::integer),
                    );
                    list.push(
                        Json::object()
                            .with("as_of", as_of)
                            .with("rate", Json::string(FxRate::rate_text(r.rate))),
                    )?;
                }
                fx.set(ccy.clone(), list)?;
            }
            doc = doc.with("fx_rates", fx);
        }
        if !self.curves.is_empty() {
            let mut curves = Json::object();
            for (name, history) in &self.curves {
                let texts = self.curves_text.get(name);
                let mut list = Json::array();
                for (k, snap) in history.iter().enumerate() {
                    let as_of = texts.and_then(|t| t.get(k)).map_or_else(
                        || Json::integer(snap.as_of_ns),
                        |s| s.parse::<i64>().map_or_else(|_| Json::string(s.clone()), Json::integer),
                    );
                    let mut quotes = Json::array();
                    for q in &snap.quotes {
                        let mut j = Json::object()
                            .with("type", Json::string(q.kind.as_str()))
                            .with("years", Json::string(q.years.to_string()))
                            .with("rate", Json::string(q.rate.to_string()));
                        if let Some(f) = q.kind.frequency() {
                            j = j.with("frequency", Json::integer(i64::from(f)));
                        }
                        quotes.push(j)?;
                    }
                    list.push(Json::object().with("as_of", as_of).with("quotes", quotes))?;
                }
                curves.set(name.clone(), list)?;
            }
            doc = doc.with("curves", curves);
        }
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> std::result::Result<RequestInstruments, Vec<FieldError>> {
        let mut errors = Vec::new();
        RequestInstruments::parse(&Json::parse(text).unwrap(), &mut errors).ok_or(errors)
    }

    #[test]
    fn reads_a_future_and_a_foreign_stock() {
        let r = parse(
            r#"{"instruments":[
                {"symbol":"ESZ6","type":"future","multiplier":50,"tick_size":"0.25","expiry":"2026-12-18"},
                {"symbol":"SAP","currency":"EUR","tick_size":0.01}],
              "fx_rates":{"EUR":[{"as_of":"2026-01-02","rate":"1.0845"}]}}"#,
        )
        .unwrap();
        assert_eq!(r.instruments.len(), 2);
        assert_eq!(r.instruments[0].1.multiplier, Ratio::from_units(50).unwrap());
        assert_eq!(r.instruments[1].1.currency, "EUR");
        let setup = r.resolve(&["SAP".into(), "ESZ6".into(), "AAPL".into()]).unwrap();
        assert_eq!(setup.instruments.get(&1).unwrap().symbol, "ESZ6");
        assert_eq!(setup.fx.get("EUR").unwrap()[0].rate, 1_084_500_000_000);
        setup.validate().unwrap();
    }

    #[test]
    fn stored_form_reparses_identically() {
        let r = parse(
            r#"{"base_currency":"EUR","instruments":[{"symbol":"ESZ6","type":"future","currency":"USD",
                "multiplier":50,"tick_size":0.25,"expiry":"2026-12-18"}],
                "fx_rates":{"USD":[{"as_of":"2026-01-02","rate":0.92}]}}"#,
        )
        .unwrap();
        let stored = r.write_into(Json::object()).unwrap();
        let again = parse(&stored.dump()).unwrap();
        assert_eq!(again.instruments, r.instruments);
        assert_eq!(again.fx, r.fx);
        assert_eq!(again.base_currency, r.base_currency);
    }

    #[test]
    fn reports_every_problem() {
        let errs = parse(
            r#"{"base_currency":"usd","instruments":[{"symbol":"X","type":"future","tick_size":0.25},
                {"symbol":"Y","multiplier":-1},{"nope":1}],"fx_rates":{"EUR":[{"as_of":"x","rate":0}]}}"#,
        )
        .unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"base_currency"));
        assert!(fields.contains(&"instruments[0]")); // future without expiry
        assert!(fields.contains(&"instruments[1].multiplier"));
        assert!(fields.contains(&"instruments[1].tick_size"));
        assert!(fields.contains(&"instruments[2].nope"));
        assert!(fields.contains(&"fx_rates.EUR[0].as_of"));
    }

    #[test]
    fn unknown_symbols_are_skipped() {
        let r = parse(r#"{"instruments":[{"symbol":"ZZZ","tick_size":0.01,"currency":"EUR"}]}"#).unwrap();
        let s = r.resolve(&["AAPL".into()]).unwrap();
        assert!(s.instruments.is_empty());
        assert!(s.validate().is_ok()); // EUR not needed: nothing in the run uses it
    }
}
