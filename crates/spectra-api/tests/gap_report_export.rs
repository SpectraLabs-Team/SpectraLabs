//! M16 Task 7: every gap report export is stored as a new report version.
//!
//! The database tests skip when `DATABASE_URL` is unset, like the other API flows. The fixture
//! follows `tear_sheet_export.rs` (rows inserted directly, with no backtest runner) and creates the
//! gap report the way `gap_flow.rs` does, through `POST /api/v1/paper/gap/analyze` with supplied
//! fills.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

mod common;

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

/// A signed request as `user` in `ws`.
fn signed(method: &str, path: &str, user: Uuid, ws: Uuid, body: Vec<u8>) -> Request<Body> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
    let u = user.to_string();
    let w = ws.to_string();
    let rid = "req-gap-report-export";
    let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u, &w, rid, &body));
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("x-spectra-user", u)
        .header("x-spectra-workspace", w)
        .header("x-request-id", rid)
        .header("x-spectra-timestamp", ts)
        .header("x-spectra-signature", sig)
        .body(Body::from(body))
        .unwrap()
}

/// Status, the content type, the report version header, and the body bytes.
async fn fetch(app: &Router, path: &str, user: Uuid, ws: Uuid) -> (u16, String, Option<String>, Vec<u8>) {
    let res = app.clone().oneshot(signed("GET", path, user, ws, Vec::new())).await.unwrap();
    let status = res.status().as_u16();
    let header = |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).map(String::from);
    let content_type = header("content-type").unwrap_or_default();
    let version = header("x-report-version");
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, content_type, version, bytes)
}

async fn fetch_json(
    app: &Router,
    method: &str,
    path: &str,
    user: Uuid,
    ws: Uuid,
    body: &Value,
) -> (u16, Value) {
    let body = if body.is_null() { Vec::new() } else { body.to_string().into_bytes() };
    let res = app.clone().oneshot(signed(method, path, user, ws, body)).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

struct Fixture {
    app: Router,
    pool: PgPool,
    user: Uuid,
    ws: Uuid,
    strategy: Uuid,
    run: Uuid,
    gap_report: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let (user, ws, strategy) = (spectra_db::new_id(), spectra_db::new_id(), spectra_db::new_id());
        let (job, run) = (spectra_db::new_id(), spectra_db::new_id());

        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
            .bind(user)
            .bind(format!("gap-export-{}@example.com", user.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Gap report export test')")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, $4)")
            .bind(strategy)
            .bind(ws)
            .bind("Gap export strategy")
            .bind("Paper strategy for the gap report export test")
            .execute(&pool)
            .await
            .unwrap();
        // The backtest the paper trading is compared against: a succeeded job and its run.
        sqlx::query(
            "INSERT INTO jobs (id, workspace_id, kind, status, finished_at, created_by) \
             VALUES ($1, $2, 'backtest', 'succeeded', now(), $3)",
        )
        .bind(job)
        .bind(ws)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO runs (id, workspace_id, job_id, kind, status, started_at, finished_at, created_by) \
             VALUES ($1, $2, $3, 'backtest', 'succeeded', now(), now(), $4)",
        )
        .bind(run)
        .bind(ws)
        .bind(job)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));

        // The gap report, through the analysis endpoint (as gap_flow.rs does).
        let payload = json!({
            "strategy_id": strategy,
            "backtest_run_id": run,
            "max_slippage_drift_bps": 5.0,
            "max_timing_latency_ms": 200.0,
            "paper_fills": [{
                "symbol": "SPY", "side": "buy", "qty": 100.0, "price": 450.50,
                "timestamp_ns": 300_000_000, "fee": 2.0, "slippage_bps": 10.0, "order_id": "p-1"
            }],
            "backtest_fills": [{
                "symbol": "SPY", "side": "buy", "qty": 100.0, "price": 450.00,
                "timestamp_ns": 0, "fee": 1.0, "slippage_bps": 2.0, "bar_price": 450.00, "order_id": "b-1"
            }]
        });
        let (status, body) = fetch_json(&app, "POST", "/api/v1/paper/gap/analyze", user, ws, &payload).await;
        assert_eq!(status, 200, "{body}");
        let gap_report = Uuid::parse_str(body["id"].as_str().expect("gap report id")).unwrap();
        assert_eq!(
            body["backtest_run_id"].as_str(),
            Some(run.to_string().as_str()),
            "the gap report links the run"
        );
        assert_eq!(body["strategy_id"].as_str(), Some(strategy.to_string().as_str()));

        Self { app, pool, user, ws, strategy, run, gap_report }
    }

    fn export_path(&self, format: &str) -> String {
        format!("/api/v1/paper/gap/reports/{}/export?format={format}", self.gap_report)
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM reports WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM backtest_paper_gap_reports WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ =
            sqlx::query("DELETE FROM runs WHERE workspace_id = $1").bind(self.ws).execute(&self.pool).await;
        let _ =
            sqlx::query("DELETE FROM jobs WHERE workspace_id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM strategies WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await;
    }
}

#[tokio::test]
async fn every_gap_report_export_is_a_new_version_and_lists_with_its_links() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // Three exports of the same report: versions 1, 2 and 3. The two CSVs are byte-identical and
    // still get their own versions (gap reports are not deduped, as for sweeps).
    let (status, content_type, version, first_csv) = fetch(&f.app, &f.export_path("csv"), f.user, f.ws).await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/csv; charset=utf-8");
    assert_eq!(version.as_deref(), Some("1"));
    let text = String::from_utf8(first_csv.clone()).unwrap();
    assert!(text.starts_with("# SpectraLabs M10 Backtest-to-Paper Gap Report\n"), "{text}");
    assert!(text.contains(&format!("# Gap Report ID: {}\n", f.gap_report)));
    assert!(text.contains(&format!("# Backtest Run ID: {}\n", f.run)));
    assert!(text.contains("\nsymbol,side,qty,paper_price,backtest_price,"));
    assert!(text.contains("\"SPY\",buy,100,450.5,450,"), "{text}");

    let (status, content_type, version, pdf) = fetch(&f.app, &f.export_path("PDF"), f.user, f.ws).await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/pdf");
    assert_eq!(version.as_deref(), Some("2"));
    assert!(pdf.starts_with(b"%PDF-1.4"), "the response is a PDF");
    let drawn = common::drawn_text(&pdf);
    assert!(drawn.contains("Backtest-to-Paper Gap Report"), "{drawn}");
    assert!(drawn.contains(&format!("Gap report {}", f.gap_report)), "{drawn}");
    assert!(!drawn.contains("Quantitative Strategy Tear-Sheet"), "{drawn}");

    let (status, content_type, version, third_csv) = fetch(&f.app, &f.export_path("csv"), f.user, f.ws).await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/csv; charset=utf-8");
    assert_eq!(version.as_deref(), Some("3"), "an identical export is still a new version");
    assert_eq!(third_csv, first_csv);

    // The run's gap reports: three versions, newest first, each linked to its backtest run and
    // paper strategy.
    let path = format!("/api/v1/reports?run_kind=paper_gap&run_id={}&report_type=gap", f.gap_report);
    let (status, body) = fetch_json(&f.app, "GET", &path, f.user, f.ws, &Value::Null).await;
    assert_eq!(status, 200, "{body}");
    let list = body.as_array().expect("a list of versions");
    let versions: Vec<i64> = list.iter().map(|v| v["version"].as_i64().unwrap()).collect();
    assert_eq!(versions, vec![3, 2, 1]);
    let formats: Vec<&str> = list.iter().map(|v| v["format"].as_str().unwrap()).collect();
    assert_eq!(formats, vec!["csv", "pdf", "csv"]);
    for v in list {
        assert_eq!(v["report_type"], "gap");
        assert_eq!(v["run_kind"], "paper_gap");
        assert_eq!(v["run_id"].as_str(), Some(f.gap_report.to_string().as_str()));
        assert_eq!(v["backtest_run_id"].as_str(), Some(f.run.to_string().as_str()));
        assert_eq!(v["paper_strategy_id"].as_str(), Some(f.strategy.to_string().as_str()));
    }

    f.clean_up().await;
}

#[tokio::test]
async fn another_workspaces_gap_report_is_not_found() {
    let Some(pool) = pool().await else { return };
    let mine = Fixture::setup(pool.clone()).await;
    let theirs = Fixture::setup(pool).await;

    let path = format!("/api/v1/paper/gap/reports/{}/export?format=csv", theirs.gap_report);
    let (status, _, _, _) = fetch(&mine.app, &path, mine.user, mine.ws).await;
    assert_eq!(status, 404);

    mine.clean_up().await;
    theirs.clean_up().await;
}

#[tokio::test]
async fn exporting_an_unknown_gap_report_is_not_found() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let path = format!("/api/v1/paper/gap/reports/{}/export?format=csv", spectra_db::new_id());
    let (status, _, _, _) = fetch(&f.app, &path, f.user, f.ws).await;
    assert_eq!(status, 404);

    f.clean_up().await;
}

#[tokio::test]
async fn an_unknown_export_format_is_rejected() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let (status, _, version, _) = fetch(&f.app, &f.export_path("xlsx"), f.user, f.ws).await;
    assert_eq!(status, 422);
    assert_eq!(version, None, "nothing is stored");

    f.clean_up().await;
}
