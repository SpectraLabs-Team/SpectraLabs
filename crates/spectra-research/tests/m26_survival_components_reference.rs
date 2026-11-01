//! M26 task 3e: known-answer reference tests for Strategy Survival Lab
//! components 1–3 (`src/survival.rs`):
//! - Component 1: Cost sensitivity
//! - Component 2: Execution delay
//! - Component 3: Parameter stability

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

/// Simple buy-and-hold strategy: buys on bar 1 and sells on bar `last_bar`.
struct BuyAndHold {
    last_bar: usize,
    fraction: f64,
}

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == 1 && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = self.fraction * ctx.equity().to_f64() / price;
            ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
        }
        if n == self.last_bar && !ctx.position(1).is_zero() {
            let pos = ctx.position(1);
            ctx.market(1, Side::Sell, pos);
        }
    }
}

// -----------------------------------------------------------------------------
// Component 1: Cost sensitivity
// -----------------------------------------------------------------------------

#[test]
fn cost_sensitivity_passes_when_stressed_sharpe_and_all_scenarios_exceed_half_baseline() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);
    let n = bars.len();
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> {
        Ok(Box::new(BuyAndHold { last_bar: n, fraction: 0.95 }))
    };
    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let base = rep.baseline.sharpe;
    let stressed = rep.costs.stressed_sharpe;
    assert!(base > 0.0, "baseline Sharpe must be positive, got {base}");
    assert!(stressed >= 0.5 * base, "stressed Sharpe {stressed} must be >= 0.5 * baseline {base}");
    assert_eq!(rep.costs.execution_levels.len(), 12, "must evaluate all 12 execution cost scenarios");
    assert!(
        rep.costs.execution_levels.iter().all(|s| s.verdict == Verdict::Pass),
        "all 12 execution scenarios must pass"
    );
    assert_eq!(rep.costs.verdict, Verdict::Pass);
}

#[test]
fn cost_sensitivity_warns_when_stressed_sharpe_falls_between_zero_and_half_baseline() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);
    let n = bars.len();
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> {
        Ok(Box::new(BuyAndHold { last_bar: n, fraction: 0.95 }))
    };

    let base_rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();
    let ratio = base_rep.costs.stressed_sharpe / base_rep.baseline.sharpe;

    // Set warn ratio higher than the actual ratio so stressed Sharpe is strictly in (0, warn_ratio * base).
    let sc = SurvivalConfig { degradation_warn_ratio: (ratio + 0.005).min(1.0), ..SurvivalConfig::default() };
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc).unwrap();

    let base = rep.baseline.sharpe;
    let stressed = rep.costs.stressed_sharpe;
    assert!(stressed > 0.0, "stressed Sharpe must be strictly positive");
    assert!(
        stressed < sc.degradation_warn_ratio * base,
        "stressed Sharpe must be below warn ratio * baseline"
    );
    assert_eq!(rep.costs.verdict, Verdict::Warn);
}

#[test]
fn cost_sensitivity_fails_when_stressed_sharpe_is_zero_or_negative() {
    // Strategy that gains a tiny amount (0.02%) on baseline, but 5 bps extra slippage (10 bps round trip)
    // completely wipes out the gain into a net loss (stressed Sharpe < 0.0).
    let mut closes = vec![100.0; 60];
    closes[30] = 100.02; // +2 bps gain on bar 30
    let bars = bars_from(&closes);

    struct Scalper;
    impl Strategy for Scalper {
        fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
            let n = ctx.history(bars[0].instrument).len();
            if n == 29 && ctx.position(1).is_zero() {
                ctx.market(1, Side::Buy, Qty::from_f64(100.0).unwrap());
            }
            if n == 30 && !ctx.position(1).is_zero() {
                ctx.market(1, Side::Sell, ctx.position(1));
            }
        }
    }

    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(Scalper)) };
    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let stressed = rep.costs.stressed_sharpe;
    assert!(stressed <= 0.0, "stressed Sharpe {stressed} must be <= 0.0");
    assert_eq!(rep.costs.verdict, Verdict::Fail);
}

// -----------------------------------------------------------------------------
// Component 2: Execution delay
// -----------------------------------------------------------------------------

#[test]
fn execution_delay_passes_when_one_bar_late_sharpe_exceeds_half_baseline() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);
    let n = bars.len();
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> {
        Ok(Box::new(BuyAndHold { last_bar: n, fraction: 0.95 }))
    };
    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    let base = rep.baseline.sharpe;
    let late = rep.delay.one_bar_sharpe;
    assert!(base > 0.0, "baseline Sharpe must be positive, got {base}");
    assert!(late >= 0.5 * base, "one-bar-late Sharpe {late} must be >= 0.5 * baseline {base}");
    assert_eq!(rep.delay.verdict, Verdict::Pass);
}

#[test]
fn execution_delay_warns_when_one_bar_late_sharpe_falls_between_zero_and_half_baseline() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);
    let n = bars.len();
    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> {
        Ok(Box::new(BuyAndHold { last_bar: n, fraction: 0.95 }))
    };

    let base_rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();
    let ratio = base_rep.delay.one_bar_sharpe / base_rep.baseline.sharpe;

    let sc = SurvivalConfig { degradation_warn_ratio: (ratio + 0.005).min(1.0), ..SurvivalConfig::default() };
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &ParamSet::default(), &sc).unwrap();

    let base = rep.baseline.sharpe;
    let late = rep.delay.one_bar_sharpe;
    assert!(late > 0.0, "one-bar-late Sharpe must be strictly positive");
    assert!(
        late < sc.degradation_warn_ratio * base,
        "one-bar-late Sharpe must be below warn ratio * baseline"
    );
    assert_eq!(rep.delay.verdict, Verdict::Warn);
}

#[test]
fn execution_delay_fails_when_one_bar_late_sharpe_is_zero_or_negative() {
    // Closes path:
    // Bar 1: 100
    // Bar 2: 150 (Order placed on bar 1 executes at bar 2 with delay 0 -> buys at 150)
    // Bar 3: 200 (Order placed on bar 2 executes at bar 3 with delay 0 -> sells at 200 -> +33% profit!)
    // Bar 4: 50  (With delay 1: buy order executes at bar 3 (price 200), sell order executes at bar 4 (price 50) -> -75% loss!)
    let mut closes = vec![100.0; 60];
    closes[1] = 150.0;
    closes[2] = 200.0;
    closes[3] = 50.0;
    let bars = bars_from(&closes);

    struct QuickTrade;
    impl Strategy for QuickTrade {
        fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
            let n = ctx.history(bars[0].instrument).len();
            if n == 1 && ctx.position(1).is_zero() {
                ctx.market(1, Side::Buy, Qty::from_f64(100.0).unwrap());
            }
            if n == 2 && !ctx.position(1).is_zero() {
                ctx.market(1, Side::Sell, ctx.position(1));
            }
        }
    }

    let factory = move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(QuickTrade)) };
    let rep = run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap();

    assert!(rep.baseline.sharpe > 0.0, "baseline Sharpe must be positive");
    let late = rep.delay.one_bar_sharpe;
    assert!(late <= 0.0, "one-bar-late Sharpe {late} must be <= 0.0");
    assert_eq!(rep.delay.verdict, Verdict::Fail);
}

// -----------------------------------------------------------------------------
// Component 3: Parameter stability
// -----------------------------------------------------------------------------

struct ParameterControlledStrategy {
    gain_fraction: f64,
    exit_bar: usize,
}

impl Strategy for ParameterControlledStrategy {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == 1 && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = self.gain_fraction * ctx.equity().to_f64() / price;
            if qty > 0.0 {
                ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
            }
        }
        if n == self.exit_bar && !ctx.position(1).is_zero() {
            let pos = ctx.position(1);
            ctx.market(1, Side::Sell, pos);
        }
    }
}

#[test]
fn parameter_stability_insufficient_when_fewer_than_four_valid_neighbours() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);

    // Factory rejects parameters outside [95, 105], so perturbations (75, 90, 110, 125) are all rejected.
    let factory = move |params: &ParamSet| -> Result<Box<dyn Strategy>> {
        let val = params.get("param")?;
        if !(95.0..=105.0).contains(&val) {
            return Err(Error::InvalidArgument("invalid neighbour parameter".into()));
        }
        Ok(Box::new(ParameterControlledStrategy { gain_fraction: 0.95, exit_bar: 59 }))
    };

    let params = ParamSet::from_pairs([("param", 100.0)]);
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &params, &SurvivalConfig::default())
            .unwrap();

    assert!(rep.stability.neighbours.len() < 4, "fewer than 4 neighbours evaluated");
    assert_eq!(rep.stability.neighbours.len(), 0);
    assert_eq!(rep.stability.skipped, 4);
    assert_eq!(rep.stability.verdict, Verdict::Insufficient);
}

#[test]
fn parameter_stability_passes_when_share_positive_at_least_eighty_percent_and_median_healthy() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);

    let factory = move |params: &ParamSet| -> Result<Box<dyn Strategy>> {
        let _ = params.get("param")?;
        Ok(Box::new(ParameterControlledStrategy { gain_fraction: 0.95, exit_bar: 59 }))
    };

    let params = ParamSet::from_pairs([("param", 100.0)]);
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &params, &SurvivalConfig::default())
            .unwrap();

    assert_eq!(rep.stability.neighbours.len(), 4);
    assert_eq!(rep.stability.share_positive, 1.0);
    assert!(rep.stability.share_positive >= 0.80);
    assert!(rep.stability.median_sharpe >= 0.5 * rep.baseline.sharpe);
    assert_eq!(rep.stability.verdict, Verdict::Pass);
}

#[test]
fn parameter_stability_warns_when_share_positive_between_fifty_and_eighty_percent() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);

    // Candidates for 100.0 are 75, 90, 110, 125.
    // Baseline (100.0) trades normally with fraction 0.95.
    // If param == 75, gain_fraction is 0.0 (no trades, Sharpe == 0.0).
    // The other 3 (90, 110, 125) trade normally with fraction 0.95 (positive Sharpe).
    // positive = 3, share_positive = 3 / 4 = 0.75, which is in [0.50, 0.80).
    let factory = move |params: &ParamSet| -> Result<Box<dyn Strategy>> {
        let val = params.get("param")?;
        let frac = if (val - 75.0).abs() < 1e-3 { 0.0 } else { 0.95 };
        Ok(Box::new(ParameterControlledStrategy { gain_fraction: frac, exit_bar: 59 }))
    };

    let params = ParamSet::from_pairs([("param", 100.0)]);
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &params, &SurvivalConfig::default())
            .unwrap();

    assert_eq!(rep.stability.neighbours.len(), 4);
    assert!((rep.stability.share_positive - 0.75).abs() < 1e-9, "share positive must be 0.75");
    assert!(rep.stability.share_positive >= 0.50 && rep.stability.share_positive < 0.80);
    assert_eq!(rep.stability.verdict, Verdict::Warn);
}

#[test]
fn parameter_stability_warns_when_median_sharpe_falls_below_half_baseline() {
    // Realistic price series: choppy start, surge into bar 20, pullback into bar 25, followed by chop.
    #[rustfmt::skip]
    let closes = vec![
        100.0, 100.0, 103.27, 104.09, 104.27, 100.83, 100.33, 99.69, 99.54, 98.42,
        98.34, 97.5, 95.22, 96.75, 98.3, 101.37, 102.95, 105.68, 107.84, 110.38,
        112.69, 110.5, 107.94, 105.0, 102.73, 100.87, 100.36, 100.0, 100.31, 100.23,
        99.85, 99.73, 100.1, 101.1, 100.47, 100.16, 99.76, 98.56, 98.1, 97.6,
        98.15, 98.09, 97.29, 97.61, 97.43, 96.59, 96.3, 96.02, 95.6, 95.61,
        94.55, 94.42, 94.9, 95.3, 95.83, 96.37, 97.09, 96.55, 96.96, 96.06,
        95.77, 94.86, 95.36, 96.0, 95.9, 96.76, 96.43, 96.5, 96.58, 96.06,
        96.27, 97.21, 96.72, 97.15, 97.08, 97.94, 98.18, 98.51, 98.83, 98.95, 98.26,
    ];
    let bars = bars_from(&closes);

    // Strategy parameter directly controls the holding period (exit bar):
    // Baseline (holding = 20) exits right at the peak on bar 20.
    // Neighbours hold for 15, 18, 22, 25 bars:
    // - 15 exits before the peak (misses top gains)
    // - 18 exits near the peak
    // - 22 holds 2 bars into the pullback
    // - 25 holds 5 bars into the pullback
    // Each parameter value produces a distinct exit trade, different fill price, and different Sharpe.
    let factory = move |params: &ParamSet| -> Result<Box<dyn Strategy>> {
        let holding_bars = params.get("holding_bars")? as usize;
        Ok(Box::new(ParameterControlledStrategy { gain_fraction: 0.95, exit_bar: holding_bars }))
    };

    let params = ParamSet::from_pairs([("holding_bars", 20.0)]);
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &params, &SurvivalConfig::default())
            .unwrap();

    assert_eq!(rep.stability.neighbours.len(), 4);
    assert_eq!(rep.stability.share_positive, 1.0);
    assert!(
        rep.stability.median_sharpe < 0.5 * rep.baseline.sharpe,
        "median Sharpe {} must be < 0.5 * baseline {}",
        rep.stability.median_sharpe,
        rep.baseline.sharpe
    );
    assert!(rep.stability.median_sharpe > 0.0, "median Sharpe must be positive");
    assert_eq!(rep.stability.verdict, Verdict::Warn);
}

#[test]
fn parameter_stability_fails_when_share_positive_below_fifty_percent() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let bars = bars_from(&closes);

    // Candidates for 100.0 are 75, 90, 110, 125.
    // Baseline (100.0) trades normally with fraction 0.95 (so baseline traded == true).
    // Only neighbour 125 trades (positive). 75, 90, 110 do not trade (Sharpe 0.0).
    // positive = 1, share_positive = 1 / 4 = 0.25 < 0.50.
    let factory = move |params: &ParamSet| -> Result<Box<dyn Strategy>> {
        let val = params.get("param")?;
        let frac = if (val - 100.0).abs() < 1e-3 || (val - 125.0).abs() < 1e-3 { 0.95 } else { 0.0 };
        Ok(Box::new(ParameterControlledStrategy { gain_fraction: frac, exit_bar: 59 }))
    };

    let params = ParamSet::from_pairs([("param", 100.0)]);
    let rep =
        run_survival_lab(&bars, &factory, &BacktestConfig::default(), &params, &SurvivalConfig::default())
            .unwrap();

    assert_eq!(rep.stability.neighbours.len(), 4);
    assert!((rep.stability.share_positive - 0.25).abs() < 1e-9, "share positive must be 0.25");
    assert!(rep.stability.share_positive < 0.50);
    assert_eq!(rep.stability.verdict, Verdict::Fail);
}
