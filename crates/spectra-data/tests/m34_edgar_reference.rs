//! M34: EDGAR Company Facts parsing, against a hand-written fixture.
//!
//! The fixture is shaped like `data.sec.gov/api/xbrl/companyfacts` for a company whose fiscal year
//! ends in late September. Accession numbers name their filing:
//!
//! | accn        | form   | fy   | fp | filed      | own period           |
//! |-------------|--------|------|----|------------|----------------------|
//! | `A-FY-23`   | 10-K   | 2023 | FY | 2023-11-03 | year to 2023-09-30   |
//! | `A-Q1-24`   | 10-Q   | 2024 | Q1 | 2024-02-02 | quarter to 2023-12-30|
//! | `A-Q1-24A`  | 10-Q/A | 2024 | Q1 | 2024-03-15 | quarter to 2023-12-30|
//! | `A-Q2-24`   | 10-Q   | 2024 | Q2 | 2024-05-03 | quarter to 2024-03-30|
//! | `A-Q1-25`   | 10-Q   | 2025 | Q1 | 2025-01-31 | quarter to 2024-12-28|

use spectra_data::edgar::{known_at_ns, parse_company_facts};
use spectra_data::fundamentals::{FundamentalRecord, PointInTimeFundamentalsStore};

const FACTS: &str = r#"{
  "cik": 320193,
  "entityName": "Example Inc.",
  "facts": {
    "us-gaap": {
      "Revenues": {"units": {
        "USD": [
          {"start":"2023-10-01","end":"2023-12-30","val":119575000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
          {"start":"2022-09-25","end":"2022-12-31","val":117154000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
          {"start":"2023-10-01","end":"2023-12-30","val":119600000000,"accn":"A-Q1-24A","fy":2024,"fp":"Q1","form":"10-Q/A","filed":"2024-03-15"},
          {"start":"2023-10-01","end":"2023-12-30","val":119600000000,"accn":"A-Q1-25","fy":2025,"fp":"Q1","form":"10-Q","filed":"2025-01-31"},
          {"start":"2024-09-29","end":"2024-12-28","val":124300000000,"accn":"A-Q1-25","fy":2025,"fp":"Q1","form":"10-Q","filed":"2025-01-31"},
          {"start":"2023-10-01","end":"2024-03-30","val":210328000000,"accn":"A-Q2-24","fy":2024,"fp":"Q2","form":"10-Q","filed":"2024-05-03"},
          {"start":"2022-09-25","end":"2023-09-30","val":383285000000,"accn":"A-FY-23","fy":2023,"fp":"FY","form":"10-K","filed":"2023-11-03"}
        ],
        "EUR": [
          {"start":"2023-10-01","end":"2023-12-30","val":1,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
        ]
      }},
      "RevenueFromContractWithCustomerExcludingAssessedTax": {"units": {"USD": [
        {"start":"2023-10-01","end":"2023-12-30","val":999,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
        {"start":"2023-12-31","end":"2024-03-30","val":90753000000,"accn":"A-Q2-24","fy":2024,"fp":"Q2","form":"10-Q","filed":"2024-05-03"}
      ]}},
      "NetIncomeLoss": {"units": {"USD": [
        {"start":"2023-10-01","end":"2023-12-30","val":-1000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
      ]}},
      "GrossProfit": {"units": {"USD": [
        {"start":"2023-07-02","end":"2023-09-30","val":40427000000,"accn":"A-FY-23","fy":2023,"fp":"FY","form":"10-K","filed":"2023-11-03"}
      ]}},
      "Assets": {"units": {"USD": [
        {"end":"2023-12-30","val":353514000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
        {"end":"2023-09-30","val":352583000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
        {"end":"2023-09-30","val":352583000000,"accn":"A-FY-23","fy":2023,"fp":"FY","form":"10-K","filed":"2023-11-03"}
      ]}},
      "Liabilities": {"units": {"USD": [
        {"end":"2023-12-30","val":290437000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"},
        {"end":"2023-12-30","val":290000000000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
      ]}},
      "EarningsPerShareDiluted": {"units": {"USD/shares": [
        {"start":"2023-10-01","end":"2023-12-30","val":2.18,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
      ]}}
    },
    "dei": {
      "EntityCommonStockSharesOutstanding": {"units": {"shares": [
        {"end":"2024-01-19","val":15441881000,"accn":"A-Q1-24","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
      ]}}
    }
  }
}"#;

fn find<'a>(records: &'a [FundamentalRecord], metric: &str, period: &str) -> Vec<&'a FundamentalRecord> {
    records.iter().filter(|r| r.metric == metric && r.period == period).collect()
}

fn at(date: &str) -> i64 {
    known_at_ns(date).unwrap()
}

#[test]
fn every_kept_value_and_revision() {
    let records = parse_company_facts(FACTS, "example").unwrap();
    let summary: Vec<(&str, &str, u32, f64)> = records
        .iter()
        .map(|r| (r.metric.as_str(), r.period.as_str(), r.restatement_index, r.value))
        .collect();
    // Sorted by metric, period, revision. Dropped: the prior-year comparative quarter (never its
    // filing's own period), the 6-month year-to-date revenue, the EUR revenue, the repeat of the
    // restated Q1 revenue in A-Q1-25, the comparative 2023-09-30 assets in A-Q1-24, and both
    // liabilities values (one filing reports two different numbers for the same date).
    assert_eq!(
        summary,
        vec![
            ("eps_diluted", "2024-Q1", 0, 2.18),
            ("gross_profit", "2023-Q4", 0, 40_427_000_000.0),
            ("net_income", "2024-Q1", 0, -1_000_000.0),
            ("revenue", "2023-FY", 0, 383_285_000_000.0),
            ("revenue", "2024-Q1", 0, 119_575_000_000.0),
            ("revenue", "2024-Q1", 1, 119_600_000_000.0),
            ("revenue", "2024-Q2", 0, 90_753_000_000.0),
            ("revenue", "2025-Q1", 0, 124_300_000_000.0),
            ("shares_outstanding", "2024-Q1", 0, 15_441_881_000.0),
            ("total_assets", "2023-FY", 0, 352_583_000_000.0),
            ("total_assets", "2024-Q1", 0, 353_514_000_000.0),
        ]
    );
    assert!(records.iter().all(|r| r.symbol == "EXAMPLE"));
}

#[test]
fn revisions_carry_their_own_filing_and_known_at_time() {
    let records = parse_company_facts(FACTS, "EXAMPLE").unwrap();
    let q1 = find(&records, "revenue", "2024-Q1");
    assert_eq!((q1[0].filing_ref.as_str(), q1[0].publication_ts_ns), ("A-Q1-24", at("2024-02-02")));
    assert_eq!((q1[1].filing_ref.as_str(), q1[1].publication_ts_ns), ("A-Q1-24A", at("2024-03-15")));
    assert_eq!((q1[0].fiscal_year, q1[0].fiscal_quarter), (2024, 1));
}

#[test]
fn the_preferred_concept_wins_and_others_fill_gaps() {
    let records = parse_company_facts(FACTS, "EXAMPLE").unwrap();
    // Revenues and RevenueFromContract... both report Q1 FY2024 in A-Q1-24: Revenues is listed
    // first, so its 119.575B is kept and the other concept's 999 is not.
    assert!(records.iter().all(|r| r.value != 999.0));
    // Only RevenueFromContract... reports Q2 FY2024, so it supplies that quarter.
    assert_eq!(find(&records, "revenue", "2024-Q2")[0].filing_ref, "A-Q2-24");
}

#[test]
fn periods_come_from_the_filing_that_first_reported_the_value() {
    let records = parse_company_facts(FACTS, "EXAMPLE").unwrap();
    // A quarter reported on its own in a 10-K is the fourth quarter.
    let q4 = find(&records, "gross_profit", "2023-Q4");
    assert_eq!((q4[0].fiscal_year, q4[0].fiscal_quarter), (2023, 4));
    // A year-end balance first reported by the 10-K keeps the 10-K's label, even though the next
    // 10-Q repeats it.
    let fy = find(&records, "total_assets", "2023-FY");
    assert_eq!((fy.len(), fy[0].filing_ref.as_str(), fy[0].fiscal_quarter), (1, "A-FY-23", 0));
    // Units: dollar amounts and per-share amounts are USD; share counts have no currency.
    assert_eq!(find(&records, "eps_diluted", "2024-Q1")[0].currency, "USD");
    assert_eq!(find(&records, "shares_outstanding", "2024-Q1")[0].currency, "");
}

#[test]
fn loaded_into_the_store_a_value_is_unknown_until_its_known_at_time() {
    let mut store = PointInTimeFundamentalsStore::new();
    store.ingest(parse_company_facts(FACTS, "EXAMPLE").unwrap());
    // Every record passes the quality checks: the negative is net income, which may be negative.
    assert_eq!(store.quarantined_records().len(), 0);

    let q1 = |as_of: i64| store.query_point_in_time("EXAMPLE", "2024-Q1", "revenue", as_of).map(|r| r.value);
    let original = at("2024-02-02");
    let restated = at("2024-03-15");
    // Filed 2024-02-02, known from 2024-02-03 12:00 UTC: one nanosecond earlier it is unknown.
    assert_eq!(q1(original - 1), None);
    assert_eq!(q1(original), Some(119_575_000_000.0));
    assert_eq!(q1(restated - 1), Some(119_575_000_000.0));
    assert_eq!(q1(restated), Some(119_600_000_000.0));
    // The A-Q1-25 filing repeats the restated value, so nothing changes after it.
    assert_eq!(q1(at("2025-01-31")), Some(119_600_000_000.0));
}

#[test]
fn parsing_is_deterministic() {
    let a = parse_company_facts(FACTS, "EXAMPLE").unwrap();
    let b = parse_company_facts(FACTS, "EXAMPLE").unwrap();
    assert_eq!(a, b);
    let mut sa = PointInTimeFundamentalsStore::new();
    let mut sb = PointInTimeFundamentalsStore::new();
    sa.ingest(a);
    sb.ingest(b);
    assert_eq!(sa.dataset_version_id(), sb.dataset_version_id());
}
