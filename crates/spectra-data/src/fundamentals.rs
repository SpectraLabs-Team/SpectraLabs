//! Fundamentals Data & Point-in-Time Store (M34).
//!
//! Stores company fundamentals with point-in-time publication dates so backtests
//! and screeners never use information that was not yet publicly known on the trade date.
//! Restatements are recorded as new revisions rather than overwriting historical values.

use spectra_core::sha256_hex;

/// Listing status of an instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ListingStatus {
    Active,
    Delisted,
    Suspended,
}

impl ListingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Delisted => "DELISTED",
            Self::Suspended => "SUSPENDED",
        }
    }
}

/// Symbol master entry with sector, industry, country and status history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolMetadata {
    pub symbol: String,
    pub company_name: String,
    pub sector: String,
    pub industry: String,
    pub exchange: String,
    pub country: String,
    pub status: ListingStatus,
    /// When this metadata record became effective (nanoseconds UTC).
    pub effective_from_ns: i64,
    /// When this status was superseded (None if currently active/current).
    pub effective_to_ns: Option<i64>,
}

/// Symbol master manager tracking current listings and historical status changes.
#[derive(Debug, Clone, Default)]
pub struct SymbolMaster {
    entries: Vec<SymbolMetadata>,
}

impl SymbolMaster {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Add a metadata record to the symbol master.
    pub fn add_entry(&mut self, entry: SymbolMetadata) {
        self.entries.push(entry);
    }

    /// Look up all active symbols as of a given timestamp.
    /// Excludes delisted or suspended symbols.
    pub fn active_symbols_at(&self, as_of_ns: i64) -> Vec<&SymbolMetadata> {
        self.entries
            .iter()
            .filter(|e| {
                e.status == ListingStatus::Active
                    && e.effective_from_ns <= as_of_ns
                    && e.effective_to_ns.is_none_or(|to| as_of_ns < to)
            })
            .collect()
    }

    /// Returns the full lifecycle history of a symbol, including past delistings.
    pub fn symbol_history(&self, symbol: &str) -> Vec<&SymbolMetadata> {
        self.entries.iter().filter(|e| e.symbol.eq_ignore_ascii_case(symbol)).collect()
    }

    /// Check whether a symbol is currently active as of `as_of_ns`.
    pub fn is_active_at(&self, symbol: &str, as_of_ns: i64) -> bool {
        self.entries.iter().any(|e| {
            e.symbol.eq_ignore_ascii_case(symbol)
                && e.status == ListingStatus::Active
                && e.effective_from_ns <= as_of_ns
                && e.effective_to_ns.is_none_or(|to| as_of_ns < to)
        })
    }
}

/// One fundamental data point with exact point-in-time publication tracking.
#[derive(Debug, Clone, PartialEq)]
pub struct FundamentalRecord {
    pub symbol: String,
    /// Period identifier, e.g. "2025-Q1", "2025-FY".
    pub period: String,
    /// Fiscal year.
    pub fiscal_year: u16,
    /// Fiscal quarter (1..=4) or 0 for Full Year.
    pub fiscal_quarter: u8,
    /// Metric name, e.g. "revenue", "net_income", "total_assets", "eps".
    pub metric: String,
    /// Reported value.
    pub value: f64,
    /// Currency code, e.g. "USD", "EUR", "INR".
    pub currency: String,
    /// Exact nanosecond timestamp when the filing became public (10-Q/10-K).
    pub publication_ts_ns: i64,
    /// Restatement revision index: 0 = initial filing, 1+ = restatement.
    pub restatement_index: u32,
    /// Filing identifier or SEC accession number.
    pub filing_ref: String,
}

/// A quarantined record that failed fundamentals data quality checks.
#[derive(Debug, Clone, PartialEq)]
pub struct QuarantinedFundamental {
    pub record: FundamentalRecord,
    pub reason: String,
    pub category: String,
}

/// Point-in-time fundamentals store providing look-ahead free access to company financial history.
#[derive(Debug, Clone, Default)]
pub struct PointInTimeFundamentalsStore {
    records: Vec<FundamentalRecord>,
    quarantine: Vec<QuarantinedFundamental>,
}

impl PointInTimeFundamentalsStore {
    pub fn new() -> Self {
        Self { records: Vec::new(), quarantine: Vec::new() }
    }

    /// Ingests records after running data-quality checks. Invalid rows are quarantined.
    pub fn ingest(&mut self, records: Vec<FundamentalRecord>) {
        for record in records {
            if let Some(err) = Self::check_quality(&record) {
                self.quarantine.push(QuarantinedFundamental { record, reason: err.1, category: err.0 });
            } else {
                self.records.push(record);
            }
        }
    }

    /// Returns data quality check errors: (Category, Reason) if invalid.
    fn check_quality(r: &FundamentalRecord) -> Option<(String, String)> {
        // 1. Check for impossible negative metrics
        let metric_lower = r.metric.to_ascii_lowercase();
        if (metric_lower == "revenue"
            || metric_lower == "total_assets"
            || metric_lower == "shares_outstanding")
            && r.value < 0.0
        {
            return Some((
                "IMPOSSIBLE_NEGATIVE".into(),
                format!("Metric {} cannot be negative (got {})", r.metric, r.value),
            ));
        }

        // 2. Check for NaN or Inf
        if !r.value.is_finite() {
            return Some(("NON_FINITE".into(), format!("Metric {} value is not finite", r.metric)));
        }

        None
    }

    /// Queries the value of `metric` for `symbol` and `period` as known on `as_of_ts_ns`.
    /// Returns the most recent revision published on or before `as_of_ts_ns`.
    ///
    /// Guarantees:
    /// - If queried before the initial filing was published, returns `None`.
    /// - If queried after initial filing but before restatement, returns the initial filing value.
    /// - If queried after restatement, returns the latest restated value.
    pub fn query_point_in_time(
        &self,
        symbol: &str,
        period: &str,
        metric: &str,
        as_of_ts_ns: i64,
    ) -> Option<&FundamentalRecord> {
        self.records
            .iter()
            .filter(|r| {
                r.symbol.eq_ignore_ascii_case(symbol)
                    && r.period.eq_ignore_ascii_case(period)
                    && r.metric.eq_ignore_ascii_case(metric)
                    && r.publication_ts_ns <= as_of_ts_ns
            })
            .max_by_key(|r| (r.publication_ts_ns, r.restatement_index))
    }

    /// Queries the latest known annual or quarterly value of `metric` for `symbol` as of `as_of_ts_ns`.
    pub fn query_latest_as_of(
        &self,
        symbol: &str,
        metric: &str,
        as_of_ts_ns: i64,
    ) -> Option<&FundamentalRecord> {
        self.records
            .iter()
            .filter(|r| {
                r.symbol.eq_ignore_ascii_case(symbol)
                    && r.metric.eq_ignore_ascii_case(metric)
                    && r.publication_ts_ns <= as_of_ts_ns
            })
            .max_by_key(|r| (r.publication_ts_ns, r.restatement_index))
    }

    /// Returns list of quarantined rows for dataset browser inspections.
    pub fn quarantined_records(&self) -> &[QuarantinedFundamental] {
        &self.quarantine
    }

    /// Returns total accepted records count.
    pub fn accepted_count(&self) -> usize {
        self.records.len()
    }

    /// Computes deterministic content-addressed version string.
    pub fn dataset_version_id(&self) -> String {
        let mut text = String::new();
        for r in &self.records {
            text.push_str(&format!(
                "{}:{}:{}:{}:{}:{};",
                r.symbol, r.period, r.metric, r.value, r.publication_ts_ns, r.restatement_index
            ));
        }
        format!("fund_v_{}", &sha256_hex(text.as_bytes())[..12])
    }
}

/// Interface for market data fundamentals providers.
pub trait FundamentalsProvider {
    /// Fetch fundamentals for the given symbol.
    ///
    /// # Errors
    /// Returns `Err(String)` if fundamentals cannot be fetched.
    fn fetch_fundamentals(&self, symbol: &str) -> Result<Vec<FundamentalRecord>, String>;
}

/// Test fundamentals provider with reproducible fixtures including restatements.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockFundamentalsProvider;

impl FundamentalsProvider for MockFundamentalsProvider {
    fn fetch_fundamentals(&self, symbol: &str) -> Result<Vec<FundamentalRecord>, String> {
        let mut list = Vec::new();
        let sym = symbol.to_uppercase();

        // 2024-Q1 initial filing published on 2024-04-15
        let ts_apr15 = 1_713_139_200_000_000_000i64;
        // 2024-Q1 restatement published on 2024-08-10 with 10-Q/A
        let ts_aug10 = 1_723_248_000_000_000_000i64;

        if sym == "AAPL" {
            // Initial Q1 revenue: $90.75B
            list.push(FundamentalRecord {
                symbol: "AAPL".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "revenue".into(),
                value: 90_753_000_000.0,
                currency: "USD".into(),
                publication_ts_ns: ts_apr15,
                restatement_index: 0,
                filing_ref: "0000320193-24-000006".into(),
            });

            // Restated Q1 revenue on Aug 10: $90.80B
            list.push(FundamentalRecord {
                symbol: "AAPL".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "revenue".into(),
                value: 90_800_000_000.0,
                currency: "USD".into(),
                publication_ts_ns: ts_aug10,
                restatement_index: 1,
                filing_ref: "0000320193-24-000010".into(),
            });

            // Net income
            list.push(FundamentalRecord {
                symbol: "AAPL".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "net_income".into(),
                value: 23_636_000_000.0,
                currency: "USD".into(),
                publication_ts_ns: ts_apr15,
                restatement_index: 0,
                filing_ref: "0000320193-24-000006".into(),
            });
        }

        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_in_time_query_with_restatement() {
        let mut store = PointInTimeFundamentalsStore::new();
        let provider = MockFundamentalsProvider;
        let records = provider.fetch_fundamentals("AAPL").unwrap();
        store.ingest(records);

        let ts_before_filing = 1_700_000_000_000_000_000i64; // Before Apr 15
        let ts_between = 1_715_000_000_000_000_000i64; // May (between initial and restatement)
        let ts_after_restatement = 1_725_000_000_000_000_000i64; // Sept (after Aug 10 restatement)

        // 1. Before filing: not known yet -> None
        assert!(store.query_point_in_time("AAPL", "2024-Q1", "revenue", ts_before_filing).is_none());

        // 2. In May: returns original un-restated value ($90.753B)
        let rec_may = store
            .query_point_in_time("AAPL", "2024-Q1", "revenue", ts_between)
            .expect("should find initial filing");
        assert_eq!(rec_may.value, 90_753_000_000.0);
        assert_eq!(rec_may.restatement_index, 0);

        // 3. In Sept: returns restated value ($90.800B)
        let rec_sept = store
            .query_point_in_time("AAPL", "2024-Q1", "revenue", ts_after_restatement)
            .expect("should find restated filing");
        assert_eq!(rec_sept.value, 90_800_000_000.0);
        assert_eq!(rec_sept.restatement_index, 1);
    }

    #[test]
    fn test_symbol_master_delisting_and_history() {
        let mut sm = SymbolMaster::new();
        let ts_start = 1_000_000_000_000_000_000i64;
        let ts_delisted = 1_500_000_000_000_000_000i64;

        sm.add_entry(SymbolMetadata {
            symbol: "ENRON".into(),
            company_name: "Enron Corp".into(),
            sector: "Energy".into(),
            industry: "Oil & Gas".into(),
            exchange: "NYSE".into(),
            country: "USA".into(),
            status: ListingStatus::Active,
            effective_from_ns: ts_start,
            effective_to_ns: Some(ts_delisted),
        });

        sm.add_entry(SymbolMetadata {
            symbol: "ENRON".into(),
            company_name: "Enron Corp".into(),
            sector: "Energy".into(),
            industry: "Oil & Gas".into(),
            exchange: "NYSE".into(),
            country: "USA".into(),
            status: ListingStatus::Delisted,
            effective_from_ns: ts_delisted,
            effective_to_ns: None,
        });

        // While active
        assert!(sm.is_active_at("ENRON", ts_start + 100));
        assert_eq!(sm.active_symbols_at(ts_start + 100).len(), 1);

        // After delisting: excluded from active lists
        assert!(!sm.is_active_at("ENRON", ts_delisted + 100));
        assert_eq!(sm.active_symbols_at(ts_delisted + 100).len(), 0);

        // Still retained in history
        let hist = sm.symbol_history("ENRON");
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn test_fundamentals_quarantine_bad_rows() {
        let mut store = PointInTimeFundamentalsStore::new();
        let bad_records = vec![
            FundamentalRecord {
                symbol: "XYZ".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "revenue".into(),
                value: -500.0, // Impossible negative revenue
                currency: "USD".into(),
                publication_ts_ns: 1000,
                restatement_index: 0,
                filing_ref: "test".into(),
            },
            FundamentalRecord {
                symbol: "XYZ".into(),
                period: "2024-Q1".into(),
                fiscal_year: 2024,
                fiscal_quarter: 1,
                metric: "net_income".into(),
                value: -100.0, // Negative net income is valid (loss)
                currency: "USD".into(),
                publication_ts_ns: 1000,
                restatement_index: 0,
                filing_ref: "test".into(),
            },
        ];

        store.ingest(bad_records);
        assert_eq!(store.accepted_count(), 1);
        assert_eq!(store.quarantined_records().len(), 1);
        assert_eq!(store.quarantined_records()[0].category, "IMPOSSIBLE_NEGATIVE");
    }

    #[test]
    fn test_dataset_versioning_deterministic() {
        let mut store1 = PointInTimeFundamentalsStore::new();
        let mut store2 = PointInTimeFundamentalsStore::new();

        let rec = vec![FundamentalRecord {
            symbol: "MSFT".into(),
            period: "2024-Q2".into(),
            fiscal_year: 2024,
            fiscal_quarter: 2,
            metric: "revenue".into(),
            value: 62_020_000_000.0,
            currency: "USD".into(),
            publication_ts_ns: 1000,
            restatement_index: 0,
            filing_ref: "test".into(),
        }];

        store1.ingest(rec.clone());
        store2.ingest(rec);

        assert_eq!(store1.dataset_version_id(), store2.dataset_version_id());
        assert!(store1.dataset_version_id().starts_with("fund_v_"));
    }
}
