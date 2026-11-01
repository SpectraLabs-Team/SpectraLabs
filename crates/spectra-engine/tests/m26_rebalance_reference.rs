//! M26 task 3d: known-answer reference tests for rebalance trade sizing.
//!
//! Source of the sizing rule:
//! - `crates/spectra-engine/src/rebalance.rs`, `Rebalancer::trade_for` (lines 154–180)
//! - `effective_lot` (lines 222–235)
//!
//! Sizing rule summary:
//! 1. `close = ctx.unit_value(id, ctx.history(id).last()?.close)?` (if close <= 0.0, returns None).
//! 2. `want = targets.get(&id).unwrap_or(0.0) * equity / close`.
//! 3. `want_raw = (want * SCALE as f64).trunc()`.
//!    If `!want_raw.is_finite() || want_raw.abs() >= 9.0e18`, returns None.
//! 4. `lot = effective_lot(lot_size, ctx.lot_size(id))`.
//!    If `lot > 0`, `target = target / lot * lot` (truncation towards zero).
//! 5. `delta = target.checked_sub(ctx.position(id).raw())?`.
//!    If `delta == 0`, returns None.
//! 6. `qty = Qty::from_raw(delta.checked_abs()?)`.
//!    If `qty * close < min_trade_notional`, returns None.
//! 7. Side: Buy if `delta > 0`, else Sell.

use spectra_core::{Bar, InstrumentId, LedgerConfig, Money, Price, Qty, Ratio, Side, SCALE};
use spectra_engine::{
    run_backtest, BacktestConfig, Context, OrderState, RebalanceConfig, Rebalancer, WeightStrategy,
};

/// Helper to generate uniform flat daily bars for instrument 1.
fn flat_bars(n: usize, price: f64) -> Vec<Bar> {
    let p = Price::from_f64(price).unwrap();
    (0..n)
        .map(|i| Bar {
            ts_ns: (i as i64 + 1) * 86_400_000_000_000,
            known_at_ns: (i as i64 + 1) * 86_400_000_000_000,
            instrument: 1,
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Qty::from_raw(10_000_000 * SCALE),
        })
        .collect()
}

/// Strategy that returns scheduled target weights bar-by-bar.
struct ScheduledWeights {
    bar: usize,
    schedule: Vec<Vec<(InstrumentId, f64)>>,
}

impl ScheduledWeights {
    fn new(schedule: Vec<Vec<(InstrumentId, f64)>>) -> Self {
        Self { bar: 0, schedule }
    }
}

impl WeightStrategy for ScheduledWeights {
    fn weights(&mut self, _ctx: &Context<'_>, _bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
        let res = self.schedule.get(self.bar).cloned();
        self.bar += 1;
        res
    }
}

/// Standard test backtest config with 100,000 cash.
fn cfg_100k() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() }
}

/// RebalanceConfig with gross_cap = 1.0 (no scaling under 100% equity).
fn cfg_rebalance_cap1() -> RebalanceConfig {
    RebalanceConfig { gross_cap: 1.0, ..RebalanceConfig::default() }
}

#[test]
fn sell_from_overweight_position_produces_exact_sell_quantity() {
    // -------------------------------------------------------------------------
    // Case 1: Sell from an overweight position.
    //
    // Arithmetic:
    // Bar 0:
    //   close = 100.0, initial cash = 100,000.0, equity = 100,000.0
    //   target weight = 0.50
    //   want = 0.50 * 100,000.0 / 100.0 = 500.0
    //   want_raw = trunc(500.0 * 1e9) = 500,000,000,000
    //   lot = 1e9 (1 share) -> target = 500,000,000,000 (500 shares)
    //   position = 0 -> delta = +500,000,000,000 -> Side::Buy 500.0 shares.
    //
    // Bar 1:
    //   Order 0 filled at open: position is now +500 shares, cash is 50,000.0.
    //   close = 100.0, equity = 50,000.0 + 500 * 100.0 = 100,000.0.
    //   New target weight = 0.20 (position is overweight at 50% vs 20% target).
    //   want = 0.20 * 100,000.0 / 100.0 = 200.0
    //   want_raw = trunc(200.0 * 1e9) = 200,000,000,000
    //   lot = 1e9 -> target = 200,000,000,000 (200 shares)
    //   delta = target - position = 200,000,000,000 - 500,000,000,000
    //         = -300,000,000,000 (-300 shares)
    //   delta < 0 -> Side::Sell, qty = |-300,000,000,000| = 300,000,000,000 (300.0 shares).
    //
    // Bar 2:
    //   Order 1 filled: position = 500 - 300 = 200 shares (exactly on 20% target).
    // -------------------------------------------------------------------------
    let bars = flat_bars(3, 100.0);
    let sched = vec![vec![(1, 0.50)], vec![(1, 0.20)], vec![(1, 0.20)]];
    let mut strat = Rebalancer::new(ScheduledWeights::new(sched), cfg_rebalance_cap1());

    let r = run_backtest(&bars, &mut strat, &cfg_100k()).unwrap();
    r.ledger.check_invariants().unwrap();

    assert_eq!(r.orders.len(), 2, "must have exactly 2 orders (initial buy and rebalance sell)");

    // Assert initial buy order on bar 0:
    let o0 = &r.orders[0];
    assert_eq!(o0.submit_step, 0);
    assert_eq!(o0.side, Side::Buy);
    assert_eq!(o0.qty, Qty::from_raw(500 * SCALE));

    // Assert rebalance sell order on bar 1:
    let o1 = &r.orders[1];
    assert_eq!(o1.submit_step, 1);
    assert_eq!(o1.side, Side::Sell);
    assert_eq!(o1.qty, Qty::from_raw(300 * SCALE));
    assert_eq!(o1.state, OrderState::Filled);

    // Final position matches target exactly:
    assert_eq!(r.ledger.position(1).qty, Qty::from_raw(200 * SCALE));
}

#[test]
fn position_already_at_target_produces_no_trade_on_subsequent_bars() {
    // -------------------------------------------------------------------------
    // Case 2: Position already at target (delta == 0 branch).
    //
    // Arithmetic:
    // Bar 0:
    //   target weight = 0.50 -> want = 500.0 -> Buy 500 shares submitted.
    //
    // Bar 1:
    //   Order 0 filled at open: position = 500 shares, cash = 50,000, equity = 100,000.
    //   target weight = 0.50
    //   want = 0.50 * 100,000.0 / 100.0 = 500.0
    //   target = 500 * 1e9
    //   position = 500 * 1e9
    //   delta = target - position = 500 * 1e9 - 500 * 1e9 = 0
    //   delta == 0 -> trade_for returns None.
    //   No order submitted on bar 1.
    //
    // Bar 2:
    //   Same target weight 0.50 -> delta == 0 -> None.
    //   No order submitted on bar 2.
    // -------------------------------------------------------------------------
    let bars = flat_bars(4, 100.0);
    let sched = vec![vec![(1, 0.50)], vec![(1, 0.50)], vec![(1, 0.50)], vec![(1, 0.50)]];
    let mut strat = Rebalancer::new(ScheduledWeights::new(sched), cfg_rebalance_cap1());

    let r = run_backtest(&bars, &mut strat, &cfg_100k()).unwrap();
    r.ledger.check_invariants().unwrap();

    // Exactly 1 order in the entire backtest (the initial buy on bar 0):
    assert_eq!(r.orders.len(), 1, "only bar 0 submits an order; delta == 0 produces None on subsequent bars");
    assert_eq!(r.orders[0].submit_step, 0);
    assert_eq!(r.orders[0].side, Side::Buy);
    assert_eq!(r.orders[0].qty, Qty::from_raw(500 * SCALE));

    // Position remains steady at target:
    assert_eq!(r.ledger.position(1).qty, Qty::from_raw(500 * SCALE));
}

#[test]
fn non_positive_price_is_rejected_before_trading() {
    // -------------------------------------------------------------------------
    // Case 3: Zero and negative price rejection before trading.
    //
    // Comment: this is the reachable protection. In `crates/spectra-engine/src/backtest.rs`
    // line 2232, `validate_bars` rejects any dataset containing a bar where
    // `close <= 0` with `Error::InvalidArgument("invalid bars: bar N: non-positive price")`.
    // The `close <= 0.0` guard inside `trade_for` is defense-in-depth and cannot
    // be reached through valid bars.
    // -------------------------------------------------------------------------

    // Subcase 3a: Zero price bar
    let bars_zero = vec![Bar {
        ts_ns: 86_400_000_000_000,
        known_at_ns: 86_400_000_000_000,
        instrument: 1,
        open: Price::ZERO,
        high: Price::ZERO,
        low: Price::ZERO,
        close: Price::ZERO,
        volume: Qty::from_raw(10_000_000 * SCALE),
    }];
    let mut strat_zero = Rebalancer::new(ScheduledWeights::new(vec![vec![(1, 0.50)]]), cfg_rebalance_cap1());
    let err_zero = run_backtest(&bars_zero, &mut strat_zero, &cfg_100k()).unwrap_err();
    let msg_zero = err_zero.to_string();
    assert!(
        msg_zero.contains("non-positive price"),
        "zero price bar must be rejected with 'non-positive price', got: {msg_zero}"
    );

    // Subcase 3b: Negative price bar
    let p_neg = Price::from_raw(-50 * SCALE);
    let bars_neg = vec![Bar {
        ts_ns: 86_400_000_000_000,
        known_at_ns: 86_400_000_000_000,
        instrument: 1,
        open: p_neg,
        high: p_neg,
        low: p_neg,
        close: p_neg,
        volume: Qty::from_raw(10_000_000 * SCALE),
    }];
    let mut strat_neg = Rebalancer::new(ScheduledWeights::new(vec![vec![(1, 0.50)]]), cfg_rebalance_cap1());
    let err_neg = run_backtest(&bars_neg, &mut strat_neg, &cfg_100k()).unwrap_err();
    let msg_neg = err_neg.to_string();
    assert!(
        msg_neg.contains("non-positive price"),
        "negative price bar must be rejected with 'non-positive price', got: {msg_neg}"
    );
}

#[test]
fn overflow_guard_on_nano_price_submits_no_trade_documenting_current_behavior() {
    // -------------------------------------------------------------------------
    // Case 4: Overflow guard (|want_raw| >= 9.0e18).
    //
    // Arithmetic:
    //   close = Price::from_raw(1) = 1e-9 (1 nano-currency unit)
    //   initial_cash = 100,000.0, equity = 100,000.0
    //   target weight = 0.50
    //   want = 0.50 * 100,000.0 / 1e-9 = 5.0e13
    //   want_raw = trunc(5.0e13 * 1e9) = 5.0e22
    //   |want_raw| = 5.0e22 >= 9.0e18 -> overflow guard in trade_for triggers!
    //   trade_for returns None silently.
    //
    // Comment:
    // documents current behaviour, Task 7 will change this (silent default to explicit error).
    // -------------------------------------------------------------------------
    let nano_price = Price::from_raw(1);
    let bars = vec![
        Bar {
            ts_ns: 86_400_000_000_000,
            known_at_ns: 86_400_000_000_000,
            instrument: 1,
            open: nano_price,
            high: nano_price,
            low: nano_price,
            close: nano_price,
            volume: Qty::from_raw(10_000_000 * SCALE),
        },
        Bar {
            ts_ns: 2 * 86_400_000_000_000,
            known_at_ns: 2 * 86_400_000_000_000,
            instrument: 1,
            open: nano_price,
            high: nano_price,
            low: nano_price,
            close: nano_price,
            volume: Qty::from_raw(10_000_000 * SCALE),
        },
    ];

    let sched = vec![vec![(1, 0.50)], vec![(1, 0.50)]];
    let mut strat = Rebalancer::new(ScheduledWeights::new(sched), cfg_rebalance_cap1());

    let r = run_backtest(&bars, &mut strat, &cfg_100k()).unwrap();
    r.ledger.check_invariants().unwrap();

    // The overflow guard returned None, so no order was submitted:
    assert!(
        r.orders.is_empty(),
        "overflow guard (|want_raw| >= 9.0e18) silently produces no trade in current behaviour"
    );
    assert_eq!(r.ledger.position(1).qty, Qty::ZERO);
}

#[test]
fn whole_lot_boundary_positive_target_exact_lot() {
    // -------------------------------------------------------------------------
    // Case 5a: Positive target landing exactly on a lot multiple.
    //
    // Lot size: 100 shares (100 * 1e9 raw).
    // Initial cash = 100,000.0, close = 100.0.
    //
    // Arithmetic:
    //   w = 0.40 -> want = 0.40 * 100,000 / 100 = 400.0
    //   want_raw = 400 * 1e9
    //   lot = 100 * 1e9
    //   target = (400 * 1e9 / 100 * 1e9) * 100 * 1e9 = 4 * 100 * 1e9 = 400 * 1e9 (exact)
    //   delta = 400 * 1e9 - 0 = +400 * 1e9 -> Buy 400.0 shares.
    // -------------------------------------------------------------------------
    let rc_lot100 = RebalanceConfig {
        gross_cap: 1.0,
        lot_size: Qty::from_raw(100 * SCALE),
        ..RebalanceConfig::default()
    };

    let bars = flat_bars(2, 100.0);
    let mut strat = Rebalancer::new(ScheduledWeights::new(vec![vec![(1, 0.40)]]), rc_lot100);
    let res = run_backtest(&bars, &mut strat, &cfg_100k()).unwrap();
    assert_eq!(res.orders.len(), 1);
    assert_eq!(res.orders[0].side, Side::Buy);
    assert_eq!(
        res.orders[0].qty,
        Qty::from_raw(400 * SCALE),
        "target exactly on lot multiple (400) trades exactly 400 shares"
    );
}

#[test]
fn whole_lot_boundary_positive_target_just_under_rounds_down() {
    // -------------------------------------------------------------------------
    // Case 5b: Positive target just under a lot multiple (499.9 shares -> rounds down to 400).
    //
    // Lot size: 100 shares (100 * 1e9 raw).
    // Close = 100.0, Equity = 100,000.0.
    //
    // Arithmetic:
    //   w = 0.4999 -> want = 0.4999 * 100,000 / 100 = 499.9
    //   want_raw = trunc(499.9 * 1e9) = +499,900,000,000
    //   lot = 100 * 1e9 = 100,000,000,000
    //   target = (499,900,000,000 / 100,000,000,000) * 100,000,000,000
    //          = 4 * 100,000,000,000 = +400,000,000,000 (rounds down, discarding 99.9 shares)
    //   delta = +400 * 1e9 - 0 = +400 * 1e9 -> Buy 400.0 shares.
    // -------------------------------------------------------------------------
    let rc_lot100 = RebalanceConfig {
        gross_cap: 1.0,
        lot_size: Qty::from_raw(100 * SCALE),
        ..RebalanceConfig::default()
    };

    let bars = flat_bars(2, 100.0);
    let mut strat = Rebalancer::new(ScheduledWeights::new(vec![vec![(1, 0.4999)]]), rc_lot100);
    let res = run_backtest(&bars, &mut strat, &cfg_100k()).unwrap();
    assert_eq!(res.orders.len(), 1);
    assert_eq!(res.orders[0].side, Side::Buy);
    assert_eq!(
        res.orders[0].qty,
        Qty::from_raw(400 * SCALE),
        "positive target just under 500 (499.9) rounds down to 400 shares"
    );
}

#[test]
fn whole_lot_boundary_negative_short_target_truncates_toward_zero() {
    // -------------------------------------------------------------------------
    // Case 5c: Negative (short) target just above a lot multiple in magnitude
    //          (-499.9 shares -> truncates toward zero to -400, NOT floor to -500).
    //
    // Configuration: allow_short = true, max_gross_leverage = 2.0.
    // Close = 100.0, Equity = 100,000.0, Lot size = 100 shares.
    //
    // Arithmetic:
    //   w = -0.4999 -> want = -0.4999 * 100,000 / 100 = -499.9
    //   want_raw = trunc(-499.9 * 1e9) = -499,900,000,000 (negative sign)
    //   lot = 100 * 1e9 = 100,000,000,000
    //   In Rust integer division:
    //     -499,900,000,000 / 100,000,000,000 = -4  (truncation toward zero, NOT -5 floor)
    //     target = -4 * 100,000,000,000 = -400,000,000,000 (-400 shares)
    //   delta = -400 * 1e9 - 0 = -400 * 1e9 -> delta < 0 -> Side::Sell, qty = 400.0 shares.
    // -------------------------------------------------------------------------
    let rc_lot100 = RebalanceConfig {
        gross_cap: 1.0,
        lot_size: Qty::from_raw(100 * SCALE),
        ..RebalanceConfig::default()
    };
    let short_cfg = BacktestConfig {
        ledger: LedgerConfig {
            allow_short: true,
            max_gross_leverage: Some(Ratio::from_raw(2 * SCALE)),
            ..LedgerConfig::default()
        },
        initial_cash: Money::from_raw(100_000 * SCALE),
        ..BacktestConfig::default()
    };

    let bars = flat_bars(2, 100.0);
    let mut strat = Rebalancer::new(ScheduledWeights::new(vec![vec![(1, -0.4999)]]), rc_lot100);
    let res = run_backtest(&bars, &mut strat, &short_cfg).unwrap();
    assert_eq!(res.orders.len(), 1);
    assert_eq!(res.orders[0].side, Side::Sell);
    assert_eq!(
        res.orders[0].qty,
        Qty::from_raw(400 * SCALE),
        "short target of -499.9 truncates toward zero to -400 (400 sell), not floor to -500"
    );
}
