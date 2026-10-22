//! Module 8 integration tests: Pre-trade Risk Enforcement, Limit Controls,
//! Emergency Kill Switches, Decision Audit Logs, and Risk Metrics.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
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

struct Fixture {
    app: Router,
    pool: PgPool,
    dir: PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-risk-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM risk_decisions WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM risk_kill_switches WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM risk_limits WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await;
        let _ = std::fs::remove_dir_all(&self.dir);
    }

    async fn call_in(&self, ws: Uuid, method: &str, path: &str, body: &str) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws) = (self.user.to_string(), ws.to_string());
        let req_id = spectra_db::new_id().to_string();
        let sig =
            sign(SECRET.as_bytes(), &canonical(method, path, &ts, &user, &ws, &req_id, body.as_bytes()));
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn call(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let text = if body.is_null() { String::new() } else { body.to_string() };
        self.call_in(self.ws, method, path, &text).await
    }
}

#[tokio::test]
async fn limits_crud_and_fallbacks() {
    let Some(pool) = pool().await else { return };
    let fx = Fixture::new(pool);

    // 1. Initially returns default limits
    let (code, res) = fx.call("GET", "/api/v1/risk/limits", &Value::Null).await;
    assert_eq!(code, 200);
    assert_eq!(res["max_gross_exposure"], 1_000_000.0);
    assert_eq!(res["max_position_size"], 100_000.0);

    // 2. Upsert custom workspace limits
    let payload = json!({
        "strategy_id": null,
        "max_gross_exposure": 500_000.0,
        "max_net_exposure": 250_000.0,
        "max_position_size": 50_000.0,
        "max_concentration_pct": 0.20,
        "max_leverage": 2.5,
        "max_drawdown_pct": 0.10,
        "max_daily_loss": 10_000.0,
        "max_var_pct": 0.04,
        "size_reduction_enabled": true
    });
    let (code, res) = fx.call("PUT", "/api/v1/risk/limits", &payload).await;
    assert_eq!(code, 200);
    assert_eq!(res["max_gross_exposure"], 500_000.0);
    assert_eq!(res["max_position_size"], 50_000.0);
    assert_eq!(res["max_leverage"], 2.5);

    // 3. GET reflects updated limits
    let (code, res) = fx.call("GET", "/api/v1/risk/limits", &Value::Null).await;
    assert_eq!(code, 200);
    assert_eq!(res["max_gross_exposure"], 500_000.0);
    assert_eq!(res["max_position_size"], 50_000.0);

    fx.clean_up().await;
}

#[tokio::test]
async fn pre_trade_approvals_reductions_and_rejections() {
    let Some(pool) = pool().await else { return };
    let fx = Fixture::new(pool);

    // Configure limits: max position size $20,000, reduction enabled
    let payload = json!({
        "strategy_id": null,
        "max_gross_exposure": 100_000.0,
        "max_net_exposure": 100_000.0,
        "max_position_size": 20_000.0,
        "max_concentration_pct": 0.30,
        "max_leverage": 2.0,
        "max_drawdown_pct": 0.15,
        "max_daily_loss": 5_000.0,
        "max_var_pct": 0.05,
        "size_reduction_enabled": true
    });
    let (code, _) = fx.call("PUT", "/api/v1/risk/limits", &payload).await;
    assert_eq!(code, 200);

    // Case 1: Order within limits -> Approved
    let order_ok = json!({
        "strategy_id": null,
        "symbol": "AAPL",
        "side": "buy",
        "qty": 50.0,
        "price": 100.0,
        "portfolio": {
            "cash": 100_000.0,
            "equity": 100_000.0,
            "peak_equity": 100_000.0,
            "daily_start_equity": 100_000.0,
            "positions": {}
        }
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/check", &order_ok).await;
    assert_eq!(code, 200);
    assert_eq!(res["verdict"], "approved");
    assert_eq!(res["requested_qty"], 50.0);
    assert_eq!(res["approved_qty"], 50.0);
    assert!(res["breaches"].as_array().unwrap().is_empty());
    assert!(res["decision_id"].as_str().is_some());

    // Case 2: Order exceeds position limit ($30,000 > $20,000 limit) -> Reduced to 200 shares
    let order_exceeds = json!({
        "strategy_id": null,
        "symbol": "MSFT",
        "side": "buy",
        "qty": 300.0,
        "price": 100.0,
        "portfolio": {
            "cash": 100_000.0,
            "equity": 100_000.0,
            "peak_equity": 100_000.0,
            "daily_start_equity": 100_000.0,
            "positions": {}
        }
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/check", &order_exceeds).await;
    assert_eq!(code, 200);
    assert_eq!(res["verdict"], "reduced");
    assert_eq!(res["requested_qty"], 300.0);
    assert_eq!(res["approved_qty"], 200.0);
    assert_eq!(res["breaches"][0]["code"], "position_size_exceeded");

    // Case 3: Portfolio in severe drawdown (>15%) -> Rejected
    let order_drawdown = json!({
        "strategy_id": null,
        "symbol": "GOOG",
        "side": "buy",
        "qty": 10.0,
        "price": 100.0,
        "portfolio": {
            "cash": 75_000.0,
            "equity": 75_000.0,
            "peak_equity": 100_000.0, // 25% drawdown > 15% limit
            "daily_start_equity": 100_000.0,
            "positions": {}
        }
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/check", &order_drawdown).await;
    assert_eq!(code, 200);
    assert_eq!(res["verdict"], "rejected");
    assert_eq!(res["approved_qty"], 0.0);
    assert_eq!(res["breaches"][0]["code"], "drawdown_exceeded");

    // Verify audit logs record all 3 checks
    let (code, res) = fx.call("GET", "/api/v1/risk/audit", &Value::Null).await;
    assert_eq!(code, 200);
    let list = res.as_array().unwrap();
    assert!(list.len() >= 3);

    fx.clean_up().await;
}

#[tokio::test]
async fn emergency_kill_switch_lifecycle() {
    let Some(pool) = pool().await else { return };
    let fx = Fixture::new(pool);

    // 1. Initially no kill switch active
    let (code, res) = fx.call("GET", "/api/v1/risk/kill-switch", &Value::Null).await;
    assert_eq!(code, 200);
    assert!(res.as_array().unwrap().is_empty());

    // 2. Trigger emergency workspace kill switch
    let trip_req = json!({
        "strategy_id": null,
        "active": true,
        "reason": "Market anomaly detected across multiple pairs."
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/kill-switch", &trip_req).await;
    assert_eq!(code, 200);
    assert_eq!(res["active"], true);
    assert_eq!(res["reason"], "Market anomaly detected across multiple pairs.");

    // 3. Pre-trade check is rejected due to active kill switch
    let order = json!({
        "strategy_id": null,
        "symbol": "NVDA",
        "side": "buy",
        "qty": 10.0,
        "price": 100.0,
        "portfolio": null
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/check", &order).await;
    assert_eq!(code, 200);
    assert_eq!(res["verdict"], "rejected");
    assert_eq!(res["approved_qty"], 0.0);
    assert_eq!(res["breaches"][0]["code"], "kill_switch_active");

    // 4. Clear emergency kill switch
    let clear_req = json!({
        "strategy_id": null,
        "active": false,
        "reason": "Anomaly verified as benign; resume trading."
    });
    let (code, res) = fx.call("POST", "/api/v1/risk/kill-switch", &clear_req).await;
    assert_eq!(code, 200);
    assert_eq!(res["active"], false);

    // 5. Pre-trade check passes after clearance
    let (code, res) = fx.call("POST", "/api/v1/risk/check", &order).await;
    assert_eq!(code, 200);
    assert_eq!(res["verdict"], "approved");
    assert_eq!(res["approved_qty"], 10.0);

    // 6. Metrics summary endpoint returns status
    let (code, res) = fx.call("GET", "/api/v1/risk/metrics", &Value::Null).await;
    assert_eq!(code, 200);
    assert_eq!(res["kill_switch_active"], false);
    assert!(res["gross_exposure_limit"].as_f64().unwrap() > 0.0);

    fx.clean_up().await;
}
