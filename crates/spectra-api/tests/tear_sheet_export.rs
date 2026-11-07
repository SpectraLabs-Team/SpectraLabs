//! M16 Task 6: the tear sheet export route stores a PDF as a report version.
//!
//! The database tests skip when `DATABASE_URL` is unset, like the other API flows. The fixture
//! follows `m11_attribution_access.rs`: rows are inserted directly, with no backtest runner.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::json;
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
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

/// A signed GET as `user` in `ws`.
fn signed_get(path: &str, user: Uuid, ws: Uuid) -> Request<Body> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
    let u = user.to_string();
    let w = ws.to_string();
    let rid = "req-tear-sheet-export";
    let sig = sign(SECRET.as_bytes(), &canonical("GET", path, &ts, &u, &w, rid, b""));
    Request::builder()
        .method("GET")
        .uri(path)
        .header("x-spectra-user", u)
        .header("x-spectra-workspace", w)
        .header("x-request-id", rid)
        .header("x-spectra-timestamp", ts)
        .header("x-spectra-signature", sig)
        .body(Body::empty())
        .unwrap()
}

/// Status, the report version header, and the body bytes.
async fn fetch(app: &Router, path: &str, user: Uuid, ws: Uuid) -> (u16, Option<String>, Vec<u8>) {
    let res = app.clone().oneshot(signed_get(path, user, ws)).await.unwrap();
    let status = res.status().as_u16();
    let version = res.headers().get("x-report-version").and_then(|v| v.to_str().ok()).map(String::from);
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, version, bytes)
}

struct Fixture {
    app: Router,
    pool: PgPool,
    user: Uuid,
    ws: Uuid,
    succeeded: Uuid,
    running: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let (user, ws) = (spectra_db::new_id(), spectra_db::new_id());
        let (succeeded, running) = (spectra_db::new_id(), spectra_db::new_id());

        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
            .bind(user)
            .bind(format!("tear-sheet-{}@example.com", user.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Tear sheet export test')")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .execute(&pool)
            .await
            .unwrap();

        let summary = json!({ "summary": {
            "metrics": { "total_return": 0.042, "periods": 252, "annualized_volatility": 0.15, "sharpe": 1.2 },
            "performance": { "cagr": 0.04, "sortino": 1.5, "calmar": 0.8 },
            "drawdown": { "max_drawdown": -0.0525, "longest_underwater_periods": 40 },
            "run": { "config": { "initial_cash": "100000" } },
            "activity": { "final_equity": 104_200.0, "realized_pnl": 4200.0 }
        } });
        sqlx::query(
            "INSERT INTO jobs (id, workspace_id, kind, status, finished_at, result, created_by) \
             VALUES ($1, $2, 'backtest', 'succeeded', now(), $3, $4)",
        )
        .bind(succeeded)
        .bind(ws)
        .bind(summary)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO jobs (id, workspace_id, kind, status, created_by) \
             VALUES ($1, $2, 'backtest', 'running', $3)",
        )
        .bind(running)
        .bind(ws)
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
        Self { app, pool, user, ws, succeeded, running }
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM reports WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ =
            sqlx::query("DELETE FROM jobs WHERE workspace_id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await;
    }
}

#[tokio::test]
async fn tear_sheet_export_stores_a_pdf_version_for_a_succeeded_backtest() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let path = format!("/api/v1/backtests/{}/tear-sheet/export", f.succeeded);
    let (status, version, body) = fetch(&f.app, &path, f.user, f.ws).await;
    assert_eq!(status, 200);
    assert_eq!(version.as_deref(), Some("1"));
    assert!(body.starts_with(b"%PDF-1.4"), "the response is a PDF");

    // The same sheet again: identical bytes, so the newest version is reused, not a new one.
    let (status, version, again) = fetch(&f.app, &path, f.user, f.ws).await;
    assert_eq!(status, 200);
    assert_eq!(version.as_deref(), Some("1"), "identical content reuses version 1");
    assert_eq!(again, body);

    f.clean_up().await;
}

#[tokio::test]
async fn tear_sheet_export_of_an_unknown_backtest_is_not_found() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let path = format!("/api/v1/backtests/{}/tear-sheet/export", spectra_db::new_id());
    let (status, _, _) = fetch(&f.app, &path, f.user, f.ws).await;
    assert_eq!(status, 404);

    f.clean_up().await;
}

#[tokio::test]
async fn tear_sheet_export_of_a_running_backtest_is_a_conflict() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let path = format!("/api/v1/backtests/{}/tear-sheet/export", f.running);
    let (status, _, _) = fetch(&f.app, &path, f.user, f.ws).await;
    assert_eq!(status, 409);

    f.clean_up().await;
}

#[tokio::test]
async fn tear_sheet_export_over_the_size_limit_is_refused_and_not_stored() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // One strategy whose name is over 5 MiB: the table cell is drawn as is, so the PDF is too.
    let name = "a".repeat(spectra_db::reports::REPORT_MAX_BYTES + 1);
    let huge = spectra_db::new_id();
    let summary = json!({ "summary": {
        "metrics": { "total_return": 0.042 },
        "strategies": { "list": [ { "strategy": name, "allocation": 50_000 } ] }
    } });
    sqlx::query(
        "INSERT INTO jobs (id, workspace_id, kind, status, finished_at, result, created_by) \
         VALUES ($1, $2, 'backtest', 'succeeded', now(), $3, $4)",
    )
    .bind(huge)
    .bind(f.ws)
    .bind(summary)
    .bind(f.user)
    .execute(&f.pool)
    .await
    .unwrap();

    let path = format!("/api/v1/backtests/{huge}/tear-sheet/export");
    let res = f.app.clone().oneshot(signed_get(&path, f.user, f.ws)).await.unwrap();
    assert_eq!(res.status().as_u16(), 413);
    for h in ["x-report-id", "x-report-version", "x-report-sha256", "content-disposition"] {
        assert!(res.headers().get(h).is_none(), "no {h} header on a refused report");
    }
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "payload_too_large", "{err}");
    assert_eq!(err["message"], "Report is too large to store.");
    assert_eq!(err["detail"], "Report is too large to store.");

    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM reports WHERE workspace_id = $1 AND run_id = $2")
            .bind(f.ws)
            .bind(huge)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(rows, 0, "the refused report leaves no row");

    f.clean_up().await;
}
