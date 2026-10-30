//! Target-weight mode (M3 task 8): strategies that say what share of equity
//! each instrument should be, instead of placing orders.
//!
//! [`Rebalancer`] wraps a [`WeightStrategy`] and is itself an ordinary
//! [`Strategy`], so the engine, its timing contract and its fingerprints are
//! unchanged: weights become market orders at the next open, exactly as if a
//! strategy had placed them.
//!
//! On each bar the wrapped strategy may return new targets. The rebalancer
//! then trades towards them when its [`RebalanceRule`] says so:
//!  * weights are sanitized: non-finite ones dropped, negative ones set to
//!    zero unless the ledger allows shorting, and all of them scaled down so
//!    total gross weight stays within `gross_cap` of the ledger's leverage limit;
//!  * the target quantity is `weight * equity / close`, rounded towards zero
//!    to whole lots;
//!  * trades smaller than `min_trade_notional` are skipped;
//!  * instruments no longer in the targets are closed;
//!  * sells are submitted before buys, so the cash they free is there first;
//!  * nothing new is sent while the previous rebalance still has open orders
//!    (an execution delay), so orders never stack.

use std::collections::{BTreeMap, BTreeSet};

use spectra_core::{Bar, InstrumentId, Qty, Side};

use crate::backtest::{Context, OrderId, Strategy};

/// A strategy that sets target weights instead of placing orders.
pub trait WeightStrategy {
    /// Called once before the first bar.
    fn on_start(&mut self, _ctx: &Context<'_>) {}
    /// New target weights (share of equity per instrument; negative is short),
    /// or `None` to keep the current targets. Called on every bar, after fills
    /// and marks, with that timestamp's bars.
    fn weights(&mut self, ctx: &Context<'_>, bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>>;
}

/// When to trade towards the targets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RebalanceRule {
    /// On every bar.
    EveryBar,
    /// Every N bars (counted from the first bar), and whenever the targets change.
    EveryNBars(usize),
    /// Whenever an instrument's actual weight is more than this far from its
    /// target (e.g. 0.05 = 5 percentage points), and whenever the targets change.
    Drift(f64),
}

/// How targets turn into orders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RebalanceConfig {
    /// When to trade.
    pub rule: RebalanceRule,
    /// Share of the leverage limit the targets may use in total (e.g. 0.98
    /// leaves 2% for costs and gaps at the next open). Without a leverage
    /// limit, the cap is this many times equity.
    pub gross_cap: f64,
    /// Quantities are rounded towards zero to a multiple of this (e.g. 1 for
    /// whole shares). Zero means no rounding beyond the fixed-point unit.
    pub lot_size: Qty,
    /// Trades whose notional (at the close) is below this are skipped.
    pub min_trade_notional: f64,
}

impl Default for RebalanceConfig {
    fn default() -> Self {
        Self { rule: RebalanceRule::EveryBar, gross_cap: 0.98, lot_size: Qty::ONE, min_trade_notional: 0.0 }
    }
}

/// Runs a [`WeightStrategy`] as a [`Strategy`]. See the module documentation.
#[derive(Debug)]
pub struct Rebalancer<S> {
    inner: S,
    cfg: RebalanceConfig,
    targets: BTreeMap<InstrumentId, f64>,
    /// Targets changed since the last rebalance.
    changed: bool,
    /// Bars seen so far.
    bar: usize,
    /// Orders of the last rebalance, checked before sending more.
    pending: Vec<OrderId>,
    /// Every instrument this rebalancer has traded.
    traded: BTreeSet<InstrumentId>,
}

impl<S: WeightStrategy> Rebalancer<S> {
    /// Wraps `inner`.
    pub fn new(inner: S, cfg: RebalanceConfig) -> Self {
        Self {
            inner,
            cfg,
            targets: BTreeMap::new(),
            changed: false,
            bar: 0,
            pending: Vec::new(),
            traded: BTreeSet::new(),
        }
    }

    /// The current targets, after sanitizing.
    pub fn targets(&self) -> &BTreeMap<InstrumentId, f64> {
        &self.targets
    }

    fn set_targets(&mut self, ctx: &Context<'_>, raw: Vec<(InstrumentId, f64)>) {
        let policy = ctx.ledger_policy();
        let mut t: BTreeMap<InstrumentId, f64> = BTreeMap::new();
        for (id, w) in raw {
            if w.is_finite() {
                *t.entry(id).or_default() += if policy.allow_short { w } else { w.max(0.0) };
            }
        }
        t.retain(|_, w| *w != 0.0);
        let limit = policy.max_gross_leverage.map_or(1.0, spectra_core::Fixed::to_f64);
        let cap = (limit * self.cfg.gross_cap).max(0.0);
        let gross: f64 = t.values().map(|w| w.abs()).sum();
        if gross > cap {
            let k = cap / gross;
            for w in t.values_mut() {
                *w *= k;
            }
        }
        if t != self.targets {
            self.targets = t;
            self.changed = true;
        }
    }

    /// Instruments with a target or a position this rebalancer opened.
    fn universe(&self, ctx: &Context<'_>) -> BTreeSet<InstrumentId> {
        let mut u: BTreeSet<InstrumentId> = self.targets.keys().copied().collect();
        u.extend(self.traded.iter().copied().filter(|&id| !ctx.position(id).is_zero()));
        u
    }

    fn due(&self, ctx: &Context<'_>, equity: f64) -> bool {
        match self.cfg.rule {
            RebalanceRule::EveryBar => true,
            RebalanceRule::EveryNBars(n) => self.changed || self.bar.is_multiple_of(n.max(1)),
            RebalanceRule::Drift(th) => {
                self.changed
                    || (equity > 0.0
                        && self.universe(ctx).into_iter().any(|id| {
                            let actual = ctx.position_value(id).to_f64() / equity;
                            (actual - self.targets.get(&id).copied().unwrap_or(0.0)).abs() > th
                        }))
            }
        }
    }

    /// The order (side, quantity) that moves `id` to its target, if any.
    fn trade_for(&self, ctx: &Context<'_>, id: InstrumentId, equity: f64) -> Option<(Side, Qty)> {
        // Base-currency value of one unit (price converted, times the multiplier);
        // exactly the close for a plain equity.
        let close = ctx.unit_value(id, ctx.history(id).last()?.close)?;
        if close <= 0.0 {
            return None;
        }
        let want = self.targets.get(&id).copied().unwrap_or(0.0) * equity / close;
        let lot = effective_lot(self.cfg.lot_size.raw(), ctx.lot_size(id).map(spectra_core::Fixed::raw));
        let want_raw = (want * spectra_core::SCALE as f64).trunc();
        if !want_raw.is_finite() || want_raw.abs() >= 9.0e18 {
            return None;
        }
        let mut target = want_raw as i64;
        if lot > 0 {
            target = target / lot * lot; // towards zero
        }
        let delta = target.checked_sub(ctx.position(id).raw())?;
        if delta == 0 {
            return None;
        }
        let qty = Qty::from_raw(delta.checked_abs()?);
        if qty.to_f64() * close < self.cfg.min_trade_notional {
            return None;
        }
        Some((if delta > 0 { Side::Buy } else { Side::Sell }, qty))
    }
}

impl<S: WeightStrategy> Strategy for Rebalancer<S> {
    fn on_start(&mut self, ctx: &mut Context<'_>) {
        self.inner.on_start(ctx);
    }

    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        if let Some(w) = self.inner.weights(ctx, bars) {
            self.set_targets(ctx, w);
        }
        let waiting = self
            .pending
            .iter()
            .any(|&id| id >= 1 && ctx.orders().get((id - 1) as usize).is_some_and(|o| o.state.is_open()));
        let equity = ctx.equity().to_f64();
        if !waiting && self.due(ctx, equity) {
            let mut sells = Vec::new();
            let mut buys = Vec::new();
            for id in self.universe(ctx) {
                match self.trade_for(ctx, id, equity) {
                    Some((Side::Sell, q)) => sells.push((id, q)),
                    Some((Side::Buy, q)) => buys.push((id, q)),
                    None => {}
                }
            }
            self.pending.clear();
            for (id, q) in sells {
                self.pending.push(ctx.market(id, Side::Sell, q));
                self.traded.insert(id);
            }
            for (id, q) in buys {
                self.pending.push(ctx.market(id, Side::Buy, q));
                self.traded.insert(id);
            }
            self.changed = false;
        }
        self.bar += 1;
    }
}

/// The lot a rebalancer trades in: the strategy's lot, raised to a whole
/// multiple of the instrument's own lot when the instrument sets one.
fn effective_lot(strategy_lot: i64, instrument_lot: Option<i64>) -> i64 {
    match instrument_lot {
        Some(inst) if inst > 0 => {
            if strategy_lot <= inst {
                inst
            } else {
                ((strategy_lot + inst - 1) / inst).saturating_mul(inst)
            }
        }
        _ => strategy_lot,
    }
}

#[cfg(test)]
mod lot_tests {
    use super::effective_lot;

    #[test]
    fn instrument_lot_wins_when_coarser() {
        assert_eq!(effective_lot(0, None), 0);
        assert_eq!(effective_lot(100, None), 100);
        assert_eq!(effective_lot(0, Some(1)), 1);
        assert_eq!(effective_lot(1, Some(5)), 5);
        assert_eq!(effective_lot(7, Some(5)), 10);
        assert_eq!(effective_lot(10, Some(5)), 10);
    }
}
