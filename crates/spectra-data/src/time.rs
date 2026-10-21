//! Exact decimal and timestamp parsing, and UTC timestamp formatting.

use spectra_core::fixed::MAX_RAW;

/// How timestamps in a file are written. Bare numbers are never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFormat {
    /// ISO 8601 text.
    #[default]
    Iso8601,
    /// Integer seconds since the epoch.
    EpochSeconds,
    /// Integer milliseconds since the epoch.
    EpochMillis,
    /// Integer microseconds since the epoch.
    EpochMicros,
    /// Integer nanoseconds since the epoch.
    EpochNanos,
}

/// Timestamp parsing options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeOptions {
    /// Format of the timestamp column.
    pub format: TimeFormat,
    /// Offset applied to ISO timestamps that carry no zone designator, in
    /// minutes east of UTC (e.g. 330 for IST). Zoned timestamps ignore it.
    pub naive_utc_offset_minutes: i32,
    /// Optional DST-aware timezone (e.g. `America/New_York`, `Europe/London`).
    /// When present, overrides `naive_utc_offset_minutes` for naive timestamps.
    pub timezone: Option<crate::timezone::Timezone>,
}

/// A parsed timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedTime {
    /// Nanoseconds since the Unix epoch, UTC.
    pub ts_ns: i64,
    /// ISO text without a zone designator.
    pub naive: bool,
}

/// The C `isspace` set (space, \t, \n, \v, \f, \r).
pub(crate) fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

pub(crate) fn trim(s: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && is_space(s[a]) {
        a += 1;
    }
    while b > a && is_space(s[b - 1]) {
        b -= 1;
    }
    &s[a..b]
}

fn pow10(n: u32) -> i128 {
    10i128.pow(n)
}

/// Exact decimal text to fixed-point raw units (1e-8). Accepts an optional
/// sign, digits with an optional decimal point, and an optional exponent
/// ("1.5e2"). Returns `None` for malformed text, anything that would need
/// rounding (more than 8 significant decimals), or values out of range.
pub fn parse_fixed(text: &str) -> Option<i64> {
    let s = trim(text.as_bytes());
    let mut i = 0;
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut mant: i128 = 0;
    let mut digits = 0;
    let mut frac: i32 = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() {
            digits += 1;
            if digits > 30 {
                return None;
            }
            mant = mant * 10 + i128::from(c - b'0');
            seen_digit = true;
            if seen_dot {
                frac += 1;
            }
        } else if c == b'.' && !seen_dot {
            seen_dot = true;
        } else {
            break;
        }
        i += 1;
    }
    if !seen_digit {
        return None;
    }
    let mut exp10: i32 = 0;
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        let mut eneg = false;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            eneg = s[i] == b'-';
            i += 1;
        }
        let mut ed = 0;
        while i < s.len() && s[i].is_ascii_digit() {
            ed += 1;
            if ed > 4 {
                return None;
            }
            exp10 = exp10 * 10 + i32::from(s[i] - b'0');
            i += 1;
        }
        if ed == 0 {
            return None;
        }
        if eneg {
            exp10 = -exp10;
        }
    }
    if i != s.len() {
        return None;
    }
    if mant == 0 {
        return Some(0);
    }
    let shift = 8 + exp10 - frac; // raw = mant * 10^shift
    if shift >= 0 {
        if shift > 30 {
            return None;
        }
        let p = pow10(shift as u32);
        if mant > i128::from(MAX_RAW) / p {
            return None;
        }
        mant *= p;
    } else {
        if -shift > 30 {
            return None; // would need rounding
        }
        let p = pow10((-shift) as u32);
        if mant % p != 0 {
            return None; // more than 8 significant decimals
        }
        mant /= p;
    }
    if mant > i128::from(MAX_RAW) {
        return None;
    }
    Some(if neg { -(mant as i64) } else { mant as i64 })
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + i64::from(doe) - 719_468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if m == 2 && is_leap(y) {
        29
    } else {
        DAYS[(m - 1) as usize]
    }
}

/// Reads exactly `n` digits at `pos`; `None` on failure.
fn read_digits(s: &[u8], pos: &mut usize, n: usize) -> Option<i64> {
    if *pos + n > s.len() {
        return None;
    }
    let mut v = 0i64;
    for &c in &s[*pos..*pos + n] {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + i64::from(c - b'0');
    }
    *pos += n;
    Some(v)
}

fn narrow_ns(v: i128) -> Option<i64> {
    if v > i128::from(MAX_RAW) || v < -i128::from(MAX_RAW) {
        None
    } else {
        Some(v as i64)
    }
}

/// ISO 8601 forms: `YYYY-MM-DD`, `YYYY-MM-DD[T ]HH:MM[:SS[.fraction]]`
/// followed by an optional `Z`, `+HH:MM`, `+HHMM` or `+HH`. Epoch formats need
/// integer text. `None` for anything invalid, including impossible dates.
pub fn parse_timestamp(text: &str, opts: &TimeOptions) -> Option<ParsedTime> {
    let s = trim(text.as_bytes());
    if s.is_empty() {
        return None;
    }

    if opts.format != TimeFormat::Iso8601 {
        let mut i = 0;
        let mut neg = false;
        if s[i] == b'-' || s[i] == b'+' {
            neg = s[i] == b'-';
            i += 1;
        }
        if i >= s.len() {
            return None;
        }
        let mut v: i128 = 0;
        for &c in &s[i..] {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + i128::from(c - b'0');
            if v > i128::from(MAX_RAW) {
                return None;
            }
        }
        let unit: i128 = match opts.format {
            TimeFormat::EpochSeconds => 1_000_000_000,
            TimeFormat::EpochMillis => 1_000_000,
            TimeFormat::EpochMicros => 1_000,
            _ => 1,
        };
        let ns = narrow_ns(if neg { -v } else { v } * unit)?;
        return Some(ParsedTime { ts_ns: ns, naive: false });
    }

    let mut pos = 0;
    let year = read_digits(s, &mut pos, 4)?;
    if pos >= s.len() || s[pos] != b'-' {
        return None;
    }
    pos += 1;
    let month = read_digits(s, &mut pos, 2)?;
    if !(1..=12).contains(&month) || pos >= s.len() || s[pos] != b'-' {
        return None;
    }
    pos += 1;
    let day = read_digits(s, &mut pos, 2)?;
    if day < 1 || day > i64::from(days_in_month(year, month as u32)) {
        return None;
    }

    let mut hour = 0i64;
    let mut minute = 0i64;
    let mut second = 0i64;
    let mut frac_ns = 0i64;
    if pos < s.len() && (s[pos] == b'T' || s[pos] == b' ') {
        pos += 1;
        hour = read_digits(s, &mut pos, 2)?;
        if hour > 23 || pos >= s.len() || s[pos] != b':' {
            return None;
        }
        pos += 1;
        minute = read_digits(s, &mut pos, 2)?;
        if minute > 59 {
            return None;
        }
        if pos < s.len() && s[pos] == b':' {
            pos += 1;
            second = read_digits(s, &mut pos, 2)?;
            if second > 59 {
                return None;
            }
            if pos < s.len() && s[pos] == b'.' {
                pos += 1;
                let mut nd = 0;
                while pos < s.len() && s[pos].is_ascii_digit() {
                    if nd >= 9 {
                        return None;
                    }
                    frac_ns = frac_ns * 10 + i64::from(s[pos] - b'0');
                    nd += 1;
                    pos += 1;
                }
                if nd == 0 {
                    return None;
                }
                while nd < 9 {
                    frac_ns *= 10;
                    nd += 1;
                }
            }
        }
    }

    let mut naive = true;
    let mut offset_min = match opts.timezone {
        Some(tz) => i64::from(tz.offset_minutes(year, month as u32, day as u32, hour as u32, minute as u32)),
        None => i64::from(opts.naive_utc_offset_minutes),
    };
    if pos < s.len() {
        if s[pos] == b'Z' {
            naive = false;
            offset_min = 0;
            pos += 1;
        } else if s[pos] == b'+' || s[pos] == b'-' {
            let neg = s[pos] == b'-';
            pos += 1;
            let oh = read_digits(s, &mut pos, 2)?;
            if oh > 23 {
                return None;
            }
            let mut om = 0;
            if pos < s.len() && s[pos] == b':' {
                pos += 1;
            }
            if pos < s.len() {
                om = read_digits(s, &mut pos, 2)?;
                if om > 59 {
                    return None;
                }
            }
            naive = false;
            offset_min = (if neg { -1 } else { 1 }) * (oh * 60 + om);
        }
    }
    if pos != s.len() {
        return None;
    }

    let days = days_from_civil(year, month as u32, day as u32);
    let secs = i128::from(days) * 86_400 + i128::from(hour * 3_600 + minute * 60 + second - offset_min * 60);
    let ns = narrow_ns(secs * 1_000_000_000 + i128::from(frac_ns))?;
    Some(ParsedTime { ts_ns: ns, naive })
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z` in UTC (fraction only when non-zero, trimmed).
pub fn format_timestamp(ts_ns: i64) -> String {
    let mut secs = ts_ns / 1_000_000_000;
    let mut nanos = ts_ns % 1_000_000_000;
    if nanos < 0 {
        nanos += 1_000_000_000;
        secs -= 1;
    }
    let mut days = secs / 86_400;
    let mut rem = secs % 86_400;
    if rem < 0 {
        rem += 86_400;
        days -= 1;
    }
    let (y, m, d) = civil_from_days(days);
    let mut out = format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60);
    if nanos != 0 {
        let frac = format!(".{nanos:09}");
        out.push_str(frac.trim_end_matches('0'));
    }
    out.push('Z');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_are_exact_or_refused() {
        assert_eq!(parse_fixed("190.25"), Some(19_025_000_000));
        assert_eq!(parse_fixed(" -0.00000001 "), Some(-1));
        assert_eq!(parse_fixed("1.5e2"), Some(15_000_000_000));
        assert_eq!(parse_fixed("+3"), Some(300_000_000));
        assert_eq!(parse_fixed("0.000000001"), None); // would need rounding
        assert_eq!(parse_fixed("1.000000010"), Some(100_000_001)); // trailing zero is exact
        assert_eq!(parse_fixed("abc"), None);
        assert_eq!(parse_fixed("1.2.3"), None);
        assert_eq!(parse_fixed("1e"), None);
        assert_eq!(parse_fixed("99999999999"), None); // out of range
        assert_eq!(parse_fixed("."), None);
        assert_eq!(parse_fixed("0e99999"), None); // more than 4 exponent digits
        assert_eq!(parse_fixed("0"), Some(0));
    }

    #[test]
    fn timestamps() {
        let iso = TimeOptions::default();
        assert_eq!(parse_timestamp("1970-01-01", &iso), Some(ParsedTime { ts_ns: 0, naive: true }));
        assert_eq!(parse_timestamp("2020-01-01T00:00:00Z", &iso).unwrap().ts_ns, 1_577_836_800_000_000_000);
        assert_eq!(
            parse_timestamp("2020-01-01T05:30:00+05:30", &iso).unwrap().ts_ns,
            1_577_836_800_000_000_000
        );
        assert_eq!(parse_timestamp("2020-01-01 05:30+0530", &iso).unwrap().ts_ns, 1_577_836_800_000_000_000);
        assert_eq!(parse_timestamp("2020-01-01T00:00:00.5Z", &iso).unwrap().ts_ns, 1_577_836_800_500_000_000);
        assert!(parse_timestamp("2021-02-29", &iso).is_none());
        assert!(parse_timestamp("2020-13-01", &iso).is_none());
        assert!(parse_timestamp("2020-01-01T24:00", &iso).is_none());
        assert!(parse_timestamp("2020-01-01T00:00:00.1234567891Z", &iso).is_none());
        let ist = TimeOptions { format: TimeFormat::Iso8601, naive_utc_offset_minutes: 330, timezone: None };
        assert_eq!(parse_timestamp("2020-01-01T05:30", &ist).unwrap().ts_ns, 1_577_836_800_000_000_000);
        let secs =
            TimeOptions { format: TimeFormat::EpochSeconds, naive_utc_offset_minutes: 0, timezone: None };
        assert_eq!(parse_timestamp("1577836800", &secs).unwrap().ts_ns, 1_577_836_800_000_000_000);
        assert!(parse_timestamp("1.5", &secs).is_none());
        assert!(parse_timestamp("99999999999999999", &secs).is_none());

        // DST-aware timezone parsing
        let ny = TimeOptions {
            format: TimeFormat::Iso8601,
            naive_utc_offset_minutes: 0,
            timezone: Some(crate::timezone::Timezone::AmericaNewYork),
        };
        // Winter: 2024-01-15 09:30 EST (UTC-5) -> 14:30 UTC
        let winter_parsed = parse_timestamp("2024-01-15 09:30:00", &ny).unwrap();
        assert_eq!(format_timestamp(winter_parsed.ts_ns), "2024-01-15T14:30:00Z");
        // Summer: 2024-06-15 09:30 EDT (UTC-4) -> 13:30 UTC
        let summer_parsed = parse_timestamp("2024-06-15 09:30:00", &ny).unwrap();
        assert_eq!(format_timestamp(summer_parsed.ts_ns), "2024-06-15T13:30:00Z");
    }

    #[test]
    fn formatting_round_trips() {
        for ts in [0, 1, -1, 1_577_836_800_500_000_000, -86_400_000_000_001, 9_000_000_000_123_456_789] {
            let s = format_timestamp(ts);
            assert_eq!(parse_timestamp(&s, &TimeOptions::default()).map(|p| p.ts_ns), Some(ts), "{s}");
        }
        assert_eq!(format_timestamp(1_577_836_800_500_000_000), "2020-01-01T00:00:00.5Z");
    }
}
