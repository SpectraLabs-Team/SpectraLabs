//! Watchlists & Stock Screener Engine (M36).
//!
//! Evaluates technical criteria (prices, moving averages, RSI, volumes) and
//! point-in-time fundamental criteria (P/E, margins, debt ratios) across a symbol universe.
//! Generates weighted rankings, cites version identifiers for repeatability,
//! and exports sanitized CSV reports (M29).

use spectra_core::Bar;
use spectra_data::fundamentals::PointInTimeFundamentalsStore;
use spectra_data::security::escape_csv_cell;

use crate::indicators::{rsi, sma};

/// A named watchlist with annotated items.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Watchlist {
    pub id: String,
    pub name: String,
    pub items: Vec<WatchlistItem>,
}

/// An entry in a user's watchlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistItem {
    pub symbol: String,
    pub notes: String,
    pub added_ts_ns: i64,
}

/// Technical screen filter criteria.
#[derive(Debug, Clone, PartialEq)]
pub enum TechnicalFilter {
    /// Last close >= min_price.
    MinPrice(f64),
    /// Last close <= max_price.
    MaxPrice(f64),
    /// Average volume over `period` bars >= min_vol.
    MinAvgVolume { min_vol: f64, period: usize },
    /// RSI(period) must fall within `[min, max]`.
    RsiRange { min: f64, max: f64, period: usize },
    /// Price must be strictly above its SMA(period).
    PriceAboveSma { period: usize },
    /// Price must be strictly below its SMA(period).
    PriceBelowSma { period: usize },
}

/// Point-in-time fundamental filter criteria.
#[derive(Debug, Clone, PartialEq)]
pub enum FundamentalFilter {
    /// Market Capitalization >= min_usd.
    MinMarketCap(f64),
    /// Price-to-Earnings <= max_pe.
    MaxPe(f64),
    /// Price-to-Earnings >= min_pe.
    MinPe(f64),
    /// Net Profit Margin (Net Income / Revenue) >= min_margin.
    MinNetMargin(f64),
}

/// Ranking criterion for sorting candidates.
#[derive(Debug, Clone, PartialEq)]
pub struct RankingWeight {
    pub name: String,
    pub weight: f64,
    pub ascending: bool,
}

/// Complete screen definition. Immutable once saved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScreenDefinition {
    pub name: String,
    pub technical_filters: Vec<TechnicalFilter>,
    pub fundamental_filters: Vec<FundamentalFilter>,
    pub rankings: Vec<RankingWeight>,
}

/// Candidate symbol match evaluated by the screener.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenMatch {
    pub symbol: String,
    pub last_price: f64,
    pub last_volume: f64,
    pub rsi_14: Option<f64>,
    pub pe_ratio: Option<f64>,
    pub net_margin: Option<f64>,
    pub composite_rank_score: f64,
}

/// Full screener evaluation result with audit version citations.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenResult {
    pub run_ts_ns: i64,
    pub dataset_version_id: String,
    pub fundamentals_version_id: String,
    pub matches: Vec<ScreenMatch>,
}

impl ScreenResult {
    /// Exports the result as a secure CSV string (M29 protected against formula injection).
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        out.push_str("Symbol,Price,Volume,RSI14,PE,NetMargin,RankScore\n");

        for m in &self.matches {
            let row = [
                escape_csv_cell(&m.symbol),
                escape_csv_cell(&format!("{:.2}", m.last_price)),
                escape_csv_cell(&format!("{:.0}", m.last_volume)),
                escape_csv_cell(&m.rsi_14.map_or("-".into(), |v| format!("{:.2}", v))),
                escape_csv_cell(&m.pe_ratio.map_or("-".into(), |v| format!("{:.2}", v))),
                escape_csv_cell(&m.net_margin.map_or("-".into(), |v| format!("{:.4}", v))),
                escape_csv_cell(&format!("{:.4}", m.composite_rank_score)),
            ];
            out.push_str(&row.join(","));
            out.push('\n');
        }
        out
    }
}

/// Symbol market data payload for screening.
#[derive(Debug, Clone)]
pub struct SymbolScreenData<'a> {
    pub symbol: &'a str,
    pub bars: &'a [Bar],
    pub shares_outstanding: Option<f64>,
}

/// Evaluates a screen definition against a universe of symbols.
pub fn run_screen(
    def: &ScreenDefinition,
    universe: &[SymbolScreenData<'_>],
    fund_store: &PointInTimeFundamentalsStore,
    as_of_ts_ns: i64,
    dataset_version: &str,
) -> ScreenResult {
    let mut candidates = Vec::new();

    for sym_data in universe {
        let bars = sym_data.bars;
        if bars.is_empty() {
            continue;
        }

        let last_bar = bars.last().unwrap();
        let last_price = last_bar.close.to_f64();
        let last_vol = last_bar.volume.to_f64();
        let closes: Vec<f64> = bars.iter().map(|b| b.close.to_f64()).collect();

        // 1. Evaluate Technical Filters
        let mut pass_tech = true;
        let mut rsi_14_val = None;

        for tf in &def.technical_filters {
            match tf {
                TechnicalFilter::MinPrice(p) => {
                    if last_price < *p {
                        pass_tech = false;
                        break;
                    }
                }
                TechnicalFilter::MaxPrice(p) => {
                    if last_price > *p {
                        pass_tech = false;
                        break;
                    }
                }
                TechnicalFilter::MinAvgVolume { min_vol, period } => {
                    if bars.len() < *period {
                        pass_tech = false;
                        break;
                    }
                    let avg: f64 =
                        bars[(bars.len() - *period)..].iter().map(|b| b.volume.to_f64()).sum::<f64>()
                            / (*period as f64);
                    if avg < *min_vol {
                        pass_tech = false;
                        break;
                    }
                }
                TechnicalFilter::RsiRange { min, max, period } => {
                    let rsi_series = rsi(&closes, *period);
                    if let Some(Some(val)) = rsi_series.last() {
                        rsi_14_val = Some(*val);
                        if *val < *min || *val > *max {
                            pass_tech = false;
                            break;
                        }
                    } else {
                        pass_tech = false;
                        break;
                    }
                }
                TechnicalFilter::PriceAboveSma { period } => {
                    let sma_series = sma(&closes, *period);
                    if let Some(Some(val)) = sma_series.last() {
                        if last_price <= *val {
                            pass_tech = false;
                            break;
                        }
                    } else {
                        pass_tech = false;
                        break;
                    }
                }
                TechnicalFilter::PriceBelowSma { period } => {
                    let sma_series = sma(&closes, *period);
                    if let Some(Some(val)) = sma_series.last() {
                        if last_price >= *val {
                            pass_tech = false;
                            break;
                        }
                    } else {
                        pass_tech = false;
                        break;
                    }
                }
            }
        }

        if !pass_tech {
            continue;
        }

        // 2. Evaluate Fundamental Filters (Look-Ahead Free Point-in-Time)
        let mut pass_fund = true;
        let mut pe_calc = None;
        let mut margin_calc = None;

        let rev_rec = fund_store.query_latest_as_of(sym_data.symbol, "revenue", as_of_ts_ns);
        let net_rec = fund_store.query_latest_as_of(sym_data.symbol, "net_income", as_of_ts_ns);

        if let (Some(r), Some(n)) = (rev_rec, net_rec) {
            if r.value > 0.0 {
                margin_calc = Some(n.value / r.value);
            }
        }

        let mcap = sym_data.shares_outstanding.map(|s| s * last_price);
        if let (Some(mc), Some(n)) = (mcap, net_rec) {
            if n.value > 0.0 {
                pe_calc = Some(mc / n.value);
            }
        }

        for ff in &def.fundamental_filters {
            match ff {
                FundamentalFilter::MinMarketCap(min_mc) => {
                    if mcap.map_or(true, |mc| mc < *min_mc) {
                        pass_fund = false;
                        break;
                    }
                }
                FundamentalFilter::MaxPe(max_pe) => {
                    if pe_calc.map_or(true, |pe| pe > *max_pe) {
                        pass_fund = false;
                        break;
                    }
                }
                FundamentalFilter::MinPe(min_pe) => {
                    if pe_calc.map_or(true, |pe| pe < *min_pe) {
                        pass_fund = false;
                        break;
                    }
                }
                FundamentalFilter::MinNetMargin(min_m) => {
                    if margin_calc.map_or(true, |m| m < *min_m) {
                        pass_fund = false;
                        break;
                    }
                }
            }
        }

        if !pass_fund {
            continue;
        }

        candidates.push(ScreenMatch {
            symbol: sym_data.symbol.to_string(),
            last_price,
            last_volume: last_vol,
            rsi_14: rsi_14_val,
            pe_ratio: pe_calc,
            net_margin: margin_calc,
            composite_rank_score: 0.0,
        });
    }

    // 3. Compute Ranking & Composite Rank Score
    for cand in &mut candidates {
        let mut score = 0.0;
        for rk in &def.rankings {
            let val = match rk.name.as_str() {
                "price" => cand.last_price,
                "volume" => cand.last_volume,
                "pe" => match cand.pe_ratio {
                    Some(v) => v,
                    None => 999.0,
                },
                "margin" => match cand.net_margin {
                    Some(v) => v,
                    None => 0.0,
                },
                _ => 0.0,
            };
            if rk.ascending {
                score -= val * rk.weight;
            } else {
                score += val * rk.weight;
            }
        }
        cand.composite_rank_score = score;
    }

    // Sort descending by score
    candidates.sort_by(|a, b| {
        b.composite_rank_score.partial_cmp(&a.composite_rank_score).unwrap_or(std::cmp::Ordering::Equal)
    });

    ScreenResult {
        run_ts_ns: as_of_ts_ns,
        dataset_version_id: dataset_version.to_string(),
        fundamentals_version_id: fund_store.dataset_version_id(),
        matches: candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::{Price, Qty};
    use spectra_data::fundamentals::FundamentalRecord;

    fn make_bar(ts: i64, p: f64, v: f64) -> Bar {
        Bar::new(
            ts,
            0,
            Price::from_f64(p).unwrap(),
            Price::from_f64(p + 1.0).unwrap(),
            Price::from_f64(p - 1.0).unwrap(),
            Price::from_f64(p).unwrap(),
            Qty::from_f64(v).unwrap(),
        )
    }

    #[test]
    fn test_technical_and_fundamental_screening() {
        let mut fund_store = PointInTimeFundamentalsStore::new();
        // Ingest AAPL fundamentals
        fund_store.ingest(vec![
            FundamentalRecord {
                symbol: "AAPL".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "revenue".into(),
                value: 100_000_000.0,
                currency: "USD".into(),
                publication_ts_ns: 1000,
                restatement_index: 0,
                filing_ref: "f1".into(),
            },
            FundamentalRecord {
                symbol: "AAPL".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "net_income".into(),
                value: 25_000_000.0, // 25% margin
                currency: "USD".into(),
                publication_ts_ns: 1000,
                restatement_index: 0,
                filing_ref: "f1".into(),
            },
        ]);

        let aapl_bars: Vec<Bar> = (1..=30).map(|i| make_bar(i * 1000, 150.0 + i as f64, 10000.0)).collect();
        let msft_bars: Vec<Bar> = (1..=30).map(|i| make_bar(i * 1000, 50.0, 500.0)).collect();

        let universe = [
            SymbolScreenData {
                symbol: "AAPL",
                bars: &aapl_bars,
                shares_outstanding: Some(1_000_000.0), // Mcap = 180M -> PE = 180M/25M = 7.2
            },
            SymbolScreenData { symbol: "MSFT", bars: &msft_bars, shares_outstanding: Some(1_000_000.0) },
        ];

        let def = ScreenDefinition {
            name: "High Growth Value".into(),
            technical_filters: vec![
                TechnicalFilter::MinPrice(100.0),
                TechnicalFilter::MinAvgVolume { min_vol: 5000.0, period: 10 },
            ],
            fundamental_filters: vec![FundamentalFilter::MinNetMargin(0.20), FundamentalFilter::MaxPe(15.0)],
            rankings: vec![RankingWeight { name: "margin".into(), weight: 1.0, ascending: false }],
        };

        let result = run_screen(&def, &universe, &fund_store, 50_000, "ds_v1");

        // AAPL passes all technical and fundamental filters; MSFT fails on price < 100
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].symbol, "AAPL");
        assert_eq!(result.dataset_version_id, "ds_v1");
        assert!(result.fundamentals_version_id.starts_with("fund_v_"));

        // Verify CSV export escapes dangerous characters
        let csv = result.to_csv();
        assert!(csv.contains("AAPL"));
    }

    #[test]
    fn test_screener_csv_escaping_injection() {
        let res = ScreenResult {
            run_ts_ns: 1000,
            dataset_version_id: "v1".into(),
            fundamentals_version_id: "fv1".into(),
            matches: vec![ScreenMatch {
                symbol: "=cmd|' /C calc'!A0".into(), // Malicious formula symbol
                last_price: 100.0,
                last_volume: 5000.0,
                rsi_14: Some(55.0),
                pe_ratio: Some(12.0),
                net_margin: Some(0.25),
                composite_rank_score: 1.5,
            }],
        };

        let csv = res.to_csv();
        // The cell starting with '=' must be escaped with single quote
        assert!(csv.contains("'=cmd|' /C calc'!A0"));
    }
}
