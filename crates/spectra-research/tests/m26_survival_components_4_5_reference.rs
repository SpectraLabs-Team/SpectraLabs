//! M26 task 3e: known-answer reference tests for Strategy Survival Lab
//! components 4 and 5 (`src/survival.rs`):
//! - Component 4: Time-period robustness
//! - Component 5: Trade bootstrap

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

/// Strategy that executes repeated round-trip trades: buys on odd bars, sells on even bars.
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

fn make_segment(n: usize, mean: f64, amp: f64, phase: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = f64::from(i as u32);
            mean + amp * (t * 1.3 + phase).sin()
        })
        .collect()
}

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

fn path_from_trade_pnls(pnls: &[f64]) -> Vec<f64> {
    let mut closes = Vec::with_capacity(pnls.len() * 2 + 1);
    closes.push(1000.0);
    for &pnl in pnls {
        closes.push(1000.0);
        closes.push(1000.0 + pnl);
    }
    closes
}

// -----------------------------------------------------------------------------
// Component 4: Time-period robustness
// -----------------------------------------------------------------------------

#[test]
fn time_insufficient_when_fewer_than_eighty_timestamps() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 + f64::from(i)).collect();
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

    println!("Component 4 Insufficient: subperiods={}", rep.time.subperiods.len());
    assert!(bars.len() < 80);
    assert!(rep.time.subperiods.is_empty());
    assert_eq!(rep.time.verdict, Verdict::Insufficient);
}

#[test]
fn time_passes_when_share_at_least_seventy_five_and_second_half_healthy() {
    let mut r = Vec::new();
    r.extend(make_segment(20, 0.0006, 0.010, 0.1));
    r.extend(make_segment(20, 0.0010, 0.010, 0.2));
    r.extend(make_segment(20, 0.0008, 0.010, 0.3));
    r.extend(make_segment(20, 0.0012, 0.010, 0.4));
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

    let s = &rep.time;
    println!("Component 4 Pass:");
    println!("  subperiods: {:?}", s.subperiods.iter().map(|p| p.sharpe).collect::<Vec<_>>());
    println!(
        "  share_positive: {}, first_half: {}, second_half: {}",
        s.share_positive, s.first_half_sharpe, s.second_half_sharpe
    );

    assert_eq!(s.subperiods.len(), 4);
    assert_eq!(s.share_positive, 1.0);
    assert!(s.share_positive >= 0.75);
    assert!(s.second_half_sharpe >= 0.5 * s.first_half_sharpe);
    for p in &s.subperiods {
        assert!(p.sharpe > 0.0, "subperiod Sharpe must be positive");
        assert!(p.sharpe >= 0.5 && p.sharpe <= 3.5, "realistic Sharpe range");
    }
    // Verify distinct subperiods
    for i in 0..3 {
        assert!((s.subperiods[i].sharpe - s.subperiods[i + 1].sharpe).abs() > 0.05);
    }
    assert_eq!(s.verdict, Verdict::Pass);
}

#[test]
fn time_warns_when_share_between_fifty_and_seventy_five_percent() {
    let mut r = Vec::new();
    r.extend(make_segment(20, 0.0009, 0.010, 0.1));
    r.extend(make_segment(20, -0.0003, 0.010, 0.2));
    r.extend(make_segment(20, 0.0011, 0.010, 0.3));
    r.extend(make_segment(20, -0.0005, 0.010, 0.4));
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

    let s = &rep.time;
    println!("Component 4 Warn (share):");
    println!("  subperiods: {:?}", s.subperiods.iter().map(|p| p.sharpe).collect::<Vec<_>>());
    println!(
        "  share_positive: {}, first_half: {}, second_half: {}",
        s.share_positive, s.first_half_sharpe, s.second_half_sharpe
    );

    assert_eq!(s.subperiods.len(), 4);
    assert!((s.share_positive - 0.50).abs() < 1e-9);
    assert!(s.share_positive >= 0.50 && s.share_positive < 0.75);
    assert!(s.second_half_sharpe >= 0.5 * s.first_half_sharpe);
    for i in 0..3 {
        assert!((s.subperiods[i].sharpe - s.subperiods[i + 1].sharpe).abs() > 0.05);
    }
    assert_eq!(s.verdict, Verdict::Warn);
}

#[test]
fn time_warns_when_second_half_sharpe_falls_below_half_first_half() {
    let mut r = Vec::new();
    r.extend(make_segment(20, 0.0009, 0.008, 0.1));
    r.extend(make_segment(20, 0.0011, 0.008, 0.2));
    r.extend(make_segment(20, 0.0003, 0.012, 0.3));
    r.extend(make_segment(20, 0.0004, 0.012, 0.4));
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

    let s = &rep.time;
    println!("Component 4 Warn (second half):");
    println!("  subperiods: {:?}", s.subperiods.iter().map(|p| p.sharpe).collect::<Vec<_>>());
    println!(
        "  share_positive: {}, first_half: {}, second_half: {}",
        s.share_positive, s.first_half_sharpe, s.second_half_sharpe
    );

    assert_eq!(s.subperiods.len(), 4);
    assert_eq!(s.share_positive, 1.0);
    assert!(s.share_positive >= 0.75);
    assert!(s.first_half_sharpe > 0.0);
    assert!(s.second_half_sharpe > 0.0);
    assert!(s.second_half_sharpe < 0.5 * s.first_half_sharpe);
    assert_eq!(s.verdict, Verdict::Warn);
}

#[test]
fn time_fails_when_share_positive_below_fifty_percent() {
    let mut r = Vec::new();
    r.extend(make_segment(20, -0.0003, 0.010, 0.1));
    r.extend(make_segment(20, -0.0004, 0.010, 0.2));
    r.extend(make_segment(20, -0.0001, 0.010, 0.3));
    r.extend(make_segment(20, 0.0016, 0.010, 0.4));
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

    let s = &rep.time;
    println!("Component 4 Fail (share):");
    println!("  subperiods: {:?}", s.subperiods.iter().map(|p| p.sharpe).collect::<Vec<_>>());
    println!(
        "  share_positive: {}, first_half: {}, second_half: {}",
        s.share_positive, s.first_half_sharpe, s.second_half_sharpe
    );

    assert_eq!(s.subperiods.len(), 4);
    assert!((s.share_positive - 0.25).abs() < 1e-9);
    assert!(s.share_positive < 0.50);
    assert_eq!(s.verdict, Verdict::Fail);
}

#[test]
fn time_fails_when_second_half_sharpe_is_zero_or_negative() {
    let mut r = Vec::new();
    r.extend(make_segment(20, 0.0007, 0.010, 0.1));
    r.extend(make_segment(20, 0.0010, 0.010, 0.2));
    r.extend(vec![0.0; 40]);
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

    let s = &rep.time;
    println!("Component 4 Fail (second half zero):");
    println!("  first_half: {}, second_half: {}", s.first_half_sharpe, s.second_half_sharpe);

    assert!(s.first_half_sharpe > 0.0);
    assert_eq!(s.second_half_sharpe, 0.0, "second half Sharpe must be exactly 0.0");
    assert_eq!(s.verdict, Verdict::Fail);
}

// -----------------------------------------------------------------------------
// Component 5: Trade bootstrap
// -----------------------------------------------------------------------------

#[test]
fn trade_bootstrap_insufficient_when_fewer_than_ten_closed_trades() {
    let pnls = [100.0; 3];
    let closes = path_from_trade_pnls(&pnls);
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

    println!("Component 5 Insufficient: trades={}", rep.sample.trades);
    assert!(rep.sample.trades < 10);
    assert_eq!(rep.trades.verdict, Verdict::Insufficient);
}

#[test]
fn trade_bootstrap_passes_when_loss_prob_healthy_and_reshuffle_healthy() {
    let pnls = [100.0; 12];
    let closes = path_from_trade_pnls(&pnls);
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

    let r = &rep.trades.resampling;
    println!(
        "Component 5 Pass: loss_prob={}, reshuffle_prob={}",
        r.bootstrap_prob_loss, r.reshuffle_prob_worse_drawdown
    );

    assert_eq!(rep.sample.trades, 12);
    assert!(r.bootstrap_prob_loss <= 0.10);
    assert!(r.reshuffle_prob_worse_drawdown <= 0.80);
    assert_eq!(rep.trades.verdict, Verdict::Pass);
}

#[test]
fn trade_bootstrap_passes_at_loss_prob_boundary_ten_percent() {
    let mut pnls = vec![-140.0; 3];
    pnls.extend(vec![100.0; 8]);
    let closes = path_from_trade_pnls(&pnls);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let base_rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let loss_prob = base_rep.trades.resampling.bootstrap_prob_loss;
    println!("Component 5 Loss Boundary 10%: loss_prob={loss_prob}");

    // At exact boundary (loss_prob == threshold): threshold comparison is strict '>',
    // so loss_prob <= threshold does not trigger Warn; it passes.
    let sc_boundary = SurvivalConfig { bootstrap_loss_warn: loss_prob, ..Default::default() };
    let rep_boundary =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_boundary)
            .unwrap();
    assert_eq!(rep_boundary.trades.verdict, Verdict::Pass);

    // Infinitesimally lower threshold triggers Warn:
    let sc_warn = SurvivalConfig { bootstrap_loss_warn: loss_prob - 1e-4, ..Default::default() };
    let rep_warn =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_warn)
            .unwrap();
    assert_eq!(rep_warn.trades.verdict, Verdict::Warn);
}

#[test]
fn trade_bootstrap_passes_at_reshuffle_boundary_eighty_percent() {
    let mut pnls = vec![200.0; 9];
    pnls.push(-350.0);
    let closes = path_from_trade_pnls(&pnls);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let base_rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let reshuffle_prob = base_rep.trades.resampling.reshuffle_prob_worse_drawdown;
    println!("Component 5 Reshuffle Boundary 80%: reshuffle_prob={reshuffle_prob}");

    // At exact boundary (reshuffle_prob == threshold): threshold comparison is strict '>',
    // so reshuffle_prob <= threshold does NOT escalate; it passes.
    let sc_boundary = SurvivalConfig { lucky_ordering_warn: reshuffle_prob, ..Default::default() };
    let rep_boundary =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_boundary)
            .unwrap();
    assert_eq!(rep_boundary.trades.verdict, Verdict::Pass);

    // Infinitesimally lower threshold escalates to Warn:
    let sc_warn = SurvivalConfig { lucky_ordering_warn: reshuffle_prob - 1e-4, ..Default::default() };
    let rep_warn =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_warn)
            .unwrap();
    assert_eq!(rep_warn.trades.verdict, Verdict::Warn);
}

#[test]
fn trade_bootstrap_warns_when_loss_prob_between_ten_and_twenty_five_percent() {
    let mut pnls = vec![-140.0; 3];
    pnls.extend(vec![100.0; 8]);
    let closes = path_from_trade_pnls(&pnls);
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

    let r = &rep.trades.resampling;
    println!(
        "Component 5 Warn (loss prob): loss_prob={}, reshuffle_prob={}",
        r.bootstrap_prob_loss, r.reshuffle_prob_worse_drawdown
    );

    assert_eq!(rep.sample.trades, 11);
    assert!(r.bootstrap_prob_loss > 0.10 && r.bootstrap_prob_loss <= 0.25);
    assert!(r.reshuffle_prob_worse_drawdown <= 0.80);
    assert_eq!(rep.trades.verdict, Verdict::Warn);
}

#[test]
fn trade_bootstrap_warns_at_loss_prob_boundary_twenty_five_percent() {
    let mut pnls = vec![-140.0; 3];
    pnls.extend(vec![100.0; 8]);
    let closes = path_from_trade_pnls(&pnls);
    let bars = bars_from(&closes);
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(RepeatedTrader)) };

    let base_rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let loss_prob = base_rep.trades.resampling.bootstrap_prob_loss;
    println!("Component 5 Loss Boundary 25%: loss_prob={loss_prob}");

    // At exact fail boundary (loss_prob == bootstrap_loss_fail):
    // loss_prob > fail is false, so it does NOT fail; it warns.
    let sc_boundary = SurvivalConfig { bootstrap_loss_fail: loss_prob, ..Default::default() };
    let rep_boundary =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_boundary)
            .unwrap();
    assert_eq!(rep_boundary.trades.verdict, Verdict::Warn);

    // Infinitesimally lower fail threshold escalates to Fail:
    let sc_fail = SurvivalConfig { bootstrap_loss_fail: loss_prob - 1e-4, ..Default::default() };
    let rep_fail =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc_fail)
            .unwrap();
    assert_eq!(rep_fail.trades.verdict, Verdict::Fail);
}

#[test]
fn trade_bootstrap_warns_when_reshuffle_prob_exceeds_eighty_percent() {
    let mut pnls = vec![200.0; 9];
    pnls.push(-350.0);
    let closes = path_from_trade_pnls(&pnls);
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

    let r = &rep.trades.resampling;
    println!(
        "Component 5 Warn (reshuffle): loss_prob={}, reshuffle_prob={}",
        r.bootstrap_prob_loss, r.reshuffle_prob_worse_drawdown
    );

    assert_eq!(rep.sample.trades, 10);
    assert!(r.bootstrap_prob_loss <= 0.10);
    assert!(r.reshuffle_prob_worse_drawdown > 0.80);
    assert_eq!(rep.trades.verdict, Verdict::Warn);
}

#[test]
fn trade_bootstrap_fails_when_loss_prob_exceeds_twenty_five_percent() {
    let mut pnls = vec![-200.0; 5];
    pnls.extend(vec![100.0; 6]);
    let closes = path_from_trade_pnls(&pnls);
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

    let r = &rep.trades.resampling;
    println!(
        "Component 5 Fail: loss_prob={}, reshuffle_prob={}",
        r.bootstrap_prob_loss, r.reshuffle_prob_worse_drawdown
    );

    assert_eq!(rep.sample.trades, 11);
    assert!(r.bootstrap_prob_loss > 0.25);
    assert_eq!(rep.trades.verdict, Verdict::Fail);
}
