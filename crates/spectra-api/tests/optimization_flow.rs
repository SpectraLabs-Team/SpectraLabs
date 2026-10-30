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

    /// A GET whose response is a file: status, headers and raw bytes.
    async fn get_raw(&self, ws: Uuid, path: &str) -> (u16, axum::http::HeaderMap, Vec<u8>) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, wss) = (self.user.to_string(), ws.to_string());
        let req_id = spectra_db::new_id().to_string();
        let sig = sign(SECRET.as_bytes(), &canonical("GET", path, &ts, &user, &wss, &req_id, b""));
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", wss)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .body(Body::empty())
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, bytes)
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

#[tokio::test]
async fn holdout_sealed_direct_read_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // 1. Seal holdout
    let (status, holdout) = f
        .call(
            "POST",
            "/api/v1/optimization/holdout/seal",
            &json!({
                "name": "Strict Vault",
                "dataset_version_id": ds_ver,
                "budget": 3,
                "notes": "Direct read lockout test"
            }),
        )
        .await;
    assert_eq!(status, 201, "status: {status}, body: {holdout}");

    // 2. Direct read of bars MUST be refused (403 holdout_sealed)
    let (status, refused_bars) =
        f.call("GET", &format!("/api/v1/datasets/versions/{ds_ver}/bars"), &Value::Null).await;
    assert_eq!(status, 403, "status: {status}, body: {refused_bars}");
    assert_eq!(refused_bars["code"], "holdout_sealed");

    // 3. Direct backtest queueing MUST be refused (403 holdout_sealed)
    let (status, refused_bt) = f
        .call(
            "POST",
            "/api/v1/backtests",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "buy_and_hold"
            }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused_bt}");
    assert_eq!(refused_bt["code"], "holdout_sealed");

    // 4. Sweep queueing on sealed holdout MUST be refused (403 holdout_sealed)
    let (status, refused_sweep) = f
        .call(
            "POST",
            "/api/v1/optimization/sweep",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "buy_and_hold",
                "param_grid": { "p": [1.0, 2.0] }
            }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused_sweep}");
    assert_eq!(refused_sweep["code"], "holdout_sealed");

    // 5. Walk-forward on sealed holdout MUST be refused (403 holdout_sealed)
    let (status, refused_wf) = f
        .call(
            "POST",
            "/api/v1/optimization/walkforward",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "buy_and_hold",
                "train_size": 100,
                "test_size": 20,
                "param_grid": { "p": [1.0] }
            }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused_wf}");
    assert_eq!(refused_wf["code"], "holdout_sealed");

    f.clean_up().await;
}

#[tokio::test]
async fn holdout_viewer_role_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // Set user role to 'viewer' in workspace_members
    let _ = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer') \
         ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'viewer'",
    )
    .bind(f.ws)
    .bind(f.user)
    .execute(&f.pool)
    .await;

    // 1. Viewer cannot seal a holdout (403 viewer_read_only)
    let (status, refused_seal) = f
        .call(
            "POST",
            "/api/v1/optimization/holdout/seal",
            &json!({
                "name": "Viewer Vault",
                "dataset_version_id": ds_ver,
                "budget": 5
            }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused_seal}");
    assert_eq!(refused_seal["code"], "viewer_read_only");

    // 2. Viewer cannot evaluate a holdout (403 viewer_read_only)
    let dummy_id = spectra_db::new_id();
    let (status, refused_eval) = f
        .call(
            "POST",
            &format!("/api/v1/optimization/holdout/{dummy_id}/evaluate"),
            &json!({ "strategy": "buy_and_hold" }),
        )
        .await;
    assert_eq!(status, 403, "status: {status}, body: {refused_eval}");
    assert_eq!(refused_eval["code"], "viewer_read_only");

    f.clean_up().await;
}

#[tokio::test]
async fn sweep_search_budget_cap_enforced() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;

    // Create grid with 40 * 30 = 1200 combinations (> 1000 budget cap)
    let fast_vals: Vec<f64> = (1..=40).map(f64::from).collect();
    let slow_vals: Vec<f64> = (1..=30).map(f64::from).collect();

    let (status, res) = f
        .call(
            "POST",
            "/api/v1/optimization/sweep",
            &json!({
                "dataset_version_id": ds_ver,
                "strategy": "sma_cross",
                "param_grid": {
                    "fast": fast_vals,
                    "slow": slow_vals
                }
            }),
        )
        .await;

    assert_eq!(status, 422, "status: {status}, body: {res}");
    assert_eq!(res["code"], "search_budget_exceeded");
    f.clean_up().await;
}

#[tokio::test]
async fn sweep_export_report_flow() {
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
                }
            }),
        )
        .await;
    assert_eq!(status, 202);
    let sweep_id = sweep["id"].as_str().unwrap();

    // 2. Export sweep report as CSV
    let (status, res) =
        f.call("GET", &format!("/api/v1/optimization/sweep/{sweep_id}/export"), &Value::Null).await;
    assert_eq!(status, 200, "status: {status}, body: {res}");

    // 3. Export sweep report as PDF
    let (status, res_pdf) = f
        .call("GET", &format!("/api/v1/optimization/sweep/{sweep_id}/export?format=pdf"), &Value::Null)
        .await;
    assert_eq!(status, 200, "status: {status}, body: {res_pdf}");
    f.clean_up().await;
}

/// M16: every export of a sweep is stored as a new version of its validation report, linked to the
/// sweep, and can be listed and downloaded again byte for byte.
#[tokio::test]
async fn sweep_exports_are_versioned_reports() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    let ds_ver = f.dataset_version().await;
    let (status, sweep) = f
        .call(
            "POST",
            "/api/v1/optimization/sweep",
            &json!({ "dataset_version_id": ds_ver, "strategy": "sma_cross",
                     "param_grid": { "fast": [5.0, 10.0], "slow": [20.0, 30.0] } }),
        )
        .await;
    assert_eq!(status, 202);
    let sweep_id = sweep["id"].as_str().unwrap().to_string();
    let header = |h: &axum::http::HeaderMap, k: &str| h.get(k).unwrap().to_str().unwrap().to_string();

    // Three exports: versions 1, 2, 3 of one report, whatever the format.
    let export = format!("/api/v1/optimization/sweep/{sweep_id}/export");
    let (s1, h1, csv1) = f.get_raw(f.ws, &export).await;
    let (s2, h2, pdf2) = f.get_raw(f.ws, &format!("{export}?format=pdf")).await;
    let (s3, h3, _) = f.get_raw(f.ws, &export).await;
    assert_eq!((s1, s2, s3), (200, 200, 200));
    assert_eq!(header(&h1, "content-type"), "text/csv; charset=utf-8");
    assert_eq!(header(&h2, "content-type"), "application/pdf");
    assert!(header(&h1, "content-disposition").ends_with("-validation-report.csv\""));
    assert!(pdf2.starts_with(b"%PDF-1.4"));
    let versions: Vec<String> = [&h1, &h2, &h3].iter().map(|h| header(h, "x-report-version")).collect();
    assert_eq!(versions, ["1", "2", "3"]);
    let report_id = header(&h1, "x-report-id");
    assert!([&h2, &h3].iter().all(|h| header(h, "x-report-id") == report_id));

    // The run lists its report versions, newest first.
    let (status, list) =
        f.call("GET", &format!("/api/v1/reports?run_kind=sweep&run_id={sweep_id}"), &Value::Null).await;
    assert_eq!(status, 200, "{list}");
    let list = list.as_array().unwrap();
    assert_eq!(list.iter().map(|v| v["version"].as_i64().unwrap()).collect::<Vec<_>>(), [3, 2, 1]);
    assert!(list.iter().all(|v| v["report_type"] == "validation" && v["run_id"] == sweep_id.as_str()));
    assert_eq!(list[2]["created_by"], f.user.to_string());
    assert_eq!(list[1]["format"], "pdf");
    assert_eq!(list[2]["download_url"], format!("/api/v1/reports/{report_id}/versions/1"));

    // Version 1 downloads byte for byte, with its hash.
    let (status, h, body) = f.get_raw(f.ws, &format!("/api/v1/reports/{report_id}/versions/1")).await;
    assert_eq!(status, 200);
    assert_eq!(body, csv1);
    assert_eq!(header(&h, "x-report-sha256"), spectra_core::sha256_hex(&csv1));
    assert_eq!(header(&h, "x-report-sha256"), list[2]["sha256"].as_str().unwrap());
    let (status, versions) =
        f.call("GET", &format!("/api/v1/reports/{report_id}/versions"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(versions.as_array().unwrap().len(), 3);

    // Unknown version, unknown run kind, and another workspace.
    assert_eq!(f.get_raw(f.ws, &format!("/api/v1/reports/{report_id}/versions/99")).await.0, 404);
    assert_eq!(
        f.call("GET", &format!("/api/v1/reports?run_kind=job&run_id={sweep_id}"), &Value::Null).await.0,
        422
    );
    let other = spectra_db::new_id();
    assert_eq!(f.get_raw(other, &format!("/api/v1/reports/{report_id}/versions/1")).await.0, 404);
    let (_, empty) =
        f.call_in(other, "GET", &format!("/api/v1/reports?run_kind=sweep&run_id={sweep_id}"), "").await;
    assert_eq!(empty, json!([]));
    f.clean_up().await;
}
