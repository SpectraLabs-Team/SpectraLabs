//! Module 4 integration tests: Parameter Sweeps, Walk-Forward Validation, Overfitting
//! (PSR, DSR, MinTRL, CSCV PBO), and Holdout Vault with metered access enforcement.

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
        let dir = std::env::temp_dir().join(format!("spectra-opt-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    async fn clean_up(&self) {
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

#[tokio::test]
async fn overfitting_calculation_endpoint() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);

    // Provide 5 trials returns each with 10 observations
    let returns_matrix = vec![
        vec![0.01, -0.005, 0.02, 0.015, -0.01, 0.005, 0.03, -0.02, 0.01, 0.02],
        vec![0.005, 0.002, -0.01, 0.01, 0.005, -0.005, 0.015, 0.01, 0.005, -0.01],
        vec![-0.01, -0.02, 0.005, 0.01, -0.01, 0.005, -0.005, 0.01, 0.005, 0.0],
        vec![0.02, 0.015, 0.01, -0.005, 0.02, 0.01, -0.01, 0.025, 0.01, 0.015],
        vec![0.0, 0.005, -0.005, 0.01, -0.01, 0.015, -0.005, 0.005, 0.01, -0.005],
    ];

    let (status, body) = f
        .call(
            "POST",
            "/api/v1/optimization/overfitting",
            &json!({
                "returns_matrix": returns_matrix,
                "benchmark_sharpe": 0.0,
                "confidence": 0.95
            }),
        )
        .await;

    assert_eq!(status, 200, "status: {status}, body: {body}");
    assert!(body["psr"].as_f64().is_some());
    assert!(body["dsr"].as_f64().is_some());
    assert!(body["min_trl"].as_f64().is_some());
    assert!(body["pbo"].as_f64().is_some());
    assert_eq!(body["num_trials"], 5);
    assert_eq!(body["num_observations"], 10);
    f.clean_up().await;
}

#[tokio::test]
async fn parameter_sweep_flow() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // 1. Queue sweep
    let (status, sweep) = f
        .call(
            "POST",
            "/api/v1/optimization/sweep",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "sma_cross",
                "param_grid": {
                    "fast": [5.0, 10.0],
                    "slow": [20.0, 30.0]
                },
                "method": "grid",
                "objective": "sharpe"
            }),
        )
        .await;

    assert_eq!(status, 202, "status: {status}, body: {sweep}");
    let sweep_id = sweep["id"].as_str().unwrap();

    // 2. List sweeps
    let (status, list) = f.call("GET", "/api/v1/optimization/sweeps", &Value::Null).await;
    assert_eq!(status, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|s| s["id"] == sweep_id));

    // 3. Get sweep detail
    let (status, detail) =
        f.call("GET", &format!("/api/v1/optimization/sweep/{sweep_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(detail["sweep"]["id"], sweep_id);
    assert!(detail["trials"].is_array());
    f.clean_up().await;
}

#[tokio::test]
async fn walk_forward_flow() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // 1. Queue walk-forward
    let (status, wf) = f
        .call(
            "POST",
            "/api/v1/optimization/walkforward",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "sma_cross",
                "mode": "rolling",
                "train_size": 200,
                "test_size": 50,
                "param_grid": {
                    "fast": [5.0, 10.0],
                    "slow": [20.0, 30.0]
                },
                "objective": "sharpe"
            }),
        )
        .await;

    assert_eq!(status, 202, "status: {status}, body: {wf}");
    let report_id = wf["id"].as_str().unwrap();

    // 2. List walk-forwards
    let (status, list) = f.call("GET", "/api/v1/optimization/walkforward", &Value::Null).await;
    assert_eq!(status, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|r| r["id"] == report_id));

    // 3. Get walk-forward detail
    let (status, detail) =
        f.call("GET", &format!("/api/v1/optimization/walkforward/{report_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(detail["id"], report_id);
    assert_eq!(detail["mode"], "rolling");
    f.clean_up().await;
}

#[tokio::test]
async fn holdout_vault_budget_lockout_flow() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // 1. Seal holdout with budget = 2
    let (status, holdout) = f
        .call(
            "POST",
            "/api/v1/optimization/holdout/seal",
            &json!({
                "name": "Q4 Out-Of-Sample Vault",
                "dataset_version_id": ds_ver,
                "budget": 2,
                "notes": "Strict lockdown test"
            }),
        )
        .await;

    assert_eq!(status, 201, "status: {status}, body: {holdout}");
    let holdout_id = holdout["id"].as_str().unwrap();
    assert_eq!(holdout["budget"], 2);
    assert_eq!(holdout["evaluations_used"], 0);

    // 2. List holdouts
    let (status, list) = f.call("GET", "/api/v1/optimization/holdout", &Value::Null).await;
    assert_eq!(status, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|h| h["id"] == holdout_id));

    // 3. First evaluation (1 of 2)
    let (status, eval1) = f
        .call(
            "POST",
            &format!("/api/v1/optimization/holdout/{holdout_id}/evaluate"),
            &json!({ "strategy": "buy_and_hold" }),
        )
        .await;
    assert_eq!(status, 200, "status: {status}, body: {eval1}");
    assert_eq!(eval1["evaluation_num"], 1);

    // Check status
    let (status, detail) =
        f.call("GET", &format!("/api/v1/optimization/holdout/{holdout_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(detail["holdout"]["evaluations_used"], 1);

    // 4. Second evaluation (2 of 2 - exhausts budget)
    let (status, eval2) = f
        .call(
            "POST",
            &format!("/api/v1/optimization/holdout/{holdout_id}/evaluate"),
            &json!({ "strategy": "buy_and_hold" }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(eval2["evaluation_num"], 2);

    // Check status
    let (_, detail) =
        f.call("GET", &format!("/api/v1/optimization/holdout/{holdout_id}"), &Value::Null).await;
    assert_eq!(detail["holdout"]["evaluations_used"], 2);

    // 5. Third evaluation MUST be refused (403 holdout_budget_exhausted)
    let (status, refused) = f
        .call(
            "POST",
            &format!("/api/v1/optimization/holdout/{holdout_id}/evaluate"),
            &json!({ "strategy": "buy_and_hold" }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused}");
    assert_eq!(refused["code"], "holdout_budget_exhausted");
    f.clean_up().await;
}
