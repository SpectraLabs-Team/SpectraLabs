//! Integration test for `latest_prices` and market order pricing against Postgres.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::dataset_prices::latest_prices;
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{DataStore, Dataset, DatasetInfo};
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
    strategy: Uuid,
    dataset_version_id: Uuid,
}

impl Fixture {
    async fn setup(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-prices-{}", spectra_db::new_id().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = spectra_db::new_id();

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Prices test workspace')")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .execute(&pool)
            .await
            .unwrap();

        let user = spectra_db::new_id();
        sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, '!')")
            .bind(user)
            .bind(format!("user-{}@example.com", user.simple()))
            .execute(&pool)
            .await
            .unwrap();

        let strategy = spectra_db::new_id();
        sqlx::query(
            "INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, 'Pricing test strat')",
        )
        .bind(strategy)
        .bind(ws)
        .bind(format!("strat-{}", strategy.simple()))
        .execute(&pool)
        .await
        .unwrap();

        // Symbol table with AAPL, GOOG, and MSFT (sorted).
        let symbols = SymbolTable::from_symbols(["AAPL", "GOOG", "MSFT"]);
        let inst_aapl = symbols.id("AAPL").unwrap();
        let inst_msft = symbols.id("MSFT").unwrap();

        // 80 daily bars for AAPL and MSFT so volatility estimation succeeds in risk checks.
        // AAPL latest close is 155.0, MSFT latest close is 300.0. GOOG has no bars.
        let day_ns: i64 = 86_400_000_000_000;
        let start_ns: i64 = 1_704_153_600_000_000_000; // 2024-01-02
        let n_bars = 80;
        let mut bars = Vec::with_capacity(n_bars * 2);
        for i in 0..n_bars {
            let ts_ns = start_ns + i64::try_from(i).unwrap() * day_ns;
            let aapl_c = if i == n_bars - 1 {
                155.0
            } else if i % 2 == 0 {
                150.0
            } else {
                151.0
            };
            bars.push(Bar {
                ts_ns,
                known_at_ns: ts_ns,
                instrument: inst_aapl,
                open: Price::from_f64(aapl_c).unwrap(),
                high: Price::from_f64(aapl_c + 1.0).unwrap(),
                low: Price::from_f64(aapl_c - 1.0).unwrap(),
                close: Price::from_f64(aapl_c).unwrap(),
                volume: Qty::from_units(10_000).unwrap(),
            });

            let msft_c = if i == n_bars - 1 {
                300.0
            } else if i % 2 == 0 {
                298.0
            } else {
                302.0
            };
            bars.push(Bar {
                ts_ns,
                known_at_ns: ts_ns,
                instrument: inst_msft,
                open: Price::from_f64(msft_c).unwrap(),
                high: Price::from_f64(msft_c + 1.0).unwrap(),
                low: Price::from_f64(msft_c - 1.0).unwrap(),
                close: Price::from_f64(msft_c).unwrap(),
                volume: Qty::from_units(10_000).unwrap(),
            });
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
                dataset_name: "Test dataset",
                checksum: &checksum,
                row_count: i64::try_from(dataset.bars.len()).unwrap(),
                byte_size,
                quality_report: serde_json::json!({}),
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

        Self { app, pool, dir, user, ws, strategy, dataset_version_id: reg.id }
    }

    async fn call(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        self.call_as(method, path, self.user, self.ws, body).await
    }

    async fn call_as(&self, method: &str, path: &str, user: Uuid, ws: Uuid, body: &Value) -> (u16, Value) {
        let payload = if body.is_null() { Vec::new() } else { serde_json::to_vec(body).unwrap() };
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let u_str = user.to_string();
        let ws_str = ws.to_string();
        let sig =
            sign(SECRET.as_bytes(), &canonical(method, path, &ts, &u_str, &ws_str, "req-dp-test", &payload));

        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("x-spectra-user", u_str)
            .header("x-spectra-workspace", ws_str)
            .header("x-request-id", "req-dp-test")
            .header("x-spectra-timestamp", ts)
            .header("x-spectra-signature", sig)
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .unwrap();

        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
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
        let _ = sqlx::query("DELETE FROM holdouts WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM holdout_access_log WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM dataset_versions WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM datasets WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM strategies WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await;
        let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ------------------------------------------------------------------ latest_prices integration

#[tokio::test]
async fn latest_close_per_symbol_is_returned() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // Latest version resolved automatically
    let prices = latest_prices(&f.pool, f.ws, &f.dir, None, &["AAPL", "MSFT"]).await.unwrap();
    assert_eq!(prices.get("AAPL"), Some(&155.0));
    assert_eq!(prices.get("MSFT"), Some(&300.0));

    // Explicit dataset version specified
    let prices_explicit =
        latest_prices(&f.pool, f.ws, &f.dir, Some(f.dataset_version_id), &["AAPL", "MSFT"]).await.unwrap();
    assert_eq!(prices_explicit.get("AAPL"), Some(&155.0));
    assert_eq!(prices_explicit.get("MSFT"), Some(&300.0));

    f.clean_up().await;
}

#[tokio::test]
async fn symbol_with_no_bars_is_rejected() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // GOOG is in the symbol table but has no bars in the dataset
    let err = latest_prices(&f.pool, f.ws, &f.dir, None, &["GOOG"]).await.unwrap_err();
    assert_eq!(err, "no bars for GOOG");

    // UNKNOWN is not in the symbol table at all
    let err_unknown = latest_prices(&f.pool, f.ws, &f.dir, None, &["UNKNOWN"]).await.unwrap_err();
    assert_eq!(err_unknown, "no bars for UNKNOWN");

    f.clean_up().await;
}

#[tokio::test]
async fn sealed_holdout_version_is_rejected() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let holdout_id = spectra_db::new_id();
    spectra_db::optimization::insert_holdout(
        &f.pool,
        holdout_id,
        f.ws,
        "Vault 1",
        f.dataset_version_id,
        5,
        "notes",
        None,
    )
    .await
    .unwrap();

    let err = latest_prices(&f.pool, f.ws, &f.dir, Some(f.dataset_version_id), &["AAPL"]).await.unwrap_err();
    assert!(err.contains("is sealed in holdout vault: Vault 1"), "expected sealed holdout error, got: {err}");

    f.clean_up().await;
}

#[tokio::test]
async fn workspace_with_no_dataset_version_returns_expected_error() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let empty_ws = spectra_db::new_id();
    let err = latest_prices(&f.pool, empty_ws, &f.dir, None, &["AAPL"]).await.unwrap_err();
    assert_eq!(err, "no dataset version to price fills from");

    f.clean_up().await;
}

// ------------------------------------------------------------------ API pricing behavior

#[tokio::test]
async fn market_order_fills_at_dataset_close_ignoring_client_fill_price() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // AAPL latest close in the fixture dataset is 155.0.
    // Client sends fill_price = 999.0, which should be ignored.
    let order = json!({
        "strategy_id": f.strategy,
        "symbol": "AAPL",
        "side": "buy",
        "order_type": "market",
        "qty": 10.0,
        "fill_price": 999.0,
        "fill_now": true,
    });
    let (status, body) = f.call("POST", "/api/v1/paper/orders", &order).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["status"], "filled", "{body}");

    let fill = &body["fills"][0];
    let fill_price = fill["price"].as_f64().unwrap();
    // With 5 bps slippage on a buy at 155.0: 155.0 * 1.0005 = 155.0775.
    let expected_fill = 155.0 * 1.0005;
    assert!(
        (fill_price - expected_fill).abs() < 1e-4,
        "fill price was {fill_price}, expected {expected_fill} (derived from dataset close 155.0, ignoring 999.0)"
    );

    f.clean_up().await;
}

#[tokio::test]
async fn symbol_with_no_bars_returns_422_and_places_no_order() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    // GOOG is in the symbol table but has no bars in the dataset.
    let order = json!({
        "strategy_id": f.strategy,
        "symbol": "GOOG",
        "side": "buy",
        "order_type": "market",
        "qty": 10.0,
        "fill_now": true,
    });
    let (status, body) = f.call("POST", "/api/v1/paper/orders", &order).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["message"], "no bars for GOOG");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM paper_orders WHERE workspace_id = $1")
        .bind(f.ws)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "no order should be placed when pricing fails");

    f.clean_up().await;
}

#[tokio::test]
async fn workspace_with_no_dataset_version_returns_422() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::setup(pool).await;

    let empty_ws = spectra_db::new_id();
    sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, 'Empty Workspace')")
        .bind(empty_ws)
        .bind(format!("ws-{}", empty_ws.simple()))
        .execute(&f.pool)
        .await
        .unwrap();

    let empty_strat = spectra_db::new_id();
    sqlx::query(
        "INSERT INTO strategies (id, workspace_id, name, description) VALUES ($1, $2, $3, 'Empty Strat')",
    )
    .bind(empty_strat)
    .bind(empty_ws)
    .bind(format!("strat-{}", empty_strat.simple()))
    .execute(&f.pool)
    .await
    .unwrap();

    let order = json!({
        "strategy_id": empty_strat,
        "symbol": "AAPL",
        "side": "buy",
        "order_type": "market",
        "qty": 10.0,
        "fill_now": true,
    });
    let (status, body) = f.call_as("POST", "/api/v1/paper/orders", f.user, empty_ws, &order).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["message"], "no dataset version to price fills from");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM paper_orders WHERE workspace_id = $1")
        .bind(empty_ws)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "no order should be placed when no dataset version exists");

    let _ =
        sqlx::query("DELETE FROM strategies WHERE workspace_id = $1").bind(empty_ws).execute(&f.pool).await;
    let _ = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(empty_ws).execute(&f.pool).await;
    f.clean_up().await;
}
