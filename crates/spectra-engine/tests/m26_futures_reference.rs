//! M26 task 3f (part 3, futures roll): reference-value tests for `roll_ts`
//! (`src/futures.rs`), which moves back n business days from expiry.
//!
//! These use the weekday-only calendar (no exchange), so the expected dates are
//! closed forms: 2026-11-02 is a Monday, so one business day before it is the
//! preceding Friday. Checked by hand against the calendar.
//!
//! M26 task 7: the 400-day guard returns an explicit Error::Data if left business days
//! cannot be found within 400 calendar days.

use spectra_core::Error;
use spectra_engine::bonds::days_from_civil;
use spectra_engine::futures::roll_ts;

const DAY_NS: i64 = 86_400_000_000_000;

fn day(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d) * DAY_NS
}

#[test]
fn one_business_day_before_a_monday_expiry_is_the_friday() {
    // 2026-11-02 (Monday) minus one business day: 2026-10-30 (Friday).
    assert_eq!(roll_ts(day(2026, 11, 2), 1, None, 1).unwrap(), day(2026, 10, 30));
}

#[test]
fn business_days_skip_the_weekend() {
    // Monday expiry, n = 2: Friday 10-30, Thursday 10-29.
    assert_eq!(roll_ts(day(2026, 11, 2), 2, None, 1).unwrap(), day(2026, 10, 29));
    // n = 5 is the previous Monday, 10-26; n = 6 is Friday 10-23.
    assert_eq!(roll_ts(day(2026, 11, 2), 5, None, 1).unwrap(), day(2026, 10, 26));
    assert_eq!(roll_ts(day(2026, 11, 2), 6, None, 1).unwrap(), day(2026, 10, 23));
}

#[test]
fn a_mid_week_expiry_counts_back_over_the_weekend() {
    // Wednesday 2026-11-04, n = 3: Tuesday 11-03, Monday 11-02, Friday 10-30.
    assert_eq!(roll_ts(day(2026, 11, 4), 3, None, 1).unwrap(), day(2026, 10, 30));
}

#[test]
fn zero_business_days_is_the_expiry_day_itself_at_midnight() {
    // With n = 0 nothing is counted, so the expiry day's start is returned.
    assert_eq!(roll_ts(day(2026, 11, 2), 0, None, 1).unwrap(), day(2026, 11, 2));
}

#[test]
fn roll_ts_exceeding_400_days_guard_returns_error() {
    // Arithmetic for 400-day roll guard:
    // With expiry at Monday 2026-11-02 and n = 401 business days on a weekday-only calendar (exchange: None):
    // The loop steps backward day-by-day up to 400 days (guard == 400).
    // In 400 calendar days, there are 57 full weeks (399 days) plus 1 day (Sunday 2026-11-01, a non-business day).
    // Each full week has 5 business days, yielding exactly 57 * 5 = 285 business days encountered.
    // Thus left = 401 - 285 = 116 business days remain unfound after 400 calendar days.
    // The guard halts the search and returns Error::Data("futures roll did not find 116 business days within 400 days for 1").
    let expiry = day(2026, 11, 2);
    let res = roll_ts(expiry, 401, None, 1);
    assert!(matches!(
        res,
        Err(Error::Data(msg))
            if msg == "futures roll did not find 116 business days within 400 days for 1"
    ));
}
