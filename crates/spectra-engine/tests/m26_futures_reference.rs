//! M26 task 3f (part 3, futures roll): reference-value tests for `roll_ts`
//! (`src/futures.rs`), which moves back n business days from expiry.
//!
//! These use the weekday-only calendar (no exchange), so the expected dates are
//! closed forms: 2026-11-02 is a Monday, so one business day before it is the
//! preceding Friday. Checked by hand against the calendar.
//!
//! Not covered here: the exchange holiday calendar (needs its own reference
//! list) and the 400-day guard (a documented silent stop, task 7).

use spectra_engine::bonds::days_from_civil;
use spectra_engine::futures::roll_ts;

const DAY_NS: i64 = 86_400_000_000_000;

fn day(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d) * DAY_NS
}

#[test]
fn one_business_day_before_a_monday_expiry_is_the_friday() {
    // 2026-11-02 (Monday) minus one business day: 2026-10-30 (Friday).
    assert_eq!(roll_ts(day(2026, 11, 2), 1, None), day(2026, 10, 30));
}

#[test]
fn business_days_skip_the_weekend() {
    // Monday expiry, n = 2: Friday 10-30, Thursday 10-29.
    assert_eq!(roll_ts(day(2026, 11, 2), 2, None), day(2026, 10, 29));
    // n = 5 is the previous Monday, 10-26; n = 6 is Friday 10-23.
    assert_eq!(roll_ts(day(2026, 11, 2), 5, None), day(2026, 10, 26));
    assert_eq!(roll_ts(day(2026, 11, 2), 6, None), day(2026, 10, 23));
}

#[test]
fn a_mid_week_expiry_counts_back_over_the_weekend() {
    // Wednesday 2026-11-04, n = 3: Tuesday 11-03, Monday 11-02, Friday 10-30.
    assert_eq!(roll_ts(day(2026, 11, 4), 3, None), day(2026, 10, 30));
}

#[test]
fn zero_business_days_is_the_expiry_day_itself_at_midnight() {
    // With n = 0 nothing is counted, so the expiry day's start is returned.
    assert_eq!(roll_ts(day(2026, 11, 2), 0, None), day(2026, 11, 2));
}
