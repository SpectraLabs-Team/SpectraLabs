//! M26 task 3e: known-answer reference tests for Strategy Survival Lab
//! components 6, 7, and 8 (`src/survival.rs`):
//! - Component 6: Sample size and concentration
//! - Component 7: Regime sensitivity
//! - Component 8: Benchmark-relative

use spectra_core::*;
use spectra_engine::*;
use spectra_research::*;

/// Bars from a fixed close path.
fn bars_from(closes: &[f64]) -> Vec<Bar> {
    closes
        .iter()
        .enumerate()
        .map(|(i, &c)| Bar {
            ts_ns: i as i64 + 1,
            known_at_ns: i as i64 + 1,
            instrument: 1,
            open: Price::from_f64(c).unwrap(),
            close: Price::from_f64(c).unwrap(),
            high: Price::from_f64(c * 1.001).unwrap(),
            low: Price::from_f64(c * 0.999).unwrap(),
            volume: Qty::from_f64(1e9).unwrap(),
        })
        .collect()
}

/// Closes path from returns.
fn path_from_returns(returns: &[f64]) -> Vec<f64> {
    let mut closes = Vec::with_capacity(returns.len() + 1);
    let mut c = 100.0;
    closes.push(c);
    for &r in returns {
        c *= 1.0 + r;
        closes.push(c);
    }
    closes
}

/// Generates a price path that yields the exact trade PnLs via `RepeatedTrader`.
/// Each trade round-trip is 2 bars: buy at 1000.0, sell at 1000.0 + pnl.
fn path_from_trade_pnls(pnls: &[f64]) -> Vec<f64> {
    let mut closes = Vec::with_capacity(pnls.len() * 2 + 1);
    closes.push(1000.0);
    for &pnl in pnls {
        closes.push(1000.0);
        closes.push(1000.0 + pnl);
    }
    closes
}

/// Strategy that does not place any trades.
struct NoTrades;

impl Strategy for NoTrades {
    fn on_bar(&mut self, _ctx: &mut Context<'_>, _bars: &[Bar]) {}
}

/// Strategy that buys on odd bars and sells on even bars (1 unit per trade).
struct RepeatedTrader;

impl Strategy for RepeatedTrader {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n % 2 == 1 && ctx.position(1).is_zero() {
            ctx.market(1, Side::Buy, Qty::from_f64(1.0).unwrap());
        } else if n % 2 == 0 && !ctx.position(1).is_zero() {
            ctx.market(1, Side::Sell, ctx.position(1));
        }
    }
}

/// Strategy that buys 95% of equity on bar 1 and holds.
struct BuyAndHold;

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == 1 && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = 0.95 * ctx.equity().to_f64() / price;
            ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
        }
    }
}

/// Strategy that stays in cash until `start_bar`, then buys 95% of equity and holds.
struct DelayedBuyAndHold {
    start_bar: usize,
}

impl Strategy for DelayedBuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == self.start_bar && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = 0.95 * ctx.equity().to_f64() / price;
            ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
        }
    }
}

/// Strategy that buys 100% of equity on bar 1, leaving 0 cash so equity return
/// mirrors price return identically.
struct FullBuyAndHold;

impl Strategy for FullBuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == 1 && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = ctx.equity().to_f64() / price;
            ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
        }
    }
}

/// Mean-reverting strategy: buys when price dips below 98.5, sells when price rises above 101.5.
struct MeanReversionTrader;

impl Strategy for MeanReversionTrader {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let price = bars[0].close.to_f64();
        if price < 98.5 && ctx.position(1).is_zero() {
            ctx.market(1, Side::Buy, Qty::from_f64(300.0).unwrap());
        } else if price > 101.5 && !ctx.position(1).is_zero() {
            ctx.market(1, Side::Sell, ctx.position(1));
        }
    }
}

/// Strategy that loses money heavily by buying peaks and selling troughs.
struct HeavyLossTrader;

impl Strategy for HeavyLossTrader {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n % 2 == 1 && ctx.position(1).is_zero() {
            ctx.market(1, Side::Buy, Qty::from_f64(500.0).unwrap());
        } else if n % 2 == 0 && !ctx.position(1).is_zero() {
            ctx.market(1, Side::Sell, ctx.position(1));
        }
    }
}

// =============================================================================
// Component 6: Sample Size and Concentration
// =============================================================================

#[test]
fn sample_size_insufficient_when_zero_trades() {
    let closes = vec![100.0; 40];
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(NoTrades)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!("Component 6 Zero Trades: trades={}, verdict={:?}", s.trades, s.verdict);
    assert_eq!(s.trades, 0);
    assert_eq!(s.verdict, Verdict::Insufficient);
}

#[test]
fn sample_size_insufficient_boundary_at_nine_trades() {
    // Exact boundary: 9 trades < trades_insufficient_below (10).
    // All 9 trade PnLs are strictly distinct: 6 wins, 3 losses.
    let trades = vec![20.0, 22.0, -28.0, 24.0, 26.0, -34.0, 28.0, 30.0, -40.0];
    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Boundary 9 Trades: trades={}, top_trade_share={:.4}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 9);
    assert!(s.trades < 10);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Insufficient);
}

#[test]
fn sample_size_warn_boundary_at_ten_trades_with_low_concentration() {
    // Exact boundary: 10 trades == trades_insufficient_below (10).
    // 10 <= trades < 30 triggers Warn. Top-trade share is well below 0.50 (fine concentration).
    // All 10 trade PnLs are strictly distinct: 7 wins, 3 losses.
    let trades = vec![20.0, 22.0, -35.0, 24.0, 26.0, -42.0, 28.0, 30.0, -48.0, 32.0];
    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Boundary 10 Trades: trades={}, top_trade_share={:.4}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 10);
    assert!(s.trades < 30);
    assert!(s.top_trade_share <= 0.50);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Warn);
}

#[test]
fn sample_size_and_concentration_pass_with_thirty_trades_and_fine_concentration() {
    // Exact boundary: 30 trades == trades_warn_below (30).
    // 20 distinct wins (30, 32, ..., 68) and 10 distinct losses (-50, -52, ..., -68).
    // Best trade is 68.0, win sum is 980.0, top trade share is ~0.0694 <= 0.50.
    let mut trades = Vec::with_capacity(30);
    let mut w = 0usize;
    let mut l = 0usize;
    for i in 0..30 {
        if i % 3 == 0 && l < 10 {
            trades.push(-50.0 - 2.0 * f64::from(l as u32));
            l += 1;
        } else if w < 20 {
            trades.push(30.0 + 2.0 * f64::from(w as u32));
            w += 1;
        }
    }

    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Pass 30 Trades: trades={}, top_trade_share={:.4}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 30);
    assert!(s.trades >= 30);
    assert!(s.top_trade_share <= 0.50);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Pass);
}

#[test]
fn sample_concentration_pass_boundary_at_top_trade_share_exactly_fifty_percent() {
    // Exact boundary: top_trade_share == 0.50 with 30 trades.
    // 20 distinct wins: 1..18 (sum 171), 79, and 250 (best).
    // win_sum = 171 + 79 + 250 = 500.0, best = 250.0 -> best / win_sum = 0.5000000000 exactly!
    // 10 distinct losses: -18, -20, ..., -36.
    // All 30 trades have distinct PnL values.
    let mut wins: Vec<f64> = (1..=18).map(f64::from).collect();
    wins.push(79.0);
    wins.push(250.0);

    let losses: Vec<f64> = (0..10).map(|i| -18.0 - 2.0 * f64::from(i)).collect();

    let mut trades = Vec::with_capacity(30);
    let mut w = 0usize;
    let mut l = 0usize;
    for i in 0..30 {
        if i % 3 == 0 && l < losses.len() {
            trades.push(losses[l]);
            l += 1;
        } else if w < wins.len() {
            trades.push(wins[w]);
            w += 1;
        }
    }

    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Boundary 50% Share: trades={}, top_trade_share={:.10}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 30);
    assert_eq!(s.top_trade_share, 0.50);
    assert!(s.top_trade_share <= 0.50);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Pass);
}

#[test]
fn sample_concentration_warns_when_top_trade_share_between_fifty_and_eighty_percent() {
    // 30 trades, top-trade share in (0.50, 0.80].
    // 20 distinct wins: 1..19 (sum 190) and 380 (best).
    // win_sum = 190 + 380 = 570.0, best = 380.0 -> share = 380 / 570 = 2/3 ≈ 0.6667.
    // 10 distinct losses: -30, -33, ..., -57.
    let mut wins: Vec<f64> = (1..=19).map(f64::from).collect();
    wins.push(380.0);

    let losses: Vec<f64> = (0..10).map(|i| -30.0 - 3.0 * f64::from(i)).collect();

    let mut trades = Vec::with_capacity(30);
    let mut w = 0usize;
    let mut l = 0usize;
    for i in 0..30 {
        if i % 3 == 0 && l < losses.len() {
            trades.push(losses[l]);
            l += 1;
        } else if w < wins.len() {
            trades.push(wins[w]);
            w += 1;
        }
    }

    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Warn 66.7% Share: trades={}, top_trade_share={:.4}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 30);
    assert!(s.top_trade_share > 0.50);
    assert!(s.top_trade_share <= 0.80);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Warn);
}

#[test]
fn sample_concentration_warn_boundary_at_top_trade_share_exactly_eighty_percent() {
    // Exact boundary: top_trade_share == 0.80 with 30 trades.
    // 20 distinct wins: 1..19 (sum 190) and 760 (best).
    // win_sum = 190 + 760 = 950.0, best = 760.0 -> share = 760 / 950 = 0.8000000000 exactly!
    // Since condition for Fail is `top_trade_share > 0.80`, exactly 0.80 is Warn, not Fail.
    // 10 distinct losses: -40, -45, ..., -85.
    let mut wins: Vec<f64> = (1..=19).map(f64::from).collect();
    wins.push(760.0);

    let losses: Vec<f64> = (0..10).map(|i| -40.0 - 5.0 * f64::from(i)).collect();

    let mut trades = Vec::with_capacity(30);
    let mut w = 0usize;
    let mut l = 0usize;
    for i in 0..30 {
        if i % 3 == 0 && l < losses.len() {
            trades.push(losses[l]);
            l += 1;
        } else if w < wins.len() {
            trades.push(wins[w]);
            w += 1;
        }
    }

    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Boundary 80% Share: trades={}, top_trade_share={:.10}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 30);
    assert_eq!(s.top_trade_share, 0.80);
    assert!(s.top_trade_share <= 0.80);
    assert!(s.top_trade_share > 0.50);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Warn);
}

#[test]
fn sample_concentration_fails_when_top_trade_share_exceeds_eighty_percent() {
    // 30 trades, top-trade share > 0.80.
    // 20 distinct wins: 1..19 (sum 190) and 850 (best).
    // win_sum = 190 + 850 = 1040.0, best = 850.0 -> share = 850 / 1040 ≈ 0.8173 > 0.80.
    // 10 distinct losses: -45, -50, ..., -90.
    let mut wins: Vec<f64> = (1..=19).map(f64::from).collect();
    wins.push(850.0);

    let losses: Vec<f64> = (0..10).map(|i| -45.0 - 5.0 * f64::from(i)).collect();

    let mut trades = Vec::with_capacity(30);
    let mut w = 0usize;
    let mut l = 0usize;
    for i in 0..30 {
        if i % 3 == 0 && l < losses.len() {
            trades.push(losses[l]);
            l += 1;
        } else if w < wins.len() {
            trades.push(wins[w]);
            w += 1;
        }
    }

    let closes = path_from_trade_pnls(&trades);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let s = &rep.sample;
    println!(
        "Component 6 Fail >80% Share: trades={}, top_trade_share={:.4}, sharpe={:.2}, verdict={:?}",
        s.trades, s.top_trade_share, rep.baseline.sharpe, s.verdict
    );

    assert_eq!(s.trades, 30);
    assert!(s.top_trade_share > 0.80);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(s.verdict, Verdict::Fail);
}

// =============================================================================
// Component 7: Regime Sensitivity
// =============================================================================

fn make_regime_path(drift_low: f64, drift_med: f64, drift_high: f64) -> Vec<f64> {
    let mut r = Vec::with_capacity(95);
    // 45 periods of low volatility (20 warmup + 25 low vol periods):
    for i in 0..45 {
        let t = f64::from(i as u32);
        r.push(drift_low + 0.0005 * (t * 0.7).sin());
    }
    // 25 periods of medium volatility:
    for i in 0..25 {
        let t = f64::from(i as u32);
        r.push(drift_med + 0.006 * (t * 0.8).sin());
    }
    // 25 periods of high volatility:
    for i in 0..25 {
        let t = f64::from(i as u32);
        r.push(drift_high + 0.025 * (t * 0.9).sin());
    }
    path_from_returns(&r)
}

#[test]
fn regime_sensitivity_insufficient_when_fewer_than_sixty_periods() {
    // 50 periods total (20 warmup -> 30 labelled periods < 60 needed).
    let r: Vec<f64> = (0..50).map(|i| 0.001 + 0.002 * (f64::from(i as u32) * 0.5).sin()).collect();
    let closes = path_from_returns(&r);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let rg = &rep.regimes;
    println!("Component 7 Insufficient Periods: buckets={}, verdict={:?}", rg.buckets.len(), rg.verdict);
    assert!(rg.buckets.is_empty());
    assert_eq!(rg.verdict, Verdict::Insufficient);
}

#[test]
fn regime_sensitivity_insufficient_when_a_bucket_has_fewer_than_ten_periods() {
    // 95 periods, but with regime_min_per_bucket configured to 30 (> 25 per bucket),
    // each bucket has 25 periods < 30, so too_small is triggered.
    let closes = make_regime_path(0.0010, 0.0012, 0.0015);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let sc = SurvivalConfig { regime_min_per_bucket: 30, ..Default::default() };
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc).unwrap();

    let rg = &rep.regimes;
    println!(
        "Component 7 Insufficient Bucket Count: buckets={}, periods={:?}, verdict={:?}",
        rg.buckets.len(),
        rg.buckets.iter().map(|b| b.periods).collect::<Vec<_>>(),
        rg.verdict
    );

    assert_eq!(rg.buckets.len(), 3);
    assert!(rg.buckets.iter().any(|b| b.periods < 30));
    assert_eq!(rg.verdict, Verdict::Insufficient);
}

#[test]
fn regime_sensitivity_passes_when_all_three_buckets_have_positive_mean() {
    let closes = make_regime_path(0.0008, 0.0010, 0.0012);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let rg = &rep.regimes;
    let m0 = rg.buckets[0].mean_return;
    let m1 = rg.buckets[1].mean_return;
    let m2 = rg.buckets[2].mean_return;

    println!("Component 7 Pass (all positive):");
    println!("  means: m0={m0:.8}, m1={m1:.8}, m2={m2:.8}");
    println!("  sharpe={:.2}, verdict={:?}", rep.baseline.sharpe, rg.verdict);

    assert_eq!(rg.buckets.len(), 3);
    assert!(m0 > 0.0);
    assert!(m1 > 0.0);
    assert!(m2 > 0.0);
    // Values used in rule comparison must be distinct:
    assert!((m0 - m1).abs() > 1e-9);
    assert!((m1 - m2).abs() > 1e-9);
    assert!((m0 - m2).abs() > 1e-9);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(rg.verdict, Verdict::Pass);
}

#[test]
fn regime_sensitivity_passes_at_boundary_with_bucket_mean_exactly_zero() {
    // Strategy stays in cash until bar 48, so during all 25 periods of bucket 0 (t <= 46),
    // strategy return is exactly 0.0000000000!
    // Since 0.0 < 0.0 is false, 0 buckets lose money -> Pass!
    let closes = make_regime_path(0.0008, 0.0010, 0.0012);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> {
        Ok(Box::new(DelayedBuyAndHold { start_bar: 48 }))
    };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let rg = &rep.regimes;
    let m0 = rg.buckets[0].mean_return;
    let m1 = rg.buckets[1].mean_return;
    let m2 = rg.buckets[2].mean_return;

    println!("Component 7 Boundary (mean == 0.0):");
    println!("  means: m0={m0:.10}, m1={m1:.8}, m2={m2:.8}");
    println!("  sharpe={:.2}, verdict={:?}", rep.baseline.sharpe, rg.verdict);

    assert_eq!(rg.buckets.len(), 3);
    assert_eq!(m0, 0.0);
    assert!(m1 > 0.0);
    assert!(m2 > 0.0);
    assert!((m0 - m1).abs() > 1e-9);
    assert!((m1 - m2).abs() > 1e-9);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(rg.verdict, Verdict::Pass);
}

#[test]
fn regime_sensitivity_warns_when_exactly_one_bucket_has_negative_mean() {
    // High vol bucket has negative drift (-0.0020), so bucket 2 mean < 0.
    // Buckets 0 and 1 have positive mean. Exactly 1 negative bucket -> Warn.
    let closes = make_regime_path(0.0012, 0.0015, -0.0020);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let rg = &rep.regimes;
    let m0 = rg.buckets[0].mean_return;
    let m1 = rg.buckets[1].mean_return;
    let m2 = rg.buckets[2].mean_return;

    println!("Component 7 Warn (1 negative bucket):");
    println!("  means: m0={m0:.8}, m1={m1:.8}, m2={m2:.8}");
    println!("  sharpe={:.2}, verdict={:?}", rep.baseline.sharpe, rg.verdict);

    assert_eq!(rg.buckets.len(), 3);
    assert!(m0 > 0.0);
    assert!(m1 > 0.0);
    assert!(m2 < 0.0);
    assert!((m0 - m1).abs() > 1e-9);
    assert!((m1 - m2).abs() > 1e-9);
    assert!((m0 - m2).abs() > 1e-9);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(rg.verdict, Verdict::Warn);
}

#[test]
fn regime_sensitivity_fails_when_two_or_three_buckets_have_negative_mean() {
    // Med vol bucket (-0.0020) and high vol bucket (-0.0025) both have negative drift.
    // Buckets 1 and 2 mean < 0. Exactly 2 negative buckets -> Fail.
    let closes = make_regime_path(0.0015, -0.0020, -0.0025);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let rg = &rep.regimes;
    let m0 = rg.buckets[0].mean_return;
    let m1 = rg.buckets[1].mean_return;
    let m2 = rg.buckets[2].mean_return;

    println!("Component 7 Fail (2 negative buckets):");
    println!("  means: m0={m0:.8}, m1={m1:.8}, m2={m2:.8}");
    println!("  sharpe={:.2}, verdict={:?}", rep.baseline.sharpe, rg.verdict);

    assert_eq!(rg.buckets.len(), 3);
    assert!(m0 > 0.0);
    assert!(m1 < 0.0);
    assert!(m2 < 0.0);
    assert!((m0 - m1).abs() > 1e-9);
    assert!((m1 - m2).abs() > 1e-9);
    assert!((m0 - m2).abs() > 1e-9);
    assert_eq!(rg.verdict, Verdict::Fail);
}

// =============================================================================
// Component 8: Benchmark-Relative
// =============================================================================

#[test]
fn benchmark_relative_insufficient_when_benchmark_has_zero_variance() {
    // Constant prices: benchmark variance is 0.0, so compute_benchmark_metrics
    // sets valid = false, leaving verdict as Insufficient.
    let closes = vec![100.0; 80];
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let bm = &rep.benchmark;
    println!("Component 8 Insufficient: valid={}, verdict={:?}", bm.metrics.valid, bm.verdict);
    assert!(!bm.metrics.valid);
    assert_eq!(bm.verdict, Verdict::Insufficient);
}

#[test]
fn benchmark_relative_passes_when_alpha_and_information_ratio_positive() {
    // Benchmark oscillates around 100 with sine wave plus harmonic variation.
    // MeanReversionTrader buys below 98.5 and sells above 101.5, generating active alpha > 0 and IR > 0.
    let closes: Vec<f64> = (0..80)
        .map(|i| {
            let t = f64::from(i as u32);
            100.0 + 5.0 * (t * 0.15).sin() + 3.0 * (t * 0.5).cos()
        })
        .collect();
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(MeanReversionTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let bm = &rep.benchmark;
    let m = &bm.metrics;
    println!("Component 8 Pass:");
    println!(
        "  beta={:.4}, alpha={:.4}, IR={:.4}, sharpe={:.2}, verdict={:?}",
        m.beta, m.alpha_annualized, m.information_ratio, rep.baseline.sharpe, bm.verdict
    );

    assert!(m.valid);
    assert!(m.alpha_annualized > 0.0);
    assert!(m.information_ratio > 0.0);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(bm.verdict, Verdict::Pass);
}

#[test]
fn benchmark_relative_warn_boundary_at_alpha_and_information_ratio_exactly_zero() {
    // When strategy buys 100% of equity (FullBuyAndHold), equity curve return is
    // identically equal to benchmark return (s == b).
    // As proven mathematically: beta == 1.0, alpha == 0.0, and tracking error == 0.0 <= 1e-15,
    // so information_ratio == 0.0.
    // The verdict rule: alpha <= 0.0 || IR <= 0.0 -> Warn.
    // This tests the exact 0.0 boundary for both alpha and information ratio simultaneously.
    let mut closes = vec![100.0, 100.0];
    for i in 1..79 {
        let t = f64::from(i as u32);
        closes.push(100.0 + 0.1 * t + 2.0 * (t * 0.3).sin());
    }
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(FullBuyAndHold)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let bm = &rep.benchmark;
    let m = &bm.metrics;
    println!("Component 8 Boundary 0.0 Alpha & IR:");
    println!(
        "  beta={:.4}, alpha={:.10}, IR={:.10}, sharpe={:.2}, verdict={:?}",
        m.beta, m.alpha_annualized, m.information_ratio, rep.baseline.sharpe, bm.verdict
    );

    assert!(m.valid);
    assert!(m.alpha_annualized.abs() < 1e-9);
    assert_eq!(m.information_ratio, 0.0);
    assert!(m.alpha_annualized <= 0.0);
    assert!(m.information_ratio <= 0.0);
    assert!(rep.baseline.sharpe >= 0.5 && rep.baseline.sharpe <= 3.0);
    assert_eq!(bm.verdict, Verdict::Warn);
}

#[test]
fn benchmark_relative_never_produces_fail_verdict() {
    // Even under severe losses and deeply negative alpha / IR, Component 8
    // never emits Verdict::Fail by design. It only produces Pass, Warn, or Insufficient.
    let closes: Vec<f64> = (0..80)
        .map(|i| {
            let t = f64::from(i as u32);
            if i % 2 == 1 {
                120.0 + t
            } else {
                80.0 - t
            }
        })
        .collect();
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(HeavyLossTrader)) };

    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let bm = &rep.benchmark;
    let m = &bm.metrics;
    println!("Component 8 Never Fail:");
    println!(
        "  beta={:.4}, alpha={:.4}, IR={:.4}, verdict={:?}",
        m.beta, m.alpha_annualized, m.information_ratio, bm.verdict
    );

    assert!(m.valid);
    assert!(m.alpha_annualized < 0.0);
    assert!(m.information_ratio < 0.0);
    assert_eq!(bm.verdict, Verdict::Warn);
    assert_ne!(bm.verdict, Verdict::Fail);
}
