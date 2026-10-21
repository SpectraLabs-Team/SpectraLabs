//! Bars of one symbol reduced to at most N points for a chart (M1 task 6).
//!
//! The selected bars are split into N consecutive groups of (nearly) equal
//! size, and each group becomes one OHLCV point: the first open, the highest
//! high, the lowest low, the last close, the summed volume, at the group's
//! first timestamp. Nothing is interpolated, so every point is real data.

use spectra_core::InstrumentId;

use crate::dataset::Dataset;

/// One chart point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChartPoint {
    /// Nanoseconds since the epoch of the first bar in the group.
    pub ts_ns: i64,
    /// Fixed-point raw values (8 decimals).
    pub open: i64,
    /// See `open`.
    pub high: i64,
    /// See `open`.
    pub low: i64,
    /// See `open`.
    pub close: i64,
    /// Summed volume, fixed-point raw (saturating).
    pub volume: i64,
}

/// The bars of `instrument` with `from_ns <= ts <= to_ns`, as at most
/// `max_points` points (at least 1), and how many bars that covered.
pub fn chart_points(
    d: &Dataset,
    instrument: InstrumentId,
    from_ns: i64,
    to_ns: i64,
    max_points: usize,
) -> (Vec<ChartPoint>, usize) {
    let bars: Vec<_> = d
        .bars
        .iter()
        .filter(|b| b.instrument == instrument && b.ts_ns >= from_ns && b.ts_ns <= to_ns)
        .collect();
    let n = bars.len();
    let groups = max_points.max(1).min(n);
    let mut out = Vec::with_capacity(groups);
    for g in 0..groups {
        let (start, end) = (g * n / groups, (g + 1) * n / groups);
        let chunk = &bars[start..end];
        let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else { continue };
        out.push(ChartPoint {
            ts_ns: first.ts_ns,
            open: first.open.raw(),
            high: chunk.iter().map(|b| b.high.raw()).max().unwrap_or(first.high.raw()),
            low: chunk.iter().map(|b| b.low.raw()).min().unwrap_or(first.low.raw()),
            close: last.close.raw(),
            volume: chunk.iter().fold(0i64, |acc, b| acc.saturating_add(b.volume.raw())),
        });
    }
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{load_csv, CsvOptions};

    fn dataset(days: usize) -> Dataset {
        let mut csv = String::from("date,symbol,open,high,low,close,volume\n");
        for i in 0..days {
            let day = 1_704_153_600 + i as i64 * 86_400; // from 2024-01-02
            let p = 100 + i;
            csv += &format!("{day},AAA,{p},{},{},{}.5,10\n", p + 2, p - 1, p);
            csv += &format!("{day},BBB,50,51,49,50,1\n");
        }
        let opt = CsvOptions {
            time: crate::TimeOptions { format: crate::TimeFormat::EpochSeconds, naive_utc_offset_minutes: 0 },
            ..CsvOptions::default()
        };
        load_csv(csv.as_bytes(), &opt).unwrap()
    }

    #[test]
    fn groups_keep_real_ohlcv() {
        let d = dataset(10);
        let a = d.symbols.id("AAA").unwrap();
        let (all, n) = chart_points(&d, a, i64::MIN, i64::MAX, 1000);
        assert_eq!((all.len(), n), (10, 10), "fewer bars than points: one point per bar");
        let (two, _) = chart_points(&d, a, i64::MIN, i64::MAX, 2);
        assert_eq!(two.len(), 2);
        let first = two[0];
        assert_eq!(first.ts_ns, all[0].ts_ns);
        assert_eq!(first.open, all[0].open);
        assert_eq!(first.close, all[4].close);
        assert_eq!(first.high, all[..5].iter().map(|p| p.high).max().unwrap());
        assert_eq!(first.low, all[0].low);
        assert_eq!(first.volume, 5 * 10 * 100_000_000);
    }

    #[test]
    fn a_range_and_a_symbol_select_bars() {
        let d = dataset(10);
        let b = d.symbols.id("BBB").unwrap();
        let from = 1_704_153_600 * 1_000_000_000 + 2 * 86_400 * 1_000_000_000;
        let to = from + 2 * 86_400 * 1_000_000_000;
        let (pts, n) = chart_points(&d, b, from, to, 1000);
        assert_eq!((pts.len(), n), (3, 3));
        assert!(pts.iter().all(|p| p.close == 50 * 100_000_000));
        let (one, after) = chart_points(&d, b, to + 1, i64::MAX, 0);
        assert_eq!((one.len(), after), (1, 5), "max_points 0 means 1; days 5 to 9 are after the range");
        assert!(chart_points(&d, 99, i64::MIN, i64::MAX, 10).0.is_empty(), "unknown instrument");
    }
}
