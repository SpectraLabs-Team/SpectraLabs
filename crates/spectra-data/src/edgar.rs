//! SEC EDGAR fundamentals (M34, provider decision DL-1).
//!
//! Turns the two EDGAR documents the fundamentals refresh needs into store input:
//!
//!  * [`TickerMap::parse`]: `company_tickers.json`, to find a symbol's CIK.
//!  * [`parse_company_facts`]: a company's XBRL Company Facts
//!    (`/api/xbrl/companyfacts/CIK##########.json`), as [`FundamentalRecord`]s with the time each
//!    value became known and its revision number.
//!
//! Parsing only: fetching is done by the refresh job in `spectra-api`, so this crate stays
//! standard-library only. The rules (known-at time, periods, revisions, metric concepts) are in
//! `docs/models/m34-fundamentals-plan.md`, section 7.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::RangeInclusive;

use spectra_core::Json;

use crate::fundamentals::FundamentalRecord;
use crate::time::days_from_civil;

const NS_PER_DAY: i64 = 86_400 * 1_000_000_000;
const NS_PER_HOUR: i64 = 3_600 * 1_000_000_000;

/// A value is known from this hour (UTC) on the day after its filing date: EDGAR gives the filing
/// date but not the acceptance time, and accepts filings until 22:00 ET.
pub const KNOWN_AT_HOUR_UTC: i64 = 12;

/// Durations of this many days are quarters; other durations except years are year-to-date
/// figures, which are dropped.
pub const QUARTER_DAYS: RangeInclusive<i64> = 80..=100;

/// Durations of this many days are fiscal years.
pub const YEAR_DAYS: RangeInclusive<i64> = 350..=380;

/// One metric and the XBRL concepts that report it, in order of preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricConcepts {
    /// Store metric name.
    pub metric: &'static str,
    /// XBRL taxonomy: `us-gaap` or `dei`.
    pub taxonomy: &'static str,
    /// Concepts, most preferred first.
    pub concepts: &'static [&'static str],
    /// EDGAR unit key, e.g. `USD`, `USD/shares`, `shares`.
    pub unit: &'static str,
}

/// The metrics loaded from EDGAR (section 7 of the M34 plan).
pub const METRICS: &[MetricConcepts] = &[
    MetricConcepts {
        metric: "revenue",
        taxonomy: "us-gaap",
        concepts: &["Revenues", "RevenueFromContractWithCustomerExcludingAssessedTax", "SalesRevenueNet"],
        unit: "USD",
    },
    MetricConcepts { metric: "net_income", taxonomy: "us-gaap", concepts: &["NetIncomeLoss"], unit: "USD" },
    MetricConcepts {
        metric: "operating_income",
        taxonomy: "us-gaap",
        concepts: &["OperatingIncomeLoss"],
        unit: "USD",
    },
    MetricConcepts { metric: "gross_profit", taxonomy: "us-gaap", concepts: &["GrossProfit"], unit: "USD" },
    MetricConcepts { metric: "total_assets", taxonomy: "us-gaap", concepts: &["Assets"], unit: "USD" },
    MetricConcepts {
        metric: "total_liabilities",
        taxonomy: "us-gaap",
        concepts: &["Liabilities"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "stockholders_equity",
        taxonomy: "us-gaap",
        concepts: &["StockholdersEquity"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "current_assets",
        taxonomy: "us-gaap",
        concepts: &["AssetsCurrent"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "current_liabilities",
        taxonomy: "us-gaap",
        concepts: &["LiabilitiesCurrent"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "cash_and_equivalents",
        taxonomy: "us-gaap",
        concepts: &["CashAndCashEquivalentsAtCarryingValue"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "long_term_debt",
        taxonomy: "us-gaap",
        concepts: &["LongTermDebtNoncurrent", "LongTermDebt"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "operating_cash_flow",
        taxonomy: "us-gaap",
        concepts: &["NetCashProvidedByUsedInOperatingActivities"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "capital_expenditures",
        taxonomy: "us-gaap",
        concepts: &["PaymentsToAcquirePropertyPlantAndEquipment"],
        unit: "USD",
    },
    MetricConcepts {
        metric: "eps_diluted",
        taxonomy: "us-gaap",
        concepts: &["EarningsPerShareDiluted"],
        unit: "USD/shares",
    },
    MetricConcepts {
        metric: "shares_outstanding",
        taxonomy: "dei",
        concepts: &["EntityCommonStockSharesOutstanding"],
        unit: "shares",
    },
];

/// Why an EDGAR document could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgarError {
    /// The text is not valid JSON.
    Json(String),
    /// The JSON is valid but not shaped like the EDGAR document.
    Shape(String),
}

impl fmt::Display for EdgarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(why) => write!(f, "EDGAR response is not valid JSON: {why}"),
            Self::Shape(why) => write!(f, "EDGAR response has an unexpected shape: {why}"),
        }
    }
}

impl std::error::Error for EdgarError {}

/// The time a value filed on `filed` (`YYYY-MM-DD`) is treated as known: [`KNOWN_AT_HOUR_UTC`] on
/// the following day, in nanoseconds since the Unix epoch. `None` if `filed` is not a valid date.
pub fn known_at_ns(filed: &str) -> Option<i64> {
    Some((day_number(filed)? + 1) * NS_PER_DAY + KNOWN_AT_HOUR_UTC * NS_PER_HOUR)
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days_in_month).contains(&d).then(|| days_from_civil(y, m, d))
}

/// EDGAR's ticker to CIK map (`company_tickers.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickerMap {
    by_ticker: HashMap<String, (u64, Option<String>)>,
}

impl TickerMap {
    /// Parses `company_tickers.json`: an object of `{"cik_str": 320193, "ticker": "AAPL",
    /// "title": "Apple Inc."}` entries.
    ///
    /// # Errors
    /// [`EdgarError`] if the text is not JSON or an entry lacks its CIK or ticker.
    pub fn parse(text: &str) -> Result<Self, EdgarError> {
        let doc = Json::parse(text).map_err(|e| EdgarError::Json(e.to_string()))?;
        let entries =
            doc.entries().map_err(|_| EdgarError::Shape("the ticker map is not an object".into()))?;
        let mut by_ticker = HashMap::with_capacity(entries.len());
        for (key, entry) in entries {
            let shape = |what: &str| EdgarError::Shape(format!("ticker entry {key} has no {what}"));
            let cik = entry.find("cik_str").and_then(|v| v.as_int().ok()).ok_or_else(|| shape("cik_str"))?;
            let cik = u64::try_from(cik).map_err(|_| shape("valid cik_str"))?;
            let ticker = entry.find("ticker").and_then(|v| v.as_str().ok()).ok_or_else(|| shape("ticker"))?;
            let title = entry.find("title").and_then(|v| v.as_str().ok()).map(str::to_string);
            by_ticker.entry(ticker.to_ascii_uppercase()).or_insert((cik, title));
        }
        Ok(Self { by_ticker })
    }

    /// The CIK of `ticker` (any case), if EDGAR lists it.
    pub fn cik(&self, ticker: &str) -> Option<u64> {
        self.by_ticker.get(&ticker.to_ascii_uppercase()).map(|(cik, _)| *cik)
    }

    /// The company name EDGAR lists for `ticker`.
    pub fn title(&self, ticker: &str) -> Option<&str> {
        self.by_ticker.get(&ticker.to_ascii_uppercase()).and_then(|(_, title)| title.as_deref())
    }

    /// Number of tickers in the map.
    pub fn len(&self) -> usize {
        self.by_ticker.len()
    }

    /// True if the map has no tickers.
    pub fn is_empty(&self) -> bool {
        self.by_ticker.is_empty()
    }
}

/// The Company Facts path for a CIK, e.g. `/api/xbrl/companyfacts/CIK0000320193.json`.
pub fn company_facts_path(cik: u64) -> String {
    format!("/api/xbrl/companyfacts/CIK{cik:010}.json")
}

/// What a value measures over: a quarter, a fiscal year, or a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Span {
    Quarter,
    Year,
    Instant,
}

/// One reported value as EDGAR lists it.
#[derive(Debug, Clone)]
struct Entry {
    start: Option<String>,
    end: String,
    value: f64,
    accn: String,
    filed: String,
    fy: Option<i64>,
    fp: Option<String>,
}

/// Parses a company's Company Facts into store records for `symbol`.
///
/// For every metric in [`METRICS`], each value is identified by its own dates; its period label
/// comes from the first filing in which it was the filing's own period (values only ever seen as
/// comparatives are dropped); its revisions are the filings that changed it, in filing order, each
/// known from [`known_at_ns`] of its filing date. Within one filing, a value reported twice with
/// different numbers is ambiguous and that filing is ignored for it. Records come back sorted by
/// metric, period and revision.
///
/// # Errors
/// [`EdgarError`] if the text is not JSON or has no `facts` object.
pub fn parse_company_facts(text: &str, symbol: &str) -> Result<Vec<FundamentalRecord>, EdgarError> {
    let doc = Json::parse(text).map_err(|e| EdgarError::Json(e.to_string()))?;
    let facts = doc.find("facts").ok_or_else(|| EdgarError::Shape("no facts object".into()))?;
    let symbol = symbol.trim().to_ascii_uppercase();
    let mut out = Vec::new();
    for spec in METRICS {
        out.extend(metric_records(facts, spec, &symbol));
    }
    out.sort_by(|a, b| {
        (&a.metric, &a.period, a.restatement_index).cmp(&(&b.metric, &b.period, b.restatement_index))
    });
    Ok(out)
}

/// The entries of one metric, merged across its concepts: for the same value in the same filing,
/// the more preferred concept wins.
fn metric_entries(facts: &Json, spec: &MetricConcepts) -> Vec<(Span, Entry)> {
    let mut seen: HashMap<(Option<String>, String, String), usize> = HashMap::new();
    let mut entries: Vec<(Span, Entry)> = Vec::new();
    let mut conflicted = Vec::new();
    for concept in spec.concepts {
        let Some(list) = facts
            .find(spec.taxonomy)
            .and_then(|t| t.find(concept))
            .and_then(|c| c.find("units"))
            .and_then(|u| u.find(spec.unit))
            .and_then(|l| l.items().ok())
        else {
            continue;
        };
        let mut from_this_concept: HashMap<(Option<String>, String, String), usize> = HashMap::new();
        for item in list {
            let Some((span, entry)) = read_entry(item) else { continue };
            let key = (entry.start.clone(), entry.end.clone(), entry.accn.clone());
            if let Some(&i) = from_this_concept.get(&key) {
                // The same value twice in one filing: fine if equal, ambiguous if not.
                if entries[i].1.value.to_bits() != entry.value.to_bits() {
                    conflicted.push(i);
                }
                continue;
            }
            if seen.contains_key(&key) {
                continue; // a more preferred concept already reported it in this filing
            }
            seen.insert(key.clone(), entries.len());
            from_this_concept.insert(key, entries.len());
            entries.push((span, entry));
        }
    }
    conflicted.sort_unstable();
    conflicted.dedup();
    for i in conflicted.into_iter().rev() {
        entries.remove(i);
    }
    entries
}

/// One EDGAR entry, if it has the fields needed and a kept span.
fn read_entry(item: &Json) -> Option<(Span, Entry)> {
    let text = |key: &str| item.find(key).and_then(|v| v.as_str().ok()).map(str::to_string);
    let end = text("end")?;
    let end_day = day_number(&end)?;
    let start = text("start");
    let span = match &start {
        None => Span::Instant,
        Some(s) => {
            let days = end_day - day_number(s)?;
            if QUARTER_DAYS.contains(&days) {
                Span::Quarter
            } else if YEAR_DAYS.contains(&days) {
                Span::Year
            } else {
                return None;
            }
        }
    };
    let filed = text("filed")?;
    day_number(&filed)?;
    let entry = Entry {
        start,
        end,
        value: item.find("val")?.as_double().ok()?,
        accn: text("accn")?,
        filed,
        fy: item.find("fy").and_then(|v| v.as_int().ok()),
        fp: text("fp"),
    };
    Some((span, entry))
}

/// Store records for one metric.
fn metric_records(facts: &Json, spec: &MetricConcepts, symbol: &str) -> Vec<FundamentalRecord> {
    let entries = metric_entries(facts, spec);

    // A filing's own period, per span: its latest end date. Earlier ones are comparatives.
    let mut own_end: HashMap<(&str, Span), &str> = HashMap::new();
    for (span, e) in &entries {
        let slot = own_end.entry((e.accn.as_str(), *span)).or_insert(e.end.as_str());
        if e.end.as_str() > *slot {
            *slot = e.end.as_str();
        }
    }

    // Group by the value's own dates; read each group in filing order.
    let mut groups: BTreeMap<(Span, Option<&str>, &str), Vec<&Entry>> = BTreeMap::new();
    for (span, e) in &entries {
        groups.entry((*span, e.start.as_deref(), e.end.as_str())).or_default().push(e);
    }
    let currency = if spec.unit.starts_with("USD") { "USD" } else { "" };

    let mut out = Vec::new();
    for ((span, _, end), mut list) in groups {
        list.sort_by(|a, b| (&a.filed, &a.accn).cmp(&(&b.filed, &b.accn)));
        let original = list.iter().find(|e| own_end.get(&(e.accn.as_str(), span)) == Some(&end));
        let Some((label, fiscal_year, fiscal_quarter)) = original.and_then(|e| period_label(span, e)) else {
            continue; // only ever a comparative, or the filing has no fiscal period
        };
        let mut last: Option<f64> = None;
        let mut revision = 0u32;
        for e in list {
            if last.is_some_and(|v| v.to_bits() == e.value.to_bits()) {
                continue;
            }
            let Some(known_at) = known_at_ns(&e.filed) else { continue };
            if last.is_some() {
                revision += 1;
            }
            last = Some(e.value);
            out.push(FundamentalRecord {
                symbol: symbol.to_string(),
                period: label.clone(),
                fiscal_year,
                fiscal_quarter,
                metric: spec.metric.to_string(),
                value: e.value,
                currency: currency.to_string(),
                publication_ts_ns: known_at,
                restatement_index: revision,
                filing_ref: e.accn.clone(),
            });
        }
    }
    out
}

/// The period label (`2024-Q1`, `2024-FY`), fiscal year and quarter (0 for a full year) of a value
/// from the filing that first reported it as its own period.
fn period_label(span: Span, original: &Entry) -> Option<(String, u16, u8)> {
    let fy = u16::try_from(original.fy?).ok()?;
    let fp = original.fp.as_deref()?;
    let quarter = match (span, fp) {
        (Span::Year, _) | (Span::Instant, "FY") => 0,
        (_, "Q1") => 1,
        (_, "Q2") => 2,
        (_, "Q3") => 3,
        // A 10-K that reports its fourth quarter on its own is the fourth quarter.
        (_, "Q4") | (Span::Quarter, "FY") => 4,
        _ => return None,
    };
    let label = if quarter == 0 { format!("{fy}-FY") } else { format!("{fy}-Q{quarter}") };
    Some((label, fy, quarter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_at_is_noon_utc_the_day_after_filing() {
        // 2024-02-02 is day 19755; the next day starts at 19756 * 86400 s = 1_706_918_400 s.
        assert_eq!(known_at_ns("2024-02-02"), Some((1_706_918_400 + 12 * 3600) * 1_000_000_000));
        assert_eq!(known_at_ns("2024-02-30"), None);
        assert_eq!(known_at_ns("2024-2-02"), None);
        assert_eq!(known_at_ns("not a date"), None);
        // A leap day is a valid filing date.
        assert!(known_at_ns("2024-02-29").is_some());
        assert_eq!(known_at_ns("2023-02-29"), None);
    }

    #[test]
    fn company_facts_path_pads_the_cik() {
        assert_eq!(company_facts_path(320_193), "/api/xbrl/companyfacts/CIK0000320193.json");
    }

    #[test]
    fn ticker_map_finds_ciks_in_any_case() {
        let map = TickerMap::parse(
            r#"{"0":{"cik_str":320193,"ticker":"AAPL","title":"Apple Inc."},
                "1":{"cik_str":789019,"ticker":"MSFT","title":"MICROSOFT CORP"}}"#,
        )
        .unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.cik("aapl"), Some(320_193));
        assert_eq!(map.title("MSFT"), Some("MICROSOFT CORP"));
        assert_eq!(map.cik("NOPE"), None);
        assert!(matches!(TickerMap::parse("[]"), Err(EdgarError::Shape(_))));
        assert!(matches!(TickerMap::parse("{"), Err(EdgarError::Json(_))));
        assert!(matches!(TickerMap::parse(r#"{"0":{"ticker":"X"}}"#), Err(EdgarError::Shape(_))));
    }

    #[test]
    fn company_facts_without_facts_is_refused() {
        assert!(matches!(parse_company_facts(r#"{"cik":1}"#, "X"), Err(EdgarError::Shape(_))));
        assert!(matches!(parse_company_facts("nope", "X"), Err(EdgarError::Json(_))));
        assert_eq!(parse_company_facts(r#"{"facts":{}}"#, "X").unwrap(), vec![]);
    }
}
