//! Integration tests for M12 Alpha Workbench API routes and M42 Sentiment Gate.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping test_alpha_workbench_flow: DATABASE_URL not set");
        return None;
    };
    let pool = connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but database unreachable");
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
    let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u_str, &ws_str, "req-alpha-1", body));
    vec![
        ("x-spectra-user", u_str),
        ("x-spectra-workspace", ws_str),
        ("x-request-id", "req-alpha-1".into()),
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

#[tokio::test]
async fn test_alpha_workbench_flow_and_sentiment_gate() {
    let Some(pool) = pool().await else { return };

    let ws = spectra_db::new_id();
    let other_ws = spectra_db::new_id();
    let editor_user = spectra_db::new_id();
    let viewer_user = spectra_db::new_id();

    // Seed test workspaces and roles
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(ws)
        .bind(format!("ws-{}", ws.simple()))
        .bind("Alpha Test WS")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)")
        .bind(other_ws)
        .bind(format!("ws-{}", other_ws.simple()))
        .bind("Other WS")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
        .bind(editor_user)
        .bind(format!("editor-{}@example.com", editor_user.simple()))
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
        .bind(viewer_user)
        .bind(format!("viewer-{}@example.com", viewer_user.simple()))
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'editor')")
        .bind(ws)
        .bind(editor_user)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')")
        .bind(ws)
        .bind(viewer_user)
        .execute(&pool)
        .await
        .unwrap();

    let mut cfg = ApiConfig::new(Secret::new(SECRET));
    cfg.cors_origins = vec!["http://localhost:3000".into()];
    let app = router(cfg, Some(pool.clone()));

    // 1. Task 7 Gate Initial Check: M42 sentiment is BLOCKED before any signal test passes owner threshold
    let req = signed_req("GET", "/api/v1/alpha/m42/sentiment", &editor_user, &ws, Vec::new());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 403, "sentiment score must be blocked before signal test passes");

    // 2. Viewer Role Check: viewer cannot run a signal test (403)
    let payload = json!({
        "signal_name": "momentum_5d",
    });
    let req = signed_req(
        "POST",
        "/api/v1/alpha/signal-tests",
        &viewer_user,
        &ws,
        serde_json::to_vec(&payload).unwrap(),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 403, "viewer should not be allowed to run signal tests");

    // 3. Editor Runs a Passing Signal Test with Owner Threshold (Tasks 1-4, 6, 7)
    let payload = json!({
        "signal_name": "sentiment",
        "threshold": {
            "min_mean_ic": -1.0, // lenient threshold to ensure pass
            "min_t_stat": -10.0,
            "max_turnover": 1.0
        }
    });
    let req = signed_req(
        "POST",
        "/api/v1/alpha/signal-tests",
        &editor_user,
        &ws,
        serde_json::to_vec(&payload).unwrap(),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let test_view: Value = serde_json::from_slice(&body).unwrap();
    let test_id_str = test_view["id"].as_str().unwrap();
    let test_id = Uuid::parse_str(test_id_str).unwrap();

    assert_eq!(test_view["signal_name"], "sentiment");
    assert_eq!(test_view["passed_gate"], true);
    assert_eq!(test_view["disclaimer"], "research only, not a prediction");

    // 4. Access Test: Other workspaces get 404 (Isolation)
    let req = signed_req(
        "GET",
        &format!("/api/v1/alpha/signal-tests/{test_id}"),
        &editor_user,
        &other_ws,
        Vec::new(),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404, "cross-workspace access should return 404");

    // 5. Workspace member can get details (200)
    let req =
        signed_req("GET", &format!("/api/v1/alpha/signal-tests/{test_id}"), &viewer_user, &ws, Vec::new());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // 6. Task 5 & M16 Export: Stored versioned PDF and JSON reports
    let req = signed_req(
        "GET",
        &format!("/api/v1/alpha/signal-tests/{test_id}/export?format=pdf"),
        &editor_user,
        &ws,
        Vec::new(),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("x-report-id"));
    assert!(resp.headers().contains_key("x-report-version"));
    assert!(resp.headers().contains_key("x-report-sha256"));
    let pdf_body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(pdf_body.starts_with(b"%PDF-"), "export should produce valid PDF");

    // 7. Task 7 Gate Success: M42 sentiment score is now ACTIVE and UNBLOCKED
    let req =
        signed_req("GET", "/api/v1/alpha/m42/sentiment?signal_name=sentiment", &viewer_user, &ws, Vec::new());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200, "sentiment score must be visible after passing signal test");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let sent_resp: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(sent_resp["status"], "active");
}
