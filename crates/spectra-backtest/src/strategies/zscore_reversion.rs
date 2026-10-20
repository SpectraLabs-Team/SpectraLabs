//! Z-score mean reversion: z = (close - mean) / stdev over a rolling window of
//! closes. Long below -entry, out once z rises above -exit; with shorting on,
//! short above +entry and out once z falls below +exit.

use std::collections::BTreeMap;

use spectra_core::{Bar, InstrumentId, Qty, Result, Side};
use spectra_engine::{Context, OrderId, Strategy};
use spectra_research::ParamSet;

use super::util::qty;
use super::{ParamKind, ParamSpec};
use crate::config::FieldError;

pub(super) const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "lookback",
        kind: ParamKind::Int,
        default: 20.0,
        min: 3.0,
        max: 5000.0,
        description: "Window for the rolling mean and standard deviation, in bars",
    },
    ParamSpec {
        name: "entry_z",
        kind: ParamKind::Float,
        default: 2.0,
        min: 0.1,
        max: 10.0,
        description: "Enter when the close is this many standard deviations from the mean",
    },
    ParamSpec {
        name: "exit_z",
        kind: ParamKind::Float,
        default: 0.5,
        min: 0.0,
        max: 10.0,
        description: "Exit once it is back within this many (must be below entry_z)",
    },
    ParamSpec {
        name: "qty",
        kind: ParamKind::Float,
        default: 100.0,
        min: 0.000_000_01,
        max: 1e9,
        description: "Units per position",
    },
    ParamSpec {
        name: "allow_short",
        kind: ParamKind::Bool,
        default: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Also trade the short side (the configuration must allow shorting)",
    },
];

pub(super) fn check(p: &ParamSet) -> Option<FieldError> {
    let (entry, exit) = (p.get("entry_z").ok()?, p.get("exit_z").ok()?);
    (exit >= entry).then(|| FieldError {
        field: "params.exit_z".into(),
        message: format!("must be below entry_z ({exit} >= {entry})"),
    })
}

pub(super) fn build(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(ZScore {
        lookback: p.get_int("lookback")? as usize,
        entry: p.get("entry_z")?,
        exit: p.get("exit_z")?,
        unit: qty(p.get("qty")?),
        shorts: p.get("allow_short")? != 0.0,
        last_order: BTreeMap::new(),
    }))
}

#[derive(Debug)]
struct ZScore {
    lookback: usize,
    entry: f64,
    exit: f64,
    unit: Qty,
    shorts: bool,
    /// Each instrument's most recent order, to avoid stacking orders while one waits.
    last_order: BTreeMap<InstrumentId, OrderId>,
}

/// z-score of the last close against the last `n` closes (sample stdev).
fn zscore(h: &[Bar], n: usize) -> Option<f64> {
    let w = &h[h.len() - n..];
    let xs: Vec<f64> = w.iter().map(|b| b.close.to_f64()).collect();
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1) as f64;
    let sd = var.sqrt();
    (sd > 0.0).then(|| (xs[n - 1] - mean) / sd)
}

impl Strategy for ZScore {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            let h = ctx.history(b.instrument);
            // With an execution delay, wait for the last order instead of stacking another.
            let waiting = self
                .last_order
                .get(&b.instrument)
                .is_some_and(|&id| ctx.orders().get((id - 1) as usize).is_some_and(|o| o.state.is_open()));
            if h.len() < self.lookback || waiting {
                continue;
            }
            let Some(z) = zscore(h, self.lookback) else { continue };
            let pos = ctx.position(b.instrument);
            let order = if pos.raw() > 0 {
                (z > -self.exit).then_some((Side::Sell, pos))
            } else if pos.raw() < 0 {
                (z < self.exit).then(|| pos.checked_abs().ok().map(|q| (Side::Buy, q))).flatten()
            } else if z < -self.entry {
                Some((Side::Buy, self.unit))
            } else if self.shorts && z > self.entry {
                Some((Side::Sell, self.unit))
            } else {
                None
            };
            if let Some((side, q)) = order {
                let id = ctx.market(b.instrument, side, q);
                self.last_order.insert(b.instrument, id);
            }
        }
    }
}
