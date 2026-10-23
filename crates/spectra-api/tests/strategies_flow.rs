//! M2a tasks 3, 4 and 6 end-to-end: saved strategies, idempotent versions,
//! backtests queued from a version, and parity between version and direct runs.
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
use spectra_api::worker::BacktestWorkerEnv;
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::{Dataset, DatasetInfo};
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
    store: ArtifactBackend,
    dir: PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-strat-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, store, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn worker_env(&self) -> BacktestWorkerEnv {
        BacktestWorkerEnv::new(self.pool.clone(), self.store.clone(), self.dir.join("data"), 1 << 30)
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

    async fn call_as(&self, user: Uuid, ws: Uuid, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user_str, ws_str) = (user.to_string(), ws.to_string());
        let req_id = spectra_db::new_id().to_string();
        let text = if body.is_null() { String::new() } else { body.to_string() };
        let sig = sign(
            SECRET.as_bytes(),
            &canonical(method, path, &ts, &user_str, &ws_str, &req_id, text.as_bytes()),
        );
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header("X-Spectra-User", user_str)
            .header("X-Spectra-Workspace", ws_str)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(text))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// A dataset version in the fixture's workspace (rows only; nothing is run here).
    async fn dataset_version(&self) -> Uuid {
        spectra_db::identity::ensure_identity(&self.pool, Some(self.user), self.ws)
            .await
            .expect("ensure identity for workspace");
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
    f.clean_up();
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
    f.clean_up();
}

async fn claim_job(pool: &PgPool, job_id: Uuid, worker_id: &str) -> spectra_db::jobs::Job {
    sqlx::query_as(
        "UPDATE jobs SET status = 'running', attempts = attempts + 1, started_at = now(), heartbeat_at = now(), \
         worker_id = $1, progress = 0, error = NULL \
         WHERE id = $2 AND status = 'queued' \
         RETURNING *",
    )
    .bind(worker_id)
    .bind(job_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[allow(clippy::similar_names)]
async fn parity_between_saved_version_and_direct_builtin_run() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    spectra_db::identity::ensure_identity(&f.pool, Some(f.user), f.ws).await.unwrap();

    // 1. Ingest/register a real dataset with bars so sma_cross produces trades and equity.
    let mut bars = Vec::with_capacity(120);
    let mut price = 100.0f64;
    for i in 0..120 {
        let drift = if (i / 20) % 2 == 0 { 0.5 } else { -0.5 };
        price += drift;
        let p = Price::from_raw((price * spectra_core::fixed::SCALE as f64).round() as i64);
        let ts_ns = (1_700_000_000 + i * 86_400) * 1_000_000_000;
        bars.push(Bar {
            ts_ns,
            known_at_ns: ts_ns,
            instrument: 0,
            open: p,
            high: Price::from_raw(p.raw() + 50 * spectra_core::fixed::SCALE),
            low: Price::from_raw(p.raw() - 50 * spectra_core::fixed::SCALE),
            close: p,
            volume: Qty::from_raw(1000 * spectra_core::fixed::SCALE),
        });
    }
    let symbols = SymbolTable { symbols: vec!["SPY".to_string()] };
    let checksum = compute_dataset_id(&bars, &symbols);
    let dataset = Dataset {
        bars: bars.clone(),
        symbols: symbols.clone(),
        info: DatasetInfo {
            id: checksum.clone(),
            bars_sha256: spectra_core::bars_sha256(&bars),
            ..DatasetInfo::default()
        },
        quality: spectra_data::dataset::assess_quality(&bars, &symbols, &spectra_data::CsvOptions::default()),
    };
    let raw = spectra_data::datastore::encode_dataset(&dataset);
    let blob_sha = spectra_core::sha256_hex(&raw);
    f.store.put(raw.clone()).await.unwrap();
    spectra_data::DataStore::new(f.dir.join("data")).unwrap().put(&dataset).unwrap();
    let reg = spectra_db::datasets::register_version(
        &f.pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: f.ws,
            dataset_name: "SPY 120 bars",
            checksum: &checksum,
            row_count: dataset.bars.len() as i64,
            byte_size: raw.len() as i64,
            quality_report: serde_json::json!({ "symbols": ["SPY"], "rows": 120 }),
            created_by: Some(f.user),
            blob_sha256: Some(&blob_sha),
            source: None,
        },
    )
    .await
    .unwrap();
    let dataset_version_id = reg.id;

    // 2. Save sma_cross with fast=10, slow=50 as a strategy version.
    let (status, created) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({
                "name": "SPY Trend Parity",
                "description": "SMA cross parity test",
                "builtin": "sma_cross",
                "params": { "fast": 10, "slow": 50 }
            }),
        )
        .await;
    assert_eq!(status, 201, "{created}");
    let version_id = created["versions"][0]["id"].as_str().unwrap().to_string();
    let builtin_sha = spectra_backtest::builtin("sma_cross").unwrap().source_sha256();
    assert_eq!(created["versions"][0]["strategy_sha256"], builtin_sha);

    // 3. Queue a run started from the saved version (Run A).
    let (status, resp_a) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({
                "dataset_version_id": dataset_version_id,
                "strategy_version_id": version_id,
            }),
        )
        .await;
    assert_eq!(status, 202, "{resp_a}");
    let job_a_id = Uuid::parse_str(resp_a["id"].as_str().unwrap()).unwrap();

    // 4. Queue a direct built-in run on the same data with the same parameters (Run B).
    let (status, resp_b) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({
                "dataset_version_id": dataset_version_id,
                "strategy": "sma_cross",
                "params": { "fast": 10, "slow": 50 },
            }),
        )
        .await;
    assert_eq!(status, 202, "{resp_b}");
    let job_b_id = Uuid::parse_str(resp_b["id"].as_str().unwrap()).unwrap();

    // 5. Worker execution: claim and run both jobs to completion.
    let worker_env = f.worker_env();
    let worker_id = "test-worker-parity";

    let job_a = claim_job(&f.pool, job_a_id, worker_id).await;
    let run_a_id =
        spectra_api::worker::run_backtest_job(&worker_env, &job_a, worker_id).await.expect("job A succeeds");

    let job_b = claim_job(&f.pool, job_b_id, worker_id).await;
    let run_b_id =
        spectra_api::worker::run_backtest_job(&worker_env, &job_b, worker_id).await.expect("job B succeeds");

    // 6. Verify backtests via API: both succeeded and match on all fingerprints and metrics.
    let (status, view_a) = f.call("GET", &format!("/api/v1/backtests/{job_a_id}"), &Value::Null).await;
    assert_eq!(status, 200, "{view_a}");
    assert_eq!(view_a["status"], "succeeded");
    assert_eq!(view_a["run_id"], run_a_id.to_string());

    let (status, view_b) = f.call("GET", &format!("/api/v1/backtests/{job_b_id}"), &Value::Null).await;
    assert_eq!(status, 200, "{view_b}");
    assert_eq!(view_b["status"], "succeeded");
    assert_eq!(view_b["run_id"], run_b_id.to_string());

    // Check fingerprints parity:
    assert_eq!(
        view_a["fingerprints"]["result_sha256"], view_b["fingerprints"]["result_sha256"],
        "result hashes must match exactly"
    );
    assert_eq!(view_a["fingerprints"]["strategy_sha256"], view_b["fingerprints"]["strategy_sha256"]);
    assert_eq!(view_a["fingerprints"]["strategy_sha256"], builtin_sha);
    assert_eq!(view_a["fingerprints"]["dataset_sha256"], view_b["fingerprints"]["dataset_sha256"]);
    assert_eq!(view_a["fingerprints"]["config_sha256"], view_b["fingerprints"]["config_sha256"]);

    // Check summary metrics parity:
    assert_eq!(view_a["summary"], view_b["summary"], "metrics summary must match exactly");

    // 7. Check database run records:
    // "Done when: a user saves sma_cross with fast=10, slow=50, runs it on a dataset,
    // and the run record shows the version ID and the code hash."
    let row_a: (Option<Uuid>, String, String, Value, Value) = sqlx::query_as(
        "SELECT strategy_version_id, strategy, strategy_sha256, params, metrics FROM runs WHERE id = $1",
    )
    .bind(run_a_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();

    let row_b: (Option<Uuid>, String, String, Value, Value) = sqlx::query_as(
        "SELECT strategy_version_id, strategy, strategy_sha256, params, metrics FROM runs WHERE id = $1",
    )
    .bind(run_b_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();

    assert_eq!(
        row_a.0,
        Some(Uuid::parse_str(&version_id).unwrap()),
        "run record shows the strategy version ID"
    );
    assert_eq!(row_a.1, "sma_cross");
    assert_eq!(row_a.2, builtin_sha, "run record shows the code hash");

    assert_eq!(row_b.0, None, "direct run has no strategy_version_id");
    assert_eq!(row_b.1, "sma_cross");
    assert_eq!(row_b.2, builtin_sha);

    assert_eq!(row_a.3, row_b.3, "recorded parameters match");
    assert_eq!(row_a.4, row_b.4, "recorded metrics match");

    // 8. Artifacts and chart series parity via API:
    let (status, series_a) =
        f.call("GET", &format!("/api/v1/backtests/{job_a_id}/series?kind=equity"), &Value::Null).await;
    assert_eq!(status, 200);
    let (status, series_b) =
        f.call("GET", &format!("/api/v1/backtests/{job_b_id}/series?kind=equity"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(series_a["points"], series_b["points"], "equity curves match");

    let (status, art_a) =
        f.call("GET", &format!("/api/v1/backtests/{job_a_id}/artifacts/equity_csv"), &Value::Null).await;
    assert_eq!(status, 200);
    let (status, art_b) =
        f.call("GET", &format!("/api/v1/backtests/{job_b_id}/artifacts/equity_csv"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(art_a, art_b, "equity CSV artifacts match");

    // 9. Running the saved version a second time reproduces identical result_sha256:
    let (status, resp_c) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({
                "dataset_version_id": dataset_version_id,
                "strategy_version_id": version_id,
            }),
        )
        .await;
    assert_eq!(status, 202);
    let job_c_id = Uuid::parse_str(resp_c["id"].as_str().unwrap()).unwrap();
    let job_c = claim_job(&f.pool, job_c_id, worker_id).await;
    let _run_c_id =
        spectra_api::worker::run_backtest_job(&worker_env, &job_c, worker_id).await.expect("job C succeeds");
    let (_, view_c) = f.call("GET", &format!("/api/v1/backtests/{job_c_id}"), &Value::Null).await;
    assert_eq!(
        view_c["fingerprints"]["result_sha256"], view_a["fingerprints"]["result_sha256"],
        "re-running reproduces identical result_sha256"
    );

    // Clean up
    sqlx::query("DELETE FROM run_artifacts WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM runs WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
    sqlx::query("DELETE FROM jobs WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
    sqlx::query("DELETE FROM dataset_versions WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM datasets WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
    sqlx::query("DELETE FROM strategy_versions WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM strategies WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
    f.clean_up();
}

#[tokio::test]
async fn m2b_strategy_studio_lifecycle_drafts_versions_diff_and_param_sets() {
    let Some(pool) = pool().await else {
        eprintln!("skipping: DATABASE_URL not set");
        return;
    };
    let f = Fixture::new(pool);

    // 1. Validation endpoint (POST /api/v1/strategies/validate)
    // Valid code:
    let valid_code = r"
        param fast: int = 10, min = 2, max = 50;
        param slow: int = 30, min = 5, max = 200;

        on_bar {
            let f = sma(fast);
            let s = sma(slow);
            if f > s {
                target_shares(100);
            } else {
                target_shares(0);
            }
        }
    ";
    let (status, val_resp) =
        f.call("POST", "/api/v1/strategies/validate", &json!({ "code": valid_code })).await;
    assert_eq!(status, 200);
    assert_eq!(val_resp["valid"], true);
    assert_eq!(val_resp["parameters"].as_array().unwrap().len(), 2);
    assert_eq!(val_resp["parameters"][0]["name"], "fast");

    // Invalid code (loop keyword):
    let (status, val_err) =
        f.call("POST", "/api/v1/strategies/validate", &json!({ "code": "while true { buy(); }" })).await;
    assert_eq!(status, 200);
    assert_eq!(val_err["valid"], false);
    assert!(!val_err["errors"].as_array().unwrap().is_empty());

    // Invalid code (lookahead index):
    let (status, val_lookahead) = f
        .call("POST", "/api/v1/strategies/validate", &json!({ "code": "on_bar { let x = close[-1]; }" }))
        .await;
    assert_eq!(status, 200);
    assert_eq!(val_lookahead["valid"], false);
    assert!(val_lookahead["errors"][0]["message"].as_str().unwrap().to_lowercase().contains("lookahead"));

    // 2. Create Strategy
    let (status, strat_resp) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({
                "name": "DSL Momentum Strategy",
                "kind": "custom",
                "description": "Restricted DSL strategy testing draft & version flow",
            }),
        )
        .await;
    assert_eq!(status, 201);
    let strat_id = Uuid::parse_str(strat_resp["strategy"]["id"].as_str().unwrap()).unwrap();

    // 3. Draft Flow (PUT / GET /api/v1/strategies/{id}/draft)
    let (status, draft_save) = f
        .call(
            "PUT",
            &format!("/api/v1/strategies/{strat_id}/draft"),
            &json!({
                "code": valid_code,
                "parameters": { "fast": 12, "slow": 24 },
            }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(draft_save["saved"], true);

    let (status, draft_get) =
        f.call("GET", &format!("/api/v1/strategies/{strat_id}/draft"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(draft_get["strategy_id"], strat_id.to_string());
    assert_eq!(draft_get["code"], valid_code);
    assert_eq!(draft_get["parameters"]["fast"], 12);

    // 4. Save Version & Deduplication
    let (status, v1_resp) = f
        .call(
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions"),
            &json!({
                "code": valid_code,
                "parameters": { "fast": 12, "slow": 24 },
                "notes": "Initial v1 draft save",
            }),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(v1_resp["created"], true);
    let v1_id = v1_resp["version"]["id"].as_str().unwrap().to_string();
    assert_eq!(v1_resp["version"]["version"], 1);

    // Save exact same text & parameters again -> Dedup returns exact same version
    let (status, v1_dup) = f
        .call(
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions"),
            &json!({
                "code": valid_code,
                "parameters": { "fast": 12, "slow": 24 },
                "notes": "Redundant click",
            }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(v1_dup["created"], false);
    assert_eq!(v1_dup["version"]["id"], v1_id);
    assert_eq!(v1_dup["version"]["version"], 1);

    // Modify code and save version 2
    let v2_code = valid_code.replace("target_shares(100)", "target_shares(200)");
    let (status, v2_resp) = f
        .call(
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions"),
            &json!({
                "code": v2_code,
                "parameters": { "fast": 15, "slow": 30 },
                "notes": "Version 2 doubled position size",
            }),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(v2_resp["created"], true);
    let _v2_id = v2_resp["version"]["id"].as_str().unwrap().to_string();
    assert_eq!(v2_resp["version"]["version"], 2);

    // 5. Version Diff API (GET /api/v1/strategies/{id}/diff)
    let (status, diff_resp) =
        f.call("GET", &format!("/api/v1/strategies/{strat_id}/diff?from=1&to=2"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(diff_resp["from_version"], 1);
    assert_eq!(diff_resp["to_version"], 2);
    let lines = diff_resp["code_diff"].as_array().unwrap();
    let has_addition = lines
        .iter()
        .any(|l| l["kind"] == "added" && l["text"].as_str().unwrap().contains("target_shares(200)"));
    let has_deletion = lines.iter().any(|l| {
        (l["kind"] == "deleted" || l["kind"] == "removed")
            && l["text"].as_str().unwrap().contains("target_shares(100)")
    });
    assert!(has_addition && has_deletion, "diff accurately reported line changes");

    let params_changed = &diff_resp["parameter_diff"]["changed"];
    assert!(params_changed.get("fast").is_some());
    assert!(params_changed.get("slow").is_some());

    // 6. Parameter Sets API (Task 10)
    let (status, pset_resp) = f
        .call(
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions/{v1_id}/parameter_sets"),
            &json!({
                "name": "Conservative Preset",
                "parameters": { "fast": 20, "slow": 50 },
            }),
        )
        .await;
    assert_eq!(status, 201);
    let pset_id = pset_resp["id"].as_str().unwrap().to_string();

    let (status, pset_list) = f
        .call("GET", &format!("/api/v1/strategies/{strat_id}/versions/{v1_id}/parameter_sets"), &Value::Null)
        .await;
    assert_eq!(status, 200);
    let psets = pset_list["parameter_sets"].as_array().unwrap();
    assert!(psets.iter().any(|p| p["name"] == "Conservative Preset"));

    let (status, pset_single) =
        f.call("GET", &format!("/api/v1/strategies/parameter_sets/{pset_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(pset_single["name"], "Conservative Preset");
    assert_eq!(pset_single["parameters"]["fast"], 20);

    // Clean up
    sqlx::query("DELETE FROM strategy_parameter_sets WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM strategy_drafts WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM strategy_versions WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM strategies WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await.unwrap();
    f.clean_up();
}

#[tokio::test]
async fn strategy_sharing_viewer_can_run_and_cannot_edit() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let dataset = f.dataset_version().await;

    // 1. Owner creates workspace and saves strategy version (M2 Task 13)
    let owner_id = f.user;
    let _ = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner') ON CONFLICT DO NOTHING",
    )
    .bind(f.ws)
    .bind(owner_id)
    .execute(&f.pool)
    .await;

    let (status, created) = f
        .call(
            "POST",
            "/api/v1/strategies",
            &json!({
                "name": "Shared SMA Strategy",
                "description": "SMA on SPY",
                "builtin": "sma_cross",
                "params": { "fast": 10, "slow": 50 }
            }),
        )
        .await;
    assert_eq!(status, 201, "Failed creating strategy: {created}");
    let strat_id = created["strategy"]["id"].as_str().unwrap();
    let version_id = created["versions"][0]["id"].as_str().unwrap();

    // 2. Add second user as read-only viewer in the same workspace
    let viewer_id = spectra_db::new_id();
    spectra_db::identity::ensure_identity(&f.pool, Some(viewer_id), f.ws).await.unwrap();
    let _ =
        sqlx::query("UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2")
            .bind(f.ws)
            .bind(viewer_id)
            .execute(&f.pool)
            .await;

    // 3. Viewer can READ the shared strategy and its version
    let (status, strat_view) =
        f.call_as(viewer_id, f.ws, "GET", &format!("/api/v1/strategies/{strat_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(strat_view["name"], "Shared SMA Strategy");

    let (status, ver_view) = f
        .call_as(viewer_id, f.ws, "GET", &format!("/api/v1/strategies/versions/{version_id}"), &Value::Null)
        .await;
    assert_eq!(status, 200);
    assert_eq!(ver_view["strategy_id"], strat_id);

    // 4. Task 13: Viewer can RUN a backtest using the shared strategy version
    let (status, job_res) = f
        .call_as(
            viewer_id,
            f.ws,
            "POST",
            "/api/v1/jobs",
            &json!({
                "kind": "backtest",
                "payload": {
                    "dataset_version_id": dataset,
                    "strategy_version_id": version_id
                }
            }),
        )
        .await;
    assert_eq!(status, 202, "Viewer must be able to run backtest on shared strategy version: {job_res}");

    // 5. Task 13: Viewer CANNOT edit, save versions, modify drafts or add parameter sets (403 Forbidden)
    let (status, err) = f
        .call_as(
            viewer_id,
            f.ws,
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions"),
            &json!({
                "builtin": "sma_cross",
                "parameters": { "fast": 15, "slow": 60 }
            }),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(err["code"], "viewer_read_only");

    let (status, err) = f
        .call_as(
            viewer_id,
            f.ws,
            "POST",
            &format!("/api/v1/strategies/{strat_id}/draft"),
            &json!({ "code": "param fast = 12;", "parameters": {} }),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(err["code"], "viewer_read_only");

    let (status, err) = f
        .call_as(
            viewer_id,
            f.ws,
            "POST",
            &format!("/api/v1/strategies/{strat_id}/versions/{version_id}/parameters"),
            &json!({ "name": "Unauthorized preset", "parameters": {} }),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(err["code"], "viewer_read_only");

    // Clean up
    let _ = sqlx::query("DELETE FROM strategy_versions WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await;
    let _ = sqlx::query("DELETE FROM strategies WHERE workspace_id = $1").bind(f.ws).execute(&f.pool).await;
    let _ = sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
        .bind(f.ws)
        .execute(&f.pool)
        .await;
    f.clean_up();
}
