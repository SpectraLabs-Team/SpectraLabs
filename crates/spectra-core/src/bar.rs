//! OHLCV bars, their validation and their fingerprint.

use crate::fixed::{Price, Qty};
use crate::ledger::InstrumentId;
use crate::sha256::Sha256;

/// One bar for one instrument. The bar covers `[ts_ns, next ts)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bar {
    /// Bar start time, nanoseconds since the Unix epoch (UTC).
    pub ts_ns: i64,
    /// Time when this bar's values became known (nanoseconds since Unix epoch UTC).
    /// Defaults to `ts_ns` for standard historical bars without revisions.
    pub known_at_ns: i64,
    /// Instrument id (index into the dataset's symbol table).
    pub instrument: InstrumentId,
    /// Open price.
    pub open: Price,
    /// High price.
    pub high: Price,
    /// Low price.
    pub low: Price,
    /// Close price.
    pub close: Price,
    /// Volume.
    pub volume: Qty,
}

impl Bar {
    /// Constructs a standard bar where `known_at_ns == ts_ns`.
    pub const fn new(
        ts_ns: i64,
        instrument: InstrumentId,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Qty,
    ) -> Self {
        Self { ts_ns, known_at_ns: ts_ns, instrument, open, high, low, close, volume }
    }

    /// Constructs a bar with an explicit `known_at_ns`.
    pub const fn with_known_at(
        ts_ns: i64,
        known_at_ns: i64,
        instrument: InstrumentId,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Qty,
    ) -> Self {
        Self { ts_ns, known_at_ns, instrument, open, high, low, close, volume }
    }
}

/// Checks that bars are usable by the backtester:
///  * strictly increasing by (timestamp, instrument), or with increasing `known_at_ns` for revisions
///  * all prices > 0 and low <= open, close <= high
///  * volume >= 0
///  * `known_at_ns >= ts_ns` (values cannot be known before the bar occurs)
///
/// Returns `None` when valid, otherwise a description of the first problem.
pub fn validate_bars(bars: &[Bar]) -> Option<String> {
    for (i, b) in bars.iter().enumerate() {
        let where_ = format!("bar {i}: ");
        if b.open.raw() <= 0 || b.high.raw() <= 0 || b.low.raw() <= 0 || b.close.raw() <= 0 {
            return Some(where_ + "non-positive price");
        }
        if b.low > b.open.min(b.close) || b.high < b.open.max(b.close) {
            return Some(where_ + "OHLC inconsistent (low/high do not bracket open and close)");
        }
        if b.volume.raw() < 0 {
            return Some(where_ + "negative volume");
        }
        if b.known_at_ns < b.ts_ns {
            return Some(where_ + "known_at cannot be earlier than timestamp");
        }
        if i > 0 {
            let p = &bars[i - 1];
            if b.ts_ns < p.ts_ns || (b.ts_ns == p.ts_ns && b.instrument < p.instrument) {
                return Some(where_ + "bars are not sorted by (timestamp, instrument)");
            }
            if b.ts_ns == p.ts_ns && b.instrument == p.instrument {
                if b.known_at_ns == p.known_at_ns {
                    return Some(where_ + "duplicate bar for the same timestamp and instrument");
                }
                if b.known_at_ns < p.known_at_ns {
                    return Some(where_ + "bars with revisions must be sorted by known_at");
                }
            }
        }
    }
    None
}

/// SHA-256 over the count and every field of every bar, in order. This is the
/// dataset fingerprint recorded in run records; identical to the C++ engine.
pub fn bars_sha256(bars: &[Bar]) -> String {
    let mut h = Sha256::new();
    h.update_u64(bars.len() as u64);
    for b in bars {
        h.update_i64(b.ts_ns);
        h.update_u64(u64::from(b.instrument));
        h.update_i64(b.open.raw());
        h.update_i64(b.high.raw());
        h.update_i64(b.low.raw());
        h.update_i64(b.close.raw());
        h.update_i64(b.volume.raw());
    }
    h.hex_digest()
}

/// Start offsets of each run of equal timestamps, plus `bars.len()` at the
/// end: `offsets[k]` is the first bar of the k-th unique timestamp.
pub fn timestamp_offsets(bars: &[Bar]) -> Vec<usize> {
    let mut offsets = Vec::new();
    for i in 0..bars.len() {
        if i == 0 || bars[i].ts_ns != bars[i - 1].ts_ns {
            offsets.push(i);
        }
    }
    offsets.push(bars.len());
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, id: InstrumentId, o: i64, h: i64, l: i64, c: i64) -> Bar {
        Bar {
            ts_ns: ts,
            known_at_ns: ts,
            instrument: id,
            open: Price::from_raw(o),
            high: Price::from_raw(h),
            low: Price::from_raw(l),
            close: Price::from_raw(c),
            volume: Qty::from_raw(1),
        }
    }

    #[test]
    fn validation_catches_each_problem() {
        assert_eq!(validate_bars(&[bar(1, 0, 10, 12, 9, 11)]), None);
        assert!(validate_bars(&[bar(1, 0, 0, 12, 9, 11)]).unwrap().contains("non-positive"));
        assert!(validate_bars(&[bar(1, 0, 10, 10, 9, 11)]).unwrap().contains("OHLC"));
        assert!(validate_bars(&[bar(2, 0, 10, 12, 9, 11), bar(1, 0, 10, 12, 9, 11)])
            .unwrap()
            .contains("sorted"));
        assert!(validate_bars(&[bar(1, 0, 10, 12, 9, 11), bar(1, 0, 10, 12, 9, 11)])
            .unwrap()
            .contains("duplicate"));
        let mut neg = bar(1, 0, 10, 12, 9, 11);
        neg.volume = Qty::from_raw(-1);
        assert!(validate_bars(&[neg]).unwrap().contains("negative volume"));

        let mut early_known = bar(100, 0, 10, 12, 9, 11);
        early_known.known_at_ns = 90;
        assert!(validate_bars(&[early_known]).unwrap().contains("known_at cannot be earlier"));

        let b1 = Bar::with_known_at(
            100,
            100,
            0,
            Price::from_raw(10),
            Price::from_raw(12),
            Price::from_raw(9),
            Price::from_raw(11),
            Qty::from_raw(1),
        );
        let b2_rev = Bar::with_known_at(
            100,
            200,
            0,
            Price::from_raw(10),
            Price::from_raw(13),
            Price::from_raw(9),
            Price::from_raw(12),
            Qty::from_raw(2),
        );
        assert_eq!(validate_bars(&[b1, b2_rev]), None, "valid revision with known_at > ts_ns is accepted");

        let b2_bad_order = Bar::with_known_at(
            100,
            50,
            0,
            Price::from_raw(10),
            Price::from_raw(13),
            Price::from_raw(9),
            Price::from_raw(12),
            Qty::from_raw(2),
        );
        assert!(validate_bars(&[b1, b2_bad_order]).unwrap().contains("known_at"));
    }

    #[test]
    fn fingerprint_changes_with_any_field() {
        let a = vec![bar(1, 0, 10, 12, 9, 11), bar(2, 0, 11, 12, 9, 11)];
        let mut b = a.clone();
        b[1].close = Price::from_raw(10);
        assert_ne!(bars_sha256(&a), bars_sha256(&b));
        assert_eq!(bars_sha256(&a), bars_sha256(&a.clone()));
    }

    #[test]
    fn offsets_group_equal_timestamps() {
        let bars = vec![bar(1, 0, 10, 12, 9, 11), bar(1, 1, 10, 12, 9, 11), bar(3, 0, 10, 12, 9, 11)];
        assert_eq!(timestamp_offsets(&bars), vec![0, 2, 3]);
        assert_eq!(timestamp_offsets(&[]), vec![0]);
    }
}
