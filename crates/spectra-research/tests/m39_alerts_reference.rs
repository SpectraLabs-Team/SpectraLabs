//! M39 Market & Formula Alerts Reference Tests.

use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_research::alerts::*;

fn test_bar(ts: i64, c: f64) -> Bar {
    Bar::new(
        ts,
        0,
        Price::from_f64(c).unwrap(),
        Price::from_f64(c + 1.0).unwrap(),
        Price::from_f64(c - 1.0).unwrap(),
        Price::from_f64(c).unwrap(),
        Qty::from_f64(100.0).unwrap(),
    )
}

#[test]
fn test_m39_once_only_across_three_nightly_runs() {
    let mut mgr = AlertManager::new();
    mgr.add_alert(AlertDefinition {
        id: "alert_eod".into(),
        name: "EOD Price Above 100".into(),
        condition: AlertCondition::PriceAbove { symbol: "NVDA".into(), target_price: 100.0 },
        frequency: TriggerFrequency::OnceOnly,
        created_ts_ns: 0,
        active: true,
        last_triggered_ts_ns: None,
    });

    let bars = vec![test_bar(1000, 105.0)];
    let market = [("NVDA", bars.as_slice())];

    // Nightly run 1: fires
    let r1 = mgr.evaluate_market(1000, &market);
    assert_eq!(r1.len(), 1);
    assert_eq!(r1[0].trigger_value, 105.0);

    // Nightly run 2 (next day): must NOT fire
    let r2 = mgr.evaluate_market(2000, &market);
    assert_eq!(r2.len(), 0);

    // Nightly run 3 (third day): must NOT fire
    let r3 = mgr.evaluate_market(3000, &market);
    assert_eq!(r3.len(), 0);

    // History contains exactly one logged trigger event
    assert_eq!(mgr.trigger_history().len(), 1);
}

#[test]
fn test_m39_screen_entry_alert() {
    let mut mgr = AlertManager::new();
    mgr.add_alert(AlertDefinition {
        id: "screen_alert".into(),
        name: "AAPL Enters Momentum Screen".into(),
        condition: AlertCondition::ScreenEntry { screen_id: "scr_mom_2026".into(), symbol: "AAPL".into() },
        frequency: TriggerFrequency::OnceOnly,
        created_ts_ns: 0,
        active: true,
        last_triggered_ts_ns: None,
    });

    // Run without AAPL: no alert
    let ev1 = mgr.evaluate_screen_entries("scr_mom_2026", &["MSFT".into(), "GOOGL".into()], 1000);
    assert_eq!(ev1.len(), 0);

    // Run with AAPL matching: fires alert!
    let ev2 = mgr.evaluate_screen_entries("scr_mom_2026", &["AAPL".into(), "MSFT".into()], 2000);
    assert_eq!(ev2.len(), 1);
    assert_eq!(ev2[0].symbol, "AAPL");
}
