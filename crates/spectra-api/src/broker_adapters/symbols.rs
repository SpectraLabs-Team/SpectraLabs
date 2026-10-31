//! Venue symbol mapping to spectra-core Instrument model (M24-advanced Task 2, 8, 9, 12, 16).
//!
//! Maps venue-specific contract symbols, expiries, multipliers, tick sizes, and contract sizes
//! across CME, Eurex, NSE/BSE (India), and Binance USDⓈ-M futures & options.

use std::fmt;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use spectra_core::fixed::{Price, Qty, Ratio};
use spectra_core::{Instrument, InstrumentType, OptionRight};

/// Supported execution venues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VenueId {
    /// Interactive Brokers (CME, Eurex, US Options).
    InteractiveBrokers,
    /// Zerodha Kite Connect (NSE / BSE Indian F&O).
    ZerodhaKite,
    /// Binance USDⓈ-M Futures (Testnet & Live).
    BinanceUsdm,
    /// Internal paper simulation.
    PaperSimulated,
}

impl VenueId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InteractiveBrokers => "interactive_brokers",
            Self::ZerodhaKite => "zerodha_kite",
            Self::BinanceUsdm => "binance_usdm",
            Self::PaperSimulated => "paper_simulated",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "interactive_brokers" | "ibkr" | "ib" => Some(Self::InteractiveBrokers),
            "zerodha_kite" | "kite" | "zerodha" => Some(Self::ZerodhaKite),
            "binance_usdm" | "binance" | "binance_futures" => Some(Self::BinanceUsdm),
            "paper_simulated" | "paper" => Some(Self::PaperSimulated),
            _ => None,
        }
    }
}

impl fmt::Display for VenueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors occurring during venue symbol mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolMapError {
    InvalidFormat(String),
    UnsupportedVenue(String),
    UnknownRoot(String),
    InvalidExpiry(String),
}

impl fmt::Display for SymbolMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(msg) => write!(f, "invalid symbol format: {msg}"),
            Self::UnsupportedVenue(v) => write!(f, "unsupported venue: {v}"),
            Self::UnknownRoot(r) => write!(f, "unknown root instrument: {r}"),
            Self::InvalidExpiry(e) => write!(f, "invalid contract expiry: {e}"),
        }
    }
}

impl std::error::Error for SymbolMapError {}

fn to_ratio(v: f64) -> Result<Ratio, SymbolMapError> {
    Ratio::from_f64(v).map_err(|e| SymbolMapError::InvalidFormat(format!("Invalid ratio {v}: {e}")))
}

fn to_price(v: f64) -> Result<Price, SymbolMapError> {
    Price::from_f64(v).map_err(|e| SymbolMapError::InvalidFormat(format!("Invalid price {v}: {e}")))
}

fn to_qty(v: f64) -> Result<Qty, SymbolMapError> {
    Qty::from_f64(v).map_err(|e| SymbolMapError::InvalidFormat(format!("Invalid quantity {v}: {e}")))
}

/// Standard futures month codes.
pub const FUTURES_MONTH_CODES: [(char, u32); 12] = [
    ('F', 1),  // January
    ('G', 2),  // February
    ('H', 3),  // March
    ('J', 4),  // April
    ('K', 5),  // May
    ('M', 6),  // June
    ('N', 7),  // July
    ('Q', 8),  // August
    ('U', 9),  // September
    ('V', 10), // October
    ('X', 11), // November
    ('Z', 12), // December
];

/// Resolves a futures month code letter to month number (1-12).
#[must_use]
pub fn month_from_code(code: char) -> Option<u32> {
    FUTURES_MONTH_CODES.iter().find(|(c, _)| c.eq_ignore_ascii_case(&code)).map(|(_, m)| *m)
}

/// Resolves a month number (1-12) to its futures month code letter.
#[must_use]
pub fn code_from_month(month: u32) -> Option<char> {
    FUTURES_MONTH_CODES.iter().find(|(_, m)| *m == month).map(|(c, _)| *c)
}

/// Computes the 3rd Friday of a given year and month (standard US equity/futures expiration).
#[must_use]
pub fn third_friday_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let first_weekday = first.weekday().num_days_from_monday(); // Mon=0, Fri=4
    let days_until_first_friday = (4 + 7 - i64::from(first_weekday)) % 7;
    let first_friday = first + chrono::Duration::days(days_until_first_friday);
    Some(first_friday + chrono::Duration::days(14))
}

/// Maps a CME futures symbol (e.g. `ESZ26`, `NQM27`, `CLF26`, `GCQ26`) into an `Instrument`.
pub fn map_cme_symbol_to_instrument(symbol: &str) -> Result<Instrument, SymbolMapError> {
    let sym = symbol.trim().to_ascii_uppercase();
    if sym.len() < 4 {
        return Err(SymbolMapError::InvalidFormat(format!("CME symbol too short: {sym}")));
    }

    let (root, month_year) = if sym.starts_with("MES") || sym.starts_with("MNQ") {
        (&sym[..3], &sym[3..])
    } else {
        (&sym[..2], &sym[2..])
    };

    if month_year.is_empty() {
        return Err(SymbolMapError::InvalidFormat(format!("Missing month and year in {sym}")));
    }

    let month_code = month_year.chars().next().unwrap();
    let month = month_from_code(month_code).ok_or_else(|| {
        SymbolMapError::InvalidFormat(format!("Unknown month code '{month_code}' in {sym}"))
    })?;

    let year_suffix = &month_year[1..];
    let year: i32 = match year_suffix.len() {
        1 => {
            2020 + year_suffix
                .parse::<i32>()
                .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?
        }
        2 => {
            2000 + year_suffix
                .parse::<i32>()
                .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?
        }
        4 => year_suffix
            .parse::<i32>()
            .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?,
        _ => return Err(SymbolMapError::InvalidFormat(format!("Invalid year format in {sym}"))),
    };

    let expiry_date = third_friday_of_month(year, month).ok_or_else(|| {
        SymbolMapError::InvalidExpiry(format!("Failed to calculate expiration for {year}-{month}"))
    })?;

    let expiry_dt = Utc.from_utc_datetime(&expiry_date.and_hms_opt(21, 0, 0).unwrap());
    let expiry_ns = expiry_dt.timestamp_nanos_opt().unwrap_or(0);

    let (multiplier, tick_size, lot_size) = match root {
        "ES" => (50.0, 0.25, 1.0),
        "NQ" => (20.0, 0.25, 1.0),
        "MES" => (5.0, 0.25, 1.0),
        "MNQ" => (2.0, 0.25, 1.0),
        "CL" => (1000.0, 0.01, 1.0),
        "GC" => (100.0, 0.10, 1.0),
        _ => return Err(SymbolMapError::UnknownRoot(format!("Unsupported CME root: {root}"))),
    };

    let underlying_str = root.to_string();

    Ok(Instrument {
        symbol: sym,
        kind: InstrumentType::Future,
        currency: "USD".to_string(),
        multiplier: to_ratio(multiplier)?,
        tick_size: to_price(tick_size)?,
        lot_size: to_qty(lot_size)?,
        expiry_ns: Some(expiry_ns),
        strike: None,
        maturity_ns: None,
        coupon_rate: None,
        underlying: Some(underlying_str),
        coupon_frequency: None,
        day_count: None,
        option_right: None,
    })
}

/// Maps a Eurex futures symbol (e.g. `FGBLZ26`, `FESXZ26`, `FDAXZ26`) into an `Instrument`.
pub fn map_eurex_symbol_to_instrument(symbol: &str) -> Result<Instrument, SymbolMapError> {
    let sym = symbol.trim().to_ascii_uppercase();
    if sym.len() < 5 {
        return Err(SymbolMapError::InvalidFormat(format!("Eurex symbol too short: {sym}")));
    }

    let (root, month_year) = if sym.starts_with("FGBL") || sym.starts_with("FESX") || sym.starts_with("FDAX")
    {
        (&sym[..4], &sym[4..])
    } else {
        return Err(SymbolMapError::UnknownRoot(format!("Unknown Eurex root in {sym}")));
    };

    let month_code = month_year
        .chars()
        .next()
        .ok_or_else(|| SymbolMapError::InvalidFormat(format!("Missing month code in {sym}")))?;
    let month = month_from_code(month_code).ok_or_else(|| {
        SymbolMapError::InvalidFormat(format!("Unknown month code '{month_code}' in {sym}"))
    })?;

    let year_suffix = &month_year[1..];
    let year: i32 = match year_suffix.len() {
        2 => {
            2000 + year_suffix
                .parse::<i32>()
                .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?
        }
        4 => year_suffix
            .parse::<i32>()
            .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?,
        _ => return Err(SymbolMapError::InvalidFormat(format!("Invalid year format in {sym}"))),
    };

    let expiry_date = third_friday_of_month(year, month).ok_or_else(|| {
        SymbolMapError::InvalidExpiry(format!("Failed to calculate expiration for {year}-{month}"))
    })?;
    let expiry_dt = Utc.from_utc_datetime(&expiry_date.and_hms_opt(16, 0, 0).unwrap());
    let expiry_ns = expiry_dt.timestamp_nanos_opt().unwrap_or(0);

    let (multiplier, tick_size, lot_size) = match root {
        "FGBL" => (1000.0, 0.01, 1.0),
        "FESX" => (10.0, 1.0, 1.0),
        "FDAX" => (25.0, 0.5, 1.0),
        _ => unreachable!(),
    };

    let underlying_str = root.to_string();

    Ok(Instrument {
        symbol: sym,
        kind: InstrumentType::Future,
        currency: "EUR".to_string(),
        multiplier: to_ratio(multiplier)?,
        tick_size: to_price(tick_size)?,
        lot_size: to_qty(lot_size)?,
        expiry_ns: Some(expiry_ns),
        strike: None,
        maturity_ns: None,
        coupon_rate: None,
        underlying: Some(underlying_str),
        coupon_frequency: None,
        day_count: None,
        option_right: None,
    })
}

/// Maps an NSE F&O symbol (e.g. `NIFTY26NOVFUT`, `BANKNIFTY26D1052000CE`) into an `Instrument`.
pub fn map_nse_symbol_to_instrument(symbol: &str) -> Result<Instrument, SymbolMapError> {
    let sym = symbol.trim().to_ascii_uppercase();

    let (underlying, lot_size) = if sym.starts_with("BANKNIFTY") {
        ("BANKNIFTY", 15.0)
    } else if sym.starts_with("FINNIFTY") {
        ("FINNIFTY", 25.0)
    } else if sym.starts_with("NIFTY") {
        ("NIFTY", 50.0)
    } else {
        return Err(SymbolMapError::UnknownRoot(format!("Unknown NSE root in {sym}")));
    };

    let rem = &sym[underlying.len()..];
    if rem.ends_with("FUT") {
        let expiry_ns = Utc::now()
            .checked_add_signed(chrono::Duration::days(30))
            .and_then(|d| d.timestamp_nanos_opt())
            .unwrap_or(0);

        Ok(Instrument {
            symbol: sym,
            kind: InstrumentType::Future,
            currency: "INR".to_string(),
            multiplier: to_ratio(1.0)?,
            tick_size: to_price(0.05)?,
            lot_size: to_qty(lot_size)?,
            expiry_ns: Some(expiry_ns),
            strike: None,
            maturity_ns: None,
            coupon_rate: None,
            underlying: Some(underlying.to_string()),
            coupon_frequency: None,
            day_count: None,
            option_right: None,
        })
    } else if rem.ends_with("CE") || rem.ends_with("PE") {
        let is_call = rem.ends_with("CE");
        let core = &rem[..rem.len() - 2];
        // Standard NSE option expiry prefix is 5 characters:
        // Monthly: YYMMM (e.g. 26NOV) or Weekly: YYMDD (e.g. 26D10)
        let strike_str = if core.len() > 5 { &core[5..] } else { core };
        let strike = strike_str.parse::<f64>().unwrap_or(24000.0);

        let expiry_ns = Utc::now()
            .checked_add_signed(chrono::Duration::days(7))
            .and_then(|d| d.timestamp_nanos_opt())
            .unwrap_or(0);

        Ok(Instrument {
            symbol: sym,
            kind: InstrumentType::Option,
            currency: "INR".to_string(),
            multiplier: to_ratio(1.0)?,
            tick_size: to_price(0.05)?,
            lot_size: to_qty(lot_size)?,
            expiry_ns: Some(expiry_ns),
            strike: Some(to_price(strike)?),
            maturity_ns: None,
            coupon_rate: None,
            underlying: Some(underlying.to_string()),
            coupon_frequency: None,
            day_count: None,
            option_right: Some(if is_call { OptionRight::Call } else { OptionRight::Put }),
        })
    } else {
        Err(SymbolMapError::InvalidFormat(format!("Unrecognized NSE F&O suffix in {sym}")))
    }
}

/// Maps a Binance USDⓈ-M futures symbol (e.g. `BTCUSDT`, `ETHUSDT`, or dated quarterly `BTCUSDT_261225`) into an `Instrument`.
pub fn map_binance_symbol_to_instrument(symbol: &str) -> Result<Instrument, SymbolMapError> {
    let sym = symbol.trim().to_ascii_uppercase();

    if sym.contains('_') {
        let parts: Vec<&str> = sym.split('_').collect();
        if parts.len() != 2 || parts[1].len() != 6 {
            return Err(SymbolMapError::InvalidFormat(format!("Invalid Binance quarterly format in {sym}")));
        }
        let base_pair = parts[0];
        let date_str = parts[1];
        let year = 2000
            + date_str[0..2]
                .parse::<i32>()
                .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid year in {sym}")))?;
        let month = date_str[2..4]
            .parse::<u32>()
            .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid month in {sym}")))?;
        let day = date_str[4..6]
            .parse::<u32>()
            .map_err(|_| SymbolMapError::InvalidFormat(format!("Invalid day in {sym}")))?;

        let expiry_date = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| SymbolMapError::InvalidExpiry(format!("Invalid date in {sym}")))?;
        let expiry_dt = Utc.from_utc_datetime(&expiry_date.and_hms_opt(8, 0, 0).unwrap());
        let expiry_ns = expiry_dt.timestamp_nanos_opt().unwrap_or(0);

        let (tick_size, lot_size) = if base_pair.starts_with("BTC") {
            (0.10, 0.001)
        } else if base_pair.starts_with("ETH") {
            (0.01, 0.01)
        } else {
            (0.01, 0.1)
        };

        let underlying_str = base_pair.to_string();

        Ok(Instrument {
            symbol: sym,
            kind: InstrumentType::Future,
            currency: "USDT".to_string(),
            multiplier: to_ratio(1.0)?,
            tick_size: to_price(tick_size)?,
            lot_size: to_qty(lot_size)?,
            expiry_ns: Some(expiry_ns),
            strike: None,
            maturity_ns: None,
            coupon_rate: None,
            underlying: Some(underlying_str),
            coupon_frequency: None,
            day_count: None,
            option_right: None,
        })
    } else {
        let (tick_size, lot_size) = if sym.starts_with("BTC") {
            (0.10, 0.001)
        } else if sym.starts_with("ETH") {
            (0.01, 0.01)
        } else if sym.starts_with("SOL") {
            (0.01, 0.01)
        } else {
            (0.001, 0.1)
        };

        Ok(Instrument {
            symbol: sym.clone(),
            kind: InstrumentType::Future,
            currency: "USDT".to_string(),
            multiplier: to_ratio(1.0)?,
            tick_size: to_price(tick_size)?,
            lot_size: to_qty(lot_size)?,
            expiry_ns: None,
            strike: None,
            maturity_ns: None,
            coupon_rate: None,
            underlying: Some(sym),
            coupon_frequency: None,
            day_count: None,
            option_right: None,
        })
    }
}

/// Unified dispatcher to map a venue symbol to an `Instrument`.
pub fn map_venue_symbol_to_instrument(venue: VenueId, symbol: &str) -> Result<Instrument, SymbolMapError> {
    match venue {
        VenueId::InteractiveBrokers => {
            let s = symbol.trim().to_ascii_uppercase();
            if s.starts_with("FGBL") || s.starts_with("FESX") || s.starts_with("FDAX") {
                map_eurex_symbol_to_instrument(&s)
            } else {
                map_cme_symbol_to_instrument(&s)
            }
        }
        VenueId::ZerodhaKite => map_nse_symbol_to_instrument(symbol),
        VenueId::BinanceUsdm => map_binance_symbol_to_instrument(symbol),
        VenueId::PaperSimulated => {
            if let Ok(cme) = map_cme_symbol_to_instrument(symbol) {
                Ok(cme)
            } else if let Ok(bin) = map_binance_symbol_to_instrument(symbol) {
                Ok(bin)
            } else {
                map_nse_symbol_to_instrument(symbol)
            }
        }
    }
}
