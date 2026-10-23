//! Module 9: Paper Trading tests.
//!
//! Covers:
//! - Task 1: Paper account model (one account per strategy, cash, positions, equity).
//! - Task 2: Order API (POST /api/v1/paper/orders, GET /api/v1/paper/orders).
//! - Task 3: Risk check on orders (M8 check, fail closed, rejected order stored with reason).
//! - Task 4 & 5: Fills and position/equity tracking.

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
    let sig = sign(
        SECRET.as_bytes(),
        &canonical(method, path, &ts, &u_str, &ws_str, "req-paper-1", body),
    );
    vec![
        ("x-spectra-user", u_str),
        ("x-spectra-workspace", ws_str),
        ("x-request-id", "req-paper-1".into()),
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

async fn read_json(res: axum::response::Response) -> (u16, Value) {
    let status = res.status().as_u16();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, val)
}

#[tokio::test]
async fn paper_routes_require_signature() {
    let cfg = ApiConfig::new(Secret::new(SECRET));
    let app = router(cfg, None);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/paper/orders")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn paper_routes_report_503_without_database() {
    let cfg = ApiConfig::new(Secret::new(SECRET));
    let app = router(cfg, None);
    let user = spectra_db::new_id();
    let ws = spectra_db::new_id();
    let strat = spectra_db::new_id();

    let req = signed_req(
        "GET",
        &format!("/api/v1/paper/account?strategy_id={strat}"),
        &user,
        &ws,
        vec![],
    );
    let (status, body) = read_json(app.oneshot(req).await.unwrap()).await;
    assert_eq!(status, 503);
    assert!(body["message"].as_str().unwrap().contains("Jobs need the database"));
}

#[tokio::test]
async fn paper_orders_report_503_without_database() {
    let cfg = ApiConfig::new(Secret::new(SECRET));
    let app = router(cfg, None);
    let user = spectra_db::new_id();
    let ws = spectra_db::new_id();

    let req = signed_req("POST", "/api/v1/paper/orders", &user, &ws, b"not json".to_vec());
    let (status, body) = read_json(app.oneshot(req).await.unwrap()).await;
    assert_eq!(status, 503);
    assert!(body["message"].as_str().unwrap().contains("Jobs need the database"));
}

struct TestFixture {
    app: Router,
    pool: PgPool,
    user: Uuid,
    ws: Uuid,
    strategy: Uuid,
}

impl TestFixture {
    async fn setup(pool: PgPool) -> Self {
        let user = spectra_db::new_id();
        let ws = spectra_db::new_id();
        let strategy = spectra_db::new_id();

        // Seed user and workspace
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
            .bind(user)
            .bind(format!("user-{}@example.com", user.simple()))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Test Workspace')")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, 'A test strat')",
        )
        .bind(strategy)
        .bind(ws)
        .bind(format!("strat-{}", strategy.simple()))
        .execute(&pool)
        .await
        .unwrap();

        let cfg = ApiConfig::new(Secret::new(SECRET));
        let app = router(cfg, Some(pool.clone()));

        Self {
            app,
            pool,
            user,
            ws,
            strategy,
        }
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM paper_fills WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM paper_orders WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM portfolio_positions WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM portfolio_balances WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM risk_limits WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM risk_kill_switches WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM risk_decisions WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM strategies WHERE id = $1")
            .bind(self.strategy)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(self.user)
            .execute(&self.pool)
            .await;
    }
}

#[tokio::test]
async fn paper_trading_full_lifecycle() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;

    // 1. Task 1: Paper Account Model
    // Query account: should auto-initialize with starting balance and 0 positions.
    let path = format!("/api/v1/paper/account?strategy_id={}", f.strategy);
    let req = signed_req("GET", &path, &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 200);
    assert_eq!(body["cash"], 100_000.0);
    assert_eq!(body["equity"], 100_000.0);
    assert_eq!(body["positions"].as_array().unwrap().len(), 0);

    // 2. Task 2: Order API - Place limit order without filling immediately
    let order_payload = json!({
        "strategy_id": f.strategy,
        "symbol": "AAPL",
        "side": "buy",
        "order_type": "limit",
        "qty": 50.0,
        "limit_price": 150.0,
        "fill_now": false
    });
    let req = signed_req(
        "POST",
        "/api/v1/paper/orders",
        &f.user,
        &f.ws,
        serde_json::to_vec(&order_payload).unwrap(),
    );
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 201);
    assert_eq!(body["status"], "accepted");
    assert_eq!(body["symbol"], "AAPL");
    assert_eq!(body["qty"], 50.0);
    let order_id = body["id"].as_str().unwrap().to_string();

    // List orders: verify order is present
    let path = format!("/api/v1/paper/orders?strategy_id={}", f.strategy);
    let req = signed_req("GET", &path, &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 200);
    let orders = body.as_array().unwrap();
    assert!(orders.iter().any(|o| o["id"].as_str().unwrap() == order_id));

    // Get order by id
    let path = format!("/api/v1/paper/orders/{order_id}");
    let req = signed_req("GET", &path, &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], order_id);

    // 3. Task 3: Risk Check on Orders (M8 pre-trade check)
    // Activate kill switch for the strategy
    let ks_payload = json!({
        "strategy_id": f.strategy,
        "active": true,
        "reason": "Emergency halt test"
    });
    let req = signed_req(
        "POST",
        "/api/v1/risk/kill-switch",
        &f.user,
        &f.ws,
        serde_json::to_vec(&ks_payload).unwrap(),
    );
    let (ks_status, _) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(ks_status, 200);

    // Place an opening order: should be rejected by kill switch
    let bad_order_payload = json!({
        "strategy_id": f.strategy,
        "symbol": "GOOGL",
        "side": "buy",
        "order_type": "market",
        "qty": 10.0,
        "fill_now": false
    });
    let req = signed_req(
        "POST",
        "/api/v1/paper/orders",
        &f.user,
        &f.ws,
        serde_json::to_vec(&bad_order_payload).unwrap(),
    );
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 422);
    assert_eq!(body["status"], "rejected");
    assert!(body["reject_reason"].as_str().unwrap().contains("Kill switch"));

    // Clear kill switch
    let ks_clear = json!({
        "strategy_id": f.strategy,
        "active": false,
        "reason": "Cleared"
    });
    let req = signed_req(
        "POST",
        "/api/v1/risk/kill-switch",
        &f.user,
        &f.ws,
        serde_json::to_vec(&ks_clear).unwrap(),
    );
    let _ = f.app.clone().oneshot(req).await;

    // 4. Task 4 & 5: Fills and Positions/Equity update
    // Submit a market order with fill_now: true
    let fill_order_payload = json!({
        "strategy_id": f.strategy,
        "symbol": "MSFT",
        "side": "buy",
        "order_type": "market",
        "qty": 100.0,
        "fill_price": 200.0,
        "fill_now": true
    });
    let req = signed_req(
        "POST",
        "/api/v1/paper/orders",
        &f.user,
        &f.ws,
        serde_json::to_vec(&fill_order_payload).unwrap(),
    );
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 201);
    assert_eq!(body["status"], "filled");
    assert_eq!(body["filled_qty"], 100.0);
    assert!(body["fills"].as_array().unwrap().len() >= 1);

    // Verify account state: cash deducted, position held, equity matches cash + marked positions
    let path = format!("/api/v1/paper/account?strategy_id={}", f.strategy);
    let req = signed_req("GET", &path, &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 200);
    let positions = body["positions"].as_array().unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0]["symbol"], "MSFT");
    assert_eq!(positions[0]["qty"], 100.0);

    // Cash is starting ($100,000) minus trade notional (100 * fill_price) minus fee ($1.00)
    let cash = body["cash"].as_f64().unwrap();
    let equity = body["equity"].as_f64().unwrap();
    let pos_qty = positions[0]["qty"].as_f64().unwrap();
    let pos_price = positions[0]["price"].as_f64().unwrap();

    assert!((equity - (cash + pos_qty * pos_price)).abs() < 0.01, "Equity must equal cash + marked positions");

    // Clean up DB rows
    f.clean_up().await;
}
