//! Portfolio API and runs (M15 Task 6) through the signed API: edit and delete saved portfolios,
//! run a portfolio backtest on a real dataset version, read and list runs, store the report
//! through M16, workspace scoping and roles. Needs `DATABASE_URL`; without it the tests pass
//! without doing anything.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{DataStore, Dataset, DatasetInfo};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const DAY_NS: i64 = 86_400_000_000_000;
static COUNTER: AtomicU64 = AtomicU64::new(0);

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn fresh() -> String {
    format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// A response: status, headers we read, raw body and the body as JSON (null if not JSON).
struct Reply {
    status: u16,
    report_version: Option<String>,
    content_type: Option<String>,
    bytes: Vec<u8>,
    json: Value,
}

/// A workspace with an owner and a viewer, a second workspace with its own owner, and a dataset
/// version in a temporary dataset store.
///
/// The dataset has AAA, BBB and CCC. AAA and BBB have a bar every day from 2026-01-02 to
/// 2026-03-31 (AAA rises 1 per day from 100, BBB stays at 50); CCC has no bars.
struct Fixture {
    app: Router,
    pool: PgPool,
    dir: PathBuf,
    ws: Uuid,
    owner: Uuid,
    viewer: Uuid,
    other_ws: Uuid,
    other_owner: Uuid,
    version: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let dir =
            std::env::temp_dir().join(format!("spectra-portfolio-runs-{}", spectra_db::new_id().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let (ws, other_ws) = (spectra_db::new_id(), spectra_db::new_id());
        for w in [ws, other_ws] {
            sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Portfolio runs test')")
                .bind(w)
                .bind(format!("ws-{}", w.simple()))
                .execute(&pool)
                .await
                .unwrap();
        }
        let (owner, viewer, other_owner) = (spectra_db::new_id(), spectra_db::new_id(), spectra_db::new_id());
        for (user, w, role) in
            [(owner, ws, "owner"), (viewer, ws, "viewer"), (other_owner, other_ws, "owner")]
        {
            sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
                .bind(user)
                .bind(format!("user-{}@example.com", user.simple()))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)")
                .bind(w)
                .bind(user)
                .bind(role)
                .execute(&pool)
                .await
                .unwrap();
        }

        let symbols = SymbolTable::from_symbols(["AAA", "BBB", "CCC"]);
        let (aaa, bbb) = (symbols.id("AAA").unwrap(), symbols.id("BBB").unwrap());
        let first = spectra_backtest::rebalance::Date::new(2026, 1, 2).to_days();
        let last = spectra_backtest::rebalance::Date::new(2026, 3, 31).to_days();
        let mut bars = Vec::new();
        for (i, day) in (first..=last).enumerate() {
            let ts = day * DAY_NS + 21 * 3_600_000_000_000;
            for (id, close) in [(aaa, 100.0 + i as f64), (bbb, 50.0)] {
                let p = Price::from_f64(close).unwrap();
                bars.push(Bar {
                    ts_ns: ts,
                    known_at_ns: ts,
                    instrument: id,
                    open: p,
                    high: p,
                    low: p,
                    close: p,
                    volume: Qty::from_units(10_000).unwrap(),
                });
            }
        }
        bars.sort_by_key(|b| (b.ts_ns, b.instrument));
        let id = compute_dataset_id(&bars, &symbols);
        let dataset = Dataset {
            info: DatasetInfo { id, bars_sha256: spectra_core::bars_sha256(&bars), ..DatasetInfo::default() },
            quality: spectra_data::dataset::assess_quality(
                &bars,
                &symbols,
                &spectra_data::CsvOptions::default(),
            ),
            bars,
            symbols,
        };
        let byte_size = i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).unwrap();
        let checksum = DataStore::new(dir.clone()).unwrap().put(&dataset).unwrap();
        let reg = spectra_db::datasets::register_version(
            &pool,
            &spectra_db::datasets::NewDatasetVersion {
                workspace_id: ws,
                dataset_name: "Portfolio runs dataset",
                checksum: &checksum,
                row_count: i64::try_from(dataset.bars.len()).unwrap(),
                byte_size,
                quality_report: json!({}),
                created_by: None,
                blob_sha256: None,
                source: None,
            },
        )
        .await
        .unwrap();

        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.data_dir = Some(dir.clone());
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, dir, ws, owner, viewer, other_ws, other_owner, version: reg.id }
    }

    async fn call_as(&self, method: &str, path: &str, user: Uuid, ws: Uuid, body: &Value) -> Reply {
        let payload = if body.is_null() { Vec::new() } else { serde_json::to_vec(body).unwrap() };
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (u, w) = (user.to_string(), ws.to_string());
        let rid = format!("req-portfolio-runs-{}", fresh());
        let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u, &w, &rid, &payload));
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-spectra-user", u)
            .header("x-spectra-workspace", w)
            .header("x-request-id", rid)
            .header("x-spectra-timestamp", ts)
            .header("x-spectra-signature", sig)
            .body(Body::from(payload))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let header = |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (report_version, content_type) = (header("x-report-version"), header("content-type"));
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply { status, report_version, content_type, bytes, json }
    }

    async fn owner(&self, method: &str, path: &str, body: &Value) -> Reply {
        self.call_as(method, path, self.owner, self.ws, body).await
    }

    async fn create(&self, holdings: Value) -> String {
        let r = self
            .owner(
                "POST",
                "/api/v1/portfolios",
                &json!({"name": format!("p-{}", fresh()), "holdings": holdings}),
            )
            .await;
        assert_eq!(r.status, 201, "{}", r.json);
        r.json["id"].as_str().unwrap().to_string()
    }

    fn run_body(&self) -> Value {
        json!({
            "dataset_version_id": self.version,
            "start": "2026-01-01",
            "end": "2026-03-31",
            "rule": {"calendar": "monthly"},
            "initial_cash": 100_000.0,
        })
    }

    async fn clean_up(&self) {
        for ws in [self.ws, self.other_ws] {
            for table in ["reports", "portfolio_runs", "portfolios", "dataset_versions", "datasets"] {
                let _ = sqlx::query(&format!("DELETE FROM {table} WHERE workspace_id = $1"))
                    .bind(ws)
                    .execute(&self.pool)
                    .await;
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn edit_and_delete_a_portfolio() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;
    let id = f.create(json!([{"symbol": "AAA", "weight": 0.5}, {"symbol": "BBB", "weight": 0.5}])).await;
    let path = format!("/api/v1/portfolios/{id}");

    let edited = f
        .owner("PUT", &path, &json!({"name": "Renamed", "holdings": [{"symbol": "AAA", "weight": 0.7}, {"symbol": "BBB", "weight": 0.3}]}))
        .await;
    assert_eq!(edited.status, 200, "{}", edited.json);
    assert_eq!(edited.json["name"], "Renamed");
    assert_eq!(edited.json["holdings"][0]["weight"], 0.7);

    // Same rules as create: bad weights are refused and the old holdings are kept.
    let bad = f
        .owner("PUT", &path, &json!({"name": "Renamed", "holdings": [{"symbol": "AAA", "weight": 0.7}, {"symbol": "BBB", "weight": 0.2}]}))
        .await;
    assert_eq!(bad.status, 422);
    assert!(bad.json.to_string().contains("sum to"), "{}", bad.json);
    let reread = f.owner("GET", &path, &Value::Null).await;
    assert_eq!(reread.json["holdings"][1]["weight"], 0.3);
    // Equal weight through an edit.
    let equal = f
        .owner("PUT", &path, &json!({"name": "Renamed", "method": "equal", "holdings": [{"symbol": "AAA"}, {"symbol": "BBB"}, {"symbol": "CCC"}, {"symbol": "DDD"}]}))
        .await;
    assert_eq!(equal.status, 200);
    assert!(equal.json["holdings"].as_array().unwrap().iter().all(|h| h["weight"] == 0.25));

    // Another workspace sees nothing: 404 for read, edit and delete.
    let body = json!({"name": "x", "holdings": [{"symbol": "AAA", "weight": 1.0}]});
    assert_eq!(f.call_as("GET", &path, f.other_owner, f.other_ws, &Value::Null).await.status, 404);
    assert_eq!(f.call_as("PUT", &path, f.other_owner, f.other_ws, &body).await.status, 404);
    assert_eq!(f.call_as("DELETE", &path, f.other_owner, f.other_ws, &Value::Null).await.status, 404);

    let deleted = f.owner("DELETE", &path, &Value::Null).await;
    assert_eq!(deleted.status, 200);
    assert_eq!(deleted.json["deleted"], true);
    assert_eq!(f.owner("GET", &path, &Value::Null).await.status, 404);
    assert_eq!(f.owner("DELETE", &path, &Value::Null).await.status, 404);
    f.clean_up().await;
}

#[tokio::test]
async fn run_read_list_and_report_a_portfolio_backtest() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;
    let id = f.create(json!([{"symbol": "AAA", "weight": 0.5}, {"symbol": "BBB", "weight": 0.5}])).await;

    let run = f.owner("POST", &format!("/api/v1/portfolios/{id}/runs"), &f.run_body()).await;
    assert_eq!(run.status, 201, "{}", run.json);
    assert_eq!(run.json["status"], "succeeded");
    let result = &run.json["result"];
    // Initial allocation on Jan 2, then the first trading day of February and of March.
    let dates: Vec<&str> =
        result["rebalances"].as_array().unwrap().iter().map(|r| r["date"].as_str().unwrap()).collect();
    assert_eq!(dates, ["2026-01-02", "2026-02-01", "2026-03-01"]);
    assert_eq!(result["rebalances"][0]["reasons"], json!(["initial"]));
    assert_eq!(result["rebalances"][1]["reasons"], json!(["calendar"]));
    // Weights over time: drifted before (AAA rose), back on target after.
    let feb = &result["rebalances"][1]["weights"];
    assert!(feb[0]["before"].as_f64().unwrap() > 0.5);
    assert!((feb[0]["after"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    // Every trade records its cost; paper-default charges a 1.00 fee.
    assert!(result["rebalances"][1]["trades"].as_array().unwrap().iter().all(|t| t["fee"] == 1.0));
    assert_eq!(result["cost_config"]["name"], "paper-default");
    assert_eq!(result["summary"]["rebalance_count"], 3);
    assert_eq!(result["prices"]["days"], 89);
    assert_eq!(result["equity"].as_array().unwrap().len(), 89);
    assert!(result["summary"]["total_cost"].as_f64().unwrap() > 0.0);
    // The inputs are recorded with a snapshot of the portfolio.
    assert_eq!(run.json["request"]["portfolio"]["holdings"][0]["symbol"], "AAA");
    assert_eq!(run.json["request"]["rule"]["calendar"], "monthly");
    let run_id = run.json["id"].as_str().unwrap().to_string();

    let got = f.owner("GET", &format!("/api/v1/portfolio-runs/{run_id}"), &Value::Null).await;
    assert_eq!(got.status, 200);
    assert_eq!(got.json["result"]["summary"], result["summary"]);

    let listed = f.owner("GET", &format!("/api/v1/portfolios/{id}/runs"), &Value::Null).await;
    assert_eq!(listed.status, 200);
    assert_eq!(listed.json["items"][0]["id"], run_id.as_str());
    assert_eq!(listed.json["items"][0]["status"], "succeeded");
    assert_eq!(listed.json["items"][0]["headline"], result["summary"]);
    assert!(listed.json["next_cursor"].is_null());

    // The report: stored as version 1, a valid PDF; the same run exports the same bytes and
    // reuses the version.
    let report = f.owner("GET", &format!("/api/v1/portfolio-runs/{run_id}/report"), &Value::Null).await;
    assert_eq!(report.status, 200, "{}", String::from_utf8_lossy(&report.bytes));
    assert_eq!(report.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(report.report_version.as_deref(), Some("1"));
    assert!(report.bytes.starts_with(b"%PDF-1.4"));
    let again = f.owner("GET", &format!("/api/v1/portfolio-runs/{run_id}/report"), &Value::Null).await;
    assert_eq!(again.bytes, report.bytes, "the same run gives the same report");
    assert_eq!(again.report_version.as_deref(), Some("1"));
    let reports = f
        .owner("GET", &format!("/api/v1/reports?run_kind=portfolio_backtest&run_id={run_id}"), &Value::Null)
        .await;
    assert_eq!(reports.status, 200, "{}", reports.json);
    assert_eq!(reports.json.as_array().unwrap().len(), 1);
    assert_eq!(reports.json[0]["report_type"], "portfolio");
    let stored = f.owner("GET", reports.json[0]["download_url"].as_str().unwrap(), &Value::Null).await;
    assert_eq!(stored.bytes, report.bytes);

    // Deleting the portfolio keeps the run, without the link.
    assert_eq!(f.owner("DELETE", &format!("/api/v1/portfolios/{id}"), &Value::Null).await.status, 200);
    let kept = f.owner("GET", &format!("/api/v1/portfolio-runs/{run_id}"), &Value::Null).await;
    assert_eq!(kept.status, 200);
    assert!(kept.json["portfolio_id"].is_null());
    assert_eq!(kept.json["status"], "succeeded");
    f.clean_up().await;
}

#[tokio::test]
async fn bad_runs_are_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;
    let id = f.create(json!([{"symbol": "AAA", "weight": 0.5}, {"symbol": "CCC", "weight": 0.5}])).await;
    let runs = format!("/api/v1/portfolios/{id}/runs");

    // A holding with no bars: refused with its name, never priced from anything else, and the
    // attempt is stored as a failed run.
    let missing = f.owner("POST", &runs, &f.run_body()).await;
    assert_eq!(missing.status, 422);
    assert!(missing.json.to_string().contains("No bars for CCC"), "{}", missing.json);
    let listed = f.owner("GET", &runs, &Value::Null).await;
    assert_eq!(listed.json["items"][0]["status"], "failed");
    assert!(listed.json["items"][0]["error"].as_str().unwrap().contains("CCC"));
    assert!(listed.json["items"][0]["headline"].is_null());
    // Its report does not exist.
    let failed_id = listed.json["items"][0]["id"].as_str().unwrap();
    assert_eq!(
        f.owner("GET", &format!("/api/v1/portfolio-runs/{failed_id}/report"), &Value::Null).await.status,
        409
    );

    // Invalid inputs: every field problem at once, nothing stored.
    let mut body = f.run_body();
    body["start"] = json!("2026-04-01");
    body["rule"] = json!({"calendar": "weekly"});
    body["initial_cash"] = json!(-1.0);
    let invalid = f.owner("POST", &runs, &body).await;
    assert_eq!(invalid.status, 422);
    let fields: Vec<&str> = invalid.json["details"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["field"].as_str().unwrap())
        .collect();
    assert!(
        fields.contains(&"end") && fields.contains(&"rule.calendar") && fields.contains(&"initial_cash"),
        "{fields:?}"
    );
    assert_eq!(f.owner("GET", &runs, &Value::Null).await.json["items"].as_array().unwrap().len(), 1);

    // An unknown dataset version, and another workspace's portfolio or run.
    let mut unknown = f.run_body();
    unknown["dataset_version_id"] = json!(Uuid::nil());
    assert_eq!(f.owner("POST", &runs, &unknown).await.status, 404);
    assert_eq!(f.call_as("POST", &runs, f.other_owner, f.other_ws, &f.run_body()).await.status, 404);
    assert_eq!(f.call_as("GET", &runs, f.other_owner, f.other_ws, &Value::Null).await.status, 404);
    let run_path = format!("/api/v1/portfolio-runs/{failed_id}");
    assert_eq!(f.call_as("GET", &run_path, f.other_owner, f.other_ws, &Value::Null).await.status, 404);
    f.clean_up().await;
}

#[tokio::test]
async fn viewers_read_but_cannot_write() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;
    let id = f.create(json!([{"symbol": "AAA", "weight": 0.5}, {"symbol": "BBB", "weight": 0.5}])).await;
    let run = f.owner("POST", &format!("/api/v1/portfolios/{id}/runs"), &f.run_body()).await;
    assert_eq!(run.status, 201);
    let run_id = run.json["id"].as_str().unwrap().to_string();
    let as_viewer = |method: &'static str, path: String, body: Value| {
        let f = &f;
        async move { f.call_as(method, &path, f.viewer, f.ws, &body).await }
    };

    // Reads are allowed.
    assert_eq!(as_viewer("GET", "/api/v1/portfolios".into(), Value::Null).await.status, 200);
    assert_eq!(as_viewer("GET", format!("/api/v1/portfolios/{id}"), Value::Null).await.status, 200);
    assert_eq!(as_viewer("GET", format!("/api/v1/portfolios/{id}/runs"), Value::Null).await.status, 200);
    assert_eq!(as_viewer("GET", format!("/api/v1/portfolio-runs/{run_id}"), Value::Null).await.status, 200);

    // Writes are refused with 403 viewer_read_only, and change nothing.
    let holdings = json!([{"symbol": "AAA", "weight": 1.0}]);
    for (method, path, body) in [
        ("POST", "/api/v1/portfolios".to_string(), json!({"name": "v", "holdings": holdings})),
        ("PUT", format!("/api/v1/portfolios/{id}"), json!({"name": "v", "holdings": holdings})),
        ("DELETE", format!("/api/v1/portfolios/{id}"), Value::Null),
        ("POST", format!("/api/v1/portfolios/{id}/runs"), f.run_body()),
        ("GET", format!("/api/v1/portfolio-runs/{run_id}/report"), Value::Null),
    ] {
        let r = as_viewer(method, path.clone(), body).await;
        assert_eq!(r.status, 403, "{method} {path}: {}", r.json);
        assert_eq!(r.json["code"], "viewer_read_only", "{method} {path}");
    }
    assert_eq!(f.owner("GET", &format!("/api/v1/portfolios/{id}"), &Value::Null).await.status, 200);
    let runs = f.owner("GET", &format!("/api/v1/portfolios/{id}/runs"), &Value::Null).await;
    assert_eq!(runs.json["items"].as_array().unwrap().len(), 1, "the viewer's run was not stored");
    f.clean_up().await;
}
