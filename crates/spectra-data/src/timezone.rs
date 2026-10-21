//! Deterministic, standard-library-only timezone and Daylight Saving Time (DST)
//! conversions for major financial market hubs.
//!
//! Handles conversions from local naive calendar dates/times to UTC offsets
//! taking into account historical and current daylight saving transition rules.

use crate::time::days_from_civil;

/// Supported market timezones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Timezone {
    /// Universal Coordinated Time (UTC+0, no DST).
    #[default]
    Utc,
    /// US Eastern Time (New York: EST UTC-5, EDT UTC-4).
    AmericaNewYork,
    /// US Central Time (Chicago: CST UTC-6, CDT UTC-5).
    AmericaChicago,
    /// UK London Time (London: GMT UTC+0, BST UTC+1).
    EuropeLondon,
    /// Central European Time (Frankfurt/Paris: CET UTC+1, CEST UTC+2).
    EuropeBerlin,
    /// Japan Standard Time (Tokyo: JST UTC+9, no DST).
    AsiaTokyo,
    /// Hong Kong / China Standard Time (HKT/CST UTC+8, no DST).
    AsiaHongKong,
    /// India Standard Time (Mumbai: IST UTC+5:30, no DST).
    AsiaKolkata,
    /// Custom fixed offset in minutes east of UTC.
    Fixed(i32),
}

impl Timezone {
    /// Canonical identifier for display and serialization.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Utc => "UTC",
            Self::AmericaNewYork => "America/New_York",
            Self::AmericaChicago => "America/Chicago",
            Self::EuropeLondon => "Europe/London",
            Self::EuropeBerlin => "Europe/Berlin",
            Self::AsiaTokyo => "Asia/Tokyo",
            Self::AsiaHongKong => "Asia/Hong_Kong",
            Self::AsiaKolkata => "Asia/Kolkata",
            Self::Fixed(_) => "Fixed",
        }
    }

    /// Parses a timezone name or common alias (case-insensitive, trimmed).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim();
        let lower = clean.to_ascii_lowercase();
        match lower.as_str() {
            "utc" | "z" | "gmt" => Some(Self::Utc),
            "america/new_york" | "us/eastern" | "est" | "edt" | "new_york" | "ny" => {
                Some(Self::AmericaNewYork)
            }
            "america/chicago" | "us/central" | "cst" | "cdt" | "chicago" => Some(Self::AmericaChicago),
            "europe/london" | "gb" | "uk" | "london" | "bst" => Some(Self::EuropeLondon),
            "europe/berlin" | "cet" | "cest" | "berlin" | "frankfurt" | "europe/paris" => {
                Some(Self::EuropeBerlin)
            }
            "asia/tokyo" | "jst" | "tokyo" | "japan" => Some(Self::AsiaTokyo),
            "asia/hong_kong" | "asia/shanghai" | "hkt" | "hong_kong" | "shanghai" | "china" => {
                Some(Self::AsiaHongKong)
            }
            "asia/kolkata" | "asia/calcutta" | "ist" | "kolkata" | "mumbai" | "india" => {
                Some(Self::AsiaKolkata)
            }
            _ => {
                // Try parsing as fixed minutes or offset string like "+05:30" or "-0400"
                parse_fixed_offset(clean).map(Self::Fixed)
            }
        }
    }

    /// Computes the UTC offset in minutes (east of UTC) for a naive local date and time.
    ///
    /// For example, `America/New_York` returns `-300` (-5h EST) in winter and
    /// `-240` (-4h EDT) in summer.
    #[must_use]
    pub fn offset_minutes(self, year: i64, month: u32, day: u32, hour: u32, minute: u32) -> i32 {
        match self {
            Self::Utc => 0,
            Self::AmericaNewYork => us_eastern_offset(year, month, day, hour, minute),
            Self::AmericaChicago => us_central_offset(year, month, day, hour, minute),
            Self::EuropeLondon => uk_london_offset(year, month, day, hour, minute),
            Self::EuropeBerlin => eu_central_offset(year, month, day, hour, minute),
            Self::AsiaTokyo => 540,    // UTC+9
            Self::AsiaHongKong => 480, // UTC+8
            Self::AsiaKolkata => 330,  // UTC+5:30
            Self::Fixed(m) => m,
        }
    }

    /// Computes the UTC offset in minutes given a UTC timestamp in seconds.
    #[must_use]
    pub fn offset_minutes_for_utc(self, utc_secs: i64) -> i32 {
        match self {
            Self::Utc => 0,
            Self::AsiaTokyo => 540,
            Self::AsiaHongKong => 480,
            Self::AsiaKolkata => 330,
            Self::Fixed(m) => m,
            Self::AmericaNewYork => {
                // Estimate local time with standard offset (-5h = -18,000s)
                let est_secs = utc_secs - 18_000;
                let days = est_secs.div_euclid(86_400);
                let rem = est_secs.rem_euclid(86_400) as u32;
                let (y, m, d) = crate::time::civil_from_days(days);
                let hour = rem / 3600;
                let minute = (rem % 3600) / 60;
                us_eastern_offset(y, m, d, hour, minute)
            }
            Self::AmericaChicago => {
                let est_secs = utc_secs - 21_600;
                let days = est_secs.div_euclid(86_400);
                let rem = est_secs.rem_euclid(86_400) as u32;
                let (y, m, d) = crate::time::civil_from_days(days);
                let hour = rem / 3600;
                let minute = (rem % 3600) / 60;
                us_central_offset(y, m, d, hour, minute)
            }
            Self::EuropeLondon => {
                let days = utc_secs.div_euclid(86_400);
                let rem = utc_secs.rem_euclid(86_400) as u32;
                let (y, m, d) = crate::time::civil_from_days(days);
                let hour = rem / 3600;
                let minute = (rem % 3600) / 60;
                uk_london_offset(y, m, d, hour, minute)
            }
            Self::EuropeBerlin => {
                let est_secs = utc_secs + 3600;
                let days = est_secs.div_euclid(86_400);
                let rem = est_secs.rem_euclid(86_400) as u32;
                let (y, m, d) = crate::time::civil_from_days(days);
                let hour = rem / 3600;
                let minute = (rem % 3600) / 60;
                eu_central_offset(y, m, d, hour, minute)
            }
        }
    }
}

/// Day of week for a Gregorian date: 0 = Sunday, 1 = Monday, ..., 6 = Saturday.
pub(crate) fn weekday(year: i64, month: u32, day: u32) -> u32 {
    let days = days_from_civil(year, month, day);
    // 1970-01-01 was Thursday (4).
    (days + 4).rem_euclid(7) as u32
}

/// Nth Sunday of a month (1-based nth: 1 = first Sunday, 2 = second Sunday).
fn nth_sunday(year: i64, month: u32, n: u32) -> u32 {
    let wday_first = weekday(year, month, 1);
    let first_sunday = 1 + (7 - wday_first) % 7;
    first_sunday + (n - 1) * 7
}

/// Last Sunday of a month.
fn last_sunday(year: i64, month: u32, days_in_month: u32) -> u32 {
    let wday_last = weekday(year, month, days_in_month);
    days_in_month - wday_last
}

/// US Eastern Time:
/// - 2007+: 2nd Sunday in March 02:00 -> 1st Sunday in Nov 02:00
/// - Standard: -300 min (EST), Daylight: -240 min (EDT)
fn us_eastern_offset(year: i64, month: u32, day: u32, hour: u32, _minute: u32) -> i32 {
    if is_us_dst(year, month, day, hour) {
        -240
    } else {
        -300
    }
}

/// US Central Time:
/// - Standard: -360 min (CST), Daylight: -300 min (CDT)
fn us_central_offset(year: i64, month: u32, day: u32, hour: u32, _minute: u32) -> i32 {
    if is_us_dst(year, month, day, hour) {
        -300
    } else {
        -360
    }
}

/// Check if a US local civil time falls in Daylight Saving Time.
fn is_us_dst(year: i64, month: u32, day: u32, hour: u32) -> bool {
    use std::cmp::Ordering;
    if year >= 2007 {
        // Starts 2nd Sunday of March at 02:00
        // Ends 1st Sunday of November at 02:00
        let dst_start_day = nth_sunday(year, 3, 2);
        let dst_end_day = nth_sunday(year, 11, 1);

        match month {
            4..=10 => true,
            3 => match day.cmp(&dst_start_day) {
                Ordering::Less => false,
                Ordering::Greater => true,
                Ordering::Equal => hour >= 2,
            },
            11 => match day.cmp(&dst_end_day) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => hour < 2,
            },
            _ => false,
        }
    } else {
        // Pre-2007: 1st Sunday of April to last Sunday of October
        let dst_start_day = nth_sunday(year, 4, 1);
        let dst_end_day = last_sunday(year, 10, 31);
        match month {
            5..=9 => true,
            4 => match day.cmp(&dst_start_day) {
                Ordering::Less => false,
                Ordering::Greater => true,
                Ordering::Equal => hour >= 2,
            },
            10 => match day.cmp(&dst_end_day) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => hour < 2,
            },
            _ => false,
        }
    }
}

/// UK London Time:
/// - Last Sunday in March 01:00 UTC -> Last Sunday in October 02:00 local (01:00 UTC)
/// - Standard: 0 min (GMT), Daylight: +60 min (BST)
fn uk_london_offset(year: i64, month: u32, day: u32, hour: u32, _minute: u32) -> i32 {
    use std::cmp::Ordering;
    let dst_start_day = last_sunday(year, 3, 31);
    let dst_end_day = last_sunday(year, 10, 31);

    let is_dst = match month {
        4..=9 => true,
        3 => match day.cmp(&dst_start_day) {
            Ordering::Less => false,
            Ordering::Greater => true,
            Ordering::Equal => hour >= 1,
        },
        10 => match day.cmp(&dst_end_day) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => hour < 2,
        },
        _ => false,
    };

    if is_dst {
        60
    } else {
        0
    }
}

/// Central European Time (CET / CEST):
/// - Standard: +60 min (CET), Daylight: +120 min (CEST)
fn eu_central_offset(year: i64, month: u32, day: u32, hour: u32, _minute: u32) -> i32 {
    use std::cmp::Ordering;
    let dst_start_day = last_sunday(year, 3, 31);
    let dst_end_day = last_sunday(year, 10, 31);

    let is_dst = match month {
        4..=9 => true,
        3 => match day.cmp(&dst_start_day) {
            Ordering::Less => false,
            Ordering::Greater => true,
            Ordering::Equal => hour >= 2,
        },
        10 => match day.cmp(&dst_end_day) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => hour < 3,
        },
        _ => false,
    };

    if is_dst {
        120
    } else {
        60
    }
}

/// Parses strings like "+05:30", "-0400", "+00:00", "330", "-300".
fn parse_fixed_offset(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // If it's a signed string with colon or 4 digits: e.g. "+05:30", "-0500"
    if (bytes[0] == b'+' || bytes[0] == b'-') && (s.contains(':') || bytes.len() == 5) {
        let sign = if bytes[0] == b'-' { -1 } else { 1 };
        let rest = &s[1..];
        if let Some((h_str, m_str)) = rest.split_once(':') {
            let h = h_str.parse::<i32>().ok()?;
            let m = m_str.parse::<i32>().ok()?;
            if (0..24).contains(&h) && (0..60).contains(&m) {
                return Some(sign * (h * 60 + m));
            }
        } else if rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit()) {
            let val = rest.parse::<i32>().ok()?;
            let h = val / 100;
            let m = val % 100;
            if (0..24).contains(&h) && (0..60).contains(&m) {
                return Some(sign * (h * 60 + m));
            }
        }
    }
    // Pure integer minutes, e.g. "330", "-300", "0"
    if let Ok(mins) = s.parse::<i32>() {
        if (-840..=840).contains(&mins) {
            return Some(mins);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_eastern_dst_transitions() {
        let tz = Timezone::AmericaNewYork;
        // 2024: 2nd Sunday in March is March 10; 1st Sunday in November is Nov 3.
        // Winter before DST: Jan 15 -> EST (-300 min)
        assert_eq!(tz.offset_minutes(2024, 1, 15, 9, 30), -300);
        // March 10 at 01:00 -> still EST (-300 min)
        assert_eq!(tz.offset_minutes(2024, 3, 10, 1, 0), -300);
        // March 10 at 03:00 -> EDT (-240 min)
        assert_eq!(tz.offset_minutes(2024, 3, 10, 3, 0), -240);
        // Summer: July 4 -> EDT (-240 min)
        assert_eq!(tz.offset_minutes(2024, 7, 4, 12, 0), -240);
        // Nov 3 at 01:00 -> EDT (-240 min)
        assert_eq!(tz.offset_minutes(2024, 11, 3, 1, 0), -240);
        // Nov 3 at 03:00 -> EST (-300 min)
        assert_eq!(tz.offset_minutes(2024, 11, 3, 3, 0), -300);
        // Winter: Dec 25 -> EST (-300 min)
        assert_eq!(tz.offset_minutes(2024, 12, 25, 9, 30), -300);
    }

    #[test]
    fn london_dst_transitions() {
        let tz = Timezone::EuropeLondon;
        // 2024: Last Sunday in March is March 31; Last Sunday in Oct is Oct 27.
        assert_eq!(tz.offset_minutes(2024, 1, 1, 12, 0), 0);
        assert_eq!(tz.offset_minutes(2024, 3, 31, 0, 30), 0);
        assert_eq!(tz.offset_minutes(2024, 3, 31, 2, 0), 60);
        assert_eq!(tz.offset_minutes(2024, 6, 1, 12, 0), 60);
        assert_eq!(tz.offset_minutes(2024, 10, 27, 1, 0), 60);
        assert_eq!(tz.offset_minutes(2024, 10, 27, 3, 0), 0);
    }

    #[test]
    fn fixed_timezones() {
        assert_eq!(Timezone::AsiaTokyo.offset_minutes(2024, 6, 1, 9, 0), 540);
        assert_eq!(Timezone::AsiaKolkata.offset_minutes(2024, 6, 1, 9, 0), 330);
        assert_eq!(Timezone::AsiaHongKong.offset_minutes(2024, 6, 1, 9, 0), 480);
        assert_eq!(Timezone::Utc.offset_minutes(2024, 6, 1, 9, 0), 0);
    }

    #[test]
    fn timezone_parsing() {
        assert_eq!(Timezone::parse("America/New_York"), Some(Timezone::AmericaNewYork));
        assert_eq!(Timezone::parse("US/Eastern"), Some(Timezone::AmericaNewYork));
        assert_eq!(Timezone::parse("EDT"), Some(Timezone::AmericaNewYork));
        assert_eq!(Timezone::parse("Europe/London"), Some(Timezone::EuropeLondon));
        assert_eq!(Timezone::parse("UTC"), Some(Timezone::Utc));
        assert_eq!(Timezone::parse("Asia/Kolkata"), Some(Timezone::AsiaKolkata));
        assert_eq!(Timezone::parse("+05:30"), Some(Timezone::Fixed(330)));
        assert_eq!(Timezone::parse("-0500"), Some(Timezone::Fixed(-300)));
        assert_eq!(Timezone::parse("invalid"), None);
    }
}
