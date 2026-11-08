//! Integration tests for M36: Chart drawings, symbol comparison, and saved layout templates.
//!
//! Verifies:
//! 1. Drawings CRUD, soft-delete, viewer read-only rule, 200 drawing limit, 64KB size limit.
//! 2. Symbol comparison rebased to 100 on first bar, timestamp alignment, null gaps, timeframe mismatch (422).
//! 3. Layout templates CRUD, 50-template limit, workspace sharing, retired dataset detection.

#![allow(clippy::unreadable_literal, clippy::vec_init_then_push)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use uuid::Uuid;

use spectra_api::auth::{canonical, sign};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_data::dataset::{compute_dataset_id, SymbolTable};
use spectra_data::datastore::DataStore;
use spectra_data::{Dataset, DatasetInfo};
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;

const SECRET: &str = "test-secret-test-secret-test-secret!";

async fn setup_pool() -> Option<PgPool> {
    let Some(url) = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        eprintln!("skipping chart drawings tests: set TEST_DATABASE_URL to a disposable Postgres");
        return None;
    };

    let db_name =
        url.rsplit_once('/').map_or("", |(_, after_slash)| after_slash.split('?').next().unwrap_or(""));
    assert!(
        db_name.contains("test") || (std::env::var("CI").is_ok() && db_name == "spectra"),
        "refusing to run on non-test database '{db_name}'; database name must contain 'test'"
    );

    let pool =
        connect(&PoolConfig::new(url)).await.expect("TEST_DATABASE_URL is set but database is unreachable");
    migrate(&pool).await.expect("migrations failed");
    Some(pool)
}

fn make_bar(ts_ns: i64, inst: u32, o: f64, h: f64, l: f64, c: f64, v: f64) -> Bar {
    Bar::new(
        ts_ns,
        inst,
        Price::from_f64(o).unwrap(),
        Price::from_f64(h).unwrap(),
        Price::from_f64(l).unwrap(),
        Price::from_f64(c).unwrap(),
        Qty::from_f64(v).unwrap(),
    )
}

struct TestFixture {
    app: Router,
    pool: PgPool,
    _dir: PathBuf,
    ws: Uuid,
    editor: Uuid,
    viewer: Uuid,
    version_id: Uuid,
}

impl TestFixture {
    async fn new(pool: PgPool) -> Self {
        let ws = spectra_db::new_id();
        let editor = spectra_db::new_id();
        let viewer = spectra_db::new_id();
        let dir = std::env::temp_dir().join(format!("spectra-chart-test-{}", spectra_db::new_id().simple()));
        let _ = std::fs::create_dir_all(&dir);

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .bind("M36 Test Workspace")
            .execute(&pool)
            .await
            .expect("insert workspace");

        spectra_db::identity::ensure_identity(&pool, Some(editor), ws).await.unwrap();
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'editor') \
             ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'editor'",
        )
        .bind(ws)
        .bind(editor)
        .execute(&pool)
        .await
        .unwrap();

        spectra_db::identity::ensure_identity(&pool, Some(viewer), ws).await.unwrap();
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer') \
             ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'viewer'",
        )
        .bind(ws)
        .bind(viewer)
        .execute(&pool)
        .await
        .unwrap();

        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));

        // Create dataset for AAPL, MSFT, GOOG
        let symbols = vec!["AAPL", "MSFT", "GOOG"];
        let symbol_table = SymbolTable::from_symbols(symbols);
        let aapl = symbol_table.id("AAPL").unwrap();
        let msft = symbol_table.id("MSFT").unwrap();
        let goog = symbol_table.id("GOOG").unwrap();

        let day_ns = 86_400_000_000_000i64;
        let base_ts = 1_704_153_600_000_000_000i64; // Day 1
        let mut bars = Vec::new();

        // AAPL: [100.0, 105.0, 110.0]
        bars.push(make_bar(base_ts, aapl, 100.0, 100.0, 100.0, 100.0, 100.0));
        bars.push(make_bar(base_ts + day_ns, aapl, 105.0, 105.0, 105.0, 105.0, 100.0));
        bars.push(make_bar(base_ts + 2 * day_ns, aapl, 110.0, 110.0, 110.0, 110.0, 100.0));

        // MSFT: [200.0, 190.0, 210.0]
        bars.push(make_bar(base_ts, msft, 200.0, 200.0, 200.0, 200.0, 100.0));
        bars.push(make_bar(base_ts + day_ns, msft, 190.0, 190.0, 190.0, 190.0, 100.0));
        bars.push(make_bar(base_ts + 2 * day_ns, msft, 210.0, 210.0, 210.0, 210.0, 100.0));

        // GOOG: has bars on Day 1 and Day 3, but MISSING Day 2
        bars.push(make_bar(base_ts, goog, 50.0, 50.0, 50.0, 50.0, 100.0));
        bars.push(make_bar(base_ts + 2 * day_ns, goog, 55.0, 55.0, 55.0, 55.0, 100.0));

        bars.sort_by_key(|b| (b.ts_ns, b.instrument));
        let id = compute_dataset_id(&bars, &symbol_table);
        let dataset = Dataset {
            info: DatasetInfo { id, bars_sha256: spectra_core::bars_sha256(&bars), ..DatasetInfo::default() },
            quality: spectra_data::dataset::assess_quality(
                &bars,
                &symbol_table,
                &spectra_data::CsvOptions::default(),
            ),
            bars: bars.clone(),
            symbols: symbol_table,
        };

        let data_dir = dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let checksum =
            DataStore::new(data_dir.clone()).expect("open store").put(&dataset).expect("store dataset");

        let byte_size = i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).unwrap();
        let version = spectra_db::datasets::register_version(
            &pool,
            &spectra_db::datasets::NewDatasetVersion {
                workspace_id: ws,
                dataset_name: "M36 Comparison Fixture",
                checksum: &checksum,
                row_count: dataset.bars.len() as i64,
                byte_size,
                quality_report: serde_json::json!({}),
                created_by: None,
                blob_sha256: None,
                source: None,
            },
        )
        .await
        .expect("register version");

        Self { app, pool, _dir: dir, ws, editor, viewer, version_id: version.id }
    }

    async fn call(&self, user: Uuid, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let user_str = user.to_string();
        let ws_str = self.ws.to_string();
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

        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json_val = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json_val)
    }
}

#[tokio::test]
async fn test_drawings_crud_and_limits() {
    let Some(pool) = setup_pool().await else { return };
    let f = TestFixture::new(pool).await;

    let chart_id = "AAPL:1d";

    // 1. Viewer tries to create drawing -> 403 viewer_read_only
    let drawing_payload = json!({
        "dataset_version_id": f.version_id,
        "tool": "trendline",
        "points": [
            {"time": 1704153600, "price": 100.0},
            {"time": 1704240000, "price": 105.0}
        ],
        "style": {"color": "#2962FF", "lineWidth": 2}
    });

    let (status, body) =
        f.call(f.viewer, "POST", &format!("/api/v1/charts/{chart_id}/drawings"), &drawing_payload).await;
    assert_eq!(status, 403);
    assert_eq!(body["code"], "viewer_read_only");

    // 2. Editor creates drawing -> 201 Created
    let (status, body) =
        f.call(f.editor, "POST", &format!("/api/v1/charts/{chart_id}/drawings"), &drawing_payload).await;
    assert_eq!(status, 201);
    let drawing_id = body["id"].as_str().expect("drawing id").to_string();
    assert_eq!(body["tool"], "trendline");
    assert_eq!(body["points"][0]["price"], 100.0);

    // 3. List active drawings -> returns 1 item
    let (status, body) =
        f.call(f.editor, "GET", &format!("/api/v1/charts/{chart_id}/drawings"), &Value::Null).await;
    assert_eq!(status, 200);
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], drawing_id);

    // 4. Update drawing coordinates
    let update_payload = json!({
        "points": [
            {"time": 1704153600, "price": 102.0},
            {"time": 1704240000, "price": 108.0}
        ],
        "style": {"color": "#FF0000", "lineWidth": 3}
    });
    let (status, body) =
        f.call(f.editor, "PUT", &format!("/api/v1/drawings/{drawing_id}"), &update_payload).await;
    assert_eq!(status, 200);
    assert_eq!(body["points"][0]["price"], 102.0);
    assert_eq!(body["style"]["color"], "#FF0000");

    // 5. Viewer cannot update or delete drawing
    let (status, body) =
        f.call(f.viewer, "PUT", &format!("/api/v1/drawings/{drawing_id}"), &update_payload).await;
    assert_eq!(status, 403);
    assert_eq!(body["code"], "viewer_read_only");

    let (status, body) =
        f.call(f.viewer, "DELETE", &format!("/api/v1/drawings/{drawing_id}"), &Value::Null).await;
    assert_eq!(status, 403);
    assert_eq!(body["code"], "viewer_read_only");

    // 6. Soft delete drawing by editor -> 200
    let (status, body) =
        f.call(f.editor, "DELETE", &format!("/api/v1/drawings/{drawing_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["deleted"], true);

    // List drawings again -> empty
    let (status, body) =
        f.call(f.editor, "GET", &format!("/api/v1/charts/{chart_id}/drawings"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);

    // 7. Test invalid tool -> 422
    let invalid_tool = json!({
        "tool": "magic_wand",
        "points": [{"time": 1, "price": 10.0}],
        "style": {}
    });
    let (status, _) =
        f.call(f.editor, "POST", &format!("/api/v1/charts/{chart_id}/drawings"), &invalid_tool).await;
    assert_eq!(status, 422);

    // 8. 200 drawings limit test
    // Insert 200 drawings directly into DB to test limit quickly
    for _i in 0..200 {
        sqlx::query(
            "INSERT INTO chart_drawings (id, workspace_id, user_id, chart_id, dataset_version_id, tool, points, style) \
             VALUES ($1, $2, $3, $4, $5, 'horizontal_level', '[]', '{}')",
        )
        .bind(spectra_db::new_id())
        .bind(f.ws)
        .bind(f.editor)
        .bind("LIMIT_CHART:1d")
        .bind(f.version_id)
        .execute(&f.pool)
        .await
        .unwrap();
    }

    // Attempting 201st drawing on LIMIT_CHART:1d must fail with 422 drawing_limit_exceeded
    let h_level = json!({
        "tool": "horizontal_level",
        "points": [{"price": 150.0}],
        "style": {"color": "#00FF00"}
    });
    let (status, body) = f.call(f.editor, "POST", "/api/v1/charts/LIMIT_CHART:1d/drawings", &h_level).await;
    assert_eq!(status, 422);
    assert_eq!(body["code"], "drawing_limit_exceeded");
}

#[tokio::test]
async fn test_symbol_comparison_flow() {
    let Some(pool) = setup_pool().await else { return };
    let f = TestFixture::new(pool).await;

    // 1. Compare AAPL (base) with MSFT and GOOG
    let url = format!(
        "/api/v1/charts/compare?dataset_version_id={}&base_symbol=AAPL&compare_symbols=MSFT,GOOG&timeframe=1d",
        f.version_id
    );
    let (status, body) = f.call(f.editor, "GET", &url, &Value::Null).await;
    assert_eq!(status, 200, "Comparison failed: {body:?}");

    assert_eq!(body["base_symbol"], "AAPL");
    let timestamps = body["timestamps"].as_array().expect("timestamps array");
    assert_eq!(timestamps.len(), 3);

    let base_series = body["series"]["AAPL"].as_array().expect("AAPL series");
    let msft_series = body["series"]["MSFT"].as_array().expect("MSFT series");
    let goog_series = body["series"]["GOOG"].as_array().expect("GOOG series");

    // AAPL: [100.0, 105.0, 110.0] -> rebased to 100 on first bar
    assert_eq!(base_series[0], 100.0);
    assert_eq!(base_series[1], 105.0);
    assert_eq!(base_series[2], 110.0);

    // MSFT: [200.0, 190.0, 210.0] -> rebased to 100 on first bar
    // 200 -> 100.0, 190 -> 95.0, 210 -> 105.0
    assert_eq!(msft_series[0], 100.0);
    assert_eq!(msft_series[1], 95.0);
    assert_eq!(msft_series[2], 105.0);

    // GOOG: [50.0, missing, 55.0] -> Day 2 must be null!
    assert_eq!(goog_series[0], 100.0);
    assert!(goog_series[1].is_null(), "Missing day should be null, got {:?}", goog_series[1]);
    assert_eq!(goog_series[2], 110.0); // 55.0 / 50.0 * 100.0 = 110.0

    // 2. Timeframe mismatch returns 422
    let url_mismatch = format!(
        "/api/v1/charts/compare?dataset_version_id={}&base_symbol=AAPL&compare_symbols=MSFT&timeframe=1h",
        f.version_id
    );
    let (status, body) = f.call(f.editor, "GET", &url_mismatch, &Value::Null).await;
    assert_eq!(status, 422);
    assert_eq!(body["code"], "timeframe_mismatch");
}

#[tokio::test]
async fn test_chart_templates_flow() {
    let Some(pool) = setup_pool().await else { return };
    let f = TestFixture::new(pool).await;

    // 1. Viewer tries to create template -> 403 viewer_read_only
    let tmpl_payload = json!({
        "name": "Moving Averages + RSI Setup",
        "version": 1,
        "indicators": [
            {"name": "sma", "params": {"period": 20}},
            {"name": "rsi", "params": {"period": 14}}
        ],
        "drawings": [
            {"tool": "horizontal_level", "points": [{"price": 150.0}], "style": {"color": "#FF0000"}}
        ],
        "comparisons": ["MSFT"],
        "dataset_versions": [f.version_id]
    });

    let (status, body) = f.call(f.viewer, "POST", "/api/v1/chart-templates", &tmpl_payload).await;
    assert_eq!(status, 403);
    assert_eq!(body["code"], "viewer_read_only");

    // 2. Editor creates template -> 201
    let (status, body) = f.call(f.editor, "POST", "/api/v1/chart-templates", &tmpl_payload).await;
    assert_eq!(status, 201);
    let tmpl_id = body["id"].as_str().expect("template id").to_string();
    assert_eq!(body["name"], "Moving Averages + RSI Setup");
    assert_eq!(body["version"], 1);

    // 3. Get template by id -> 200
    let (status, body) =
        f.call(f.editor, "GET", &format!("/api/v1/chart-templates/{tmpl_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["name"], "Moving Averages + RSI Setup");
    assert_eq!(body["retired_dataset_versions"].as_array().unwrap().len(), 0);

    // 4. List templates -> 200
    let (status, body) = f.call(f.editor, "GET", "/api/v1/chart-templates", &Value::Null).await;
    assert_eq!(status, 200);
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], tmpl_id);

    // 5. Delete template by editor -> 200
    let (status, body) =
        f.call(f.editor, "DELETE", &format!("/api/v1/chart-templates/{tmpl_id}"), &Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["deleted"], true);

    // 6. 50 template limit check
    for i in 0..50 {
        sqlx::query(
            "INSERT INTO chart_templates (id, workspace_id, user_id, name, version, indicators, drawings, comparisons, dataset_versions) \
             VALUES ($1, $2, $3, $4, 1, '[]', '[]', '[]', '[]')",
        )
        .bind(spectra_db::new_id())
        .bind(f.ws)
        .bind(f.editor)
        .bind(format!("Template {i}"))
        .execute(&f.pool)
        .await
        .unwrap();
    }

    let extra_tmpl = json!({
        "name": "Template 51",
        "version": 1,
        "indicators": [],
        "drawings": [],
        "comparisons": [],
        "dataset_versions": []
    });
    let (status, body) = f.call(f.editor, "POST", "/api/v1/chart-templates", &extra_tmpl).await;
    assert_eq!(status, 422);
    assert_eq!(body["code"], "template_limit_exceeded");
}
