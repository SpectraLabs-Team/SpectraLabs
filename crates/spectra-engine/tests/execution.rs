//! Execution models (M3 task 11): stop and stop-limit orders, time in force,
//! and square-root market impact. Exact fills on hand-built bars.

use spectra_core::{Bar, Bps, Money, Price, Qty, Side, SCALE};
use spectra_engine::{
    run_backtest, BacktestConfig, BacktestResult, Context, ExecutionConfig, ImpactConfig, OrderSpec,
    OrderState, OrderType, Strategy, TimeInForce,
};

const DAY: i64 = 86_400_000_000_000;

fn p(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}
fn q(v: f64) -> Qty {
    Qty::from_f64(v).unwrap()
}

/// One instrument; each tuple is (open, high, low, close, volume).
fn bars(rows: &[(f64, f64, f64, f64, f64)]) -> Vec<Bar> {
    rows.iter()
        .enumerate()
        .map(|(i, &(o, h, l, c, v))| Bar {
            ts_ns: (i as i64 + 1) * DAY,
            instrument: 1,
            open: p(o),
            high: p(h),
            low: p(l),
            close: p(c),
            volume: q(v),
        })
        .collect()
}

/// Places `spec` on the first bar, nothing else.
struct Place(Option<OrderSpec>);
impl Strategy for Place {
    fn on_bar(&mut self, ctx: &mut Context<'_>, _bars: &[Bar]) {
        if let Some(spec) = self.0.take() {
            ctx.order(spec);
        }
    }
}

fn cfg() -> BacktestConfig {
    BacktestConfig { initial_cash: Money::from_raw(1_000_000 * SCALE), ..BacktestConfig::default() }
}

fn run(rows: &[(f64, f64, f64, f64, f64)], spec: OrderSpec, c: &BacktestConfig) -> BacktestResult {
    let r = run_backtest(&bars(rows), &mut Place(Some(spec)), c).unwrap();
    r.ledger.check_invariants().unwrap();
    r
}

fn fill_prices(r: &BacktestResult) -> Vec<Price> {
    r.ledger
        .journal()
        .iter()
        .filter(|e| e.kind == spectra_core::EntryType::Fill)
        .map(|e| e.fill.price)
        .collect()
}

fn spec(side: Side, kind: OrderType, qty: f64) -> OrderSpec {
    OrderSpec::new(1, side, kind, q(qty))
}

const FLAT: (f64, f64, f64, f64, f64) = (100.0, 100.0, 100.0, 100.0, 1e6);

#[test]
fn buy_stop_fills_at_the_stop_or_a_gapped_open() {
    let s = OrderSpec { stop: p(105.0), ..spec(Side::Buy, OrderType::Stop, 10.0) };
    let touched = run(&[FLAT, (100.0, 106.0, 99.0, 104.0, 1e6)], s, &cfg());
    assert_eq!(fill_prices(&touched), [p(105.0)]);
    let gapped = run(&[FLAT, (107.0, 108.0, 106.0, 107.0, 1e6)], s, &cfg());
    assert_eq!(fill_prices(&gapped), [p(107.0)]);
    let missed = run(&[FLAT, (100.0, 104.0, 99.0, 101.0, 1e6)], s, &cfg());
    assert!(fill_prices(&missed).is_empty());
    assert_eq!(missed.orders[0].state, OrderState::Pending);
}

#[test]
fn sell_stop_fills_at_the_stop_or_a_gapped_open_with_slippage() {
    let mut c = cfg();
    c.ledger.allow_short = true;
    c.costs.slippage = Bps::from_raw(10 * SCALE); // 10 bps against the seller
    let s = OrderSpec { stop: p(95.0), ..spec(Side::Sell, OrderType::Stop, 10.0) };
    let touched = run(&[FLAT, (100.0, 101.0, 94.0, 96.0, 1e6)], s, &c);
    assert_eq!(fill_prices(&touched), [p(94.905)]);
    let gapped = run(&[FLAT, (90.0, 91.0, 89.0, 90.0, 1e6)], s, &c);
    assert_eq!(fill_prices(&gapped), [p(89.91)]);
}

#[test]
fn stop_limit_fills_within_its_limit_or_rests_as_a_limit() {
    let s = OrderSpec { stop: p(105.0), limit: p(106.0), ..spec(Side::Buy, OrderType::StopLimit, 10.0) };
    // Triggered at 105, within the 106 limit: fills at 105, no slippage.
    let within = run(&[FLAT, (100.0, 110.0, 99.0, 108.0, 1e6)], s, &cfg());
    assert_eq!(fill_prices(&within), [p(105.0)]);

    // Gaps to 108, above the limit: triggered but not filled, then fills as a limit next bar.
    let rested =
        run(&[FLAT, (108.0, 109.0, 107.0, 108.0, 1e6), (107.0, 107.0, 105.5, 106.0, 1e6)], s, &cfg());
    assert_eq!(fill_prices(&rested), [p(106.0)]);
    assert!(rested.orders[0].triggered);
    assert_eq!(rested.orders[0].state, OrderState::Filled);

    // Triggered and never back within the limit: still open, still triggered.
    let open = run(&[FLAT, (108.0, 109.0, 107.0, 108.0, 1e6), (109.0, 110.0, 108.0, 109.0, 1e6)], s, &cfg());
    assert!(fill_prices(&open).is_empty());
    assert!(open.orders[0].triggered && open.orders[0].state == OrderState::Pending);
}

#[test]
fn a_partially_filled_stop_keeps_filling_as_a_market_order() {
    let mut c = cfg();
    c.exec = ExecutionConfig { max_participation: q(0.5), ..ExecutionConfig::default() };
    let s = OrderSpec { stop: p(105.0), ..spec(Side::Buy, OrderType::Stop, 30.0) };
    // Volume 20: at most 10 a bar. Triggers on bar 2, then fills at the next opens without re-touching 105.
    let r = run(
        &[
            (100.0, 100.0, 100.0, 100.0, 20.0),
            (100.0, 106.0, 99.0, 104.0, 20.0),
            (101.0, 102.0, 100.0, 101.0, 20.0),
            (102.0, 103.0, 101.0, 102.0, 20.0),
        ],
        s,
        &c,
    );
    assert_eq!(fill_prices(&r), [p(105.0), p(101.0), p(102.0)]);
    assert_eq!(r.orders[0].state, OrderState::Filled);
}

#[test]
fn day_orders_expire_after_their_first_bar() {
    let far = OrderSpec { limit: p(50.0), tif: TimeInForce::Day, ..spec(Side::Buy, OrderType::Limit, 10.0) };
    let r = run(&[FLAT, FLAT, FLAT], far, &cfg());
    assert_eq!(r.orders[0].state, OrderState::Expired);
    assert!(fill_prices(&r).is_empty());

    // With a two-bar delay, its one bar is the third.
    let delayed =
        BacktestConfig { exec: ExecutionConfig { delay_bars: 2, ..ExecutionConfig::default() }, ..cfg() };
    let near =
        OrderSpec { limit: p(100.0), tif: TimeInForce::Day, ..spec(Side::Buy, OrderType::Limit, 10.0) };
    let r = run(&[FLAT, FLAT, FLAT, FLAT], near, &delayed);
    assert_eq!(r.orders[0].state, OrderState::Filled);
    assert_eq!(
        r.ledger.journal().iter().find(|e| e.kind == spectra_core::EntryType::Fill).unwrap().ts_ns,
        4 * DAY
    );
}

#[test]
fn good_till_date_orders_expire_after_their_date() {
    let until_bar3 = OrderSpec {
        limit: p(50.0),
        tif: TimeInForce::GoodTillDate(3 * DAY),
        ..spec(Side::Buy, OrderType::Limit, 1.0)
    };
    let r = run(&[FLAT, FLAT, FLAT, FLAT, FLAT], until_bar3, &cfg());
    assert_eq!(r.orders[0].state, OrderState::Expired);
    // Still open on its last day if the run ends there.
    let r = run(&[FLAT, FLAT, FLAT], until_bar3, &cfg());
    assert_eq!(r.orders[0].state, OrderState::Pending);
    // A date already past is refused.
    let past = OrderSpec { tif: TimeInForce::GoodTillDate(0), ..until_bar3 };
    assert_eq!(run(&[FLAT, FLAT], past, &cfg()).orders[0].state, OrderState::Rejected);
}

#[test]
fn malformed_stop_orders_are_rejected() {
    for s in [
        spec(Side::Buy, OrderType::Stop, 1.0), // no stop price
        OrderSpec { stop: p(101.0), ..spec(Side::Buy, OrderType::StopLimit, 1.0) }, // no limit
    ] {
        assert_eq!(run(&[FLAT, FLAT], s, &cfg()).orders[0].state, OrderState::Rejected);
    }
}

#[test]
fn market_impact_grows_with_size_and_volatility() {
    // Closes alternate 100, 102: every return is +2% or -1.96...%, so sigma is fixed.
    let mut rows: Vec<(f64, f64, f64, f64, f64)> =
        (0..21)
            .map(|i| {
                if i % 2 == 0 {
                    (100.0, 102.0, 100.0, 100.0, 1e4)
                } else {
                    (102.0, 102.0, 100.0, 102.0, 1e4)
                }
            })
            .collect();
    rows.push((100.0, 100.0, 100.0, 100.0, 1e4)); // the fill bar
    let impact = BacktestConfig { impact: ImpactConfig { coefficient: 1.0, window: 20 }, ..cfg() };

    /// Buys on bar 21 (index 20), so the fill is on the last bar.
    struct Late(f64, usize);
    impl Strategy for Late {
        fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
            self.1 += 1;
            if self.1 == 21 {
                ctx.market(bars[0].instrument, Side::Buy, Qty::from_f64(self.0).unwrap());
            }
        }
    }
    let price = |qty: f64, c: &BacktestConfig| {
        let r = run_backtest(&bars(&rows), &mut Late(qty, 0), c).unwrap();
        fill_prices(&r)[0].to_f64()
    };
    assert_eq!(price(100.0, &cfg()), 100.0, "no impact by default");

    // Expected: 100 * (1 + sigma * sqrt(qty / 1e4)), sigma of the 20 returns before the fill bar.
    let closes: Vec<f64> = rows[..21].iter().map(|r| r.3).collect();
    let rets: Vec<f64> = closes.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let sigma = (rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64).sqrt();
    for qty in [100.0, 2_500.0] {
        let expected = 100.0 * (1.0 + sigma * (qty / 1e4f64).sqrt());
        assert!(
            (price(qty, &impact) - expected).abs() < 1e-6,
            "qty {qty}: {} vs {expected}",
            price(qty, &impact)
        );
    }
    assert!(price(2_500.0, &impact) > price(100.0, &impact));

    // Capped at 10%.
    let huge = BacktestConfig { impact: ImpactConfig { coefficient: 100.0, window: 20 }, ..cfg() };
    assert!((price(5_000.0, &huge) - 110.0).abs() < 1e-6);
}

#[test]
fn market_impact_needs_history_and_skips_limits() {
    let impact = BacktestConfig { impact: ImpactConfig { coefficient: 1.0, window: 20 }, ..cfg() };
    // Fill on bar 2: one prior bar, no returns, no impact.
    let m =
        run(&[FLAT, (100.0, 101.0, 99.0, 100.0, 1e4)], spec(Side::Buy, OrderType::Market, 1000.0), &impact);
    assert_eq!(fill_prices(&m), [p(100.0)]);
    // Limits never take impact.
    let l = OrderSpec { limit: p(100.0), ..spec(Side::Buy, OrderType::Limit, 1000.0) };
    assert_eq!(fill_prices(&run(&[FLAT, FLAT], l, &impact)), [p(100.0)]);
}

#[test]
fn invalid_impact_settings_are_refused() {
    for imp in [ImpactConfig { coefficient: -1.0, window: 20 }, ImpactConfig { coefficient: 1.0, window: 1 }]
    {
        let c = BacktestConfig { impact: imp, ..cfg() };
        assert!(run_backtest(&bars(&[FLAT, FLAT]), &mut Place(None), &c).is_err());
    }
}

#[test]
fn new_features_change_fingerprints_only_when_used() {
    let data = bars(&[FLAT, (100.0, 106.0, 99.0, 104.0, 1e6), FLAT]);
    let limit = OrderSpec { limit: p(100.0), ..spec(Side::Buy, OrderType::Limit, 1.0) };
    let a = run_backtest(&data, &mut Place(Some(limit)), &cfg()).unwrap();
    let b =
        run_backtest(&data, &mut Place(Some(OrderSpec { tif: TimeInForce::Day, ..limit })), &cfg()).unwrap();
    assert_ne!(a.run.result_sha256, b.run.result_sha256, "time in force is part of the result");
    let with_impact = BacktestConfig { impact: ImpactConfig { coefficient: 0.5, window: 20 }, ..cfg() };
    assert_ne!(spectra_engine::config_fingerprint(&with_impact), spectra_engine::config_fingerprint(&cfg()));
    assert_eq!(
        spectra_engine::config_fingerprint(&BacktestConfig {
            impact: ImpactConfig { coefficient: 0.0, window: 5 },
            ..cfg()
        }),
        spectra_engine::config_fingerprint(&cfg()),
        "an unused window does not change the fingerprint"
    );
    let with_spread = BacktestConfig {
        costs: spectra_engine::CostModel { spread: Bps::from_f64(10.0).unwrap(), ..Default::default() },
        ..cfg()
    };
    assert_ne!(spectra_engine::config_fingerprint(&with_spread), spectra_engine::config_fingerprint(&cfg()));
    let with_vol_slip = BacktestConfig {
        costs: spectra_engine::CostModel {
            volume_slippage: Bps::from_f64(25.0).unwrap(),
            ..Default::default()
        },
        ..cfg()
    };
    assert_ne!(
        spectra_engine::config_fingerprint(&with_vol_slip),
        spectra_engine::config_fingerprint(&cfg())
    );
}

#[test]
fn spread_cost_moves_market_orders_by_half_spread() {
    // 10 bps spread => 5 bps half-spread paid on market orders
    let mut c = cfg();
    c.ledger.allow_short = true;
    c.costs.spread = Bps::from_f64(10.0).unwrap();

    let buy = spec(Side::Buy, OrderType::Market, 100.0);
    // Open is 100.0; buy fills at 100.0 + 5 bps = 100.05
    assert_eq!(fill_prices(&run(&[FLAT, FLAT], buy, &c)), [p(100.05)]);

    let sell = spec(Side::Sell, OrderType::Market, 100.0);
    // Sell fills at 100.0 - 5 bps = 99.95
    assert_eq!(fill_prices(&run(&[FLAT, FLAT], sell, &c)), [p(99.95)]);

    // Limit orders do not cross the spread
    let limit = OrderSpec { limit: p(100.0), ..spec(Side::Buy, OrderType::Limit, 100.0) };
    assert_eq!(fill_prices(&run(&[FLAT, FLAT], limit, &c)), [p(100.0)]);
}

#[test]
fn volume_based_slippage_scales_with_participation() {
    let mut c = cfg();
    c.costs.volume_slippage = Bps::from_f64(20.0).unwrap();

    // Bar volume = 1,000. Qty = 500 => 50% participation => 10 bps slippage
    let buy_half = spec(Side::Buy, OrderType::Market, 500.0);
    assert_eq!(fill_prices(&run(&[FLAT, (100.0, 101.0, 99.0, 100.0, 1_000.0)], buy_half, &c)), [p(100.1)]);

    // Qty = 250 => 25% participation => 5 bps slippage
    let buy_quarter = spec(Side::Buy, OrderType::Market, 250.0);
    assert_eq!(
        fill_prices(&run(&[FLAT, (100.0, 101.0, 99.0, 100.0, 1_000.0)], buy_quarter, &c)),
        [p(100.05)]
    );
}
