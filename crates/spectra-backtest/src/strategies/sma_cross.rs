//! SMA crossover. Long-only with one instrument, it trades exactly like the
//! `sma_cross` demo and the reference parity strategies, so its runs match
//! theirs fingerprint for fingerprint.

use spectra_core::{Bar, Qty, Result, Side};
use spectra_engine::{Context, Strategy};
use spectra_research::ParamSet;

use super::util::{qty, sma_raw};
use super::{ParamKind, ParamSpec};
use crate::config::FieldError;

pub(super) const PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "fast",
        kind: ParamKind::Int,
        default: 10.0,
        min: 1.0,
        max: 1000.0,
        description: "Fast moving-average length, in bars",
    },
    ParamSpec {
        name: "slow",
        kind: ParamKind::Int,
        default: 30.0,
        min: 2.0,
        max: 5000.0,
        description: "Slow moving-average length, in bars (must be longer than fast)",
    },
    ParamSpec {
        name: "qty",
        kind: ParamKind::Float,
        default: 100.0,
        min: 0.000_000_01,
        max: 1e9,
        description: "Units to hold per instrument while in a position",
    },
    ParamSpec {
        name: "allow_short",
        kind: ParamKind::Bool,
        default: 0.0,
        min: 0.0,
        max: 1.0,
        description: "Go short instead of flat when fast < slow (the configuration must allow shorting)",
    },
];

pub(super) fn check(p: &ParamSet) -> Option<FieldError> {
    let (fast, slow) = (p.get("fast").ok()?, p.get("slow").ok()?);
    (fast >= slow).then(|| FieldError {
        field: "params.fast".into(),
        message: format!("must be shorter than slow ({fast} >= {slow})"),
    })
}

pub(super) fn build(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(SmaCross {
        fast: p.get_int("fast")? as usize,
        slow: p.get_int("slow")? as usize,
        unit: qty(p.get("qty")?),
        shorts: p.get("allow_short")? != 0.0,
    }))
}

#[derive(Debug)]
struct SmaCross {
    fast: usize,
    slow: usize,
    unit: Qty,
    shorts: bool,
}

impl Strategy for SmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            // Futures: trade the front contract, on the chain's continuous series.
            if !ctx.is_front(b.instrument) {
                continue;
            }
            let h = ctx.signal_history(b.instrument);
            if h.len() < self.slow.max(self.fast) {
                continue;
            }
            let bullish = sma_raw(h, self.fast) > sma_raw(h, self.slow);
            let pos = ctx.position(b.instrument);
            if !self.shorts {
                if bullish && pos.is_zero() {
                    ctx.market(b.instrument, Side::Buy, self.unit);
                } else if !bullish && pos.raw() > 0 {
                    ctx.market(b.instrument, Side::Sell, pos);
                }
                continue;
            }
            // Long/short: flip through zero in one order.
            let Ok(short_size) = pos.checked_abs() else { continue };
            if bullish && pos.raw() <= 0 {
                let q = if pos.raw() < 0 { self.unit.checked_add(short_size) } else { Ok(self.unit) };
                if let Ok(q) = q {
                    ctx.market(b.instrument, Side::Buy, q);
                }
            } else if !bullish && pos.raw() > 0 {
                if let Ok(q) = pos.checked_add(self.unit) {
                    ctx.market(b.instrument, Side::Sell, q);
                }
            } else if !bullish && pos.is_zero() {
                ctx.market(b.instrument, Side::Sell, self.unit);
            }
        }
    }
}
