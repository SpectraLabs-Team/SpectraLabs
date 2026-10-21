//! M2a tasks 3 and 4 end-to-end: saved strategies, idempotent versions, and
//! backtests queued from a version.
//!
//! Needs `DATABASE_URL`; without it the test passes without doing anything,
//! like the rest of the workspace's database tests.

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

struct Fixture {
    app: Router,
    pool: PgPool,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
        Self { app, pool, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    /// Sends a signed request in `ws`; returns the status and the JSON body (Null when empty).
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

    /// A dataset version in the fixture's workspace (rows only; nothing is run here).
    async fn dataset_version(&self) -> Uuid {
        let dataset: Uuid = sqlx::query_scalar(
            "INSERT INTO datasets (workspace_id, name) VALUES ($1, 'spy daily') RETURNING id",
        )
        .bind(self.ws)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO dataset_versions (workspace_id, dataset_id, version, checksum, row_count, byte_size) \
             VALUES ($1, $2, 1, repeat('e', 64), 10, 100) RETURNING id",
        )
        .bind(self.ws)
        .bind(dataset)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

fn fields(body: &Value) -> Vec<String> {
    body["details"]["errors"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["field"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn saved_strategies_and_versions() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);

    // Validation: every problem at once, nothing created.
    let (status, body) =
        f.call("POST", "/api/v1/strategies", &json!({"name": " ", "params": {"fast": 3}})).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(fields(&body), ["name", "params"]);
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({"name": "Trend", "builtin": "sma_cross", "params": {"fast": 80, "slow": 50, "nope": 1}}),
        )
        .await;
    assert_eq!(status, 422, "{body}");
    assert!(fields(&body).contains(&"params.nope".to_string()), "{body}");
    let (_, list) = f.call("GET", "/api/v1/strategies", &Value::Null).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0, "a bad first version creates nothing");

    // Create with version 1: defaults are filled in and typed.
    let (status, created) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({"name": "Trend", "description": "SMA on SPY", "builtin": "sma_cross", "params": {"fast": 10, "slow": 50}}),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let sid = created["strategy"]["id"].as_str().unwrap().to_string();
    let v1 = &created["versions"][0];
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["builtin"], "sma_cross");
    assert_eq!(v1["params"], json!({"fast": 10, "slow": 50, "qty": 100.0, "allow_short": false}));
    assert_eq!(v1["current"], true);
    let builtin_sha = spectra_backtest::builtin("sma_cross").unwrap().source_sha256();
    assert_eq!(v1["strategy_sha256"], builtin_sha);
    let v1_id = v1["id"].as_str().unwrap().to_string();

    // The same name again is a conflict.
    let (status, body) = f.call("POST", "/api/v1/strategies", &json!({"name": "Trend"})).await;
    assert_eq!((status, body["code"].as_str()), (409, Some("strategy_exists")), "{body}");

    // Saving the same source and parameters returns the same version, however they are spelled.
    let path = format!("/api/v1/strategies/{sid}/versions");
    let (status, again) = f
        .call(
            "POST",
            &path,
            &json!({"builtin": "sma_cross", "params": {"slow": 50.0, "fast": 10, "qty": 100}}),
        )
        .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(
        (again["created"].as_bool(), again["version"]["id"].as_str()),
        (Some(false), Some(v1_id.as_str()))
    );

    // Different parameters make version 2.
    let (status, v2) =
        f.call("POST", &path, &json!({"builtin": "sma_cross", "params": {"fast": 5, "slow": 50}})).await;
    assert_eq!(status, 201, "{v2}");
    assert_eq!((v2["created"].as_bool(), v2["version"]["version"].as_i64()), (Some(true), Some(2)));
    assert_ne!(v2["version"]["config_hash"], v1["config_hash"]);

    // Detail lists versions newest first; the list shows the latest.
    let (status, detail) = f.call("GET", &format!("/api/v1/strategies/{sid}"), &Value::Null).await;
    assert_eq!(status, 200);
    let numbers: Vec<i64> =
        detail["versions"].as_array().unwrap().iter().map(|v| v["version"].as_i64().unwrap()).collect();
    assert_eq!(numbers, [2, 1]);
    let (_, list) = f.call("GET", "/api/v1/strategies", &Value::Null).await;
    assert_eq!(list["items"][0]["version_count"], 2);
    assert_eq!(list["items"][0]["latest_version"], 2);
    let (status, one) = f.call("GET", &format!("/api/v1/strategies/versions/{v1_id}"), &Value::Null).await;
    assert_eq!((status, one["version"].as_i64()), (200, Some(1)));

    // Unknown ids and other workspaces see nothing.
    let other = spectra_db::new_id();
    let (status, _) = f.call_in(other, "GET", &format!("/api/v1/strategies/{sid}"), "").await;
    assert_eq!(status, 404);
    let (status, _) = f.call_in(other, "GET", &format!("/api/v1/strategies/versions/{v1_id}"), "").await;
    assert_eq!(status, 404);
    let (status, _) = f.call_in(other, "POST", &path, r#"{"builtin":"sma_cross"}"#).await;
    assert_eq!(status, 404);
    let (status, _) = f.call("GET", "/api/v1/strategies/not-a-uuid", &Value::Null).await;
    assert_eq!(status, 404);
    let (status, body) = f.call("POST", &path, &json!({"builtin": "nope"})).await;
    assert_eq!((status, fields(&body)), (422, vec!["builtin".to_string()]));
}

#[tokio::test]
async fn backtests_from_a_strategy_version() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let (status, created) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({"name": "Trend", "builtin": "sma_cross", "params": {"fast": 10, "slow": 50}}),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let version = created["versions"][0]["id"].as_str().unwrap().to_string();
    let dataset = f.dataset_version().await;

    // Queued from the version: the job runs exactly the saved built-in and parameters.
    let (status, bt) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": dataset, "strategy_version_id": version}),
        )
        .await;
    assert_eq!(status, 202, "{bt}");
    assert_eq!(bt["request"]["strategy_version_id"], version.as_str());
    assert_eq!(bt["request"]["strategy"], "sma_cross");
    assert_eq!(bt["request"]["params"], json!({"fast": 10, "slow": 50, "qty": 100.0, "allow_short": false}));

    // The worker's view of that payload carries the version.
    let payload = spectra_core::Json::parse(&bt["request"].to_string()).unwrap();
    let job = spectra_backtest::BacktestJob::from_payload(&payload).unwrap();
    assert_eq!(job.strategy_version_id.as_deref(), Some(version.as_str()));
    assert_eq!(job.request.params.canonical(), "fast=10;slow=50;qty=100;allow_short=0");

    // The generic jobs route resolves it the same way.
    let (status, j) = f
        .call("POST", "/api/v1/jobs", &json!({"kind": "backtest", "payload": {"dataset_version_id": dataset, "strategy_version_id": version}}))
        .await;
    assert_eq!(status, 202, "{j}");
    let stored: Value = sqlx::query_scalar("SELECT payload FROM jobs WHERE id = $1")
        .bind(Uuid::parse_str(j["id"].as_str().unwrap()).unwrap())
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        (stored["strategy"].as_str(), stored["strategy_version_id"].as_str()),
        (Some("sma_cross"), Some(version.as_str()))
    );

    // A version and a strategy together are refused, so a recorded version always ran that version.
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/jobs",
            &json!({"kind": "backtest", "payload": {"dataset_version_id": dataset, "strategy_version_id": version, "strategy": "buy_and_hold", "params": {}}}),
        )
        .await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(fields(&body), ["strategy", "params"]);

    // Unknown, malformed, and another workspace's versions.
    let unknown = spectra_db::new_id();
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": dataset, "strategy_version_id": unknown}),
        )
        .await;
    assert_eq!((status, fields(&body)), (422, vec!["strategy_version_id".to_string()]), "{body}");
    let (status, _) = f
        .call("POST", "/api/v1/jobs", &json!({"kind": "backtest", "payload": {"dataset_version_id": dataset, "strategy_version_id": "v1"}}))
        .await;
    assert_eq!(status, 422);
    let other = spectra_db::new_id();
    let body = json!({"dataset_version_id": dataset, "strategy_version_id": version}).to_string();
    let (status, body) = f.call_in(other, "POST", "/api/v1/backtests", &body).await;
    assert_eq!(status, 422, "{body}");

    // A version saved against older built-in code cannot run: it would not reproduce.
    let stale: Uuid = sqlx::query_scalar(
        "INSERT INTO strategy_versions (workspace_id, strategy_id, version, builtin, source_hash, config_hash, parameters) \
         SELECT workspace_id, strategy_id, 99, builtin, repeat('0', 64), config_hash, parameters \
           FROM strategy_versions WHERE id = $1 RETURNING id",
    )
    .bind(Uuid::parse_str(&version).unwrap())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let (status, _) = f.call("GET", &format!("/api/v1/strategies/versions/{stale}"), &Value::Null).await;
    assert_eq!(status, 200);
    let (_, view) = f.call("GET", &format!("/api/v1/strategies/versions/{stale}"), &Value::Null).await;
    assert_eq!(view["current"], false);
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": dataset, "strategy_version_id": stale}),
        )
        .await;
    assert_eq!((status, body["code"].as_str()), (409, Some("strategy_version_stale")), "{body}");

    // Leave no queued work behind: the SQL checks that run after the tests count every queued job.
    sqlx::query("DELETE FROM jobs WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
}
