//! Corporate actions applied during a run (M3 task 10).
//!
//! A run on *unadjusted* prices sees the price jump when a split or dividend
//! goes ex. Passing the corporate actions to the run keeps the books right:
//!
//!  * **Split** `from:to` (2-for-1 is `from: 1, to: 2`): at the first timestamp
//!    at or after the ex-date, every position in the instrument is multiplied
//!    by `to / from` (cost basis unchanged, so no P&L appears), its last mark is
//!    divided by the same ratio, and every open order is adjusted the way
//!    brokers do it: quantities times `to / from`, limit and stop prices times
//!    `from / to`.
//!  * **Cash dividend** of `per_share`: at the first timestamp at or after the
//!    ex-date, each long position is paid `qty x per_share` and each short pays
//!    it. Open orders are not changed.
//!
//! Both are applied before that timestamp's fills, on the positions held at
//! the previous close. On the same ex-date, splits go before dividends (a
//! dividend is quoted per post-split share). Each change is a ledger journal
//! entry (`Split`, `Dividend`), so the books still re-derive from the journal.
//!
//! Running on prices that are already adjusted (a derived dataset version)
//! needs no corporate actions; passing them as well would count them twice.

use std::cmp::Ordering;

use spectra_core::{InstrumentId, Price, Qty, Sha256};

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorporateActionKind {
    /// `from` old shares become `to` new ones (2-for-1: `from: 1, to: 2`;
    /// a 1-for-10 reverse split: `from: 10, to: 1`).
    Split {
        /// Shares before.
        from: u32,
        /// Shares after.
        to: u32,
    },
    /// Cash paid per share held.
    Dividend {
        /// Amount per share, > 0.
        per_share: Price,
    },
}

/// A corporate action on one instrument, effective at the first timestamp at
/// or after `ex_ts_ns`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CorporateAction {
    /// Instrument.
    pub instrument: InstrumentId,
    /// Ex-date, nanoseconds (UTC).
    pub ex_ts_ns: i64,
    /// Split or dividend.
    pub kind: CorporateActionKind,
}

/// Largest share count on either side of a split.
pub const MAX_SPLIT_SHARES: u32 = 1_000_000;

impl CorporateAction {
    /// A split of `from` old shares into `to` new ones.
    pub const fn split(instrument: InstrumentId, ex_ts_ns: i64, from: u32, to: u32) -> Self {
        Self { instrument, ex_ts_ns, kind: CorporateActionKind::Split { from, to } }
    }

    /// A cash dividend of `per_share`.
    pub const fn dividend(instrument: InstrumentId, ex_ts_ns: i64, per_share: Price) -> Self {
        Self { instrument, ex_ts_ns, kind: CorporateActionKind::Dividend { per_share } }
    }

    /// Why this action is invalid, if it is.
    pub fn problem(&self) -> Option<String> {
        match self.kind {
            CorporateActionKind::Split { from, to } => {
                if from == 0 || to == 0 || from > MAX_SPLIT_SHARES || to > MAX_SPLIT_SHARES {
                    Some(format!("split share counts must be 1 to {MAX_SPLIT_SHARES} (got {from}:{to})"))
                } else if from == to {
                    Some(format!("a {from}:{to} split changes nothing"))
                } else {
                    None
                }
            }
            CorporateActionKind::Dividend { per_share } => {
                (per_share.raw() <= 0).then(|| "a dividend must be greater than zero".to_string())
            }
        }
    }

    /// Splits before dividends on the same ex-date.
    const fn rank(&self) -> u8 {
        match self.kind {
            CorporateActionKind::Split { .. } => 0,
            CorporateActionKind::Dividend { .. } => 1,
        }
    }
}

/// The order actions are applied in: by ex-date, splits before dividends,
/// then by instrument.
pub(crate) fn apply_order(a: &CorporateAction, b: &CorporateAction) -> Ordering {
    (a.ex_ts_ns, a.rank(), a.instrument).cmp(&(b.ex_ts_ns, b.rank(), b.instrument))
}

/// SHA-256 of the actions in application order (recorded in `config_sha256`
/// when a run has any).
pub fn corporate_actions_sha256(actions: &[CorporateAction]) -> String {
    let mut sorted = actions.to_vec();
    sorted.sort_by(apply_order);
    let mut h = Sha256::new();
    h.update_u64(sorted.len() as u64);
    for a in &sorted {
        h.update_u64(u64::from(a.instrument));
        h.update_i64(a.ex_ts_ns);
        match a.kind {
            CorporateActionKind::Split { from, to } => {
                h.update_u64(0);
                h.update_u64(u64::from(from));
                h.update_u64(u64::from(to));
            }
            CorporateActionKind::Dividend { per_share } => {
                h.update_u64(1);
                h.update_i64(per_share.raw());
            }
        }
    }
    h.hex_digest()
}

/// `raw * num / den`, rounded half away from zero, never rounding a non-zero
/// value to zero (the smallest unit keeps its sign).
pub(crate) fn scale_raw(raw: i64, num: u32, den: u32) -> i64 {
    if raw == 0 {
        return 0;
    }
    let v = spectra_core::fixed::div_round_half_away(i128::from(raw) * i128::from(num), i128::from(den));
    let v = i64::try_from(v).unwrap_or(if raw > 0 { i64::MAX } else { i64::MIN });
    if v == 0 {
        raw.signum()
    } else {
        v
    }
}

/// A quantity after a `from:to` split.
pub(crate) fn split_qty(q: Qty, from: u32, to: u32) -> Qty {
    Qty::from_raw(scale_raw(q.raw(), to, from))
}

/// A price after a `from:to` split.
pub(crate) fn split_price(p: Price, from: u32, to: u32) -> Price {
    Price::from_raw(scale_raw(p.raw(), from, to))
}
