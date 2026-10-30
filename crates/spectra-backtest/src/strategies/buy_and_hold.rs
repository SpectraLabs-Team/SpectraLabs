//! Buy and hold: on the first bar, split a share of the cash equally across the
//! instruments present and buy whole units at the next open. Never trades again.

use spectra_core::{Bar, Qty, Result, Side};
use spectra_engine::{Context, Strategy};
use spectra_research::ParamSet;

use super::{ParamKind, ParamSpec};
use crate::config::FieldError;

pub(super) const PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "allocation",
    kind: ParamKind::Float,
    default: 0.95,
    min: 0.01,
    max: 1.0,
    description: "Share of starting cash to invest; the rest covers costs and gaps at the next open",
}];

pub(super) fn check(_p: &ParamSet) -> Option<FieldError> {
    None
}

pub(super) fn build(p: &ParamSet) -> Result<Box<dyn Strategy>> {
    Ok(Box::new(BuyAndHold { allocation: p.get("allocation")?, done: false }))
}

#[derive(Debug)]
struct BuyAndHold {
    allocation: f64,
    done: bool,
}

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        if self.done {
            return;
        }
        self.done = true;
        // Futures: only each chain's front contract; the engine rolls it.
        let bars: Vec<&Bar> = bars.iter().filter(|b| ctx.is_front(b.instrument)).collect();
        if bars.is_empty() {
            return;
        }
        let budget = ctx.cash().to_f64() * self.allocation / bars.len() as f64;
        for b in bars {
            // Base-currency value of one unit (FX and multiplier included); the close for a plain equity.
            let Some(unit) = ctx.unit_value(b.instrument, b.close).filter(|u| *u > 0.0) else { continue };
            let mut units = (budget / unit).floor();
            if let Some(lot) =
                ctx.lot_size(b.instrument).map(spectra_core::Fixed::to_f64).filter(|l| *l > 0.0)
            {
                units = (units / lot).floor() * lot;
            }
            if units >= 1.0 {
                if let Ok(q) = Qty::from_units(units as i64) {
                    ctx.market(b.instrument, Side::Buy, q);
                }
            }
        }
    }
}
