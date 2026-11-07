//! Market & Formula Alerts Engine (M39).
//!
//! Evaluates price thresholds, percentage changes, custom formula conditions (M38),
//! screen entries (M36), and risk threshold breaches. Provides deduplication,
//! once-only vs repeating policies, rate-limiting, and trigger history audit logging.

use crate::formula::evaluate_formula;
use spectra_core::Bar;
use std::collections::HashMap;

/// Type of alert trigger condition.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertCondition {
    /// Fires when latest close >= target_price.
    PriceAbove { symbol: String, target_price: f64 },
    /// Fires when latest close <= target_price.
    PriceBelow { symbol: String, target_price: f64 },
    /// Fires when percentage move from baseline >= pct_threshold (e.g. 0.05 for +5%).
    PercentMove { symbol: String, baseline_price: f64, pct_threshold: f64 },
    /// Fires when custom formula evaluates to > 0.0 at the latest bar.
    Formula { symbol: String, formula: String },
    /// Fires when a symbol enters a saved screen list.
    ScreenEntry { screen_id: String, symbol: String },
}

/// Alert trigger frequency policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerFrequency {
    /// Fires exactly once, then immediately transitions to inactive/disabled.
    OnceOnly,
    /// Can fire repeatedly, subject to a minimum interval between triggers.
    Repeating { min_interval_ns: i64 },
}

/// Alert definition.
#[derive(Debug, Clone, PartialEq)]
pub struct AlertDefinition {
    pub id: String,
    pub name: String,
    pub condition: AlertCondition,
    pub frequency: TriggerFrequency,
    pub created_ts_ns: i64,
    pub active: bool,
    pub last_triggered_ts_ns: Option<i64>,
}

/// An audit log event recorded when an alert triggers.
#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub alert_id: String,
    pub alert_name: String,
    pub symbol: String,
    pub triggered_ts_ns: i64,
    pub trigger_value: f64,
    pub message: String,
}

/// Alert engine evaluating conditions and tracking trigger history.
#[derive(Debug, Clone, Default)]
pub struct AlertManager {
    alerts: HashMap<String, AlertDefinition>,
    history: Vec<AlertEvent>,
}

impl AlertManager {
    pub fn new() -> Self {
        Self { alerts: HashMap::new(), history: Vec::new() }
    }

    /// Register a new alert definition.
    pub fn add_alert(&mut self, alert: AlertDefinition) {
        self.alerts.insert(alert.id.clone(), alert);
    }

    /// Retrieve an alert by ID.
    pub fn get_alert(&self, id: &str) -> Option<&AlertDefinition> {
        self.alerts.get(id)
    }

    /// All logged trigger events.
    pub fn trigger_history(&self) -> &[AlertEvent] {
        &self.history
    }

    /// Evaluates all active alerts against the latest market bar data.
    pub fn evaluate_market(&mut self, current_ts_ns: i64, market_data: &[(&str, &[Bar])]) -> Vec<AlertEvent> {
        let mut triggered_events = Vec::new();
        let bar_map: HashMap<&str, &[Bar]> = market_data.iter().copied().collect();

        for alert in self.alerts.values_mut() {
            if !alert.active {
                continue;
            }

            // Check rate limiting if repeating
            if let (Some(last_ts), TriggerFrequency::Repeating { min_interval_ns }) =
                (alert.last_triggered_ts_ns, alert.frequency)
            {
                if current_ts_ns - last_ts < min_interval_ns {
                    continue; // Suppressed by deduplication / rate-limiting
                }
            }

            let mut fired = false;
            let mut trigger_val = 0.0;
            let mut msg = String::new();
            let mut target_symbol = String::new();

            match &alert.condition {
                AlertCondition::PriceAbove { symbol, target_price } => {
                    target_symbol = symbol.clone();
                    if let Some(bars) = bar_map.get(symbol.as_str()) {
                        if let Some(last) = bars.last() {
                            let close = last.close.to_f64();
                            if close >= *target_price {
                                fired = true;
                                trigger_val = close;
                                msg = format!("Price {} crossed above target {}", close, target_price);
                            }
                        }
                    }
                }
                AlertCondition::PriceBelow { symbol, target_price } => {
                    target_symbol = symbol.clone();
                    if let Some(bars) = bar_map.get(symbol.as_str()) {
                        if let Some(last) = bars.last() {
                            let close = last.close.to_f64();
                            if close <= *target_price {
                                fired = true;
                                trigger_val = close;
                                msg = format!("Price {} crossed below target {}", close, target_price);
                            }
                        }
                    }
                }
                AlertCondition::PercentMove { symbol, baseline_price, pct_threshold } => {
                    target_symbol = symbol.clone();
                    if let Some(bars) = bar_map.get(symbol.as_str()) {
                        if let Some(last) = bars.last() {
                            let close = last.close.to_f64();
                            let pct_change = (close - baseline_price) / baseline_price;
                            if pct_change.abs() >= *pct_threshold {
                                fired = true;
                                trigger_val = pct_change;
                                msg = format!(
                                    "Price moved by {:.2}% from baseline {} (current: {})",
                                    pct_change * 100.0,
                                    baseline_price,
                                    close
                                );
                            }
                        }
                    }
                }
                AlertCondition::Formula { symbol, formula } => {
                    target_symbol = symbol.clone();
                    if let Some(bars) = bar_map.get(symbol.as_str()) {
                        if let Ok(results) = evaluate_formula(formula, bars, 10_000) {
                            if let Some(Some(val)) = results.last() {
                                if *val > 0.0 {
                                    fired = true;
                                    trigger_val = *val;
                                    msg = format!("Formula '{}' satisfied with value {}", formula, val);
                                }
                            }
                        }
                    }
                }
                AlertCondition::ScreenEntry { .. } => {
                    // Handled in evaluate_screen_entries
                }
            }

            if fired {
                let evt = AlertEvent {
                    alert_id: alert.id.clone(),
                    alert_name: alert.name.clone(),
                    symbol: target_symbol,
                    triggered_ts_ns: current_ts_ns,
                    trigger_value: trigger_val,
                    message: msg,
                };
                alert.last_triggered_ts_ns = Some(current_ts_ns);
                if alert.frequency == TriggerFrequency::OnceOnly {
                    alert.active = false; // Disarm once-only alert
                }
                triggered_events.push(evt);
            }
        }

        self.history.extend(triggered_events.clone());
        triggered_events
    }

    /// Evaluates screen entry alerts when a new screener run completes.
    pub fn evaluate_screen_entries(
        &mut self,
        screen_id: &str,
        matching_symbols: &[String],
        current_ts_ns: i64,
    ) -> Vec<AlertEvent> {
        let mut triggered_events = Vec::new();

        for alert in self.alerts.values_mut() {
            if !alert.active {
                continue;
            }

            if let AlertCondition::ScreenEntry { screen_id: sid, symbol } = &alert.condition {
                if sid == screen_id && matching_symbols.contains(symbol) {
                    let evt = AlertEvent {
                        alert_id: alert.id.clone(),
                        alert_name: alert.name.clone(),
                        symbol: symbol.clone(),
                        triggered_ts_ns: current_ts_ns,
                        trigger_value: 1.0,
                        message: format!("Symbol {} matched screen {}", symbol, screen_id),
                    };
                    alert.last_triggered_ts_ns = Some(current_ts_ns);
                    if alert.frequency == TriggerFrequency::OnceOnly {
                        alert.active = false;
                    }
                    triggered_events.push(evt);
                }
            }
        }

        self.history.extend(triggered_events.clone());
        triggered_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::{Price, Qty};

    fn make_bar(ts: i64, c: f64) -> Bar {
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
    fn test_once_only_price_alert() {
        let mut mgr = AlertManager::new();
        mgr.add_alert(AlertDefinition {
            id: "alt_1".into(),
            name: "AAPL Above 150".into(),
            condition: AlertCondition::PriceAbove { symbol: "AAPL".into(), target_price: 150.0 },
            frequency: TriggerFrequency::OnceOnly,
            created_ts_ns: 1000,
            active: true,
            last_triggered_ts_ns: None,
        });

        let bars = vec![make_bar(2000, 155.0)];
        let market = [("AAPL", bars.as_slice())];

        // 1. Run 1: fires
        let events1 = mgr.evaluate_market(2000, &market);
        assert_eq!(events1.len(), 1);
        assert_eq!(events1[0].trigger_value, 155.0);

        // 2. Alert should now be inactive
        assert!(!mgr.get_alert("alt_1").unwrap().active);

        // 3. Run 2: does NOT fire again across subsequent runs
        let events2 = mgr.evaluate_market(3000, &market);
        assert_eq!(events2.len(), 0);
    }

    #[test]
    fn test_repeating_rate_limit_and_deduplication() {
        let mut mgr = AlertManager::new();
        let one_day_ns = 86_400_000_000_000i64;

        mgr.add_alert(AlertDefinition {
            id: "alt_repeat".into(),
            name: "AAPL Above 100 Repeating".into(),
            condition: AlertCondition::PriceAbove { symbol: "AAPL".into(), target_price: 100.0 },
            frequency: TriggerFrequency::Repeating { min_interval_ns: one_day_ns },
            created_ts_ns: 0,
            active: true,
            last_triggered_ts_ns: None,
        });

        let bars = vec![make_bar(1000, 110.0)];
        let market = [("AAPL", bars.as_slice())];

        // Day 1: fires
        let ev1 = mgr.evaluate_market(1000, &market);
        assert_eq!(ev1.len(), 1);

        // 1 hour later: suppressed by deduplication rate limit
        let ev2 = mgr.evaluate_market(1000 + 3_600_000_000_000, &market);
        assert_eq!(ev2.len(), 0);

        // Day 2 (after one_day_ns): fires again
        let ev3 = mgr.evaluate_market(1000 + one_day_ns + 10, &market);
        assert_eq!(ev3.len(), 1);
    }

    #[test]
    fn test_formula_alert_evaluation() {
        let mut mgr = AlertManager::new();
        mgr.add_alert(AlertDefinition {
            id: "alt_formula".into(),
            name: "Breakout Formula".into(),
            condition: AlertCondition::Formula { symbol: "MSFT".into(), formula: "Close > 200".into() },
            frequency: TriggerFrequency::OnceOnly,
            created_ts_ns: 0,
            active: true,
            last_triggered_ts_ns: None,
        });

        let bars = vec![make_bar(1000, 210.0)];
        let market = [("MSFT", bars.as_slice())];

        let evs = mgr.evaluate_market(1000, &market);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].symbol, "MSFT");
    }
}
