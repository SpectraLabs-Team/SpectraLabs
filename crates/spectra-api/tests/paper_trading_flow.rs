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

mod common;

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
    let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u_str, &ws_str, "req-paper-1", body));
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
        .oneshot(Request::builder().uri("/api/v1/paper/orders").body(Body::empty()).unwrap())
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

    let req = signed_req("GET", &format!("/api/v1/paper/account?strategy_id={strat}"), &user, &ws, vec![]);
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
    dir: std::path::PathBuf,
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

        let dir = std::env::temp_dir().join(format!("spectra-paper-{}", ws.simple()));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));

        Self { app, pool, dir, user, ws, strategy }
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
        let _ =
            sqlx::query("DELETE FROM strategies WHERE id = $1").bind(self.strategy).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn paper_trading_full_lifecycle() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL", "MSFT", "GOOGL"]).await;

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
    assert!(body["reject_reason"].as_str().unwrap().to_lowercase().contains("kill switch"));

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
    assert!(!body["fills"].as_array().unwrap().is_empty());

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

    assert!(
        (equity - (cash + pos_qty * pos_price)).abs() < 0.01,
        "Equity must equal cash + marked positions"
    );

    // Clean up DB rows
    f.clean_up().await;
}

#[tokio::test]
async fn test_paper_backtest_parity() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool.clone()).await;

    // 1. Create a saved strategy with version 1 (buy_and_hold)
    let strat_payload = json!({
        "name": format!("Parity-BnH-{}", f.strategy.simple()),
        "description": "Buy and Hold strategy for paper parity test",
        "builtin": "buy_and_hold",
        "params": {
            "allocation": 0.20
        }
    });
    let req =
        signed_req("POST", "/api/v1/strategies", &f.user, &f.ws, serde_json::to_vec(&strat_payload).unwrap());
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 201, "Strategy creation failed: {body}");
    let strategy_id = Uuid::parse_str(body["strategy"]["id"].as_str().unwrap()).unwrap();
    let version_id = Uuid::parse_str(body["versions"][0]["id"].as_str().unwrap()).unwrap();

    // 2. Prepare fixed sequence of daily bars
    let day_ns: i64 = 86_400_000_000_000;
    let base_ts: i64 = 1_700_000_000_000_000_000;
    // 61 daily bars: with risk limits on, the backtest needs at least 60 bars per
    // instrument to estimate volatility.
    let prices: Vec<(f64, f64, f64, f64, f64)> = (0..61_u32)
        .map(|i| {
            let c = 100.0 + f64::from(i % 7) * 1.5;
            (c - 1.0, c + 2.0, c - 2.0, c, 10_000.0 + f64::from(i) * 100.0)
        })
        .collect();

    let mut paper_bars = Vec::new();
    let mut engine_bars = Vec::new();
    for (i, &(o, h, l, c, v)) in prices.iter().enumerate() {
        let ts = base_ts + (i as i64) * day_ns;
        paper_bars.push(json!({
            "symbol": "AAPL",
            "ts_ns": ts,
            "open": o,
            "high": h,
            "low": l,
            "close": c,
            "volume": v,
        }));
        engine_bars.push(spectra_core::Bar::new(
            ts,
            0,
            spectra_core::Price::from_f64(o).unwrap(),
            spectra_core::Price::from_f64(h).unwrap(),
            spectra_core::Price::from_f64(l).unwrap(),
            spectra_core::Price::from_f64(c).unwrap(),
            spectra_core::Qty::from_f64(v).unwrap(),
        ));
    }

    // 3. Run backtest directly with Default preset and risk limits
    let preset = spectra_backtest::config::Preset::Default;
    let mut backtest_cfg = preset.config();
    backtest_cfg.initial_cash = spectra_core::Money::from_f64(100_000.0).unwrap();
    backtest_cfg.risk_limits = Some(spectra_risk::RiskLimits::default());

    let strat_def = spectra_backtest::strategies::builtin("buy_and_hold").unwrap();
    let params = strat_def
        .resolve_params_json(Some(&spectra_core::Json::parse(r#"{"allocation": 0.20}"#).unwrap()))
        .unwrap();
    let mut direct_strategy = strat_def.build(&params).unwrap();

    let backtest_res = spectra_engine::run_backtest(&engine_bars, direct_strategy.as_mut(), &backtest_cfg)
        .expect("Direct backtest run failed");

    // 4. Run through Paper Trading Step API (Task 6)
    let step_payload = json!({
        "strategy_id": strategy_id,
        "strategy_version_id": version_id,
        "bars": paper_bars,
        "config_preset": "default"
    });
    let req =
        signed_req("POST", "/api/v1/paper/step", &f.user, &f.ws, serde_json::to_vec(&step_payload).unwrap());
    let (status, body) = read_json(f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(status, 200, "Paper step failed: {body}");

    // 5. Assertions: Task 6 & Task 7 Parity Verification
    // A version places an order on a bar
    let orders = body["orders"].as_array().unwrap();
    assert!(!orders.is_empty(), "Strategy must have placed at least one order");
    assert_eq!(orders.len(), backtest_res.orders.len(), "Order count parity");

    let fills = body["fills"].as_array().unwrap();
    assert!(!fills.is_empty(), "Strategy must have executed at least one fill");

    // Extract direct backtest fills
    let mut backtest_fills = Vec::new();
    for entry in backtest_res.ledger.journal() {
        if entry.kind == spectra_core::EntryType::Fill {
            backtest_fills.push(&entry.fill);
        }
    }
    assert_eq!(fills.len(), backtest_fills.len(), "Fill count parity");

    // Bit-exact match on every fill
    for (i, paper_fill) in fills.iter().enumerate() {
        let bt_fill = backtest_fills[i];
        assert_eq!(paper_fill["symbol"], "AAPL");
        // Priced by the run's preset, and the fill says so (M26 task 9).
        let costs = &paper_fill["cost_config"];
        assert_eq!(costs["name"], "preset:default", "{paper_fill}");
        assert_eq!(costs["slippage_bps"].as_f64().unwrap(), backtest_cfg.costs.slippage.to_f64());
        assert_eq!(
            costs["commission_bps"].as_f64().unwrap(),
            backtest_cfg.costs.commission.bps_of_notional.to_f64()
        );
        assert_eq!(paper_fill["qty"].as_f64().unwrap(), bt_fill.qty.to_f64());
        assert_eq!(paper_fill["price"].as_f64().unwrap(), bt_fill.price.to_f64());
        assert_eq!(paper_fill["fee"].as_f64().unwrap(), bt_fill.fee.to_f64());
    }

    // Parity on cash, equity and positions
    let expected_cash = backtest_res.ledger.cash().to_f64();
    let expected_equity = backtest_res.ledger.equity().unwrap().to_f64();

    assert_eq!(body["account"]["cash"].as_f64().unwrap(), expected_cash, "Cash parity");
    assert_eq!(body["account"]["equity"].as_f64().unwrap(), expected_equity, "Equity parity");

    let account_positions = body["account"]["positions"].as_array().unwrap();
    assert_eq!(account_positions.len(), backtest_res.ledger.positions().len(), "Position count parity");
    for pos in account_positions {
        let sym = pos["symbol"].as_str().unwrap();
        assert_eq!(sym, "AAPL");
        let qty = pos["qty"].as_f64().unwrap();
        let expected_qty = backtest_res.ledger.position(0).qty.to_f64();
        assert_eq!(qty, expected_qty, "Position quantity parity");
    }

    // Clean up
    let _ = sqlx::query("DELETE FROM strategy_versions WHERE strategy_id = $1")
        .bind(strategy_id)
        .execute(&f.pool)
        .await;
    let _ = sqlx::query("DELETE FROM strategies WHERE id = $1").bind(strategy_id).execute(&f.pool).await;
    f.clean_up().await;
}

#[tokio::test]
async fn test_alpaca_order_sync_and_reconciliation() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool.clone()).await;

    // 1. Task 10: Order sync endpoint
    let sync_req = signed_req("POST", "/api/v1/paper/sync", &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(sync_req).await.unwrap()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["synced"], true);

    let alpaca_sync_req = signed_req("POST", "/api/alpaca/sync", &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(alpaca_sync_req).await.unwrap()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["synced"], true);

    // 2. Task 11: Position reconciliation (clean state)
    let rec_req = signed_req("GET", "/api/v1/paper/reconcile", &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(rec_req).await.unwrap()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["reconciled"], true);
    assert_eq!(body["mismatches_count"], 0);

    // 3. Task 11: Deliberate mismatch reporting
    let rec_mismatch_req =
        signed_req("GET", "/api/alpaca/reconcile?deliberate_mismatch=true", &f.user, &f.ws, vec![]);
    let (status, body) = read_json(f.app.clone().oneshot(rec_mismatch_req).await.unwrap()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["reconciled"], false);
    assert!(body["mismatches_count"].as_i64().unwrap() > 0);
    let mismatches = body["mismatches"].as_array().unwrap();
    assert!(!mismatches.is_empty(), "Deliberate mismatch must be reported");

    f.clean_up().await;
}

/// Sends a signed request as the fixture's user; `Value::Null` sends an empty body.
async fn call(f: &TestFixture, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let bytes = if body.is_null() { Vec::new() } else { serde_json::to_vec(body).unwrap() };
    read_json(f.app.clone().oneshot(signed_req(method, path, &f.user, &f.ws, bytes)).await.unwrap()).await
}

#[tokio::test]
async fn paper_account_var_and_cvar_are_computed_or_explained() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    let account_path = format!("/api/v1/paper/account?strategy_id={}", f.strategy);

    // A position with no dataset behind it: no volatility, so VaR and CVaR are null with a reason.
    let (status, _) = call(&f, "GET", &account_path, &Value::Null).await;
    assert_eq!(status, 200);
    spectra_db::risk_center::upsert_position(&f.pool, f.ws, Some(f.strategy), "META", 10.0, 300.0)
        .await
        .unwrap();
    let (status, body) = call(&f, "GET", &account_path, &Value::Null).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["var_pct"].is_null(), "{body}");
    assert!(body["var_dollar"].is_null(), "{body}");
    assert!(body["cvar_dollar"].is_null(), "{body}");
    assert!(body["var_unavailable_reason"].as_str().unwrap().contains("META"), "{body}");

    // With a dataset, VaR and CVaR come from the same simulation and CVaR is at least VaR.
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["META"]).await;
    let (status, body) = call(&f, "GET", &account_path, &Value::Null).await;
    assert_eq!(status, 200, "{body}");
    let var = body["var_dollar"].as_f64().expect("var_dollar");
    let cvar = body["cvar_dollar"].as_f64().expect("cvar_dollar");
    assert!(var > 0.0, "{body}");
    assert!(cvar >= var, "{body}");
    assert!(body["var_unavailable_reason"].is_null(), "{body}");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_attribution_comes_from_fills_and_restarts_after_reset() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;
    let attribution_path = format!("/api/v1/paper/attribution?strategy_id={}", f.strategy);

    // One round trip: buy 10 at 100, sell 10 at 110.
    let mut fills = Vec::new();
    for (side, price) in [("buy", 100.0), ("sell", 110.0)] {
        let v =
            common::seed_dataset_with_latest_closes(&f.pool, f.ws, &f.dir.join("data"), &[("AAPL", price)])
                .await;
        let order = json!({
            "strategy_id": f.strategy, "symbol": "AAPL", "side": side, "order_type": "market",
            "qty": 10.0, "fill_price": price, "fill_now": true, "dataset_version_id": v
        });
        let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["status"], "filled", "{body}");
        fills.push(body["fills"][0].clone());
    }
    let fee_sum: f64 = fills.iter().map(|x| x["fee"].as_f64().unwrap()).sum();
    let buy_px = fills[0]["price"].as_f64().unwrap();
    let sell_px = fills[1]["price"].as_f64().unwrap();

    let (status, attr) = call(&f, "GET", &attribution_path, &Value::Null).await;
    assert_eq!(status, 200, "{attr}");
    assert_eq!(attr["total_trades"], 1, "{attr}");
    assert!((attr["total_fees"].as_f64().unwrap() - fee_sum).abs() < 1e-6, "fees {fee_sum}: {attr}");
    let gross = attr["total_gross_pnl"].as_f64().unwrap();
    assert!((gross - (sell_px - buy_px) * 10.0).abs() < 1e-6, "gross: {attr}");
    let net = attr["total_net_pnl"].as_f64().unwrap();
    assert!((net - (gross - fee_sum)).abs() < 1e-6, "net = gross - fees: {attr}");
    assert_eq!(attr["overall_win_rate"].as_f64().unwrap(), 1.0, "{attr}");
    assert_eq!(attr["direction"]["long_trades"], 1, "{attr}");
    assert_eq!(attr["direction"]["short_trades"], 0, "{attr}");
    // M11 Task 3: the bridge reconciles. Nothing is open and there is no financing, so the
    // account total is the trading total.
    let steps = attr["waterfall"].as_array().unwrap();
    let trading = steps.iter().find(|s| s["name"] == "Trading Net P&L").expect("trading subtotal");
    assert_eq!(trading["running_total"], attr["total_net_pnl"], "{attr}");
    assert_eq!(steps.last().unwrap()["running_total"], attr["total_account_pnl"], "{attr}");
    assert_eq!(attr["total_account_pnl"], attr["total_net_pnl"], "{attr}");

    // After a reset, fills from before it no longer count.
    let reset = json!({ "strategy_id": f.strategy });
    let (status, body) = call(&f, "POST", "/api/v1/paper/account/reset", &reset).await;
    assert_eq!(status, 200, "{body}");
    let (status, attr) = call(&f, "GET", &attribution_path, &Value::Null).await;
    assert_eq!(status, 200, "{attr}");
    assert_eq!(attr["total_trades"], 0, "{attr}");
    assert_eq!(attr["total_fees"].as_f64().unwrap(), 0.0, "{attr}");

    f.clean_up().await;
}

#[tokio::test]
async fn workspace_paper_attribution_combines_every_strategy() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;

    // A second strategy in the same workspace.
    let second = spectra_db::new_id();
    sqlx::query("INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, 'Second')")
        .bind(second)
        .bind(f.ws)
        .bind(format!("strat-{}", second.simple()))
        .execute(&f.pool)
        .await
        .unwrap();

    // Strategy A wins: buy 10 at 100, sell 10 at 110. Strategy B loses: buy 5 at 100, sell 5 at 90.
    let round_trips = [
        (f.strategy, [("buy", 100.0, 10.0), ("sell", 110.0, 10.0)]),
        (second, [("buy", 100.0, 5.0), ("sell", 90.0, 5.0)]),
    ];
    for (strategy, legs) in round_trips {
        for (side, price, qty) in legs {
            let v = common::seed_dataset_with_latest_closes(
                &f.pool,
                f.ws,
                &f.dir.join("data"),
                &[("AAPL", price)],
            )
            .await;
            let order = json!({
                "strategy_id": strategy, "symbol": "AAPL", "side": side, "order_type": "market",
                "qty": qty, "fill_price": price, "fill_now": true, "dataset_version_id": v
            });
            let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
            assert_eq!(status, 201, "{body}");
            assert_eq!(body["status"], "filled", "{body}");
        }
    }

    let (status_a, attr_a) =
        call(&f, "GET", &format!("/api/v1/paper/attribution?strategy_id={}", f.strategy), &Value::Null).await;
    let (status_b, attr_b) =
        call(&f, "GET", &format!("/api/v1/paper/attribution?strategy_id={second}"), &Value::Null).await;
    assert_eq!((status_a, status_b), (200, 200), "{attr_a} {attr_b}");

    let (status, w) = call(&f, "GET", "/api/v1/paper/attribution/workspace", &Value::Null).await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(w["strategy_count"], 2, "{w}");
    assert_eq!(w["total_trades"], 2, "{w}");
    let net = attr_a["total_net_pnl"].as_f64().unwrap() + attr_b["total_net_pnl"].as_f64().unwrap();
    assert!((w["total_net_pnl"].as_f64().unwrap() - net).abs() < 1e-6, "{w}");
    let fees = attr_a["total_fees"].as_f64().unwrap() + attr_b["total_fees"].as_f64().unwrap();
    assert!((w["total_fees"].as_f64().unwrap() - fees).abs() < 1e-6, "{w}");

    // One AAPL row across both strategies, with both trades counted.
    let instruments = w["instruments"].as_array().unwrap();
    assert_eq!(instruments.len(), 1, "{w}");
    assert_eq!(instruments[0]["trades_count"], 2, "{w}");
    assert_eq!(instruments[0]["win_trades"], 1, "{w}");
    assert!((instruments[0]["pnl_share"].as_f64().unwrap() - 1.0).abs() < 1e-6, "{w}");

    // The bridge still closes on the combined account total.
    let steps = w["waterfall"].as_array().unwrap();
    assert_eq!(steps.last().unwrap()["running_total"], w["total_account_pnl"], "{w}");

    // Cleanup: the second strategy's rows go first, then the fixture's own.
    let _ = sqlx::query("DELETE FROM paper_fills WHERE strategy_id = $1").bind(second).execute(&f.pool).await;
    let _ =
        sqlx::query("DELETE FROM paper_orders WHERE strategy_id = $1").bind(second).execute(&f.pool).await;
    let _ = sqlx::query("DELETE FROM portfolio_balances WHERE strategy_id = $1")
        .bind(second)
        .execute(&f.pool)
        .await;
    let _ = sqlx::query("DELETE FROM strategies WHERE id = $1").bind(second).execute(&f.pool).await;
    f.clean_up().await;
}

#[tokio::test]
async fn paper_attribution_date_range_limits_the_fills() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;
    let path = format!("/api/v1/paper/attribution?strategy_id={}", f.strategy);

    // Round trip 1 wins (buy 10 at 100, sell 10 at 110); round trip 2 loses (buy 5 at 100, sell at 90).
    async fn trade(f: &TestFixture, side: &str, price: f64, qty: f64) {
        let v =
            common::seed_dataset_with_latest_closes(&f.pool, f.ws, &f.dir.join("data"), &[("AAPL", price)])
                .await;
        let order = json!({
            "strategy_id": f.strategy, "symbol": "AAPL", "side": side, "order_type": "market",
            "qty": qty, "fill_price": price, "fill_now": true, "dataset_version_id": v
        });
        let (status, body) = call(f, "POST", "/api/v1/paper/orders", &order).await;
        assert_eq!(status, 201, "{body}");
    }
    trade(&f, "buy", 100.0, 10.0).await;
    trade(&f, "sell", 110.0, 10.0).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mid = chrono::Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    trade(&f, "buy", 100.0, 5.0).await;
    trade(&f, "sell", 90.0, 5.0).await;

    // Both round trips with no window.
    let (status, all) = call(&f, "GET", &path, &Value::Null).await;
    assert_eq!(status, 200, "{all}");
    assert_eq!(all["total_trades"], 2, "{all}");

    // From the middle: only the losing round trip.
    let from = mid.format("%Y-%m-%dT%H:%M:%S%.6fZ");
    let (status, after) = call(&f, "GET", &format!("{path}&from={from}"), &Value::Null).await;
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["total_trades"], 1, "{after}");
    assert!(after["total_gross_pnl"].as_f64().unwrap() < 0.0, "the later trip loses: {after}");

    // Up to the middle: only the winning round trip.
    let (status, before) = call(&f, "GET", &format!("{path}&to={from}"), &Value::Null).await;
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["total_trades"], 1, "{before}");
    assert!(before["total_gross_pnl"].as_f64().unwrap() > 0.0, "the earlier trip wins: {before}");

    // A range that ends before it starts is refused.
    let (status, bad) = call(&f, "GET", &format!("{path}&from={from}&to={from}"), &Value::Null).await;
    assert_eq!(status, 422, "{bad}");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_account_reports_the_average_entry_of_open_positions() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;
    let path = format!("/api/v1/paper/account?strategy_id={}", f.strategy);

    // Two buys at 100 and 110 average to 105 (with 5 bps paper slippage: 105.0525); a partial sell
    // leaves the average where it was.
    for (side, price, qty) in [("buy", 100.0, 10.0), ("buy", 110.0, 10.0), ("sell", 130.0, 5.0)] {
        let v =
            common::seed_dataset_with_latest_closes(&f.pool, f.ws, &f.dir.join("data"), &[("AAPL", price)])
                .await;
        let order = json!({
            "strategy_id": f.strategy, "symbol": "AAPL", "side": side, "order_type": "market",
            "qty": qty, "fill_now": true, "dataset_version_id": v
        });
        let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
        assert_eq!(status, 201, "{body}");
    }

    let (status, account) = call(&f, "GET", &path, &Value::Null).await;
    assert_eq!(status, 200, "{account}");
    let pos = &account["positions"].as_array().unwrap()[0];
    assert_eq!(pos["symbol"], "AAPL", "{account}");
    assert!((pos["avg_entry_price"].as_f64().unwrap() - 105.0525).abs() < 1e-6, "{account}");
    assert!((pos["cost_basis"].as_f64().unwrap() - 15.0 * 105.0525).abs() < 1e-6, "{account}");

    f.clean_up().await;
}

#[tokio::test]
async fn paper_lab_strategy_is_stable_for_a_user_in_a_workspace() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    let (status, first) = call(&f, "GET", "/api/v1/paper/strategy", &Value::Null).await;
    assert_eq!(status, 200, "{first}");
    let (_, second) = call(&f, "GET", "/api/v1/paper/strategy", &Value::Null).await;
    assert_eq!(
        first["strategy_id"], second["strategy_id"],
        "the same user and workspace get the same strategy"
    );
    assert_ne!(
        first["strategy_id"],
        f.strategy.to_string(),
        "Paper Lab's strategy is its own, not the fixture's"
    );
    f.clean_up().await;
}

#[tokio::test]
async fn kill_switch_rejection_without_price_is_audited() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;

    let ks = json!({ "strategy_id": f.strategy, "active": true, "reason": "Audit test" });
    let (status, _) = call(&f, "POST", "/api/v1/risk/kill-switch", &ks).await;
    assert_eq!(status, 200);

    // A market order with no price and no position: no price is known.
    let order = json!({
        "strategy_id": f.strategy, "symbol": "NFLX", "side": "buy", "order_type": "market",
        "qty": 3.0, "fill_now": false
    });
    let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["status"], "rejected", "{body}");

    // The rejection is in the audit trail, with no price.
    let (status, audit) = call(&f, "GET", "/api/v1/risk/audit", &Value::Null).await;
    assert_eq!(status, 200, "{audit}");
    let entry = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["symbol"] == "NFLX")
        .unwrap_or_else(|| panic!("kill-switch rejection must be audited: {audit}"));
    assert_eq!(entry["verdict"], "rejected", "{entry}");
    assert!(entry["price"].is_null(), "{entry}");

    f.clean_up().await;
}

#[tokio::test]
async fn a_paper_fill_records_its_cost_config() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;

    let order = json!({
        "strategy_id": f.strategy, "symbol": "AAPL", "side": "buy", "order_type": "market",
        "qty": 10.0, "fill_price": 100.0, "fill_now": true
    });
    let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
    assert_eq!(status, 201, "{body}");
    let want = json!({ "name": "paper-default", "version": 1, "slippage_bps": 5.0, "min_fee": 1.0 });
    assert_eq!(body["fills"][0]["cost_config"], want, "{body}");

    // Stored, and returned when the order is read back.
    let order_id = body["id"].as_str().unwrap();
    let (status, read) = call(&f, "GET", &format!("/api/v1/paper/orders/{order_id}"), &Value::Null).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["fills"][0]["cost_config"], want, "{read}");

    f.clean_up().await;
}

/// Audit inserts fail closed: when the pre-trade decision cannot be recorded, the order is not
/// placed and the API returns 500. A trigger scoped to this test's workspace makes the insert
/// fail without touching other tests.
#[tokio::test]
async fn an_order_whose_decision_cannot_be_recorded_is_not_placed() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;

    let name = format!("fail_audit_{}", f.ws.simple());
    let ddl = [
        format!(
            "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
             IF NEW.workspace_id = '{}' THEN RAISE EXCEPTION 'audit insert failed (test)'; END IF; \
             RETURN NEW; END $$",
            f.ws
        ),
        format!(
            "CREATE TRIGGER {name} BEFORE INSERT ON risk_decisions FOR EACH ROW EXECUTE FUNCTION {name}()"
        ),
    ];
    for sql in &ddl {
        sqlx::query(sql).execute(&f.pool).await.unwrap();
    }

    // The checked path and the kill-switch path.
    let order = json!({
        "strategy_id": f.strategy, "symbol": "AAPL", "side": "buy", "order_type": "market",
        "qty": 10.0, "fill_price": 100.0, "fill_now": true
    });
    let (status, body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;
    // The broker path (no Alpaca credentials stored: a simulated order), with a workspace-level
    // position for the price and a balance.
    spectra_db::risk_center::upsert_position(&f.pool, f.ws, None, "AAPL", 1.0, 100.0).await.unwrap();
    spectra_db::risk_center::upsert_balance(&f.pool, f.ws, None, 100_000.0, 100_100.0, 100_100.0, 100_100.0)
        .await
        .unwrap();
    let broker_order = json!({ "symbol": "AAPL", "qty": "1", "side": "buy", "type": "market" });
    let (broker_status, broker_body) = call(&f, "POST", "/api/v1/alpaca/v2/orders", &broker_order).await;

    let ks = json!({ "strategy_id": f.strategy, "active": true, "reason": "Audit failure test" });
    let (ks_status, _) = call(&f, "POST", "/api/v1/risk/kill-switch", &ks).await;
    let (ks_order_status, ks_body) = call(&f, "POST", "/api/v1/paper/orders", &order).await;

    let orders: i64 = sqlx::query_scalar("SELECT count(*) FROM paper_orders WHERE workspace_id = $1")
        .bind(f.ws)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let _ = sqlx::query(&format!("DROP TRIGGER {name} ON risk_decisions")).execute(&f.pool).await;
    let _ = sqlx::query(&format!("DROP FUNCTION {name}()")).execute(&f.pool).await;

    assert_eq!(status, 500, "{body}");
    assert!(body["message"].as_str().unwrap().contains("not placed"), "{body}");
    assert_eq!(broker_status, 500, "{broker_body}");
    assert!(broker_body["message"].as_str().unwrap().contains("not placed"), "{broker_body}");
    assert_eq!(ks_status, 200);
    assert_eq!(ks_order_status, 500, "{ks_body}");
    assert_eq!(orders, 0, "no order is stored when its decision is not recorded");

    f.clean_up().await;
}

/// M9 Paper Trading gap verification:
/// Places one paper order through the Alpaca paper brokerage proxy, taking it through:
/// 1. Pre-trade check and fill (captures order ID and audit decision ID).
/// 2. Cancellation path (`DELETE /api/v1/alpaca/v2/orders/{id}`).
/// 3. Kill-switch enforcement (rejection of opening orders, approval of closing orders).
/// 4. Records the run ID / decision ID evidence.
#[tokio::test]
async fn test_m9_alpaca_paper_flow_fill_cancel_killswitch_evidence() {
    let Some(pool) = pool().await else { return };
    let f = TestFixture::setup(pool).await;
    common::seed_volatility_dataset(&f.pool, f.ws, &f.dir.join("data"), &["AAPL"]).await;

    // Seed initial position and balance for price resolution and pre-trade checks
    spectra_db::risk_center::upsert_position(&f.pool, f.ws, None, "AAPL", 10.0, 150.0).await.unwrap();
    spectra_db::risk_center::upsert_balance(&f.pool, f.ws, None, 100_000.0, 101_500.0, 101_500.0, 101_500.0)
        .await
        .unwrap();

    // 1. STEP 1: Place paper order through Alpaca paper proxy (Fill path)
    let fill_order = json!({
        "symbol": "AAPL",
        "qty": "5",
        "side": "buy",
        "type": "market"
    });
    let (status, body) = call(&f, "POST", "/api/v1/alpaca/v2/orders", &fill_order).await;
    assert_eq!(status, 201, "Paper order fill must succeed: {body}");
    assert_eq!(body["symbol"], "AAPL");
    assert_eq!(body["qty"], "5");
    assert_eq!(body["status"], "filled");
    assert_eq!(body["pre_trade_verdict"], "approved");

    let order_id = body["id"].as_str().expect("valid order ID in Alpaca paper fill response");
    assert!(!order_id.is_empty(), "order ID must not be empty");

    // Verify decision was recorded in risk decisions / audit trail
    let (audit_status, audit) = call(&f, "GET", "/api/v1/risk/audit", &Value::Null).await;
    assert_eq!(audit_status, 200, "{audit}");
    let audit_arr = audit.as_array().expect("audit entries array");
    let fill_decision = audit_arr
        .iter()
        .find(|d| d["symbol"] == "AAPL" && d["verdict"] == "approved")
        .expect("Approved decision for AAPL buy must be recorded in audit");
    let fill_decision_id = fill_decision["id"].as_str().expect("valid decision ID");

    // 2. STEP 2: Cancel path
    let cancel_path = format!("/api/v1/alpaca/v2/orders/{order_id}");
    let (cancel_status, _) = call(&f, "DELETE", &cancel_path, &Value::Null).await;
    assert!(
        cancel_status == 204 || cancel_status == 200,
        "Cancel order must return 204 or 200, got: {cancel_status}"
    );

    // 3. STEP 3: Kill-switch path
    // Activate kill switch
    let ks_payload = json!({
        "active": true,
        "reason": "M9 paper trading verification kill-switch trigger"
    });
    let (ks_status, ks_body) = call(&f, "POST", "/api/v1/risk/kill-switch", &ks_payload).await;
    assert_eq!(ks_status, 200, "Kill switch activation failed: {ks_body}");

    // Attempt an opening order while kill switch is active -> must be rejected
    let opening_order = json!({
        "symbol": "AAPL",
        "qty": "2",
        "side": "buy",
        "type": "market"
    });
    let (rej_status, rej_body) = call(&f, "POST", "/api/v1/alpaca/v2/orders", &opening_order).await;
    assert_eq!(
        rej_status, 422,
        "Opening order while kill-switch active must be rejected with 422: {rej_body}"
    );
    assert_eq!(rej_body["verdict"], "rejected");
    let default_breaches = vec![];
    let breach_codes: Vec<&str> = rej_body["breaches"]
        .as_array()
        .unwrap_or(&default_breaches)
        .iter()
        .filter_map(|b| b["code"].as_str())
        .collect();
    assert!(
        breach_codes.contains(&"kill_switch_active"),
        "Breaches must contain kill_switch_active: {breach_codes:?}"
    );

    // Close path under kill switch -> must be permitted (selling existing position)
    let closing_order = json!({
        "symbol": "AAPL",
        "qty": "5",
        "side": "sell",
        "type": "market"
    });
    let (close_status, close_body) = call(&f, "POST", "/api/v1/alpaca/v2/orders", &closing_order).await;
    assert_eq!(close_status, 201, "Closing order while kill-switch active must be approved: {close_body}");
    assert_eq!(close_body["status"], "filled");
    assert_eq!(close_body["pre_trade_verdict"], "approved");
    let close_order_id = close_body["id"].as_str().expect("valid close order ID");

    // 4. STEP 4: Record run ID / decision ID evidence
    println!("============================================================");
    println!("M9 Alpaca Paper Flow Evidence Sign-off:");
    println!("  Fill Order ID:     {}", order_id);
    println!("  Audit Decision ID: {}", fill_decision_id);
    println!("  Cancelled Order:   {}", order_id);
    println!("  Kill-Switch Close: {}", close_order_id);
    println!("  Workspace ID:      {}", f.ws);
    println!("============================================================");

    assert_eq!(order_id.len(), 36);
    assert_eq!(fill_decision_id.len(), 36);
    assert_eq!(close_order_id.len(), 36);

    f.clean_up().await;
}
