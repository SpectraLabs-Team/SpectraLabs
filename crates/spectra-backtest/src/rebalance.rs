//! Rebalancing (M15 Task 3): hold a portfolio at target weights, rebalancing on a calendar and/or
//! when a holding drifts from its target. Every rebalance trade is costed through a named cost
//! config (the M26 config: slippage and a minimum fee) and recorded.
//!
//! The simulator is pure: it takes closing prices and dates and returns the events and the equity
//! curve. Decisions are leak-free: the choice to rebalance on date `t` reads only prices and dates
//! up to and including `t`, and trades execute at the close of `t`. A test checks this by
//! truncating the data at every date and comparing the events.
//!
//! Sizing: each rebalance sizes its trades so that, after paying its own costs, every holding is
//! exactly at its target weight at the close. Weights after a rebalance therefore match the targets
//! to floating-point precision, and the cash left over is zero.

use std::fmt;

use crate::portfolio::{validate_weights, Holding, PortfolioError, DEFAULT_WEIGHT_TOLERANCE};

/// A civil (calendar) date. Ordering is by year, then month, then day. Std-only: this crate must
/// not depend on third-party crates, so dates are represented here rather than with `chrono`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31, valid for the month. The caller supplies a real calendar date.
    pub day: u32,
}

impl Date {
    /// A date from its parts. The caller supplies a valid calendar date.
    #[must_use]
    pub const fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Days since 1970-01-01 (proleptic Gregorian calendar; Howard Hinnant's algorithm).
    #[must_use]
    pub fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01, or `None` if its year does not fit in an `i32`.
    #[must_use]
    pub fn from_days(days: i64) -> Option<Self> {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Some(Self {
            year: i32::try_from(year).ok()?,
            month: u32::try_from(month).ok()?,
            day: u32::try_from(day).ok()?,
        })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Calendar frequency for rebalancing: on the first trading day of each new month or quarter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Monthly,
    Quarterly,
}

/// When to rebalance. At least one of the two must be set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RebalanceRule {
    /// Rebalance on each new month or quarter.
    pub calendar: Option<Frequency>,
    /// Rebalance when any holding's weight is more than this far from its target (a fraction,
    /// e.g. 0.05 for 5 percentage points). Must be in (0, 1].
    pub drift_threshold: Option<f64>,
}

/// The cost config a rebalance trade is priced with: the M26 fields, mirrored here so this crate
/// does not depend on the API crate. A fill records the name and version it was priced with.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeCostConfig {
    pub name: String,
    pub version: u32,
    /// Adverse price move on every trade, in basis points.
    pub slippage_bps: f64,
    /// Minimum commission on each trade that moves a position.
    pub min_fee: f64,
}

/// Why a rebalance could not be simulated.
#[derive(Debug, Clone, PartialEq)]
pub enum RebalanceError {
    /// The weights are not a valid portfolio.
    Weights(PortfolioError),
    /// The rule sets neither a calendar nor a drift threshold, or the threshold is out of range.
    Rule(String),
    /// The inputs disagree in shape, or the prices, dates or cash are invalid.
    Input(String),
}

impl fmt::Display for RebalanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Weights(e) => write!(f, "{e}"),
            Self::Rule(m) | Self::Input(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for RebalanceError {}

/// Why a rebalance happened on a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The first bar: the portfolio is set up at target weights.
    Initial,
    /// A new month or quarter, per the calendar rule.
    Calendar,
    /// A holding drifted past the threshold.
    Drift,
}

/// One trade in a rebalance.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceTrade {
    pub symbol: String,
    /// Signed quantity: positive buys, negative sells.
    pub qty: f64,
    /// The close the trade is priced from, before slippage.
    pub price: f64,
    /// Price after slippage: above the close for buys, below for sells.
    pub exec_price: f64,
    /// Slippage cost in currency: `|qty| * price * slippage`.
    pub slippage_cost: f64,
    /// Commission in currency.
    pub fee: f64,
}

impl RebalanceTrade {
    /// Total cost of this trade in currency: slippage plus commission.
    #[must_use]
    pub fn cost(&self) -> f64 {
        self.slippage_cost + self.fee
    }
}

/// One rebalance: its date, reasons, trades, cost and turnover.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceEvent {
    pub date: Date,
    pub reasons: Vec<Reason>,
    pub trades: Vec<RebalanceTrade>,
    /// Sum of the trades' costs in currency.
    pub cost: f64,
    /// Traded notional at the close, as a fraction of equity before the rebalance.
    pub turnover: f64,
    /// The cost config every trade was priced with.
    pub cost_config: TradeCostConfig,
}

/// The result of a rebalancing simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceRun {
    pub events: Vec<RebalanceEvent>,
    /// Equity at each close, from the first date.
    pub equity: Vec<(Date, f64)>,
    /// Costs of all rebalances in currency.
    pub total_cost: f64,
    /// Sum of the events' turnover.
    pub total_turnover: f64,
}

/// Simulate a portfolio held at `targets`, rebalanced by `rule`.
///
/// `symbols[i]` pairs with `targets[i]` and column `i` of `closes`. `dates` must be strictly
/// increasing, with one row of `closes` per date.
///
/// # Errors
/// - [`RebalanceError::Weights`] if the targets are not a valid portfolio.
/// - [`RebalanceError::Rule`] if the rule has neither a calendar nor a drift threshold, or the
///   threshold is outside (0, 1].
/// - [`RebalanceError::Input`] if the shapes disagree, dates are not increasing, a price is not
///   finite and positive, the cash is not positive, or the cost config is invalid.
pub fn simulate(
    symbols: &[String],
    targets: &[Holding],
    dates: &[Date],
    closes: &[Vec<f64>],
    initial_cash: f64,
    rule: RebalanceRule,
    cost: &TradeCostConfig,
) -> Result<RebalanceRun, RebalanceError> {
    validate_weights(targets, DEFAULT_WEIGHT_TOLERANCE).map_err(RebalanceError::Weights)?;
    if rule.calendar.is_none() && rule.drift_threshold.is_none() {
        return Err(RebalanceError::Rule(
            "a rebalance rule needs a calendar, a drift threshold, or both".into(),
        ));
    }
    if let Some(t) = rule.drift_threshold {
        if !(t.is_finite() && t > 0.0 && t <= 1.0) {
            return Err(RebalanceError::Rule(format!("drift threshold must be in (0, 1], got {t}")));
        }
    }
    if !(initial_cash.is_finite() && initial_cash > 0.0) {
        return Err(RebalanceError::Input(format!("initial cash must be positive, got {initial_cash}")));
    }
    if !(cost.slippage_bps.is_finite()
        && cost.slippage_bps >= 0.0
        && cost.min_fee.is_finite()
        && cost.min_fee >= 0.0)
    {
        return Err(RebalanceError::Input(
            "cost config must have finite, non-negative slippage and fee".into(),
        ));
    }
    let n = symbols.len();
    if n == 0 || targets.len() != n {
        return Err(RebalanceError::Input(format!("{} symbols but {} targets", n, targets.len())));
    }
    if dates.is_empty() || closes.len() != dates.len() {
        return Err(RebalanceError::Input(format!("{} dates but {} price rows", dates.len(), closes.len())));
    }
    if dates.windows(2).any(|w| w[1] <= w[0]) {
        return Err(RebalanceError::Input("dates must be strictly increasing".into()));
    }
    for (t, row) in closes.iter().enumerate() {
        if row.len() != n {
            return Err(RebalanceError::Input(format!(
                "price row {t} has {} values for {n} symbols",
                row.len()
            )));
        }
        if let Some(bad) = row.iter().find(|p| !(p.is_finite() && **p > 0.0)) {
            return Err(RebalanceError::Input(format!(
                "price {bad} on {} is not finite and positive",
                dates[t]
            )));
        }
    }

    let mut weights = Vec::with_capacity(n);
    for symbol in symbols {
        let Some(target) = targets.iter().find(|h| h.symbol == *symbol) else {
            return Err(RebalanceError::Input(format!("no target weight for {symbol}")));
        };
        weights.push(target.weight);
    }
    let slip = cost.slippage_bps / 10_000.0;
    let mut cash = initial_cash;
    let mut held = vec![0.0_f64; n];
    let mut events = Vec::new();
    let mut equity_curve = Vec::with_capacity(dates.len());
    let mut total_cost = 0.0;
    let mut total_turnover = 0.0;

    for t in 0..dates.len() {
        let px = &closes[t];
        let equity_before = cash + held.iter().zip(px).map(|(q, p)| q * p).sum::<f64>();

        let mut reasons = Vec::new();
        if t == 0 {
            reasons.push(Reason::Initial);
        } else {
            if let Some(freq) = rule.calendar {
                if period(dates[t], freq) != period(dates[t - 1], freq) {
                    reasons.push(Reason::Calendar);
                }
            }
            if let Some(threshold) = rule.drift_threshold {
                let drifted = (0..n).any(|i| {
                    let w = held[i] * px[i] / equity_before;
                    (w - weights[i]).abs() > threshold
                });
                if drifted {
                    reasons.push(Reason::Drift);
                }
            }
        }

        if !reasons.is_empty() {
            let deltas = size_trades(equity_before, &weights, &held, px, slip, cost.min_fee);
            let mut trades = Vec::new();
            let mut event_cost = 0.0;
            let mut traded_notional = 0.0;
            for i in 0..n {
                let d = deltas[i];
                if d.abs() <= 1e-12 {
                    continue;
                }
                let exec = if d > 0.0 { px[i] * (1.0 + slip) } else { px[i] * (1.0 - slip) };
                let slippage_cost = d.abs() * px[i] * slip;
                let fee = cost.min_fee;
                cash -= d * px[i] + slippage_cost + fee;
                held[i] += d;
                event_cost += slippage_cost + fee;
                traded_notional += d.abs() * px[i];
                trades.push(RebalanceTrade {
                    symbol: symbols[i].clone(),
                    qty: d,
                    price: px[i],
                    exec_price: exec,
                    slippage_cost,
                    fee,
                });
            }
            let turnover = traded_notional / equity_before;
            total_cost += event_cost;
            total_turnover += turnover;
            events.push(RebalanceEvent {
                date: dates[t],
                reasons,
                trades,
                cost: event_cost,
                turnover,
                cost_config: cost.clone(),
            });
        }

        let equity = cash + held.iter().zip(px).map(|(q, p)| q * p).sum::<f64>();
        equity_curve.push((dates[t], equity));
    }

    Ok(RebalanceRun { events, equity: equity_curve, total_cost, total_turnover })
}

/// The calendar period a date falls in, so a change of period marks a rebalance date.
fn period(date: Date, freq: Frequency) -> (i32, u32) {
    match freq {
        Frequency::Monthly => (date.year, date.month),
        Frequency::Quarterly => (date.year, (date.month - 1) / 3),
    }
}

/// Quantity changes that move each holding to its target weight of the equity left after paying
/// the rebalance's own costs. Fixed-point iteration: costs depend on trade sizes, which depend on
/// costs, and the loop converges quickly because costs are small relative to equity.
fn size_trades(equity: f64, weights: &[f64], held: &[f64], px: &[f64], slip: f64, min_fee: f64) -> Vec<f64> {
    let n = weights.len();
    let mut costs = 0.0;
    let mut deltas = vec![0.0; n];
    for _ in 0..100 {
        let net = equity - costs;
        let mut next_costs = 0.0;
        for i in 0..n {
            let target_value = weights[i] * net;
            let current_value = held[i] * px[i];
            deltas[i] = (target_value - current_value) / px[i];
            if deltas[i].abs() > 1e-12 {
                next_costs += deltas[i].abs() * px[i] * slip + min_fee;
            }
        }
        let settled = (next_costs - costs).abs() < 1e-12;
        costs = next_costs;
        if settled {
            break;
        }
    }
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::new(y, m, day)
    }

    fn syms(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn targets(pairs: &[(&str, f64)]) -> Vec<Holding> {
        pairs.iter().map(|(s, w)| Holding { symbol: (*s).to_string(), weight: *w }).collect()
    }

    fn zero_cost() -> TradeCostConfig {
        TradeCostConfig { name: "test-zero".into(), version: 1, slippage_bps: 0.0, min_fee: 0.0 }
    }

    fn paper_like() -> TradeCostConfig {
        TradeCostConfig { name: "paper-default".into(), version: 1, slippage_bps: 5.0, min_fee: 1.0 }
    }

    fn monthly() -> RebalanceRule {
        RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None }
    }

    fn weight_of(symbols: &[String], held: &[(String, f64)], px: &[f64], equity: f64, sym: &str) -> f64 {
        let i = symbols.iter().position(|s| s == sym).unwrap();
        let qty = held.iter().find(|(s, _)| s == sym).map_or(0.0, |(_, q)| *q);
        qty * px[i] / equity
    }

    #[test]
    fn hand_calculation_equal_weight_with_zero_costs() {
        // Day 1: 1000 split 50/50 at price 100 each -> 5 units each.
        // Day 2 (new month): A rises to 200, B stays at 100. Holdings: A 1000, B 500, equity 1500.
        // Target 750 each: sell A 250 (1.25 units), buy B 250 (2.5 units).
        let s = syms(&["A", "B"]);
        let dates = [d(2026, 1, 30), d(2026, 2, 2)];
        let closes = vec![vec![100.0, 100.0], vec![200.0, 100.0]];
        let run = simulate(
            &s,
            &targets(&[("A", 0.5), ("B", 0.5)]),
            &dates,
            &closes,
            1000.0,
            monthly(),
            &zero_cost(),
        )
        .unwrap();
        assert_eq!(run.events.len(), 2);
        let second = &run.events[1];
        assert_eq!(second.reasons, vec![Reason::Calendar]);
        let a = second.trades.iter().find(|t| t.symbol == "A").unwrap();
        let b = second.trades.iter().find(|t| t.symbol == "B").unwrap();
        assert!((a.qty + 1.25).abs() < 1e-9, "A trade {}", a.qty);
        assert!((b.qty - 2.5).abs() < 1e-9, "B trade {}", b.qty);
        assert!((run.equity[1].1 - 1500.0).abs() < 1e-9);
    }

    #[test]
    fn weights_hit_target_after_rebalance_with_costs() {
        let s = syms(&["A", "B", "C"]);
        let dates = [d(2026, 1, 30), d(2026, 2, 2)];
        let closes = vec![vec![100.0, 50.0, 20.0], vec![130.0, 40.0, 25.0]];
        let t = targets(&[("A", 0.5), ("B", 0.3), ("C", 0.2)]);
        let run = simulate(&s, &t, &dates, &closes, 100_000.0, monthly(), &paper_like()).unwrap();
        let ev = &run.events[1];
        // Rebuild holdings at the close of the rebalance day from the trades.
        let mut held = [0.0_f64; 3];
        for ev_i in &run.events[..2] {
            for tr in &ev_i.trades {
                let i = s.iter().position(|x| *x == tr.symbol).unwrap();
                held[i] += tr.qty;
            }
        }
        let equity = run.equity[1].1;
        let holdings: Vec<(String, f64)> = s.iter().cloned().zip(held.iter().copied()).collect();
        for h in &t {
            let w = weight_of(&s, &holdings, &closes[1], equity, &h.symbol);
            assert!((w - h.weight).abs() < 1e-9, "{} weight {w} vs {}", h.symbol, h.weight);
        }
        assert!(ev.cost > 0.0);
    }

    #[test]
    fn every_trade_has_its_cost_recorded_and_equity_pays_it() {
        let s = syms(&["A", "B"]);
        let dates = [d(2026, 1, 30), d(2026, 2, 2), d(2026, 3, 2)];
        let closes = vec![vec![100.0, 100.0], vec![200.0, 100.0], vec![150.0, 120.0]];
        let run = simulate(
            &s,
            &targets(&[("A", 0.5), ("B", 0.5)]),
            &dates,
            &closes,
            10_000.0,
            monthly(),
            &paper_like(),
        )
        .unwrap();
        let mut sum_trade_costs = 0.0;
        for ev in &run.events {
            assert!(!ev.trades.is_empty());
            let trade_costs: f64 = ev.trades.iter().map(RebalanceTrade::cost).sum();
            assert!((trade_costs - ev.cost).abs() < 1e-9, "event cost {} vs trades {}", ev.cost, trade_costs);
            for tr in &ev.trades {
                assert_eq!(tr.fee, 1.0);
                assert!(tr.slippage_cost > 0.0);
                assert_eq!(ev.cost_config, paper_like());
            }
            sum_trade_costs += trade_costs;
        }
        assert!((sum_trade_costs - run.total_cost).abs() < 1e-9);
        // Costs come out of equity: with no price move, equity falls by exactly the costs paid.
        let flat = simulate(
            &s,
            &targets(&[("A", 0.5), ("B", 0.5)]),
            &[d(2026, 1, 30), d(2026, 2, 2)],
            &[vec![100.0, 100.0], vec![100.0, 100.0]],
            10_000.0,
            monthly(),
            &paper_like(),
        )
        .unwrap();
        let final_eq = flat.equity[1].1;
        assert!(
            (10_000.0 - flat.total_cost - final_eq).abs() < 1e-6,
            "equity {final_eq}, costs {}",
            flat.total_cost
        );
    }

    #[test]
    fn calendar_rebalances_on_each_new_month_and_quarter() {
        let s = syms(&["A"]);
        let dates = [d(2026, 1, 2), d(2026, 1, 30), d(2026, 2, 2), d(2026, 3, 2), d(2026, 4, 1)];
        let closes = vec![vec![10.0]; 5];
        let t = targets(&[("A", 1.0)]);
        let monthly_run = simulate(&s, &t, &dates, &closes, 1000.0, monthly(), &zero_cost()).unwrap();
        // Initial, then Feb, Mar, Apr.
        assert_eq!(
            monthly_run.events.iter().map(|e| e.date).collect::<Vec<_>>(),
            vec![d(2026, 1, 2), d(2026, 2, 2), d(2026, 3, 2), d(2026, 4, 1)]
        );

        let quarterly = RebalanceRule { calendar: Some(Frequency::Quarterly), drift_threshold: None };
        let q_run = simulate(&s, &t, &dates, &closes, 1000.0, quarterly, &zero_cost()).unwrap();
        // Initial, then the first day of Q2 (April 1).
        assert_eq!(
            q_run.events.iter().map(|e| e.date).collect::<Vec<_>>(),
            vec![d(2026, 1, 2), d(2026, 4, 1)]
        );
    }

    #[test]
    fn drift_rule_triggers_only_past_the_threshold() {
        let s = syms(&["A", "B"]);
        let dates = [d(2026, 1, 2), d(2026, 1, 3), d(2026, 1, 4)];
        // Small drift on day 2, larger on day 3.
        let closes = vec![vec![100.0, 100.0], vec![104.0, 100.0], vec![160.0, 100.0]];
        let rule = RebalanceRule { calendar: None, drift_threshold: Some(0.10) };
        let run =
            simulate(&s, &targets(&[("A", 0.5), ("B", 0.5)]), &dates, &closes, 1000.0, rule, &zero_cost())
                .unwrap();
        let dates_hit: Vec<(Date, Vec<Reason>)> =
            run.events.iter().map(|e| (e.date, e.reasons.clone())).collect();
        assert_eq!(
            dates_hit,
            vec![(d(2026, 1, 2), vec![Reason::Initial]), (d(2026, 1, 4), vec![Reason::Drift])]
        );
    }

    #[test]
    fn rebalance_uses_only_prices_available_on_that_date() {
        // Changing prices after date t must not change any decision or trade up to t.
        let s = syms(&["A", "B", "C"]);
        let dates: Vec<Date> =
            (0..70).map(|i| Date::from_days(d(2026, 1, 1).to_days() + i64::from(i)).unwrap()).collect();
        let closes: Vec<Vec<f64>> = (0..70)
            .map(|i| {
                let x = f64::from(i);
                vec![100.0 + (x * 0.7).sin() * 20.0 + x * 0.3, 80.0 + (x * 0.4).cos() * 15.0, 50.0 + x * 0.2]
            })
            .collect();
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: Some(0.05) };
        let t = targets(&[("A", 0.4), ("B", 0.35), ("C", 0.25)]);
        let full = simulate(&s, &t, &dates, &closes, 50_000.0, rule, &paper_like()).unwrap();

        for cut in 1..dates.len() {
            // Prices after `cut` are replaced with garbage that is still valid (positive, finite).
            let mut altered = closes.clone();
            for row in altered.iter_mut().skip(cut) {
                for p in row.iter_mut() {
                    *p = *p * 3.7 + 11.0;
                }
            }
            let run = simulate(&s, &t, &dates, &altered, 50_000.0, rule, &paper_like()).unwrap();
            let up_to = |events: &[RebalanceEvent]| -> Vec<RebalanceEvent> {
                events.iter().filter(|e| e.date <= dates[cut - 1]).cloned().collect()
            };
            assert_eq!(up_to(&run.events), up_to(&full.events), "decisions changed after cut {cut}");
            assert_eq!(run.equity[..cut], full.equity[..cut], "equity changed before cut {cut}");
        }
    }

    #[test]
    fn invalid_inputs_are_refused() {
        let s = syms(&["A", "B"]);
        let dates = [d(2026, 1, 2), d(2026, 1, 3)];
        let closes = vec![vec![1.0, 1.0], vec![1.0, 1.0]];
        let good = targets(&[("A", 0.5), ("B", 0.5)]);

        let bad_weights = targets(&[("A", 0.5), ("B", 0.4)]);
        assert!(matches!(
            simulate(&s, &bad_weights, &dates, &closes, 1.0, monthly(), &zero_cost()),
            Err(RebalanceError::Weights(_))
        ));

        let no_rule = RebalanceRule { calendar: None, drift_threshold: None };
        assert!(matches!(
            simulate(&s, &good, &dates, &closes, 1.0, no_rule, &zero_cost()),
            Err(RebalanceError::Rule(_))
        ));

        let bad_threshold = RebalanceRule { calendar: None, drift_threshold: Some(1.5) };
        assert!(matches!(
            simulate(&s, &good, &dates, &closes, 1.0, bad_threshold, &zero_cost()),
            Err(RebalanceError::Rule(_))
        ));

        let unsorted = [d(2026, 1, 3), d(2026, 1, 2)];
        assert!(matches!(
            simulate(&s, &good, &unsorted, &closes, 1.0, monthly(), &zero_cost()),
            Err(RebalanceError::Input(_))
        ));

        let zero_price = vec![vec![1.0, 0.0], vec![1.0, 1.0]];
        assert!(matches!(
            simulate(&s, &good, &dates, &zero_price, 1.0, monthly(), &zero_cost()),
            Err(RebalanceError::Input(_))
        ));

        assert!(matches!(
            simulate(&s, &good, &dates, &closes, 0.0, monthly(), &zero_cost()),
            Err(RebalanceError::Input(_))
        ));
    }
}

#[cfg(test)]
mod date_tests {
    use super::Date;

    #[test]
    fn day_numbers_match_known_dates_and_round_trip() {
        assert_eq!(Date::new(1970, 1, 1).to_days(), 0);
        assert_eq!(Date::new(2000, 3, 1).to_days(), 11_017);
        assert_eq!(Date::new(2026, 11, 7).to_days(), 20_764);
        for days in -800..30_000 {
            assert_eq!(Date::from_days(days).unwrap().to_days(), days);
        }
        assert_eq!(Date::from_days(Date::new(2024, 2, 28).to_days() + 1), Some(Date::new(2024, 2, 29)));
        assert_eq!(Date::from_days(Date::new(2025, 2, 28).to_days() + 1), Some(Date::new(2025, 3, 1)));
        assert!(Date::new(2026, 1, 31) < Date::new(2026, 2, 1));
        assert_eq!(Date::new(2026, 2, 3).to_string(), "2026-02-03");
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn a_holding_without_a_target_weight_is_refused() {
        let symbols = vec!["A".to_string(), "B".to_string()];
        let targets =
            vec![Holding { symbol: "A".into(), weight: 0.5 }, Holding { symbol: "C".into(), weight: 0.5 }];
        let dates = [Date::new(2026, 1, 2), Date::new(2026, 1, 3)];
        let closes = vec![vec![1.0, 1.0], vec![1.0, 1.0]];
        let rule = RebalanceRule { calendar: None, drift_threshold: Some(0.1) };
        let cost = TradeCostConfig { name: "zero".into(), version: 1, slippage_bps: 0.0, min_fee: 0.0 };
        let err = simulate(&symbols, &targets, &dates, &closes, 100.0, rule, &cost).unwrap_err();
        assert!(matches!(err, RebalanceError::Input(ref m) if m.contains("no target weight for B")), "{err}");
    }
}
