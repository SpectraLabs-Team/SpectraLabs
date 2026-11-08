//! M34b: point-in-time reads and quarantine coverage.
//!
//! Task 4: reading a value before its revision's known-at time gets nothing, through the API and
//! through the M37 statement built from the store. Task 5: quarantined rows never reach a reader
//! or a screen. Needs a Postgres (`DATABASE_URL`); without one it passes without doing anything.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use chrono::DateTime;
use http_body_util::BodyExt;
use serde_json::Value;
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::edgar::known_at_ns;
use spectra_data::fundamentals::{FundamentalRecord, PointInTimeFundamentalsStore};
use spectra_db::fundamentals::{facts_known_by, record_load, NewLoad};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use spectra_research::{run_screen, FundamentalFilter, ScreenDefinition, SymbolScreenData};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn rec(
    symbol: &str,
    metric: &str,
    value: f64,
    revision: u32,
    filed: &str,
    filing: &str,
) -> FundamentalRecord {
    FundamentalRecord {
        symbol: symbol.into(),
        period: "2024-Q1".into(),
        fiscal_year: 2024,
        fiscal_quarter: 1,
        metric: metric.into(),
        value,
        currency: "USD".into(),
        publication_ts_ns: known_at_ns(filed).unwrap(),
        restatement_index: revision,
        filing_ref: filing.into(),
    }
}

struct Fixture {
    app: Router,
    pool: PgPool,
    ws: Uuid,
    user: Uuid,
}

impl Fixture {
    /// A workspace with fundamentals for two symbols:
    ///  * EXA, Q1 FY2024: revenue 100 filed 2024-02-02 (known 2024-02-03 12:00 UTC), restated to 105
    ///    on 2024-03-15 (known 2024-03-16 12:00 UTC); net income 10, total assets 500 and diluted
    ///    EPS 0.5 filed 2024-02-02.
    ///  * BAD, Q1 FY2024: revenue 100, and a NaN net income that the quality checks quarantine.
    async fn new(pool: PgPool) -> Self {
        let (ws, user) = (spectra_db::new_id(), spectra_db::new_id());
        spectra_db::identity::ensure_identity(&pool, Some(user), ws).await.unwrap();
        for (symbol, records) in [
            (
                "EXA",
                vec![
                    rec("EXA", "revenue", 100.0, 0, "2024-02-02", "E-Q1"),
                    rec("EXA", "revenue", 105.0, 1, "2024-03-15", "E-Q1-AMEND"),
                    rec("EXA", "net_income", 10.0, 0, "2024-02-02", "E-Q1"),
                    rec("EXA", "total_assets", 500.0, 0, "2024-02-02", "E-Q1"),
                    rec("EXA", "eps_diluted", 0.5, 0, "2024-02-02", "E-Q1"),
                ],
            ),
            (
                "BAD",
                vec![
                    rec("BAD", "revenue", 100.0, 0, "2024-02-02", "B-Q1"),
                    rec("BAD", "net_income", f64::NAN, 0, "2024-02-02", "B-Q1"),
                ],
            ),
        ] {
            let mut store = PointInTimeFundamentalsStore::new();
            store.ingest(records);
            record_load(
                &pool,
                &NewLoad {
                    workspace_id: ws,
                    symbol,
                    source: "edgar",
                    version_id: &store.dataset_version_id(),
                    accepted: store.records(),
                    quarantined: store.quarantined_records(),
                    job_id: None,
                    created_by: None,
                },
            )
            .await
            .unwrap();
        }
        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
        Self { app, pool, ws, user }
    }

    async fn get(&self, path: &str) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws, req_id) =
            (self.user.to_string(), self.ws.to_string(), spectra_db::new_id().to_string());
        let sig = sign(SECRET.as_bytes(), &canonical("GET", path, &ts, &user, &ws, &req_id, b""));
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .body(Body::empty())
            .unwrap();
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// `(metric, value, revision)` of every value known at `as_of`.
    async fn known(&self, symbol: &str, as_of: &str) -> Vec<(String, f64, i64)> {
        let (status, body) = self.get(&format!("/api/v1/fundamentals?symbol={symbol}&as_of={as_of}")).await;
        assert_eq!(status, 200, "{body}");
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["metric"].as_str().unwrap().to_string(),
                    i["value"].as_f64().unwrap(),
                    i["revision"].as_i64().unwrap(),
                )
            })
            .collect()
    }
}

fn revenue(items: &[(String, f64, i64)]) -> Option<(f64, i64)> {
    items.iter().find(|(m, ..)| m == "revenue").map(|(_, v, r)| (*v, *r))
}

#[tokio::test]
async fn a_value_read_before_its_known_at_time_is_not_there() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;

    // Filed 2024-02-02, known from 2024-02-03 12:00 UTC.
    assert_eq!(fix.known("EXA", "2024-02-02").await, vec![], "the filing day itself is too early");
    assert_eq!(fix.known("EXA", "2024-02-03T11:59:59Z").await, vec![], "one second before known-at");
    let feb = fix.known("EXA", "2024-02-03").await;
    assert_eq!(revenue(&feb), Some((100.0, 0)));
    assert_eq!(feb.len(), 4, "{feb:?}");
    // Before the amendment was known the original stands; after it, the restatement.
    assert_eq!(revenue(&fix.known("EXA", "2024-03-16T11:59:59Z").await), Some((100.0, 0)));
    assert_eq!(revenue(&fix.known("EXA", "2024-03-16").await), Some((105.0, 1)));

    let (_, only) = fix.get("/api/v1/fundamentals?symbol=exa&as_of=2024-03-16&metric=Revenue").await;
    assert_eq!(only["items"].as_array().unwrap().len(), 1);
    assert_eq!(only["items"][0]["filing_ref"], "E-Q1-AMEND");
    for bad in ["/api/v1/fundamentals?as_of=2024-03-16", "/api/v1/fundamentals?symbol=EXA&as_of=yesterday"] {
        assert_eq!(fix.get(bad).await.0, 422, "{bad}");
    }
}

#[tokio::test]
async fn statements_are_built_only_from_what_was_known() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;
    let statement = |as_of: &str| format!("/api/v1/fundamentals/statement?symbol=EXA&as_of={as_of}");

    let (status, _) = fix.get(&statement("2024-02-02")).await;
    assert_eq!(status, 404, "nothing was known yet");

    let (status, feb) = fix.get(&statement("2024-02-03")).await;
    assert_eq!(status, 200, "{feb}");
    assert_eq!((feb["period"].as_str(), feb["fiscal_year"].as_u64()), (Some("2024-Q1"), Some(2024)));
    let lines = &feb["lines"];
    assert_eq!((lines["revenue"].as_f64(), lines["net_income"].as_f64()), (Some(100.0), Some(10.0)));
    // Derived: net income 10 / diluted EPS 0.5 = 20 shares.
    assert_eq!(lines["diluted_shares_outstanding"].as_f64(), Some(20.0));
    // Lines with no value are null and listed, never zero.
    assert!(lines["inventory"].is_null());
    assert!(feb["missing"].as_array().unwrap().iter().any(|m| m == "inventory"));
    assert_eq!(
        DateTime::parse_from_rfc3339(feb["known_at"].as_str().unwrap()).unwrap().timestamp_nanos_opt(),
        known_at_ns("2024-02-02")
    );

    let (_, mar) = fix.get(&statement("2024-03-16")).await;
    assert_eq!(mar["lines"]["revenue"].as_f64(), Some(105.0));
    let (status, _) =
        fix.get("/api/v1/fundamentals/statement?symbol=EXA&as_of=2024-03-16&period=2023-fy").await;
    assert_eq!(status, 404, "no such period");
}

#[tokio::test]
async fn quarantined_rows_never_reach_a_reader_or_a_screen() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;

    // BAD's NaN net income was quarantined: readers see its revenue only.
    let bad = fix.known("BAD", "2024-03-16").await;
    assert_eq!(bad, vec![("revenue".to_string(), 100.0, 0)]);
    let (_, statement) = fix.get("/api/v1/fundamentals/statement?symbol=BAD&as_of=2024-03-16").await;
    assert!(statement["lines"]["net_income"].is_null(), "{statement}");
    let (_, quarantine) = fix.get("/api/v1/fundamentals/quarantine?symbol=BAD").await;
    assert_eq!(quarantine["items"][0]["category"], "NON_FINITE");
    assert!(quarantine["items"][0]["value"].is_null() && quarantine["items"][0]["value_text"] == "NaN");

    // A screen on a store loaded from the database. Had the NaN leaked, BAD's margin would be NaN,
    // and `NaN < 0.05` is false, so BAD would pass the margin filter: it must not.
    let as_of = DateTime::from_timestamp_nanos(known_at_ns("2024-03-15").unwrap());
    let rows = facts_known_by(&fix.pool, fix.ws, &["EXA".into(), "BAD".into()], as_of).await.unwrap();
    let mut store = PointInTimeFundamentalsStore::new();
    store.ingest(rows.iter().filter_map(spectra_db::fundamentals::FactRow::to_record).collect());
    assert!(store.quarantined_records().is_empty(), "the database only hands out accepted rows");

    let bar = |inst: u32| {
        Bar::new(
            known_at_ns("2024-03-15").unwrap(),
            inst,
            Price::from_raw(50 * 100_000_000),
            Price::from_raw(51 * 100_000_000),
            Price::from_raw(49 * 100_000_000),
            Price::from_raw(50 * 100_000_000),
            Qty::from_raw(1_000 * 100_000_000),
        )
    };
    let (exa_bars, bad_bars) = ([bar(0)], [bar(1)]);
    let universe = [
        SymbolScreenData { symbol: "EXA", bars: &exa_bars, shares_outstanding: None },
        SymbolScreenData { symbol: "BAD", bars: &bad_bars, shares_outstanding: None },
    ];
    let def = ScreenDefinition {
        name: "margin".into(),
        fundamental_filters: vec![FundamentalFilter::MinNetMargin(0.05)],
        ..ScreenDefinition::default()
    };
    let result = run_screen(&def, &universe, &store, known_at_ns("2024-03-15").unwrap(), "test-dataset");
    let symbols: Vec<&str> = result.matches.iter().map(|m| m.symbol.as_str()).collect();
    assert_eq!(symbols, ["EXA"], "EXA's margin is 10 / 105; BAD has no known net income");
    assert_eq!(result.fundamentals_version_id, store.dataset_version_id());
}
