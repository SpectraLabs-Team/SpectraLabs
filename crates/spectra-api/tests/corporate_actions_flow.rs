//! M1b task 11 end-to-end: corporate action sets, split/dividend adjustments,
//! and derived dataset version lineage.
//!
//! When DATABASE_URL is set, exercises database persistence and workspace isolation;
//! when not set, passes without doing anything (like the rest of the workspace).

use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use spectra_api::auth::{canonical, sign};
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
    dir: std::path::PathBuf,
    user: Uuid,
    ws: Uuid,
}

impl Fixture {
    fn new(pool: PgPool) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-corp-{}", spectra_db::new_id().simple()));
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn corporate_actions_crud_and_validation() {
    let Some(pool) = pool().await else {
        eprintln!("skipping corporate_actions_crud_and_validation: DATABASE_URL not set");
        return;
    };
    let f = Fixture::new(pool);

    // 1. Validation failure: empty name
    let res =
        f.app.clone().oneshot(f.req("POST", "/api/v1/corporate-actions", r#"{"name":""}"#)).await.unwrap();
    assert_eq!(res.status(), 422);

    // 2. Validation failure: invalid split ratio
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            "/api/v1/corporate-actions",
            r#"{"name":"Tech 2024","actions":[{"symbol":"AAPL","ex_date":"2020-08-31","action_kind":"split","split_from":0,"split_to":2}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 422);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["details"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["field"] == "actions[0].split_from"));

    // 3. Create valid set with both JSON actions and CSV actions
    let create_payload = r#"{
        "name": "US Tech Corporate Actions",
        "description": "2020 splits and dividends",
        "actions": [
            {"symbol":"AAPL","ex_date":"2020-08-31","action_kind":"split","split_from":1,"split_to":4}
        ],
        "csv": "symbol,ex_date,action_kind,split_from,split_to,dividend_amount\nMSFT,2021-02-17,dividend,1,1,0.56"
    }"#;
    let res =
        f.app.clone().oneshot(f.req("POST", "/api/v1/corporate-actions", create_payload)).await.unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let set_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["name"], "US Tech Corporate Actions");
    assert_eq!(created["actions"].as_array().unwrap().len(), 2);

    // 4. List sets: shows 1 set with action_count = 2
    let res = f.app.clone().oneshot(f.req("GET", "/api/v1/corporate-actions", "")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let item = list["items"].as_array().unwrap().iter().find(|i| i["id"] == set_id).unwrap();
    assert_eq!(item["action_count"], 2);

    // 5. Get set detail
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/corporate-actions/{set_id}"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["id"], set_id);
    assert_eq!(detail["actions"].as_array().unwrap().len(), 2);

    // 6. Cross-workspace isolation
    let other_ws = spectra_db::new_id();
    let res = f
        .app
        .clone()
        .oneshot(f.req_as("GET", &format!("/api/v1/corporate-actions/{set_id}"), "", other_ws))
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    // 7. Delete set
    let res = f
        .app
        .clone()
        .oneshot(f.req("DELETE", &format!("/api/v1/corporate-actions/{set_id}"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // 8. Get after delete is 404
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/corporate-actions/{set_id}"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn derive_dataset_flow() {
    let Some(pool) = pool().await else {
        eprintln!("skipping derive_dataset_flow: DATABASE_URL not set");
        return;
    };
    let f = Fixture::new(pool);

    // 1. Create a corporate action set with a 2-for-1 split on AAPL on day 2
    let create_set_payload = r#"{
        "name": "AAPL Split Set",
        "actions": [
            {"symbol":"AAPL","ex_date":"2024-01-03T00:00:00Z","action_kind":"split","split_from":1,"split_to":2}
        ]
    }"#;
    let res =
        f.app.clone().oneshot(f.req("POST", "/api/v1/corporate-actions", create_set_payload)).await.unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let set_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let set_id = set_json["id"].as_str().unwrap().to_string();

    // 2. Create base dataset in memory and store it
    let p = |v: i64| Price::from_units(v).unwrap();
    let q = |v: i64| Qty::from_units(v).unwrap();
    let bars = vec![
        Bar {
            ts_ns: 1_704_153_600_000_000_000, // 2024-01-02
            known_at_ns: 1_704_153_600_000_000_000,
            instrument: 0,
            open: p(100),
            high: p(105),
            low: p(95),
            close: p(100),
            volume: q(1000),
        },
        Bar {
            ts_ns: 1_704_240_000_000_000_000, // 2024-01-03
            known_at_ns: 1_704_240_000_000_000_000,
            instrument: 0,
            open: p(50),
            high: p(52),
            low: p(48),
            close: p(50),
            volume: q(2000),
        },
    ];
    let symbols = SymbolTable { symbols: vec!["AAPL".to_string()] };
    let d_id = compute_dataset_id(&bars, &symbols);
    let base_dataset = Dataset {
        bars: bars.clone(),
        symbols: symbols.clone(),
        info: DatasetInfo {
            id: d_id.clone(),
            bars_sha256: spectra_core::bars_sha256(&bars),
            ..DatasetInfo::default()
        },
        quality: spectra_data::dataset::assess_quality(&bars, &symbols, &spectra_data::CsvOptions::default()),
    };

    let raw = spectra_data::datastore::encode_dataset(&base_dataset);
    let blob_sha = spectra_core::sha256_hex(&raw);
    f.store.put(raw.clone()).await.unwrap();

    let reg = spectra_db::datasets::register_version(
        &f.pool,
        &spectra_db::datasets::NewDatasetVersion {
            workspace_id: f.ws,
            dataset_name: "AAPL Stock",
            checksum: &d_id,
            row_count: bars.len() as i64,
            byte_size: raw.len() as i64,
            quality_report: serde_json::json!({}),
            created_by: Some(f.user),
            blob_sha256: Some(&blob_sha),
            source: None,
        },
    )
    .await
    .unwrap();

    let source_version_id = reg.id;

    // 3. Derive adjusted version from source version
    let derive_payload =
        format!(r#"{{"corporate_action_set_id":"{set_id}","adjust_splits":true,"adjust_dividends":true}}"#);
    let res = f
        .app
        .clone()
        .oneshot(f.req(
            "POST",
            &format!("/api/v1/datasets/versions/{source_version_id}/derive"),
            &derive_payload,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let derived: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let derived_id = derived["id"].as_str().unwrap();
    assert_eq!(derived["derived_from_id"], source_version_id.to_string());
    assert_eq!(derived["derivation_kind"], "adjusted");

    // 4. Verify GET /api/v1/datasets/versions/{id} returns derivation metadata
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/datasets/versions/{derived_id}"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v_detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v_detail["derived_from_id"], source_version_id.to_string());
    assert_eq!(v_detail["derivation_kind"], "adjusted");

    // 5. Verify bars: day 1 price adjusted from 100 to 50!
    let res = f
        .app
        .clone()
        .oneshot(f.req("GET", &format!("/api/v1/datasets/versions/{derived_id}/bars?symbol=AAPL"), ""))
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let bars_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let points = bars_resp["points"].as_array().unwrap();
    assert_eq!(points.len(), 2);
    // Day 1 close price was $100, now split-adjusted to $50!
    assert_eq!(points[0]["close"], 50.0);
    assert_eq!(points[1]["close"], 50.0);
}
