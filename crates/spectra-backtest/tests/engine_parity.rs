//! M15 gap 2: the engine's monthly calendar rebalance (`spectra_engine::Rebalancer` with
//! `RebalanceRule::Calendar`) must trade like the Task 3 simulator (`spectra_backtest::rebalance`)
//! on the same prices, up to the engine's rounding to whole shares.
//!
//! Scenario: two instruments, A from 100 stepping +2% and B from 50 stepping -1%, with prices
//! changing only on the 15th of each month. Weights 50/50, zero costs, 100,000 cash, gross cap 1.0,
//! lot size 1. The engine decides on a bar's close and fills at the next bar's open; prices do not
//! change between the 1st and the 2nd, so both paths trade at the same price.

use spectra_backtest::portfolio::Holding;
use spectra_backtest::rebalance::{simulate, Date, Frequency, RebalanceRule, TradeCostConfig};
use spectra_core::{Bar, InstrumentId, Money, Price, Qty, Side, SCALE};
use spectra_engine::{
    bonds::days_from_civil, run_backtest, BacktestConfig, CalendarFrequency, Context, RebalanceConfig,
    RebalanceRule as EngineRule, Rebalancer, WeightStrategy,
};

const A: InstrumentId = 1;
const B: InstrumentId = 2;
const DAY_NS: i64 = 86_400_000_000_000;

/// Daily dates from 2026-01-01 to 2026-06-30.
fn dates() -> Vec<Date> {
    let first = Date::new(2026, 1, 1).to_days();
    let last = Date::new(2026, 6, 30).to_days();
    (first..=last).map(|d| Date::from_days(d).unwrap()).collect()
}

/// Closes of A and B on each date: one step on every 15th, none in between.
fn closes(dates: &[Date]) -> Vec<Vec<f64>> {
    let (mut a, mut b) = (100.0_f64, 50.0_f64);
    dates
        .iter()
        .map(|d| {
            if d.day == 15 {
                a *= 1.02;
                b *= 0.99;
            }
            vec![a, b]
        })
        .collect()
}

/// The same prices as engine bars (open = close), at midnight UTC of each date.
fn bars(dates: &[Date], closes: &[Vec<f64>]) -> Vec<Bar> {
    let mut out = Vec::new();
    for (d, row) in dates.iter().zip(closes) {
        let ts = days_from_civil(i64::from(d.year), d.month, d.day) * DAY_NS;
        for (id, px) in [(A, row[0]), (B, row[1])] {
            let p = Price::from_f64(px).unwrap();
            out.push(Bar {
                ts_ns: ts,
                known_at_ns: ts,
                instrument: id,
                open: p,
                high: p,
                low: p,
                close: p,
                volume: Qty::from_raw(10_000_000 * SCALE),
            });
        }
    }
    out
}

struct Fixed;
impl WeightStrategy for Fixed {
    fn weights(&mut self, _ctx: &Context<'_>, _bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
        Some(vec![(A, 0.5), (B, 0.5)])
    }
}

#[test]
fn monthly_calendar_rebalance_matches_the_simulator() {
    let dates = dates();
    let closes = closes(&dates);

    let sim = simulate(
        &["A".to_string(), "B".to_string()],
        &[Holding { symbol: "A".into(), weight: 0.5 }, Holding { symbol: "B".into(), weight: 0.5 }],
        &dates,
        &closes,
        100_000.0,
        RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None },
        &TradeCostConfig { name: "zero".into(), version: 1, slippage_bps: 0.0, min_fee: 0.0 },
    )
    .unwrap();

    let cfg = BacktestConfig { initial_cash: Money::from_raw(100_000 * SCALE), ..BacktestConfig::default() };
    let rc = RebalanceConfig {
        rule: EngineRule::Calendar(CalendarFrequency::Monthly),
        gross_cap: 1.0,
        lot_size: Qty::ONE,
        min_trade_notional: 0.0,
    };
    let eng = run_backtest(&bars(&dates, &closes), &mut Rebalancer::new(Fixed, rc), &cfg).unwrap();
    eng.ledger.check_invariants().unwrap();

    // One rebalance per month in both paths, each trading both instruments.
    let mut steps: Vec<i64> = eng.orders.iter().map(|o| o.submit_step).collect();
    steps.dedup();
    assert_eq!(steps.len(), sim.events.len(), "rebalance count: engine {steps:?}");
    assert_eq!(sim.events.len(), 6);

    for (k, ev) in sim.events.iter().enumerate() {
        let step = steps[k];
        for (id, sym) in [(A, "A"), (B, "B")] {
            let want = ev.trades.iter().find(|t| t.symbol == sym).map_or(0.0, |t| t.qty);
            let got: f64 = eng
                .orders
                .iter()
                .filter(|o| o.submit_step == step && o.instrument == id)
                .map(|o| if o.side == Side::Buy { o.qty.to_f64() } else { -o.qty.to_f64() })
                .sum();
            assert!(
                (got - want).abs() < 1.0,
                "{}: {sym} engine traded {got}, simulator {want:.4} (under one share apart expected)",
                ev.date
            );
        }
    }

    let last = closes.last().unwrap();
    let sim_final = sim.equity.last().unwrap().1;
    let eng_final = eng.equity_curve.last().unwrap().equity.to_f64();
    assert!(
        (eng_final - sim_final).abs() < last[0] + last[1],
        "final equity: engine {eng_final:.2}, simulator {sim_final:.2}"
    );
}
