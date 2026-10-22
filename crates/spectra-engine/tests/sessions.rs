//! Exchange sessions (M3 task 11 with M1's calendars): orders fill only when
//! the market is open. Intraday bars are judged by their time, daily bars by
//! their trading date, weekly bars always trade; Day orders only use up a bar
//! they could trade on.

use spectra_core::{Bar, EntryType, Money, Price, Qty, Side, SCALE};
use spectra_data::calendar::Exchange;
use spectra_engine::{
    run_backtest, BacktestConfig, BacktestResult, Context, OrderSpec, OrderState, OrderType, Strategy,
    TimeInForce,
};

const HOUR: i64 = 3_600_000_000_000;
const DAY: i64 = 24 * HOUR;

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn bar(ts_ns: i64, px: f64) -> Bar {
    let p = Price::from_f64(px).unwrap();
    Bar {
        ts_ns,
        known_at_ns: ts_ns,
        instrument: 1,
        open: p,
        high: p,
        low: p,
        close: p,
        volume: Qty::from_raw(1_000_000 * SCALE),
    }
}

/// One bar per timestamp; the price is the bar's index + 100, so fills show which bar they hit.
fn series(ts: &[i64]) -> Vec<Bar> {
    ts.iter().enumerate().map(|(i, &t)| bar(t, 100.0 + i as f64)).collect()
}

fn cfg(exchange: Option<Exchange>) -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(1_000_000 * SCALE), exchange, ..BacktestConfig::default() }
}

/// Places one order on the given bar index.
struct OrderOn(usize, OrderSpec, usize);
impl Strategy for OrderOn {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        if self.2 == self.0 {
            ctx.order(self.1);
        }
        self.2 += 1;
    }
}

fn market_on(i: usize) -> OrderOn {
    OrderOn(i, OrderSpec::new(1, Side::Buy, OrderType::Market, Qty::from_raw(SCALE)), 0)
}

fn fill_ts(r: &BacktestResult) -> Option<i64> {
    r.ledger.journal().iter().find(|e| e.kind == EntryType::Fill).map(|e| e.ts_ns)
}

#[test]
fn daily_bars_fill_on_trading_days_not_never() {
    // Daily bars at 00:00 UTC, every calendar day: Wed 3, Thu 4 (holiday), Fri 5, Sat 6, Sun 7, Mon 8 July 2024.
    let d = |day: i64| days(2024, 7, day) * DAY;
    let data = series(&[d(3), d(4), d(5), d(6), d(7), d(8)]);
    // Without a calendar, an order placed Wednesday fills on the next bar (Thursday).
    let plain = run_backtest(&data, &mut market_on(0), &cfg(None)).unwrap();
    assert_eq!(fill_ts(&plain), Some(d(4)));
    // With XNYS it skips the holiday and fills Friday. Before the fix it never filled:
    // 00:00 UTC is 20:00 in New York, outside the session.
    let ny = run_backtest(&data, &mut market_on(0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(fill_ts(&ny), Some(d(5)));
    // Placed Friday, it waits for Monday.
    let weekend = run_backtest(&data, &mut market_on(2), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(fill_ts(&weekend), Some(d(8)));
    // A 24/7 calendar never blocks.
    let crypto = run_backtest(&data, &mut market_on(2), &cfg(Some(Exchange::Continuous24_7))).unwrap();
    assert_eq!(fill_ts(&crypto), Some(d(6)));
}

#[test]
fn a_day_order_survives_a_closed_bar() {
    // A Day order placed Wednesday is not used up by the holiday; it fills Friday.
    let d = |day: i64| days(2024, 7, day) * DAY;
    let data = series(&[d(3), d(4), d(5), d(8)]);
    let spec = OrderSpec {
        tif: TimeInForce::Day,
        ..OrderSpec::new(1, Side::Buy, OrderType::Market, Qty::from_raw(SCALE))
    };
    let r = run_backtest(&data, &mut OrderOn(0, spec, 0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(r.orders[0].state, OrderState::Filled);
    assert_eq!(fill_ts(&r), Some(d(5)));
    // A limit Day order that cannot fill on its first open bar expires there.
    let limit = OrderSpec {
        limit: Price::from_f64(50.0).unwrap(),
        tif: TimeInForce::Day,
        ..OrderSpec::new(1, Side::Buy, OrderType::Limit, Qty::from_raw(SCALE))
    };
    let r = run_backtest(&data, &mut OrderOn(0, limit, 0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(r.orders[0].state, OrderState::Expired);
}

#[test]
fn intraday_bars_fill_inside_the_session() {
    // Hourly bars (UTC) on Fri 5 July 2024 (EDT, UTC-4) from 19:00 to 23:00 UTC, then Mon 8 July 13:00-15:00 UTC.
    // 19:00 UTC = 15:00 New York (open), 20:00 = 16:00 (the closing bar, open), 21:00+ closed,
    // Monday 13:00 = 09:00 (before the 09:30 open), 14:00 = 10:00 (open).
    let fri = days(2024, 7, 5) * DAY;
    let mon = days(2024, 7, 8) * DAY;
    let ts = [
        fri + 19 * HOUR,
        fri + 20 * HOUR,
        fri + 21 * HOUR,
        fri + 22 * HOUR,
        fri + 23 * HOUR,
        mon + 13 * HOUR,
        mon + 14 * HOUR,
        mon + 15 * HOUR,
    ];
    let data = series(&ts);
    let r = run_backtest(&data, &mut market_on(0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(fill_ts(&r), Some(ts[1]), "the 16:00 bar is still in the session");
    let r = run_backtest(&data, &mut market_on(1), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(fill_ts(&r), Some(ts[6]), "after the close it waits for Monday 10:00, not 09:00");
}

#[test]
fn weekly_bars_are_not_held_back() {
    // Weekly bars stamped on Mondays, one a holiday (Labor Day, 2 Sep 2024): every bar trades.
    let w = [days(2024, 8, 26), days(2024, 9, 2), days(2024, 9, 9)].map(|d| d * DAY);
    let r = run_backtest(&series(&w), &mut market_on(0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(fill_ts(&r), Some(w[1]));
}

#[test]
fn a_calendar_changes_nothing_when_every_bar_is_open() {
    // Weekday daily bars with no holidays: the same fills and equity as without a calendar.
    let d = |day: i64| days(2024, 7, day) * DAY;
    let data = series(&[d(8), d(9), d(10), d(11), d(12)]);
    let a = run_backtest(&data, &mut market_on(0), &cfg(None)).unwrap();
    let b = run_backtest(&data, &mut market_on(0), &cfg(Some(Exchange::Xnys))).unwrap();
    assert_eq!(a.equity_curve, b.equity_curve);
    assert_eq!(a.ledger.journal(), b.ledger.journal());
    assert_ne!(a.run.config_sha256, b.run.config_sha256, "the calendar is part of the configuration");
}
