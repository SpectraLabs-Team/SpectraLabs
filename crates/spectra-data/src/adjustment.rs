//! Exact fixed-point adjustment of OHLCV bars for corporate actions (splits and cash dividends).
//!
//! Standard library only. Prices and volumes are adjusted backwards in time so that the
//! latest prices reflect current economic reality while preserving historical returns and
//! technical indicator continuity.
//!
//! Principles:
//!  * Exact: All scaling operations use fixed-point arithmetic with 128-bit rational fractions
//!    reduced by greatest common divisor to avoid floating-point drift.
//!  * Invariant-preserving: Low <= min(Open, Close) and High >= max(Open, Close) are enforced
//!    after any rounding.
//!  * Multi-action compounding: Multiple splits and dividends on the same or different ex-dates
//!    compound backward chronologically.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use spectra_core::fixed::{div_round_half_away, narrow, ArithmeticError, Price, Qty};
use spectra_core::{validate_bars, Bar, Error, InstrumentId, Result};

use crate::dataset::Dataset;

/// Type of corporate action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Stock split or reverse split (e.g. 2-for-1 is from: 1, to: 2).
    Split,
    /// Cash dividend per share (paid to longs on ex-date).
    Dividend,
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Split => write!(f, "split"),
            Self::Dividend => write!(f, "dividend"),
        }
    }
}

/// A corporate action event for a specific instrument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorporateAction {
    /// Instrument ticker symbol.
    pub symbol: String,
    /// Ex-action timestamp (nanoseconds since Unix epoch, UTC).
    /// Bars strictly before this timestamp (ts_ns < ex_date_ns) receive the backward adjustment.
    pub ex_date_ns: i64,
    /// Kind of corporate action.
    pub kind: ActionKind,
    /// For split: original share count (e.g. 1 in 2-for-1).
    pub split_from: u32,
    /// For split: new share count (e.g. 2 in 2-for-1).
    pub split_to: u32,
    /// For cash dividend: dividend amount per share.
    pub dividend_amount: Price,
}

impl CorporateAction {
    /// Validates the action parameters.
    ///
    /// # Errors
    /// Returns an error string if `symbol` is empty or if split ratios or dividend amounts are invalid.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.symbol.trim().is_empty() {
            return Err("symbol cannot be empty".to_string());
        }
        match self.kind {
            ActionKind::Split => {
                if self.split_from == 0 {
                    return Err(format!("{}: split_from must be greater than zero", self.symbol));
                }
                if self.split_to == 0 {
                    return Err(format!("{}: split_to must be greater than zero", self.symbol));
                }
            }
            ActionKind::Dividend => {
                if self.dividend_amount.raw() <= 0 {
                    return Err(format!("{}: dividend amount must be positive", self.symbol));
                }
            }
        }
        Ok(())
    }
}

/// Greatest common divisor for simplifying rational multipliers.
fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.abs()
}

/// Rational multiplier `num / den`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rational {
    num: i128,
    den: i128,
}

impl Rational {
    const ONE: Self = Self { num: 1, den: 1 };

    fn new(mut num: i128, mut den: i128) -> std::result::Result<Self, ArithmeticError> {
        if den == 0 {
            return Err(ArithmeticError::DivisionByZero);
        }
        if den < 0 {
            num = -num;
            den = -den;
        }
        let g = gcd(num, den);
        if g > 1 {
            num /= g;
            den /= g;
        }
        Ok(Self { num, den })
    }

    fn mul(self, other: Self) -> std::result::Result<Self, ArithmeticError> {
        let g1 = gcd(self.num, other.den);
        let g2 = gcd(other.num, self.den);
        let n = (self.num / g1).checked_mul(other.num / g2).ok_or(ArithmeticError::Overflow)?;
        let d = (self.den / g2).checked_mul(other.den / g1).ok_or(ArithmeticError::Overflow)?;
        Self::new(n, d)
    }

    fn apply_to_price(self, p: Price) -> std::result::Result<Price, ArithmeticError> {
        let prod = i128::from(p.raw()).checked_mul(self.num).ok_or(ArithmeticError::Overflow)?;
        let raw = narrow(div_round_half_away(prod, self.den))?;
        if raw <= 0 {
            // Price must stay strictly positive
            Ok(Price::from_raw(1))
        } else {
            Ok(Price::from_raw(raw))
        }
    }

    fn apply_to_volume(self, v: Qty) -> std::result::Result<Qty, ArithmeticError> {
        let prod = i128::from(v.raw()).checked_mul(self.num).ok_or(ArithmeticError::Overflow)?;
        let raw = narrow(div_round_half_away(prod, self.den))?;
        if raw < 0 {
            Ok(Qty::from_raw(0))
        } else {
            Ok(Qty::from_raw(raw))
        }
    }
}

/// Adjusts a sequence of bars for a single symbol backwards in time.
///
/// Precondition: `bars` are for a single instrument, sorted by `ts_ns` ascending.
///
/// # Errors
/// [`Error::Data`] if an action is invalid, arithmetic overflows, or adjusted bars fail validation.
pub fn adjust_symbol_bars(bars: &[Bar], actions: &[CorporateAction]) -> Result<Vec<Bar>> {
    if bars.is_empty() || actions.is_empty() {
        return Ok(bars.to_vec());
    }

    for action in actions {
        if let Err(e) = action.validate() {
            return Err(Error::Data(e));
        }
    }

    // Sort actions by ex_date_ns ascending
    let mut sorted_actions = actions.to_vec();
    sorted_actions.sort_by_key(|a| a.ex_date_ns);

    // Group actions by ex_date_ns
    let mut actions_by_date: BTreeMap<i64, Vec<CorporateAction>> = BTreeMap::new();
    for a in sorted_actions {
        actions_by_date.entry(a.ex_date_ns).or_default().push(a);
    }

    // We walk backwards through bars (from newest to oldest).
    // As we cross an ex_date backwards (going to bars with ts_ns < ex_date), we compound
    // the price factor and volume factor.
    let mut adjusted_bars: Vec<Bar> = Vec::with_capacity(bars.len());
    let mut price_factor = Rational::ONE;
    let mut vol_factor = Rational::ONE;

    // Dates in descending order
    let mut pending_dates: Vec<(&i64, &Vec<CorporateAction>)> = actions_by_date.iter().rev().collect();

    for bar in bars.iter().rev() {
        // Apply any actions whose ex_date_ns is strictly greater than bar.ts_ns
        while let Some(&(ex_date_ns, acts)) = pending_dates.first() {
            if *ex_date_ns > bar.ts_ns {
                // We just stepped backwards across this ex_date!
                // Prior close is the close of the bar immediately before ex_date: which is this bar `bar`!
                // Apply splits first, then cash dividends
                for act in acts {
                    match act.kind {
                        ActionKind::Split => {
                            // Split: 2-for-1 (from: 1, to: 2).
                            // Prior prices are divided by (to / from), so multiplied by (from / to).
                            // Prior volumes are multiplied by (to / from).
                            let p_rat = Rational::new(i128::from(act.split_from), i128::from(act.split_to))
                                .map_err(|e| Error::Data(format!("split math error: {e}")))?;
                            let v_rat = Rational::new(i128::from(act.split_to), i128::from(act.split_from))
                                .map_err(|e| Error::Data(format!("split math error: {e}")))?;
                            price_factor = price_factor
                                .mul(p_rat)
                                .map_err(|e| Error::Data(format!("cumulative price factor overflow: {e}")))?;
                            vol_factor = vol_factor.mul(v_rat).map_err(|e| {
                                Error::Data(format!("cumulative volume factor overflow: {e}"))
                            })?;
                        }
                        ActionKind::Dividend => {
                            // Dividend: D
                            // Prior close price is bar.close (which is on the eve of ex_date).
                            // Notice: if a split happened on the same ex-date, the dividend amount is
                            // evaluated against the unadjusted prior close.
                            let prior_close = bar.close;
                            if act.dividend_amount >= prior_close {
                                return Err(Error::Data(format!(
                                    "dividend {} on {} exceeds or equals prior close price {}",
                                    act.dividend_amount, act.symbol, prior_close
                                )));
                            }
                            let num = prior_close.raw() - act.dividend_amount.raw();
                            let den = prior_close.raw();
                            let div_rat = Rational::new(i128::from(num), i128::from(den))
                                .map_err(|e| Error::Data(format!("dividend math error: {e}")))?;
                            price_factor = price_factor
                                .mul(div_rat)
                                .map_err(|e| Error::Data(format!("cumulative price factor overflow: {e}")))?;
                            // Volume is not adjusted for cash dividends.
                        }
                    }
                }
                pending_dates.remove(0);
            } else {
                break;
            }
        }

        // Adjust the bar prices and volume
        let open = price_factor
            .apply_to_price(bar.open)
            .map_err(|e| Error::Data(format!("failed to adjust open price: {e}")))?;
        let close = price_factor
            .apply_to_price(bar.close)
            .map_err(|e| Error::Data(format!("failed to adjust close price: {e}")))?;
        let raw_high = price_factor
            .apply_to_price(bar.high)
            .map_err(|e| Error::Data(format!("failed to adjust high price: {e}")))?;
        let raw_low = price_factor
            .apply_to_price(bar.low)
            .map_err(|e| Error::Data(format!("failed to adjust low price: {e}")))?;

        // Guarantee OHLC bracketing invariant
        let high = raw_high.max(open).max(close);
        let low = raw_low.min(open).min(close);
        let volume = vol_factor
            .apply_to_volume(bar.volume)
            .map_err(|e| Error::Data(format!("failed to adjust volume: {e}")))?;

        adjusted_bars.push(Bar {
            ts_ns: bar.ts_ns,
            known_at_ns: bar.known_at_ns,
            instrument: bar.instrument,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    // Since we walked backwards, reverse to restore ascending order
    adjusted_bars.reverse();

    if let Some(err) = validate_bars(&adjusted_bars) {
        return Err(Error::Data(format!("adjusted bars failed validation: {err}")));
    }

    Ok(adjusted_bars)
}

/// Derives an adjusted dataset from a base dataset and a collection of corporate actions.
///
/// # Errors
/// [`Error::Data`] if adjusting any instrument fails, merging bars fails, or derived bars fail validation.
pub fn adjust_dataset(base: &Dataset, actions: &[CorporateAction]) -> Result<Dataset> {
    if actions.is_empty() {
        return Ok(base.clone());
    }

    // Group actions by symbol
    let mut actions_by_symbol: HashMap<&str, Vec<CorporateAction>> = HashMap::new();
    for a in actions {
        actions_by_symbol.entry(&a.symbol).or_default().push(a.clone());
    }

    // Group bars by instrument
    let mut bars_by_inst: Vec<Vec<Bar>> = vec![Vec::new(); base.symbols.symbols.len()];
    for b in &base.bars {
        let idx = b.instrument as usize;
        if idx < bars_by_inst.len() {
            bars_by_inst[idx].push(*b);
        }
    }

    // Adjust bars for each instrument
    let mut adjusted_by_inst: Vec<Vec<Bar>> = Vec::with_capacity(bars_by_inst.len());
    for (inst, inst_bars) in bars_by_inst.into_iter().enumerate() {
        let symbol = &base.symbols.symbols[inst];
        if let Some(sym_actions) = actions_by_symbol.get(symbol.as_str()) {
            let adj = adjust_symbol_bars(&inst_bars, sym_actions)?;
            adjusted_by_inst.push(adj);
        } else {
            adjusted_by_inst.push(inst_bars);
        }
    }

    // Merge adjusted bars back into sorted (ts_ns, instrument) order
    let mut merged_bars: Vec<Bar> = Vec::with_capacity(base.bars.len());
    let mut indices = vec![0usize; adjusted_by_inst.len()];
    let total_bars: usize = adjusted_by_inst.iter().map(Vec::len).sum();

    while merged_bars.len() < total_bars {
        let mut min_val: Option<(i64, InstrumentId, usize)> = None;
        for (inst, bar_list) in adjusted_by_inst.iter().enumerate() {
            let idx = indices[inst];
            if idx < bar_list.len() {
                let bar = &bar_list[idx];
                match min_val {
                    None => min_val = Some((bar.ts_ns, bar.instrument, inst)),
                    Some((min_ts, min_inst, _)) => {
                        if bar.ts_ns < min_ts || (bar.ts_ns == min_ts && bar.instrument < min_inst) {
                            min_val = Some((bar.ts_ns, bar.instrument, inst));
                        }
                    }
                }
            }
        }

        if let Some((_, _, inst)) = min_val {
            merged_bars.push(adjusted_by_inst[inst][indices[inst]]);
            indices[inst] += 1;
        } else {
            break;
        }
    }

    if let Some(err) = validate_bars(&merged_bars) {
        return Err(Error::Data(format!("merged adjusted bars failed validation: {err}")));
    }

    let note = format!("Adjusted for {} corporate action(s)", actions.len());
    crate::dataset::derive_dataset(base, merged_bars, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{CsvOptions, DatasetInfo, SymbolTable};

    fn p(v: i64) -> Price {
        Price::from_units(v).unwrap()
    }

    fn q(v: i64) -> Qty {
        Qty::from_units(v).unwrap()
    }

    #[test]
    fn test_split_backward_adjustment() {
        // Stock trades at $100 before split, splits 2:1 on day 3, then trades at $50.
        let bars = vec![
            Bar {
                ts_ns: 1_000,
                known_at_ns: 1_000,
                instrument: 0,
                open: p(100),
                high: p(102),
                low: p(98),
                close: p(100),
                volume: q(1000),
            },
            Bar {
                ts_ns: 2_000,
                known_at_ns: 2_000,
                instrument: 0,
                open: p(100),
                high: p(105),
                low: p(99),
                close: p(102),
                volume: q(1200),
            },
            Bar {
                ts_ns: 3_000,
                known_at_ns: 3_000,
                instrument: 0,
                open: p(51),
                high: p(52),
                low: p(50),
                close: p(51),
                volume: q(2400),
            },
        ];

        let actions = vec![CorporateAction {
            symbol: "AAPL".to_string(),
            ex_date_ns: 3_000,
            kind: ActionKind::Split,
            split_from: 1,
            split_to: 2,
            dividend_amount: Price::ZERO,
        }];

        let adj = adjust_symbol_bars(&bars, &actions).unwrap();
        assert_eq!(adj.len(), 3);
        // Day 1 & 2 prices should be halved, volumes doubled
        assert_eq!(adj[0].open, p(50));
        assert_eq!(adj[0].high, p(51));
        assert_eq!(adj[0].low, p(49));
        assert_eq!(adj[0].close, p(50));
        assert_eq!(adj[0].volume, q(2000));

        assert_eq!(adj[1].open, p(50));
        assert_eq!(adj[1].close, p(51));
        assert_eq!(adj[1].volume, q(2400));

        // Day 3 (on/after split) prices and volumes unchanged
        assert_eq!(adj[2].open, p(51));
        assert_eq!(adj[2].close, p(51));
        assert_eq!(adj[2].volume, q(2400));
    }

    #[test]
    fn test_dividend_backward_adjustment() {
        // Stock closes at $100 on day 1.
        // Pays $2.00 dividend on day 2 (ex_date = 2000).
        // Adjustment factor = (100 - 2) / 100 = 0.98.
        let bars = vec![
            Bar {
                ts_ns: 1_000,
                known_at_ns: 1_000,
                instrument: 0,
                open: p(100),
                high: p(105),
                low: p(95),
                close: p(100),
                volume: q(1000),
            },
            Bar {
                ts_ns: 2_000,
                known_at_ns: 2_000,
                instrument: 0,
                open: p(98),
                high: p(100),
                low: p(97),
                close: p(99),
                volume: q(1500),
            },
        ];

        let actions = vec![CorporateAction {
            symbol: "AAPL".to_string(),
            ex_date_ns: 2_000,
            kind: ActionKind::Dividend,
            split_from: 0,
            split_to: 0,
            dividend_amount: p(2),
        }];

        let adj = adjust_symbol_bars(&bars, &actions).unwrap();
        assert_eq!(adj.len(), 2);
        // Day 1 prices scaled by 0.98: 100 -> 98.00
        assert_eq!(adj[0].open, p(98));
        assert_eq!(adj[0].close, p(98));
        // Volume unchanged for cash dividend
        assert_eq!(adj[0].volume, q(1000));

        // Day 2 unchanged
        assert_eq!(adj[1].open, p(98));
        assert_eq!(adj[1].close, p(99));
        assert_eq!(adj[1].volume, q(1500));
    }

    #[test]
    fn test_adjust_dataset_clears_split_outliers() {
        // Create 30 bars for a stock that splits 4:1 on day 15.
        // Before split: ~400. After split: ~100.
        let mut bars = Vec::new();
        for day in 1..=14 {
            let offset = (day % 3) - 1; // -1, 0, 1 price oscillation so MAD > 0
            bars.push(Bar {
                ts_ns: day * 86_400 * 1_000_000_000,
                known_at_ns: day * 86_400 * 1_000_000_000,
                instrument: 0,
                open: p(400 + offset),
                high: p(405 + offset),
                low: p(395 + offset),
                close: p(400 + offset),
                volume: q(1000),
            });
        }
        for day in 15..=30 {
            let offset = (day % 3) - 1;
            bars.push(Bar {
                ts_ns: day * 86_400 * 1_000_000_000,
                known_at_ns: day * 86_400 * 1_000_000_000,
                instrument: 0,
                open: p(100 + offset),
                high: p(102 + offset),
                low: p(98 + offset),
                close: p(100 + offset),
                volume: q(4000),
            });
        }

        let symbols = SymbolTable { symbols: vec!["TEST".to_string()] };
        let cfg = CsvOptions { outlier_z: 5.0, ..CsvOptions::default() };
        let report_unadj = crate::dataset::assess_quality(&bars, &symbols, &cfg);
        // Unadjusted dataset has a huge return outlier on day 15 (-75%)
        assert_eq!(report_unadj.symbols[0].outlier_returns, 1);

        let actions = vec![CorporateAction {
            symbol: "TEST".to_string(),
            ex_date_ns: 15 * 86_400 * 1_000_000_000,
            kind: ActionKind::Split,
            split_from: 1,
            split_to: 4,
            dividend_amount: Price::ZERO,
        }];

        let base_dataset = Dataset {
            bars: bars.clone(),
            symbols: symbols.clone(),
            info: DatasetInfo::default(),
            quality: report_unadj,
        };

        let adjusted = adjust_dataset(&base_dataset, &actions).unwrap();
        // Days 1..=14 prices adjusted to ~100: no return outlier anymore!
        let report_adj = crate::dataset::assess_quality(&adjusted.bars, &adjusted.symbols, &cfg);
        assert_eq!(report_adj.symbols[0].outlier_returns, 0);
        assert_eq!(adjusted.bars[0].close, p(100));
        assert_eq!(adjusted.bars[14].close, p(99));
    }
}
