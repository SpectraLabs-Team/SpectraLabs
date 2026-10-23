//! Backtest-to-Paper Gap Analysis (M10) and Observability Integration Tests.

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

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn now() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string()
}

fn signature_headers(
    method: &str,
    path: &str,
    user: &Uuid,
    ws: &Uuid,
    body: &[u8],
) -> Vec<(&'static str, String)> {
    let ts = now();
    let u_str = user.to_string();
    let ws_str = ws.to_string();
    let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u_str, &ws_str, "req-gap-1", body));
    vec![
        ("x-spectra-user", u_str),
        ("x-spectra-workspace", ws_str),
        ("x-request-id", "req-gap-1".into()),
        ("x-spectra-timestamp", ts),
        ("x-spectra-signature", sig),
    ]
}

fn signed_req(method: &str, path: &str, user: &Uuid, ws: &Uuid, body: Vec<u8>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(path).header("content-type", "application/json");
    for (k, v) in signature_headers(method, path, user, ws, &body) {
        b = b.header(k, v);
    }
    b.body(Body::from(body)).unwrap()
}

async fn response_json(res: axum::response::Response) -> (u16, Value) {
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, val)
}

fn test_app(pool: Option<PgPool>) -> Router {
    let mut cfg = ApiConfig::new(Secret::new(SECRET.to_string()));
    cfg.cors_origins = vec!["http://localhost:3000".into()];
    router(cfg, pool)
}

#[tokio::test]
async fn test_sentry_observability_endpoints() {
    let app = test_app(None);

    // 1. GET /api/v1/observability/sentry
    let req = Request::builder().uri("/api/v1/observability/sentry").body(Body::empty()).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_json(res).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("enabled").is_some());
    assert!(body.get("dsn_configured").is_some());

    // 2. POST /api/v1/observability/sentry/verify
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/observability/sentry/verify")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "message": "Automated integration test sentry ping" }).to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let (status, body) = response_json(res).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("delivered").is_some());
}

#[tokio::test]
async fn test_backtest_to_paper_gap_analysis_flow() {
    let Some(pool) = pool().await else { return };
    let app = test_app(Some(pool.clone()));

    let user_id = Uuid::now_v7();
    let ws_id = Uuid::now_v7();
    let strat_id = Uuid::now_v7();

    // Seed workspace and strategy
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .bind("gap-tester@example.com")
        .bind("hash")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(ws_id)
        .bind("Gap Workspace")
        .bind(format!("gap-ws-{}", &Uuid::now_v7().to_string()[..8]))
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
    )
    .bind(strat_id)
    .bind(ws_id)
    .bind("Gap Strategy")
    .bind("Testing backtest to paper gap")
    .execute(&pool)
    .await
    .unwrap();

    // 1. Submit gap analysis with realistic slippage and timing delta
    let payload = json!({
        "strategy_id": strat_id,
        "max_slippage_drift_bps": 5.0,
        "max_timing_latency_ms": 200.0,
        "paper_fills": [
            {
                "symbol": "SPY",
                "side": "buy",
                "qty": 100.0,
                "price": 450.50,
                "timestamp_ns": 300_000_000,
                "fee": 2.0,
                "slippage_bps": 10.0,
                "order_id": "p-1"
            }
        ],
        "backtest_fills": [
            {
                "symbol": "SPY",
                "side": "buy",
                "qty": 100.0,
                "price": 450.00,
                "timestamp_ns": 0,
                "fee": 1.0,
                "slippage_bps": 2.0,
                "bar_price": 450.00,
                "order_id": "b-1"
            }
        ]
    });

    let req =
        signed_req("POST", "/api/v1/paper/gap/analyze", &user_id, &ws_id, payload.to_string().into_bytes());
    let res = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_json(res).await;
    assert_eq!(status, 200, "{body}");

    let report_id = body["id"].as_str().expect("report id");
    assert_ne!(body["alert_status"].as_str().unwrap(), "normal");
    assert!(body["cost_gap"].as_f64().unwrap() > 0.0);
    assert!(body["slippage_gap"].as_f64().unwrap() > 0.0);

    // 2. Fetch list of reports
    let list_req = signed_req(
        "GET",
        &format!("/api/v1/paper/gap/reports?strategy_id={strat_id}"),
        &user_id,
        &ws_id,
        Vec::new(),
    );
    let res = app.clone().oneshot(list_req).await.unwrap();
    let (status, body) = response_json(res).await;
    assert_eq!(status, 200, "{body}");
    let arr = body.as_array().expect("array of reports");
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["id"].as_str().unwrap(), report_id);

    // 3. Fetch specific report detail
    let detail_req =
        signed_req("GET", &format!("/api/v1/paper/gap/reports/{report_id}"), &user_id, &ws_id, Vec::new());
    let res = app.oneshot(detail_req).await.unwrap();
    let (status, body) = response_json(res).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["id"].as_str().unwrap(), report_id);
    assert!(body["report"]["attribution"].is_object());
    assert!(body["report"]["alerts"].is_array());
}
