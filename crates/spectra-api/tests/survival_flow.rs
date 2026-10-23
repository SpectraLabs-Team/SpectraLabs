//! Module 6 integration tests: Strategy Survival Lab presets, report queuing,
//! lifecycle execution, and scorecard retrieval.

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
        let dir = std::env::temp_dir().join(format!("spectra-surv-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    async fn clean_up(&self) {
        let _ = sqlx::query("DELETE FROM survival_reports WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM survival_presets WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await;
        let _ =
            sqlx::query("DELETE FROM runs WHERE workspace_id = $1").bind(self.ws).execute(&self.pool).await;
        let _ =
            sqlx::query("DELETE FROM jobs WHERE workspace_id = $1").bind(self.ws).execute(&self.pool).await;
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
            .header("x-spectra-user", &user)
            .header("x-spectra-workspace", &ws)
            .header("x-spectra-timestamp", &ts)
            .header("x-request-id", &req_id)
            .header("x-spectra-signature", &sig)
            .header("content-type", "application/json")
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
async fn survival_presets_lifecycle_crud() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    spectra_db::identity::ensure_identity(&f.pool, Some(f.user), f.ws).await.unwrap();

    let default_cfg = json!({
        "seed": 42,
        "significance": 0.05,
        "n_simulations": 100,
        "block_length": 5
    });

    // 1. Create a preset
    let (status, created) = f
        .call(
            "POST",
            "/api/v1/survival/presets",
            &json!({
                "name": "Standard Institutional Battery",
                "config": default_cfg
            }),
        )
        .await;
    assert_eq!(status, 201, "Failed creating preset: {created}");
    let preset_id = created["id"].as_str().unwrap();
    assert_eq!(created["name"], "Standard Institutional Battery");

    // 2. List presets
    let (status, list) = f.call("GET", "/api/v1/survival/presets", &Value::Null).await;
    assert_eq!(status, 200);
    let items = list.as_array().expect("array of presets");
    assert!(items.iter().any(|p| p["id"] == preset_id));

    // 3. Update preset
    let (status, updated) = f
        .call(
            "PATCH",
            &format!("/api/v1/survival/presets/{preset_id}"),
            &json!({
                "name": "Updated Institutional Battery"
            }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(updated["name"], "Updated Institutional Battery");

    f.clean_up().await;
}

#[tokio::test]
async fn survival_lab_queue_report_and_retrieve_scorecard() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool);
    spectra_db::identity::ensure_identity(&f.pool, Some(f.user), f.ws).await.unwrap();

    // Insert a successful run to analyze
    let run_id = spectra_db::new_id();
    sqlx::query(
        "INSERT INTO runs (id, workspace_id, status, finished_at, params, metrics) \
         VALUES ($1, $2, 'succeeded', now(), '{\"strategy\":\"sma\"}', '{\"metrics\":{\"sharpe\":1.5}}')",
    )
    .bind(run_id)
    .bind(f.ws)
    .execute(&f.pool)
    .await
    .unwrap();

    let survival_cfg = json!({
        "seed": 100,
        "significance": 0.05,
        "n_simulations": 50,
        "block_length": 5
    });

    // 1. Queue a survival report
    let (status, res) = f
        .call(
            "POST",
            &format!("/api/v1/runs/{run_id}/survival"),
            &json!({
                "config": survival_cfg
            }),
        )
        .await;
    assert_eq!(status, 202, "Queuing survival report failed: {res}");
    let report_id = res["report_id"].as_str().unwrap();
    assert_eq!(res["job"]["kind"], "survival");

    // 2. List survival reports for the run
    let (status, run_reports) = f.call("GET", &format!("/api/v1/runs/{run_id}/survival"), &Value::Null).await;
    assert_eq!(status, 200);
    let reports_arr = run_reports.as_array().unwrap();
    assert_eq!(reports_arr.len(), 1);
    assert_eq!(reports_arr[0]["id"], report_id);
    assert_eq!(reports_arr[0]["status"], "queued");

    // 3. Read specific report by id
    let (status, rep) = f.call("GET", &format!("/api/v1/survival/{report_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(rep["id"], report_id);
    assert_eq!(rep["run_id"], run_id.to_string());

    // 4. Simulate worker completing report
    sqlx::query(
        "UPDATE survival_reports \
         SET status = 'succeeded', \
             worst_verdict = 'fragile', \
             verdicts = '{\"cost_fragility\":\"fragile\",\"execution_delay\":\"pass\"}', \
             finished_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(f.ws)
    .bind(Uuid::parse_str(report_id).unwrap())
    .execute(&f.pool)
    .await
    .unwrap();

    let (status, rep_updated) = f.call("GET", &format!("/api/v1/survival/{report_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(rep_updated["status"], "succeeded");
    assert_eq!(rep_updated["worst_verdict"], "fragile");

    f.clean_up().await;
}
