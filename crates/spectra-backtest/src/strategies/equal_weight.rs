//! Equal weight: every instrument seen so far gets the same share of equity,
//! rebalanced on a schedule or when weights drift. Target-weight mode (M3
//! task 8): it sets weights, and the engine's `Rebalancer` turns them into orders.

use std::collections::BTreeSet;

use spectra_core::{Bar, InstrumentId, Result};
use spectra_engine::{Context, RebalanceConfig, RebalanceRule, Rebalancer, Strategy, WeightStrategy};
use spectra_research::ParamSet;

use super::util::qty;
use super::{ParamKind, ParamSpec};
use crate::config::FieldError;

pub(super) const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "rebalance_every",
        kind: ParamKind::Int,
        default: 21.0,
        min: 0.0,
        max: 10_000.0,
        description:
            "Rebalance every this many bars (21 is about monthly on daily bars); 0 rebalances on drift",
    },
    ParamSpec {
        name: "drift",
        kind: ParamKind::Float,
        default: 0.05,
        min: 0.0,
        max: 1.0,
        description: "With rebalance_every 0: rebalance when any weight is this far from its target",
    },
    ParamSpec {
        name: "gross",
        kind: ParamKind::Float,
        default: 0.98,
        min: 0.05,
        max: 1.0,
        description: "Share of the leverage limit to invest in total; the rest covers costs",
    },
    ParamSpec {
        name: "lot_size",
        kind: ParamKind::Float,
        default: 1.0,
        min: 0.0,
        max: 1e6,
        description: "Trade in multiples of this many units (0 for any size)",
    },
    ParamSpec {
        name: "min_trade",
        kind: ParamKind::Float,
        default: 0.0,
        min: 0.0,
        max: 1e9,
        description: "Skip trades smaller than this notional",
    },
];

pub(super) fn check(_p: &ParamSet) -> Option<FieldError> {
    None
}

pub(super) fn build(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    let every = p.get_int("rebalance_every")? as usize;
    let rule =
        if every == 0 { RebalanceRule::Drift(p.get("drift")?) } else { RebalanceRule::EveryNBars(every) };
    let cfg = RebalanceConfig {
        rule,
        gross_cap: p.get("gross")?,
        lot_size: qty(p.get("lot_size")?),
        min_trade_notional: p.get("min_trade")?,
    };
    Ok(Box::new(Rebalancer::new(EqualWeight { seen: BTreeSet::new() }, cfg)))
}

#[derive(Debug)]
struct EqualWeight {
    seen: BTreeSet<InstrumentId>,
}

impl WeightStrategy for EqualWeight {
    fn weights(&mut self, _ctx: &Context<'_>, bars: &[Bar]) -> Option<Vec<(InstrumentId, f64)>> {
        let before = self.seen.len();
        self.seen.extend(bars.iter().map(|b| b.instrument));
        if self.seen.len() == before && before > 0 {
            return None; // same universe, same targets
        }
        let w = 1.0 / self.seen.len() as f64;
        Some(self.seen.iter().map(|&id| (id, w)).collect())
    }
}
