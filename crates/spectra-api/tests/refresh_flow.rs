//! M1b task 13 end-to-end: provider configs, dataset refresh schedules,
//! manual refresh triggers, worker execution, and lineage derivation.
//!
//! When DATABASE_URL is set, exercises database persistence and workspace isolation;
//! when not set, passes without doing anything (like the rest of the workspace).

use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::json;
use spectra_api::auth::{canonical, sign};
use spectra_api::providers::runner::{run_refresh, RefreshEnv, RefreshOutcome};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::Price;
use spectra_core::{Bar, Qty};
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::datastore::encode_dataset;
use spectra_data::{DataStore, Dataset, DatasetInfo};
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const DAY: i64 = 86_400_000_000_000;

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
    dir: std::path::PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-refresh-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, store, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn req(&self, method: &str, path_and_query: &str, body: &str) -> Request<Body> {
        self.req_as(method, path_and_query, body, self.ws)
    }

    fn req_as(&self, method: &str, path_and_query: &str, body: &str, ws: Uuid) -> Request<Body> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let user = self.user.to_string();
        let ws_str = ws.to_string();
        let req_id = spectra_db::new_id().to_string();
        let can = canonical(method, path_and_query, &ts, &user, &ws_str, &req_id, body.as_bytes());
        let sig = sign(SECRET.as_bytes(), &can);
        Request::builder()
            .method(method)
            .uri(path_and_query)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws_str)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn send(&self, req: Request<Body>) -> (u16, serde_json::Value) {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }
}

#[tokio::test]
async fn refresh_flow_e2e() {
    let Some(pool) = pool().await else {
        eprintln!("skipping refresh_flow_e2e: DATABASE_URL not set");
        return;
    };
    let fix = Fixture::new(pool);
    spectra_db::identity::ensure_identity(&fix.pool, Some(fix.user), fix.ws).await.unwrap();

    // 1. Create a provider config via REST API
    let create_cfg_body = json!({
        "name": "Alpaca Paper Feed",
        "provider": "fake",
        "settings": {
            "feed": "iex"
        }
    })
    .to_string();

    let (status, cfg_res) = fix.send(fix.req("POST", "/api/v1/providers/configs", &create_cfg_body)).await;
    assert_eq!(status, 200, "create provider config: {cfg_res}");
    let cfg_id: Uuid = cfg_res["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(cfg_res["name"], "Alpaca Paper Feed");
    assert_eq!(cfg_res["provider"], "fake");

    // List provider configs
    let (status, list_res) = fix.send(fix.req("GET", "/api/v1/providers/configs", "")).await;
    assert_eq!(status, 200);
    assert_eq!(list_res["items"].as_array().unwrap().len(), 1);

    // Get provider config by id
    let (status, get_res) =
        fix.send(fix.req("GET", &format!("/api/v1/providers/configs/{cfg_id}"), "")).await;
    assert_eq!(status, 200);
    assert_eq!(get_res["id"], cfg_id.to_string());

    // Workspace isolation: check other workspace
    let other_ws = spectra_db::new_id();
    let (status, other_res) =
        fix.send(fix.req_as("GET", &format!("/api/v1/providers/configs/{cfg_id}"), "", other_ws)).await;
    assert_eq!(status, 404, "other workspace cannot access config: {other_res}");

    // 2. Set up base dataset with version 1
    let ds_id = spectra_db::new_id();
    let ds_name = "test_equities";
    sqlx::query("INSERT INTO datasets (id, workspace_id, name) VALUES ($1, $2, $3)")
        .bind(ds_id)
        .bind(fix.ws)
        .bind(ds_name)
        .execute(&fix.pool)
        .await
        .unwrap();

    let syms = SymbolTable::from_symbols(&["AAPL".to_string()]);
    let base_bar = Bar::new(
        DAY,
        0,
        Price::from_raw(150 * 100_000_000),
        Price::from_raw(155 * 100_000_000),
        Price::from_raw(149 * 100_000_000),
        Price::from_raw(152 * 100_000_000),
        Qty::from_raw(10_000 * 100_000_000),
    );
    let mut base_ds = Dataset { symbols: syms, bars: vec![base_bar], ..Default::default() };
    base_ds.info =
        DatasetInfo { bars_sha256: spectra_core::bars_sha256(&base_ds.bars), ..DatasetInfo::default() };
    base_ds.info.id = compute_dataset_id(&base_ds.bars, &base_ds.symbols);

    let ds_store = DataStore::new(fix.dir.join("data")).unwrap();
    ds_store.put(&base_ds).unwrap();

    let reg = spectra_db::datasets::register_version(
        &fix.pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: fix.ws,
            dataset_name: ds_name,
            checksum: &base_ds.info.id,
            row_count: base_ds.bars.len() as i64,
            byte_size: encode_dataset(&base_ds).len() as i64,
            quality_report: serde_json::json!({}),
            created_by: Some(fix.user),
            blob_sha256: None,
            source: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(reg.version, 1);
    let v1_id = reg.id;

    // 3. Configure refresh schedule
    let sched_body = json!({
        "provider_config_id": cfg_id,
        "symbols": ["AAPL"],
        "interval_seconds": 86400,
        "lookback_bars": 5
    })
    .to_string();

    let (status, sched_res) =
        fix.send(fix.req("POST", &format!("/api/v1/datasets/{ds_id}/refresh-schedule"), &sched_body)).await;
    assert_eq!(status, 200, "create refresh schedule: {sched_res}");
    assert_eq!(sched_res["interval_seconds"], 86400);
    assert_eq!(sched_res["symbols"].as_array().unwrap().len(), 1);

    // Get schedule
    let (status, get_sched) =
        fix.send(fix.req("GET", &format!("/api/v1/datasets/{ds_id}/refresh-schedule"), "")).await;
    assert_eq!(status, 200);
    assert_eq!(get_sched["provider_config_id"], cfg_id.to_string());

    // 4. Trigger manual refresh via POST /api/v1/datasets/{id}/refresh
    let trigger_body = json!({
        "symbols": ["AAPL"]
    })
    .to_string();

    let (status, trig_res) =
        fix.send(fix.req("POST", &format!("/api/v1/datasets/{ds_id}/refresh"), &trigger_body)).await;
    assert_eq!(status, 202, "trigger refresh accepted: {trig_res}");
    assert_eq!(trig_res["status"], "queued");
    let job_id: Uuid = trig_res["job_id"].as_str().unwrap().parse().unwrap();

    // 5. Worker execution: claim the enqueued job and execute with run_refresh
    let kinds = vec!["provider_refresh".to_string()];
    let claimed = spectra_db::jobs::claim(&fix.pool, "test-worker-1", &kinds, 60)
        .await
        .unwrap()
        .expect("job should be claimed");

    assert_eq!(claimed.id, job_id);

    let renv = RefreshEnv {
        pool: fix.pool.clone(),
        store: fix.store.clone(),
        data_dir: fix.dir.join("data"),
        quota: 100 * 1024 * 1024,
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0));

    let outcome = run_refresh(&renv, &claimed, cancel, progress).await;

    match outcome {
        RefreshOutcome::Registered(val) => {
            assert_eq!(val["status"], "registered");
            assert_eq!(val["version"], 2);
            assert!(val["bars_count"].as_u64().unwrap() > 1);

            // Complete the claimed job in database
            spectra_db::jobs::complete(&fix.pool, job_id, "test-worker-1", &val).await.unwrap();
        }
        other => panic!("expected RefreshOutcome::Registered, got {other:?}"),
    }

    // Verify version 2 in database with lineage
    let versions = spectra_db::datasets::dataset_versions(&fix.pool, fix.ws, ds_id).await.unwrap();
    assert_eq!(versions.len(), 2);
    let v2 = versions.iter().find(|v| v.version == 2).unwrap();
    assert_eq!(v2.derivation_kind.as_deref(), Some("refresh"));
    assert_eq!(v2.derived_from_id, Some(v1_id));

    // 6. Delete schedule and provider config
    let (status, _) =
        fix.send(fix.req("DELETE", &format!("/api/v1/datasets/{ds_id}/refresh-schedule"), "")).await;
    assert_eq!(status, 200);

    let (status, _) = fix.send(fix.req("DELETE", &format!("/api/v1/providers/configs/{cfg_id}"), "")).await;
    assert_eq!(status, 200);
}
