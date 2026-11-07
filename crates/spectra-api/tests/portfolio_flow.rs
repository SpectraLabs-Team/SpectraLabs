//! Saved portfolios (M15 Task 1) through the signed API. Needs `DATABASE_URL`; without it the
//! tests pass without doing anything.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::{connect, migrate, seed_dev, PgPool, PoolConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use tower::ServiceExt;
use uuid::Uuid;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh() -> String {
    format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

fn app(pool: PgPool) -> Router {
    let mut cfg = ApiConfig::new(Secret::new(SECRET.to_string()));
    cfg.cors_origins = vec!["http://localhost:3000".into()];
    router(cfg, Some(pool))
}

fn signed(method: &str, path: &str, user: &Uuid, ws: &Uuid, body: Vec<u8>) -> Request<Body> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
    let (u, w) = (user.to_string(), ws.to_string());
    let rid = format!("req-portfolio-{}", fresh());
    let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u, &w, &rid, &body));
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

async fn send(app: &Router, req: Request<Body>) -> (u16, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn unique_name() -> String {
    format!("flow-{}", fresh())
}

#[tokio::test]
async fn create_list_and_get_a_portfolio() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-flow@example.com").await.unwrap();
    let app = app(pool);
    let name = unique_name();
    let body = json!({"name": name, "holdings": [{"symbol": "AAA", "weight": 0.5}, {"symbol": "BBB", "weight": 0.5}]});

    let (status, created) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["holdings"].as_array().unwrap().len(), 2);

    let (status, got) = send(
        &app,
        signed("GET", &format!("/api/v1/portfolios/{id}"), &seeded.admin_id, &seeded.workspace_id, vec![]),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(got["name"], name);
    assert_eq!(got["holdings"], created["holdings"]);

    let (status, list) =
        send(&app, signed("GET", "/api/v1/portfolios", &seeded.admin_id, &seeded.workspace_id, vec![])).await;
    assert_eq!(status, 200);
    assert!(list.as_array().unwrap().iter().any(|p| p["id"] == id));
}

#[tokio::test]
async fn invalid_weight_sum_is_refused_with_a_clear_message() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-badsum@example.com").await.unwrap();
    let app = app(pool);
    let body = json!({"name": unique_name(), "holdings": [{"symbol": "AAA", "weight": 0.5}, {"symbol": "BBB", "weight": 0.4}]});

    let (status, err) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 422);
    assert!(err.to_string().contains("sum to"), "{err}");
}

#[tokio::test]
async fn duplicate_name_is_a_conflict() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-dupe@example.com").await.unwrap();
    let app = app(pool);
    let body = json!({"name": unique_name(), "holdings": [{"symbol": "AAA", "weight": 1.0}]}).to_string();

    let (first, _) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.clone().into_bytes(),
        ),
    )
    .await;
    assert_eq!(first, 201);
    let (second, _) = send(
        &app,
        signed("POST", "/api/v1/portfolios", &seeded.admin_id, &seeded.workspace_id, body.into_bytes()),
    )
    .await;
    assert_eq!(second, 409);
}

#[tokio::test]
async fn a_portfolio_from_another_workspace_is_not_found() {
    let Some(pool) = pool().await else { return };
    let owner = seed_dev(&pool, "portfolio-api-owner@example.com").await.unwrap();
    let other = seed_dev(&pool, "portfolio-api-other@example.com").await.unwrap();
    let other_ws: Uuid = sqlx::query_scalar("SELECT id FROM workspaces WHERE personal_owner_id = $1")
        .bind(other.admin_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let app = app(pool);
    let body = json!({"name": unique_name(), "holdings": [{"symbol": "AAA", "weight": 1.0}]});

    let (status, created) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &owner.admin_id,
            &owner.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 201);
    let id = created["id"].as_str().unwrap();

    let (status, _) =
        send(&app, signed("GET", &format!("/api/v1/portfolios/{id}"), &other.admin_id, &other_ws, vec![]))
            .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn equal_weight_gives_each_of_four_assets_25_percent() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-equal@example.com").await.unwrap();
    let app = app(pool);
    let body = json!({
        "name": unique_name(),
        "method": "equal",
        "holdings": [{"symbol": "AAA"}, {"symbol": "BBB"}, {"symbol": "CCC"}, {"symbol": "DDD"}]
    });

    let (status, created) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["method"], "equal");
    let holdings = created["holdings"].as_array().unwrap();
    assert_eq!(holdings.len(), 4);
    for h in holdings {
        assert_eq!(h["weight"].as_f64().unwrap(), 0.25, "{h}");
    }
}

#[tokio::test]
async fn inverse_volatility_matches_hand_calculation_and_is_stored_as_its_method() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-invvol@example.com").await.unwrap();
    let app = app(pool);
    // vols 0.10, 0.20, 0.40 -> raw 10, 5, 2.5 -> sum 17.5
    let body = json!({
        "name": unique_name(),
        "method": "inverse_volatility",
        "holdings": [
            {"symbol": "AAA", "volatility": 0.10},
            {"symbol": "BBB", "volatility": 0.20},
            {"symbol": "CCC", "volatility": 0.40}
        ]
    });

    let (status, created) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["method"], "inverse_volatility");
    let expected = [("AAA", 10.0 / 17.5), ("BBB", 5.0 / 17.5), ("CCC", 2.5 / 17.5)];
    let holdings = created["holdings"].as_array().unwrap();
    for (symbol, want) in expected {
        let got = holdings.iter().find(|h| h["symbol"] == symbol).unwrap()["weight"].as_f64().unwrap();
        assert!((got - want).abs() < 1e-12, "{symbol}: {got} vs {want}");
    }
}

#[tokio::test]
async fn inverse_volatility_without_a_volatility_is_refused() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-novol@example.com").await.unwrap();
    let app = app(pool);
    let body = json!({
        "name": unique_name(),
        "method": "inverse_volatility",
        "holdings": [{"symbol": "AAA", "volatility": 0.1}, {"symbol": "BBB"}]
    });

    let (status, err) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 422);
    assert!(err.to_string().contains("BBB") && err.to_string().contains("volatility"), "{err}");
}

#[tokio::test]
async fn unknown_method_is_refused() {
    let Some(pool) = pool().await else { return };
    let seeded = seed_dev(&pool, "portfolio-api-method@example.com").await.unwrap();
    let app = app(pool);
    let body = json!({"name": unique_name(), "method": "risk_parity", "holdings": [{"symbol": "AAA", "weight": 1.0}]});

    let (status, _) = send(
        &app,
        signed(
            "POST",
            "/api/v1/portfolios",
            &seeded.admin_id,
            &seeded.workspace_id,
            body.to_string().into_bytes(),
        ),
    )
    .await;
    assert_eq!(status, 422);
}
