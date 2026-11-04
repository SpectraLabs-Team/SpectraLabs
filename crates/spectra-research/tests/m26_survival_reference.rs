//! M26 task 3e (part 5b): known-answer tests for the Strategy Survival Lab's
//! drawdown and tail-risk component (component 9, `src/survival.rs`).
//!
//! The component's rule (from the code): the verdict is Fail when the max
//! drawdown is at least `max_drawdown_fail` (0.40 by default), Warn when at
//! least `max_drawdown_warn` (0.20), and Pass otherwise. The test strategy
//! buys 95% of equity on the first bar and sells on the last, so the equity
//! drawdown is 95% of the price drawdown. Prices are fixed, so the expected
//! drawdowns are closed forms: a path that falls 30% from its peak gives about
//! 0.285, and one that falls 50% gives about 0.475.
//!
//! Not covered here: components 1-8. Each is a separate case, and they go in
//! later commits of this part.

use spectra_core::*;
use spectra_engine::*;
use spectra_research::*;

/// Bars from a fixed close path. No randomness, so the drawdown is exact.
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
            volume: Qty::from_f64(1e6).unwrap(),
        })
        .collect()
}

/// Buys 95% of equity on the first bar and sells the whole position on the last.
struct BuyAndSell {
    last_bar: usize,
}

impl Strategy for BuyAndSell {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        let n = ctx.history(bars[0].instrument).len();
        if n == 1 && ctx.position(1).is_zero() {
            let price = bars[0].close.to_f64();
            let qty = 0.95 * ctx.equity().to_f64() / price;
            ctx.market(1, Side::Buy, Qty::from_f64(qty).unwrap());
        }
        if n == self.last_bar && !ctx.position(1).is_zero() {
            let pos = ctx.position(1);
            ctx.market(1, Side::Sell, pos);
        }
    }
}

fn run(closes: &[f64]) -> SurvivalReport {
    let n = closes.len();
    let bars = bars_from(closes);
    let factory =
        move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndSell { last_bar: n })) };
    run_survival_lab(
        &bars,
        &factory,
        &BacktestConfig::default(),
        &ParamSet::default(),
        &SurvivalConfig::default(),
    )
    .unwrap()
}

/// 20 bars at 100, then a fall to 70 over 20 bars, then a rise to 80 over 20 bars.
/// The peak-to-trough fall is 30%.
fn thirty_percent_fall() -> Vec<f64> {
    (0..60)
        .map(|i| {
            if i < 20 {
                100.0
            } else if i < 40 {
                100.0 - 30.0 * f64::from(i - 19) / 20.0
            } else {
                70.0 + 0.5 * f64::from(i - 39)
            }
        })
        .collect()
}

/// 20 bars at 100, then a fall to 50 over 20 bars, then a rise to 60 over 20 bars.
/// The peak-to-trough fall is 50%.
fn fifty_percent_fall() -> Vec<f64> {
    (0..60)
        .map(|i| {
            if i < 20 {
                100.0
            } else if i < 40 {
                100.0 - 50.0 * f64::from(i - 19) / 20.0
            } else {
                50.0 + 0.5 * f64::from(i - 39)
            }
        })
        .collect()
}

#[test]
fn a_steadily_rising_path_has_no_drawdown_and_passes() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 * 1.01f64.powi(i)).collect();
    let r = run(&closes);
    assert!(r.risk.drawdown.valid);
    assert!(r.risk.drawdown.max_drawdown < 0.01, "drawdown {}", r.risk.drawdown.max_drawdown);
    assert_eq!(r.risk.verdict, Verdict::Pass);
}

#[test]
fn a_thirty_percent_fall_is_a_warning_not_a_failure() {
    // 95% of a 30% fall is about 0.285: above the 0.20 warning, below the 0.40 failure.
    let r = run(&thirty_percent_fall());
    assert!(r.risk.drawdown.valid);
    let dd = r.risk.drawdown.max_drawdown;
    assert!((0.27..=0.30).contains(&dd), "drawdown {dd}");
    assert_eq!(r.risk.verdict, Verdict::Warn);
}

#[test]
fn a_fifty_percent_fall_is_a_failure() {
    // 95% of a 50% fall is about 0.475: above the 0.40 failure threshold.
    let r = run(&fifty_percent_fall());
    assert!(r.risk.drawdown.valid);
    let dd = r.risk.drawdown.max_drawdown;
    assert!((0.45..=0.50).contains(&dd), "drawdown {dd}");
    assert_eq!(r.risk.verdict, Verdict::Fail);
}

#[test]
fn non_positive_prior_equity_yields_insufficient_for_risk_tail() {
    let closes: Vec<f64> = (0..60).map(|i| 100.0 + f64::from(i)).collect();
    let n = closes.len();
    let bars = bars_from(&closes);
    let factory =
        move |_: &ParamSet| -> Result<Box<dyn Strategy>> { Ok(Box::new(BuyAndSell { last_bar: n })) };
    let backtest_cfg = BacktestConfig { initial_cash: Money::ZERO, ..BacktestConfig::default() };
    let r =
        run_survival_lab(&bars, &factory, &backtest_cfg, &ParamSet::default(), &SurvivalConfig::default())
            .unwrap();
    assert_eq!(r.risk.verdict, Verdict::Insufficient);
    assert_eq!(
        r.risk.finding,
        "Tail statistics are undefined because the equity curve has non-positive prior equity."
    );
    assert_eq!(r.risk.tail, TailStats::default());
}
