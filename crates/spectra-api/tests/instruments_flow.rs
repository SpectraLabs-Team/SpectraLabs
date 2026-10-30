//! M24 end-to-end: instrument records, the workspace base currency, FX rates,
//! and their snapshot into a queued backtest.
//!
//! Needs `DATABASE_URL`; without it the test passes without doing anything,
//! like the rest of the workspace's database tests.

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
    _store: ArtifactBackend,
    dir: PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-instr-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, _store: store, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn clean_up(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
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
        spectra_db::identity::ensure_identity(&self.pool, Some(self.user), self.ws)
            .await
            .expect("ensure identity for workspace");
        let dataset: Uuid =
            sqlx::query_scalar("INSERT INTO datasets (workspace_id, name) VALUES ($1, 'mixed') RETURNING id")
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
async fn instruments_fx_and_their_snapshot_into_a_backtest() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let version = f.dataset_version().await;

    // Base currency: USD until chosen; bad codes are refused.
    let (status, body) = f.call("GET", "/api/v1/base-currency", &Value::Null).await;
    assert_eq!((status, body["base_currency"].as_str()), (200, Some("USD")), "{body}");
    let (status, _) = f.call("PUT", "/api/v1/base-currency", &json!({"base_currency": "usd"})).await;
    assert_eq!(status, 422);

    // Records: same rules as a backtest request, every problem at once.
    let (status, body) = f
        .call("POST", "/api/v1/instruments", &json!({"symbol": "ESZ6", "type": "future", "multiplier": -1}))
        .await;
    assert_eq!(status, 422, "{body}");
    let bad = fields(&body);
    assert!(bad.contains(&"multiplier".to_string()) && bad.contains(&"tick_size".to_string()), "{body}");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "ESZ6", "type": "future", "multiplier": 50, "tick_size": "0.25", "expiry": "2026-12-18"}),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["multiplier"], "50.00000000");
    let (status, _) = f
        .call(
            "POST",
            "/api/v1/instruments",
            &json!({"symbol": "ESZ6", "type": "future", "tick_size": 1, "expiry": "2027-01-01"}),
        )
        .await;
    assert_eq!(status, 409);
    let (status, _) = f
        .call("POST", "/api/v1/instruments", &json!({"symbol": "SAP", "currency": "EUR", "tick_size": 0.01}))
        .await;
    assert_eq!(status, 201);
    let (status, _) =
        f.call("POST", "/api/v1/instruments", &json!({"symbol": "AAPL", "tick_size": 0.01})).await;
    assert_eq!(status, 201);
    let (_, list) = f.call("GET", "/api/v1/instruments", &Value::Null).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 3, "{list}");

    // FX: exact rates, per base, replacing a rate at the same time.
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "EUR", "rates": [{"as_of": "2026-01-02", "rate": "1.0845"}, {"as_of": "2026-01-05", "rate": 1.09}]}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["items"][0]["rate"], "1.084500000000");
    let (status, _) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "USD", "rates": [{"as_of": "2026-01-02", "rate": 1}]}),
        )
        .await;
    assert_eq!(status, 422, "the base currency needs no rates");
    let (status, body) = f
        .call(
            "POST",
            "/api/v1/fx-rates",
            &json!({"currency": "EUR", "rates": [{"as_of": "2026-01-02", "rate": "1.0850"}]}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["items"].as_array().unwrap().len(), 2, "replaced, not duplicated");
    assert_eq!(body["items"][0]["rate"], "1.085000000000");

    // A queued backtest carries the snapshot: the base, the records that change a run
    // (the plain AAPL equity is left out) and the EUR series.
    let (status, job) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": version, "strategy": "buy_and_hold"}),
        )
        .await;
    assert_eq!(status, 202, "{job}");
    let req = &job["request"];
    assert_eq!(req["base_currency"], "USD", "{job}");
    let symbols: Vec<&str> =
        req["instruments"].as_array().unwrap().iter().map(|i| i["symbol"].as_str().unwrap()).collect();
    assert_eq!(symbols, ["ESZ6", "SAP"], "{job}");
    assert_eq!(req["fx_rates"]["EUR"].as_array().unwrap().len(), 2, "{job}");
    // and the worker will accept it as it is stored
    let payload = spectra_core::Json::parse(&req.to_string()).unwrap();
    let parsed = spectra_backtest::BacktestJob::from_payload(&payload).expect("stored payload parses");
    assert_eq!(parsed.request.instruments.instruments.len(), 2);

    // A request that gives its own instruments is taken as given.
    let (status, job) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({"dataset_version_id": version, "strategy": "buy_and_hold", "instruments": []}),
        )
        .await;
    assert_eq!(status, 202, "{job}");
    assert_eq!(job["request"]["instruments"], json!([]));

    // Delete.
    let (status, _) = f.call("DELETE", "/api/v1/instruments/SAP", &Value::Null).await;
    assert_eq!(status, 200);
    let (status, _) = f.call("DELETE", "/api/v1/instruments/SAP", &Value::Null).await;
    assert_eq!(status, 404);
    f.clean_up();
}
