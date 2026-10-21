//! Exchange calendars, trading sessions, holidays, and early closes.
//!
//! Provides deterministic session hours and market closure rules for major
//! financial exchanges in standard-library-only Rust.

use crate::time::{civil_from_days, days_from_civil};
use crate::timezone::{weekday, Timezone};

/// Supported exchange calendars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Exchange {
    /// 24/7 continuous trading (crypto, FX without weekend breaks).
    #[default]
    Continuous24_7,
    /// New York Stock Exchange and NASDAQ (US Equities).
    Xnys,
    /// Chicago Mercantile Exchange (US Futures, equity index).
    Xcme,
    /// London Stock Exchange (UK Equities).
    Xlon,
    /// Tokyo Stock Exchange (Japan Equities).
    Xtks,
    /// National Stock Exchange of India (India Equities).
    Xnse,
}

impl Exchange {
    /// Short MIC or canonical identifier.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Continuous24_7 => "24/7",
            Self::Xnys => "XNYS",
            Self::Xcme => "XCME",
            Self::Xlon => "XLON",
            Self::Xtks => "XTKS",
            Self::Xnse => "XNSE",
        }
    }

    /// Full descriptive name.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Continuous24_7 => "Continuous 24/7 (Crypto)",
            Self::Xnys => "New York Stock Exchange / NASDAQ",
            Self::Xcme => "Chicago Mercantile Exchange (US Futures)",
            Self::Xlon => "London Stock Exchange",
            Self::Xtks => "Tokyo Stock Exchange",
            Self::Xnse => "National Stock Exchange of India",
        }
    }

    /// Primary operating timezone for this exchange.
    #[must_use]
    pub fn timezone(self) -> Timezone {
        match self {
            Self::Continuous24_7 => Timezone::Utc,
            Self::Xnys => Timezone::AmericaNewYork,
            Self::Xcme => Timezone::AmericaChicago,
            Self::Xlon => Timezone::EuropeLondon,
            Self::Xtks => Timezone::AsiaTokyo,
            Self::Xnse => Timezone::AsiaKolkata,
        }
    }

    /// Parses an exchange code or common alias (case-insensitive, trimmed).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim().to_ascii_uppercase();
        match clean.as_str() {
            "24/7" | "CRYPTO" | "CONTINUOUS" | "NONE" => Some(Self::Continuous24_7),
            "XNYS" | "NYSE" | "XNAS" | "NASDAQ" | "US_EQUITIES" | "US" => Some(Self::Xnys),
            "XCME" | "CME" | "GLOBEX" | "FUTURES" => Some(Self::Xcme),
            "XLON" | "LSE" | "UK_EQUITIES" | "UK" => Some(Self::Xlon),
            "XTKS" | "TSE" | "JPX" | "TOKYO" | "JAPAN" => Some(Self::Xtks),
            "XNSE" | "NSE" | "BSE" | "INDIA" | "NIFTY" => Some(Self::Xnse),
            _ => None,
        }
    }

    /// Checks if a local calendar date is an open trading day.
    #[must_use]
    pub fn is_trading_day(self, year: i64, month: u32, day: u32) -> bool {
        match self {
            Self::Continuous24_7 => true,
            Self::Xnys => is_us_trading_day(year, month, day),
            Self::Xcme => is_cme_trading_day(year, month, day),
            Self::Xlon => is_uk_trading_day(year, month, day),
            Self::Xtks => is_tse_trading_day(year, month, day),
            Self::Xnse => is_nse_trading_day(year, month, day),
        }
    }

    /// Checks if a local calendar date has an early close.
    #[must_use]
    pub fn is_early_close(self, year: i64, month: u32, day: u32) -> bool {
        match self {
            Self::Continuous24_7 | Self::Xtks | Self::Xnse => false,
            Self::Xnys => is_us_early_close(year, month, day),
            Self::Xcme => is_cme_early_close(year, month, day),
            Self::Xlon => is_uk_early_close(year, month, day),
        }
    }

    /// Trading sessions for a local date, as slices of `(start_seconds, end_seconds)`
    /// from midnight local time. Empty when market is closed all day.
    #[must_use]
    pub fn session_intervals(self, year: i64, month: u32, day: u32) -> Vec<(u32, u32)> {
        if !self.is_trading_day(year, month, day) {
            return Vec::new();
        }
        match self {
            Self::Continuous24_7 => vec![(0, 86_400)],
            Self::Xnys => {
                let open = 9 * 3600 + 30 * 60; // 09:30
                let close = if self.is_early_close(year, month, day) {
                    13 * 3600 // 13:00 early close
                } else {
                    16 * 3600 // 16:00 regular close
                };
                vec![(open, close)]
            }
            Self::Xcme => {
                // CME Equity Index Futures:
                // Sunday opens 17:00 CT. Monday-Thursday: open all day except 16:00-17:00 CT break.
                // Friday closes at 16:00 CT (or early close 12:00 / 12:15 CT on holidays).
                let w = weekday(year, month, day);
                let early = self.is_early_close(year, month, day);
                if w == 0 {
                    // Sunday
                    vec![(17 * 3600, 86_400)]
                } else if w == 5 {
                    // Friday
                    let close = if early { 12 * 3600 + 15 * 60 } else { 16 * 3600 };
                    vec![(0, close)]
                } else {
                    // Mon - Thu
                    vec![(0, 16 * 3600), (17 * 3600, 86_400)]
                }
            }
            Self::Xlon => {
                let open = 8 * 3600; // 08:00
                let close = if self.is_early_close(year, month, day) {
                    12 * 3600 + 30 * 60 // 12:30
                } else {
                    16 * 3600 + 30 * 60 // 16:30
                };
                vec![(open, close)]
            }
            Self::Xtks => {
                // Morning session: 09:00 - 11:30
                // Afternoon session: 12:30 - 15:30 (closing at 15:30)
                vec![(9 * 3600, 11 * 3600 + 30 * 60), (12 * 3600 + 30 * 60, 15 * 3600 + 30 * 60)]
            }
            Self::Xnse => {
                // Regular session: 09:15 - 15:30 IST
                vec![(9 * 3600 + 15 * 60, 15 * 3600 + 30 * 60)]
            }
        }
    }

    /// Checks whether an exact UTC timestamp falls within an open session.
    #[must_use]
    pub fn is_session_open(self, ts_ns: i64) -> bool {
        if self == Self::Continuous24_7 {
            return true;
        }
        let tz = self.timezone();
        let utc_secs = ts_ns / 1_000_000_000;
        let offset_mins = tz.offset_minutes_for_utc(utc_secs);
        let local_secs = utc_secs + i64::from(offset_mins) * 60;
        let days = local_secs.div_euclid(86_400);
        let sec_of_day = local_secs.rem_euclid(86_400) as u32;

        let (y, m, d) = civil_from_days(days);
        for (open, close) in self.session_intervals(y, m, d) {
            // Bars are conventionally stamped at the start or end of the bar interval;
            // a bar at exact close is valid (e.g. 16:00 closing bar).
            if sec_of_day >= open && sec_of_day <= close {
                return true;
            }
        }
        false
    }

    /// Computes the elapsed active trading time (in nanoseconds) between two timestamps.
    /// Returns 0 if `end_ts_ns <= start_ts_ns`.
    #[must_use]
    pub fn trading_time_between(self, start_ts_ns: i64, end_ts_ns: i64) -> i64 {
        if end_ts_ns <= start_ts_ns {
            return 0;
        }
        if self == Self::Continuous24_7 {
            return end_ts_ns - start_ts_ns;
        }

        let tz = self.timezone();
        let start_sec = start_ts_ns / 1_000_000_000;
        let end_sec = end_ts_ns / 1_000_000_000;

        let start_offset = tz.offset_minutes_for_utc(start_sec);
        let end_offset = tz.offset_minutes_for_utc(end_sec);

        let start_local = start_sec + i64::from(start_offset) * 60;
        let end_local = end_sec + i64::from(end_offset) * 60;

        let start_day = start_local.div_euclid(86_400);
        let end_day = end_local.div_euclid(86_400);

        let start_sod = start_local.rem_euclid(86_400) as u32;
        let end_sod = end_local.rem_euclid(86_400) as u32;

        let mut total_secs = 0i64;
        for day_idx in start_day..=end_day {
            let (y, m, d) = civil_from_days(day_idx);
            let intervals = self.session_intervals(y, m, d);
            for (open, close) in intervals {
                let seg_start = if day_idx == start_day { open.max(start_sod) } else { open };
                let seg_end = if day_idx == end_day { close.min(end_sod) } else { close };
                if seg_end > seg_start {
                    total_secs += i64::from(seg_end - seg_start);
                }
            }
        }
        total_secs * 1_000_000_000
    }
}

// ---------------------------------------------------------------------------
// Holiday calculations
// ---------------------------------------------------------------------------

/// Anonymous Gregorian algorithm for Easter Sunday (Meeus/Jones/Butcher).
fn easter_sunday(year: i64) -> (u32, u32) {
    let a = (year % 19) as u32;
    let b = (year / 100) as u32;
    let c = (year % 100) as u32;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    (month, day)
}

/// Good Friday: Friday before Easter Sunday.
fn is_good_friday(year: i64, month: u32, day: u32) -> bool {
    let (em, ed) = easter_sunday(year);
    let easter_days = days_from_civil(year, em, ed);
    let target_days = days_from_civil(year, month, day);
    target_days == easter_days - 2
}

/// Easter Monday: Monday after Easter Sunday.
fn is_easter_monday(year: i64, month: u32, day: u32) -> bool {
    let (em, ed) = easter_sunday(year);
    let easter_days = days_from_civil(year, em, ed);
    let target_days = days_from_civil(year, month, day);
    target_days == easter_days + 1
}

/// Checks if a date is a weekend (Saturday or Sunday).
pub(crate) fn is_weekend(year: i64, month: u32, day: u32) -> bool {
    let w = weekday(year, month, day);
    w == 0 || w == 6
}

/// US fixed holiday observed date (Sunday -> Monday, Saturday -> Friday).
fn is_us_observed_holiday(
    year: i64,
    month: u32,
    day: u32,
    hol_month: u32,
    hol_day: u32,
    observe_saturday: bool,
) -> bool {
    let w = weekday(year, hol_month, hol_day);
    if month == hol_month && day == hol_day && (1..=5).contains(&w) {
        return true;
    }
    // Sunday observed on Monday
    if w == 0 {
        let actual_days = days_from_civil(year, hol_month, hol_day);
        let test_days = days_from_civil(year, month, day);
        if test_days == actual_days + 1 {
            return true;
        }
    }
    // Saturday observed on Friday
    if observe_saturday && w == 6 {
        let actual_days = days_from_civil(year, hol_month, hol_day);
        let test_days = days_from_civil(year, month, day);
        if test_days == actual_days - 1 {
            return true;
        }
    }
    false
}

/// US Equities (NYSE / NASDAQ) trading day check.
fn is_us_trading_day(year: i64, month: u32, day: u32) -> bool {
    if is_weekend(year, month, day) {
        return false;
    }
    let w = weekday(year, month, day);

    // New Year's Day (Jan 1, observed Monday if Sun; Saturday New Year is not observed in Dec)
    if is_us_observed_holiday(year, month, day, 1, 1, false) {
        return false;
    }
    // Martin Luther King Jr. Day (3rd Monday in Jan, since 1998)
    if year >= 1998 && month == 1 && w == 1 && (15..=21).contains(&day) {
        return false;
    }
    // Washington's Birthday / Presidents' Day (3rd Monday in Feb)
    if month == 2 && w == 1 && (15..=21).contains(&day) {
        return false;
    }
    // Good Friday
    if is_good_friday(year, month, day) {
        return false;
    }
    // Memorial Day (last Monday in May)
    if month == 5 && w == 1 && day >= 25 {
        return false;
    }
    // Juneteenth (June 19, since 2022)
    if year >= 2022 && is_us_observed_holiday(year, month, day, 6, 19, true) {
        return false;
    }
    // Independence Day (July 4)
    if is_us_observed_holiday(year, month, day, 7, 4, true) {
        return false;
    }
    // Labor Day (1st Monday in September)
    if month == 9 && w == 1 && (1..=7).contains(&day) {
        return false;
    }
    // Thanksgiving Day (4th Thursday in November)
    if month == 11 && w == 4 && (22..=28).contains(&day) {
        return false;
    }
    // Christmas Day (Dec 25)
    if is_us_observed_holiday(year, month, day, 12, 25, true) {
        return false;
    }

    true
}

/// US Early Close days (closes at 13:00 Eastern):
/// - Day after Thanksgiving (Black Friday)
/// - July 3 when July 4 falls on Tuesday-Friday
/// - Christmas Eve (Dec 24) when Dec 24 is a weekday and Christmas is observed on a weekday
fn is_us_early_close(year: i64, month: u32, day: u32) -> bool {
    let w = weekday(year, month, day);
    if is_weekend(year, month, day) {
        return false;
    }
    // Day after Thanksgiving: 4th Friday in November
    if month == 11 && w == 5 && (23..=29).contains(&day) {
        return true;
    }
    // July 3 early close when July 4 is Tue, Wed, Thu, Fri
    if month == 7 && day == 3 && (1..=4).contains(&w) {
        return true;
    }
    // Christmas Eve (Dec 24) when weekday
    if month == 12 && day == 24 && (1..=4).contains(&w) {
        return true;
    }
    false
}

/// CME Equity Index Futures trading day check.
fn is_cme_trading_day(year: i64, month: u32, day: u32) -> bool {
    let w = weekday(year, month, day);
    // Saturday is closed
    if w == 6 {
        return false;
    }
    // Major full holiday closures: New Year's Day and Christmas Day
    if is_us_observed_holiday(year, month, day, 1, 1, false)
        || is_us_observed_holiday(year, month, day, 12, 25, true)
    {
        return false;
    }
    true
}

fn is_cme_early_close(year: i64, month: u32, day: u32) -> bool {
    is_us_early_close(year, month, day)
}

/// UK (London Stock Exchange) trading day check.
fn is_uk_trading_day(year: i64, month: u32, day: u32) -> bool {
    if is_weekend(year, month, day) {
        return false;
    }
    let w = weekday(year, month, day);

    // New Year's Day (observed Mon if weekend)
    if is_us_observed_holiday(year, month, day, 1, 1, false) {
        return false;
    }
    // Good Friday & Easter Monday
    if is_good_friday(year, month, day) || is_easter_monday(year, month, day) {
        return false;
    }
    // Early May Bank Holiday (1st Monday in May)
    if month == 5 && w == 1 && (1..=7).contains(&day) {
        return false;
    }
    // Spring Bank Holiday (last Monday in May)
    if month == 5 && w == 1 && day >= 25 {
        return false;
    }
    // Summer Bank Holiday (last Monday in August)
    if month == 8 && w == 1 && day >= 25 {
        return false;
    }
    // Christmas Day and Boxing Day (Dec 25 & 26, observed Mon/Tue if weekend)
    if month == 12 && (day == 25 || day == 26) {
        return false;
    }
    // Dec 27/28 substitute bank holidays when Christmas or Boxing day was weekend
    if month == 12 && (day == 27 || day == 28) && (w == 1 || w == 2) {
        let w25 = weekday(year, 12, 25);
        if w25 == 0 || w25 == 6 {
            return false;
        }
    }

    true
}

fn is_uk_early_close(year: i64, month: u32, day: u32) -> bool {
    if is_weekend(year, month, day) {
        return false;
    }
    // Dec 24 and Dec 31
    month == 12 && (day == 24 || day == 31)
}

/// Tokyo Stock Exchange (TSE) trading day check.
fn is_tse_trading_day(year: i64, month: u32, day: u32) -> bool {
    if is_weekend(year, month, day) {
        return false;
    }
    let w = weekday(year, month, day);

    // New Year holidays (Jan 1, 2, 3)
    if month == 1 && (1..=3).contains(&day) {
        return false;
    }
    // Coming of Age Day (2nd Mon in Jan)
    if month == 1 && w == 1 && (8..=14).contains(&day) {
        return false;
    }
    // National Foundation Day (Feb 11)
    if month == 2 && (day == 11 || (day == 12 && w == 1)) {
        return false;
    }
    // Emperor's Birthday (Feb 23, since 2020)
    if year >= 2020 && month == 2 && (day == 23 || (day == 24 && w == 1)) {
        return false;
    }
    // Golden Week: Showa Day (Apr 29), Constitution Memorial Day (May 3), Greenery Day (May 4), Children's Day (May 5)
    if (month == 4 && day == 29) || (month == 5 && (3..=6).contains(&day)) {
        return false;
    }
    // Mountain Day (Aug 11)
    if month == 8 && (day == 11 || (day == 12 && w == 1)) {
        return false;
    }
    // Culture Day (Nov 3), Labor Thanksgiving (Nov 23)
    if month == 11 && (day == 3 || day == 23 || (day == 4 && w == 1) || (day == 24 && w == 1)) {
        return false;
    }
    // Market closing day Dec 31
    if month == 12 && day == 31 {
        return false;
    }

    true
}

/// National Stock Exchange of India (NSE) trading day check.
fn is_nse_trading_day(year: i64, month: u32, day: u32) -> bool {
    if is_weekend(year, month, day) {
        return false;
    }
    // Fixed national holidays
    // Republic Day (Jan 26)
    if month == 1 && day == 26 {
        return false;
    }
    // Independence Day (Aug 15)
    if month == 8 && day == 15 {
        return false;
    }
    // Mahatma Gandhi Jayanti (Oct 2)
    if month == 10 && day == 2 {
        return false;
    }
    // Good Friday
    if is_good_friday(year, month, day) {
        return false;
    }
    // Christmas (Dec 25)
    if month == 12 && day == 25 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_equities_trading_days_and_holidays() {
        let cal = Exchange::Xnys;
        // 2024 New Year's Day (Monday Jan 1) is closed
        assert!(!cal.is_trading_day(2024, 1, 1));
        // Tuesday Jan 2 is open
        assert!(cal.is_trading_day(2024, 1, 2));
        // MLK Day: Jan 15, 2024 (3rd Monday) is closed
        assert!(!cal.is_trading_day(2024, 1, 15));
        // Good Friday: Mar 29, 2024 is closed
        assert!(!cal.is_trading_day(2024, 3, 29));
        // Juneteenth: June 19, 2024 is closed
        assert!(!cal.is_trading_day(2024, 6, 19));
        // July 4, 2024 is closed
        assert!(!cal.is_trading_day(2024, 7, 4));
        // July 3, 2024 has early close
        assert!(cal.is_early_close(2024, 7, 3));
        // Black Friday Nov 29, 2024 has early close
        assert!(cal.is_early_close(2024, 11, 29));
    }

    #[test]
    fn us_equities_session_hours() {
        let cal = Exchange::Xnys;
        // Regular session on Jan 2, 2024: 09:30 to 16:00
        let sessions = cal.session_intervals(2024, 1, 2);
        assert_eq!(sessions, vec![(9 * 3600 + 30 * 60, 16 * 3600)]);

        // Early close on July 3, 2024: 09:30 to 13:00
        let early = cal.session_intervals(2024, 7, 3);
        assert_eq!(early, vec![(9 * 3600 + 30 * 60, 13 * 3600)]);

        // Weekend: empty
        assert!(cal.session_intervals(2024, 1, 6).is_empty());
    }

    #[test]
    fn us_equities_is_session_open_check() {
        let cal = Exchange::Xnys;
        // 2024-01-02 09:30:00 US Eastern (14:30:00 UTC)
        // 14:30 UTC on 2024-01-02 in ns:
        let days = days_from_civil(2024, 1, 2);
        let open_utc_ns = (days * 86_400 + 14 * 3600 + 30 * 60) * 1_000_000_000;
        assert!(cal.is_session_open(open_utc_ns));

        // 2024-01-02 09:00:00 US Eastern (pre-market, regular session is closed)
        let pre_utc_ns = (days * 86_400 + 14 * 3600) * 1_000_000_000;
        assert!(!cal.is_session_open(pre_utc_ns));

        // 2024-01-02 16:00:00 US Eastern (closing bell, included)
        let close_utc_ns = (days * 86_400 + 21 * 3600) * 1_000_000_000;
        assert!(cal.is_session_open(close_utc_ns));

        // 2024-01-02 16:01:00 US Eastern (post-market, closed)
        let after_utc_ns = (days * 86_400 + 21 * 3600 + 60) * 1_000_000_000;
        assert!(!cal.is_session_open(after_utc_ns));
    }

    #[test]
    fn trading_time_between_skips_weekends_and_nights() {
        let cal = Exchange::Xnys;
        // Friday 2024-01-05 16:00:00 ET (21:00:00 UTC)
        let fri_days = days_from_civil(2024, 1, 5);
        let fri_close = (fri_days * 86_400 + 21 * 3600) * 1_000_000_000;

        // Monday 2024-01-08 09:30:00 ET (14:30:00 UTC)
        let mon_days = days_from_civil(2024, 1, 8);
        let mon_open = (mon_days * 86_400 + 14 * 3600 + 30 * 60) * 1_000_000_000;

        // Elapsed trading time across the weekend should be ZERO
        assert_eq!(cal.trading_time_between(fri_close, mon_open), 0);

        // 1 hour within Monday session: 09:30 to 10:30
        let mon_1030 = (mon_days * 86_400 + 15 * 3600 + 30 * 60) * 1_000_000_000;
        assert_eq!(cal.trading_time_between(mon_open, mon_1030), 3600 * 1_000_000_000);
    }
}
