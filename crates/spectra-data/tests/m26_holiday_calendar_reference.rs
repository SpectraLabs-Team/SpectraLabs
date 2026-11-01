//! M26 task 3d: known-answer reference tests for exchange holiday calendars.
//!
//! Source of the rules: `crates/spectra-data/src/calendar.rs`.
//! Primary entry points:
//! - `Exchange::is_trading_day(year, month, day)`
//! - `Exchange::is_early_close(year, month, day)`
//! - `Exchange::is_trading_day_bar(ts_ns)`
//! - `Exchange::parse(code)`

use spectra_data::Exchange;

#[test]
fn weekday_trading_days_across_exchanges() {
    // -------------------------------------------------------------------------
    // Case 1: Plain Tuesday in a non-holiday week across major exchanges.
    //
    // Reasoning:
    // October 10, 2023 is a Tuesday:
    // - 2023-10-10 is a weekday (w == 2, Tuesday).
    // - Not a holiday in US (Xnys: Columbus Day on Oct 9 is a bond holiday, not equity holiday).
    // - Not a holiday in UK (Xlon).
    // - Not a holiday in Japan (Xtks: Sports Day was Oct 9).
    // - Not a holiday in India (Xnse: Gandhi Jayanti was Oct 2).
    // All 4 exchanges must classify 2023-10-10 as an open trading day.
    // -------------------------------------------------------------------------
    assert!(Exchange::Xnys.is_trading_day(2023, 10, 10), "Xnys must be open on plain Tuesday 2023-10-10");
    assert!(Exchange::Xlon.is_trading_day(2023, 10, 10), "Xlon must be open on plain Tuesday 2023-10-10");
    assert!(Exchange::Xtks.is_trading_day(2023, 10, 10), "Xtks must be open on plain Tuesday 2023-10-10");
    assert!(Exchange::Xnse.is_trading_day(2023, 10, 10), "Xnse must be open on plain Tuesday 2023-10-10");
}

#[test]
fn weekend_days_classification_across_exchanges() {
    // -------------------------------------------------------------------------
    // Case 2: Weekend days (Saturday and Sunday).
    //
    // Reasoning:
    // 2023-10-14 is Saturday (w == 6), 2023-10-15 is Sunday (w == 0).
    // - Equity exchanges (Xnys, Xlon, Xtks, Xnse):
    //   `is_weekend` checks `w == 0 || w == 6` -> closed both days.
    // - CME Futures (Xcme):
    //   `is_cme_trading_day` checks: Saturday is closed (`w == 6`), but Sunday
    //   opens at 17:00 CT for equity index futures, so Sunday is a trading day (`true`).
    // - Continuous 24/7 (Continuous24_7):
    //   Trades every calendar day including both weekend days (`true`).
    // -------------------------------------------------------------------------

    // Equity exchanges: both weekend days closed
    for ex in [Exchange::Xnys, Exchange::Xlon, Exchange::Xtks, Exchange::Xnse] {
        assert!(!ex.is_trading_day(2023, 10, 14), "{ex:?} must be closed on Saturday 2023-10-14");
        assert!(!ex.is_trading_day(2023, 10, 15), "{ex:?} must be closed on Sunday 2023-10-15");
    }

    // CME Futures: Saturday closed, Sunday open
    assert!(!Exchange::Xcme.is_trading_day(2023, 10, 14), "Xcme must be closed on Saturday 2023-10-14");
    assert!(
        Exchange::Xcme.is_trading_day(2023, 10, 15),
        "Xcme must be open on Sunday 2023-10-15 (17:00 CT session open)"
    );

    // Continuous 24/7: both weekend days open
    assert!(
        Exchange::Continuous24_7.is_trading_day(2023, 10, 14),
        "Continuous 24/7 must be open on Saturday 2023-10-14"
    );
    assert!(
        Exchange::Continuous24_7.is_trading_day(2023, 10, 15),
        "Continuous 24/7 must be open on Sunday 2023-10-15"
    );
}

#[test]
fn listed_exchange_holidays_exact_date_on_weekday() {
    // -------------------------------------------------------------------------
    // Case 3: Listed exchange holiday on the exact date (weekday, no observation needed).
    //
    // Reasoning:
    // - Xnys & Xcme: Christmas Day on Wednesday, Dec 25, 2019 (w == 3).
    //   Exact holiday on weekday: closed.
    // - Xlon: Boxing Day on Thursday, Dec 26, 2019 (w == 4).
    //   Exact holiday on weekday: closed.
    // - Xtks: Culture Day on Friday, Nov 3, 2023 (w == 5).
    //   Exact holiday on weekday: closed.
    // - Xnse: Republic Day on Thursday, Jan 26, 2023 (w == 4).
    //   Exact holiday on weekday: closed.
    // -------------------------------------------------------------------------
    assert!(
        !Exchange::Xnys.is_trading_day(2019, 12, 25),
        "Xnys must be closed on Christmas Day (Wednesday 2019-12-25)"
    );
    assert!(
        !Exchange::Xcme.is_trading_day(2019, 12, 25),
        "Xcme must be closed on Christmas Day (Wednesday 2019-12-25)"
    );
    assert!(
        !Exchange::Xlon.is_trading_day(2019, 12, 26),
        "Xlon must be closed on Boxing Day (Thursday 2019-12-26)"
    );
    assert!(
        !Exchange::Xtks.is_trading_day(2023, 11, 3),
        "Xtks must be closed on Culture Day (Friday 2023-11-03)"
    );
    assert!(
        !Exchange::Xnse.is_trading_day(2023, 1, 26),
        "Xnse must be closed on Republic Day (Thursday 2023-01-26)"
    );
}

#[test]
fn observed_holidays_sunday_to_monday_and_saturday_to_friday() {
    // -------------------------------------------------------------------------
    // Case 4: Observed holidays for Xnys.
    //
    // Subcase 4a: Sunday holiday moves to following Monday.
    // July 4, 2021 was Sunday (w == 0).
    // - Sunday July 4, 2021: closed as weekend.
    // - Monday July 5, 2021: observed Independence Day holiday -> closed (`false`).
    // - Tuesday July 6, 2021: regular trading day -> open (`true`).
    //
    // Subcase 4b: Saturday holiday moves to preceding Friday.
    // Dec 25, 2021 was Saturday (w == 6).
    // - Thursday Dec 23, 2021: regular trading day -> open (`true`).
    // - Friday Dec 24, 2021: observed Christmas holiday -> closed (`false`).
    // - Saturday Dec 25, 2021: closed as weekend (`false`).
    //
    // Subcase 4c: Jan 1 on Saturday (observe_saturday is false for Jan 1).
    // Jan 1, 2022 was Saturday (w == 6).
    // Under NYSE/US rules, a Saturday New Year's Day is NOT observed on Friday Dec 31
    // of the prior year.
    // - Friday Dec 31, 2021 must remain a trading day -> open (`true`).
    // - Saturday Jan 1, 2022: closed as weekend (`false`).
    // - Monday Jan 3, 2022: regular trading day (Jan 1 was Sat, not Sun) -> open (`true`).
    // -------------------------------------------------------------------------

    // Subcase 4a: Sunday -> Monday
    assert!(
        !Exchange::Xnys.is_trading_day(2021, 7, 5),
        "Monday 2021-07-05 must be closed as observed Independence Day (July 4 was Sun)"
    );
    assert!(
        Exchange::Xnys.is_trading_day(2021, 7, 6),
        "Tuesday 2021-07-06 must be open as normal trading day"
    );

    // Subcase 4b: Saturday -> Friday
    assert!(
        Exchange::Xnys.is_trading_day(2021, 12, 23),
        "Thursday 2021-12-23 must be open as normal trading day"
    );
    assert!(
        !Exchange::Xnys.is_trading_day(2021, 12, 24),
        "Friday 2021-12-24 must be closed as observed Christmas Day (Dec 25 was Sat)"
    );

    // Subcase 4c: Saturday New Year's Day Dec 31 unobserved
    assert!(
        Exchange::Xnys.is_trading_day(2021, 12, 31),
        "Friday Dec 31, 2021 must remain an open trading day (Saturday Jan 1 not observed on Dec 31)"
    );
    assert!(!Exchange::Xnys.is_trading_day(2022, 1, 1), "Saturday Jan 1, 2022 is closed as weekend");
    assert!(
        Exchange::Xnys.is_trading_day(2022, 1, 3),
        "Monday Jan 3, 2022 must be open as normal trading day (Jan 1 was Sat, not Sun)"
    );
}

#[test]
fn floating_holidays_and_easter_algorithms() {
    // -------------------------------------------------------------------------
    // Case 5: Floating holidays and Easter algorithms.
    //
    // Reasoning:
    // In 2023:
    // - MLK Day: 3rd Monday in Jan -> Jan 16, 2023 (closed on Xnys).
    // - Presidents' Day: 3rd Monday in Feb -> Feb 20, 2023 (closed on Xnys).
    // - Memorial Day: Last Monday in May -> May 29, 2023 (closed on Xnys).
    // - Labor Day: 1st Monday in Sep -> Sep 4, 2023 (closed on Xnys).
    // - Thanksgiving: 4th Thursday in Nov -> Nov 23, 2023 (closed on Xnys).
    //
    // Easter Algorithms:
    // - Late March Easter (2016):
    //   Easter Sunday was March 27, 2016.
    //   Good Friday (Easter - 2) = March 25, 2016 (closed on Xnys and Xlon).
    //   Easter Monday (Easter + 1) = March 28, 2016 (closed on Xlon, open on Xnys).
    //
    // - Late April Easter (2019):
    //   Easter Sunday was April 21, 2019.
    //   Good Friday (Easter - 2) = April 19, 2019 (closed on Xnys and Xlon).
    //   Easter Monday (Easter + 1) = April 22, 2019 (closed on Xlon, open on Xnys).
    // -------------------------------------------------------------------------

    // 2023 US floating holidays
    assert!(!Exchange::Xnys.is_trading_day(2023, 1, 16), "MLK Day (3rd Mon in Jan 2023) must be closed");
    assert!(
        !Exchange::Xnys.is_trading_day(2023, 2, 20),
        "Presidents' Day (3rd Mon in Feb 2023) must be closed"
    );
    assert!(
        !Exchange::Xnys.is_trading_day(2023, 5, 29),
        "Memorial Day (last Mon in May 2023) must be closed"
    );
    assert!(!Exchange::Xnys.is_trading_day(2023, 9, 4), "Labor Day (1st Mon in Sep 2023) must be closed");
    assert!(
        !Exchange::Xnys.is_trading_day(2023, 11, 23),
        "Thanksgiving Day (4th Thu in Nov 2023) must be closed"
    );

    // 2016 March Easter (Easter Sunday = March 27, 2016)
    assert!(!Exchange::Xnys.is_trading_day(2016, 3, 25), "Good Friday 2016-03-25 must be closed on Xnys");
    assert!(!Exchange::Xlon.is_trading_day(2016, 3, 25), "Good Friday 2016-03-25 must be closed on Xlon");
    assert!(!Exchange::Xlon.is_trading_day(2016, 3, 28), "Easter Monday 2016-03-28 must be closed on Xlon");
    assert!(Exchange::Xnys.is_trading_day(2016, 3, 28), "Easter Monday 2016-03-28 must be open on Xnys");

    // 2019 April Easter (Easter Sunday = April 21, 2019)
    assert!(!Exchange::Xnys.is_trading_day(2019, 4, 19), "Good Friday 2019-04-19 must be closed on Xnys");
    assert!(!Exchange::Xlon.is_trading_day(2019, 4, 19), "Good Friday 2019-04-19 must be closed on Xlon");
    assert!(!Exchange::Xlon.is_trading_day(2019, 4, 22), "Easter Monday 2019-04-22 must be closed on Xlon");
    assert!(Exchange::Xnys.is_trading_day(2019, 4, 22), "Easter Monday 2019-04-22 must be open on Xnys");
}

#[test]
fn trading_day_immediately_before_and_after_holiday() {
    // -------------------------------------------------------------------------
    // Case 6: Trading day immediately before and after a holiday.
    //
    // Reasoning:
    // 1. US Thanksgiving 2023:
    //    - Wednesday 2023-11-22: normal trading day -> open (`true`).
    //    - Thursday 2023-11-23: Thanksgiving -> closed (`false`).
    //    - Friday 2023-11-24: Black Friday (early close, but trading day) -> open (`true`).
    //
    // 2. Tokyo Culture Day 2023:
    //    - Thursday 2023-11-02: normal trading day -> open (`true`).
    //    - Friday 2023-11-03: Culture Day -> closed (`false`).
    //    - Saturday 2023-11-04 & Sunday 2023-11-05: weekend -> closed (`false`).
    //    - Monday 2023-11-06: next trading day -> open (`true`).
    // -------------------------------------------------------------------------

    // US Thanksgiving 2023
    assert!(
        Exchange::Xnys.is_trading_day(2023, 11, 22),
        "Wednesday 2023-11-22 must be open before Thanksgiving"
    );
    assert!(
        !Exchange::Xnys.is_trading_day(2023, 11, 23),
        "Thursday 2023-11-23 must be closed on Thanksgiving"
    );
    assert!(Exchange::Xnys.is_trading_day(2023, 11, 24), "Friday 2023-11-24 must be open after Thanksgiving");

    // Tokyo Culture Day 2023
    assert!(
        Exchange::Xtks.is_trading_day(2023, 11, 2),
        "Thursday 2023-11-02 must be open before Culture Day"
    );
    assert!(!Exchange::Xtks.is_trading_day(2023, 11, 3), "Friday 2023-11-03 must be closed on Culture Day");
    assert!(
        Exchange::Xtks.is_trading_day(2023, 11, 6),
        "Monday 2023-11-06 must be open as next trading day after Culture Day weekend"
    );
}

#[test]
fn consecutive_holidays_golden_week_and_uk_christmas_boxing_day() {
    // -------------------------------------------------------------------------
    // Case 7: Consecutive holidays.
    //
    // Reasoning:
    // 1. Tokyo Golden Week (2021):
    //    - April 29, 2021 (Thu): Showa Day -> closed (`false`).
    //    - April 30, 2021 (Fri): normal trading day between Showa Day and Golden Week -> open (`true`).
    //    - May 3, 2021 (Mon): Constitution Memorial Day -> closed (`false`).
    //    - May 4, 2021 (Tue): Greenery Day -> closed (`false`).
    //    - May 5, 2021 (Wed): Children's Day -> closed (`false`).
    //    - May 6, 2021 (Thu): normal trading day -> open (`true`).
    //
    //    Golden Week Substitute Holiday (2024):
    //    - May 5, 2024 was Sunday (Children's Day).
    //    - May 6, 2024 (Mon) is observed substitute holiday -> closed (`false`).
    //    - May 7, 2024 (Tue) -> open (`true`).
    //
    // 2. London Christmas & Boxing Day with Weekend Substitute Days (2021):
    //    - Dec 25, 2021 was Saturday, Dec 26, 2021 was Sunday.
    //    - Dec 27, 2021 (Mon): Christmas substitute bank holiday -> closed (`false`).
    //    - Dec 28, 2021 (Tue): Boxing Day substitute bank holiday -> closed (`false`).
    //    - Dec 29, 2021 (Wed): normal trading day -> open (`true`).
    // -------------------------------------------------------------------------

    // Tokyo Golden Week (calendar.rs line 501 treats May 3..=6 as non-trading days)
    assert!(!Exchange::Xtks.is_trading_day(2021, 4, 29), "Showa Day 2021-04-29 must be closed");
    assert!(Exchange::Xtks.is_trading_day(2021, 4, 30), "Friday 2021-04-30 before Golden Week must be open");
    assert!(
        !Exchange::Xtks.is_trading_day(2021, 5, 3),
        "Constitution Memorial Day 2021-05-03 must be closed"
    );
    assert!(!Exchange::Xtks.is_trading_day(2021, 5, 4), "Greenery Day 2021-05-04 must be closed");
    assert!(!Exchange::Xtks.is_trading_day(2021, 5, 5), "Children's Day 2021-05-05 must be closed");
    assert!(
        !Exchange::Xtks.is_trading_day(2021, 5, 6),
        "May 6 2021 is closed as part of Golden Week (3..=6 in calendar.rs)"
    );
    assert!(Exchange::Xtks.is_trading_day(2021, 5, 7), "Friday 2021-05-07 after Golden Week must be open");

    // London Christmas / Boxing Day 2021 consecutive substitute days
    assert!(
        !Exchange::Xlon.is_trading_day(2021, 12, 27),
        "Monday 2021-12-27 must be closed as substitute Christmas Day"
    );
    assert!(
        !Exchange::Xlon.is_trading_day(2021, 12, 28),
        "Tuesday 2021-12-28 must be closed as substitute Boxing Day"
    );
    assert!(
        Exchange::Xlon.is_trading_day(2021, 12, 29),
        "Wednesday 2021-12-29 must be open after substitute Christmas/Boxing Day holidays"
    );
}

#[test]
fn historical_inception_cutoffs() {
    // -------------------------------------------------------------------------
    // Case 8: Historical inception cutoffs.
    //
    // Reasoning:
    // 1. MLK Day (year >= 1998):
    //    - 1997: 3rd Monday was 1997-01-20. Pre-cutoff -> open trading day (`true`).
    //    - 1998: 3rd Monday was 1998-01-19. Cutoff year -> holiday (`false`).
    //
    // 2. Juneteenth (year >= 2022):
    //    - 2020: June 19 was Friday. Pre-cutoff -> open trading day (`true`).
    //    - 2021: June 19 was Saturday (Friday June 18 would be observed if active).
    //      Pre-cutoff -> Friday 2021-06-18 was open trading day (`true`).
    //    - 2022: June 19 was Sunday (Monday June 20 observed).
    //      Cutoff year -> Monday 2022-06-20 is closed (`false`).
    //
    // 3. Emperor's Birthday (year >= 2020 on Xtks):
    //    - 2018: Feb 23 was Friday. Pre-cutoff -> open trading day (`true`).
    //    - 2021: Feb 23 was Tuesday. Cutoff active -> closed (`false`).
    // -------------------------------------------------------------------------

    // MLK Day cutoff (1998)
    assert!(
        Exchange::Xnys.is_trading_day(1997, 1, 20),
        "3rd Monday in Jan 1997 must be open before MLK Day inception in 1998"
    );
    assert!(
        !Exchange::Xnys.is_trading_day(1998, 1, 19),
        "3rd Monday in Jan 1998 must be closed at MLK Day inception"
    );

    // Juneteenth cutoff (2022)
    assert!(
        Exchange::Xnys.is_trading_day(2020, 6, 19),
        "June 19, 2020 must be open before Juneteenth inception in 2022"
    );
    assert!(
        Exchange::Xnys.is_trading_day(2021, 6, 18),
        "Friday June 18, 2021 must be open (Juneteenth not observed on Saturday-eve before 2022)"
    );
    assert!(
        !Exchange::Xnys.is_trading_day(2022, 6, 20),
        "Monday June 20, 2022 must be closed at Juneteenth inception (June 19 was Sun)"
    );

    // Emperor's Birthday cutoff (2020)
    assert!(
        Exchange::Xtks.is_trading_day(2018, 2, 23),
        "Feb 23, 2018 must be open before Emperor's Birthday inception in 2020"
    );
    assert!(
        !Exchange::Xtks.is_trading_day(2021, 2, 23),
        "Feb 23, 2021 must be closed at Emperor's Birthday inception"
    );
}

#[test]
fn early_close_dates() {
    // -------------------------------------------------------------------------
    // Case 9: Early close dates.
    //
    // Reasoning:
    // 1. US Equities (Xnys):
    //    - Black Friday (day after Thanksgiving, 4th Friday in Nov):
    //      Nov 24, 2023 is Black Friday -> `is_early_close` is `true`.
    //      Wednesday before Thanksgiving (Nov 22, 2023) -> `is_early_close` is `false`.
    // 2. London Stock Exchange (Xlon):
    //    - Christmas Eve (Dec 24):
    //      Dec 24, 2024 is Tuesday -> `is_early_close` is `true`.
    //      Monday Dec 23, 2024 -> `is_early_close` is `false`.
    // -------------------------------------------------------------------------

    // US early close (Black Friday 2023)
    assert!(
        Exchange::Xnys.is_early_close(2023, 11, 24),
        "Black Friday 2023-11-24 must be an early close on Xnys"
    );
    assert!(
        !Exchange::Xnys.is_early_close(2023, 11, 22),
        "Wednesday 2023-11-22 must not be an early close on Xnys"
    );

    // London early close (Christmas Eve 2024)
    assert!(
        Exchange::Xlon.is_early_close(2024, 12, 24),
        "Christmas Eve 2024-12-24 must be an early close on Xlon"
    );
    assert!(!Exchange::Xlon.is_early_close(2024, 12, 23), "Dec 23, 2024 must not be an early close on Xlon");
}

#[test]
fn unknown_exchange_parse_documents_current_behavior() {
    // -------------------------------------------------------------------------
    // Case 10: Unknown exchange handling.
    //
    // documents current behaviour, Task 7 may change this (silent default to explicit error).
    //
    // Reasoning:
    // Exchange::parse returns None for unknown exchange strings.
    // In `crates/spectra-engine/src/futures.rs` (`is_business_day`), passing `None`
    // falls back to plain Monday–Friday weekdays with no holidays skipped.
    // Since `spectra-data` does not depend on `spectra-engine`, that consumer fallback
    // resides in `spectra-engine` and is tested there.
    // Here we assert that `Exchange::parse` returns None for unknown strings.
    // -------------------------------------------------------------------------
    assert_eq!(
        Exchange::parse("UNKNOWN"),
        None,
        "Exchange::parse must return None for unknown exchange codes"
    );
    assert_eq!(Exchange::parse(""), None, "Exchange::parse must return None for empty string");
    assert_eq!(Exchange::parse("FOOBAR"), None, "Exchange::parse must return None for invalid exchange code");
}
