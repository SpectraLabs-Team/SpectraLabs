//! Integration and reference tests for M35 Task 2 (Indicator Series API).
//!
//! Verifies:
//! 1. All 25 indicators called via GET /api/v1/indicators against reference fixtures.
//! 2. Warm-up contract (first non-null index for SMA, MACD, ADX, Ichimoku).
//! 3. Validation errors (unknown indicator, period 0, missing parameters, timeframe mismatch -> 422).
//! 4. Latency p95 benchmark on a 10,000 bar fixture for a 5-indicator set.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
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

async fn setup_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            panic!("set TEST_DATABASE_URL to a disposable Postgres; these tests write fixtures")
        });

    let db_name =
        url.rsplit_once('/').map_or("", |(_, after_slash)| after_slash.split('?').next().unwrap_or(""));
    assert!(
        db_name.contains("test"),
        "refusing to run on non-test database '{db_name}'; database name must contain 'test'"
    );

    let pool =
        connect(&PoolConfig::new(url)).await.expect("TEST_DATABASE_URL is set but database is unreachable");
    migrate(&pool).await.expect("migrations failed");
    pool
}

struct TestFixture {
    app: Router,
    _pool: PgPool,
    dir: std::path::PathBuf,
    user: Uuid,
    ws: Uuid,
    version_id: Uuid,
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

impl TestFixture {
    async fn new(pool: PgPool) -> Self {
        let ws = spectra_db::new_id();
        let user = spectra_db::new_id();
        let dir = std::env::temp_dir().join(format!("spectra-ind-test-{}", spectra_db::new_id().simple()));
        let _ = std::fs::create_dir_all(&dir);

        sqlx::query("INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(ws)
            .bind(format!("ws-{}", ws.simple()))
            .bind("M35 Test WS")
            .execute(&pool)
            .await
            .expect("insert workspace");

        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store);
        cfg.data_dir = Some(dir.join("data"));
        let app = router(cfg, Some(pool.clone()));

        // Build dataset containing bars for all 25 reference fixtures
        let symbol_names = vec![
            "SYM_SMA",
            "SYM_EMA",
            "SYM_WMA",
            "SYM_DEMA",
            "SYM_TEMA",
            "SYM_RSI",
            "SYM_MACD",
            "SYM_BB",
            "SYM_ATR",
            "SYM_STOCH",
            "SYM_ADX",
            "SYM_OBV",
            "SYM_VWAP",
            "SYM_ICHIMOKU",
            "SYM_PSAR",
            "SYM_CCI",
            "SYM_WILLR",
            "SYM_DONCHIAN",
            "SYM_KELTNER",
            "SYM_CMF",
            "SYM_MOM",
            "SYM_ROC",
            "SYM_MFI",
            "SYM_STDDEV",
            "SYM_TYPICAL",
            "SYM_WARMUP",
            "SYM_BENCH",
        ];
        let symbol_table = SymbolTable::from_symbols(symbol_names);

        let day_ns = 86_400_000_000_000i64;
        let base_ts = 1_704_153_600_000_000_000i64; // 2024-01-02
        let mut all_bars = Vec::new();

        // 1. SMA: [10.0, 11.0, 12.0, 13.0, 14.0, 15.0]
        let inst = symbol_table.id("SYM_SMA").unwrap();
        for (i, &p) in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 2. EMA: [10.0, 11.0, 12.0, 13.0, 14.0]
        let inst = symbol_table.id("SYM_EMA").unwrap();
        for (i, &p) in [10.0, 11.0, 12.0, 13.0, 14.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 3. WMA: [1.0, 2.0, 3.0, 4.0, 5.0]
        let inst = symbol_table.id("SYM_WMA").unwrap();
        for (i, &p) in [1.0, 2.0, 3.0, 4.0, 5.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 4. DEMA: [10.0 .. 19.0] (10 values)
        let inst = symbol_table.id("SYM_DEMA").unwrap();
        for (i, &p) in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 5. TEMA: 1..=20
        let inst = symbol_table.id("SYM_TEMA").unwrap();
        for i in 1..=20 {
            let p = i as f64;
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 6. RSI: Wilder 17 values
        let inst = symbol_table.id("SYM_RSI").unwrap();
        let rsi_vals = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03, 45.61, 46.28,
            46.28, 46.00, 46.03,
        ];
        for (i, &p) in rsi_vals.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 7. MACD: 1..=50
        let inst = symbol_table.id("SYM_MACD").unwrap();
        for i in 1..=50 {
            let p = i as f64;
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 8. BB: [10.0, 12.0, 11.0, 13.0, 12.0, 14.0, 13.0]
        let inst = symbol_table.id("SYM_BB").unwrap();
        for (i, &p) in [10.0, 12.0, 11.0, 13.0, 12.0, 14.0, 13.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 9. ATR: 4 bars
        let inst = symbol_table.id("SYM_ATR").unwrap();
        let atr_data = [
            (10.0, 12.0, 9.0, 11.0, 100.0),
            (11.0, 13.0, 10.5, 12.5, 100.0),
            (12.5, 14.0, 12.0, 13.5, 100.0),
            (13.5, 15.0, 13.0, 14.0, 100.0),
        ];
        for (i, &(o, h, l, c, v)) in atr_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 10. STOCH: 4 bars
        let inst = symbol_table.id("SYM_STOCH").unwrap();
        let stoch_data = [
            (10.0, 12.0, 8.0, 11.0, 100.0),
            (11.0, 13.0, 9.0, 12.0, 100.0),
            (12.0, 14.0, 10.0, 13.0, 100.0),
            (13.0, 15.0, 11.0, 14.0, 100.0),
        ];
        for (i, &(o, h, l, c, v)) in stoch_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 11. ADX: 40 bars
        let inst = symbol_table.id("SYM_ADX").unwrap();
        for i in 1..=40 {
            let p = i as f64;
            let low = if p > 1.0 { p - 1.0 } else { 0.5 };
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p + 2.0, low, p + 1.0, 100.0));
        }

        // 12. OBV: 3 bars
        let inst = symbol_table.id("SYM_OBV").unwrap();
        let obv_data =
            [(10.0, 11.0, 9.0, 10.0, 100.0), (10.0, 12.0, 10.0, 11.0, 50.0), (11.0, 11.5, 9.5, 9.5, 30.0)];
        for (i, &(o, h, l, c, v)) in obv_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 13. VWAP: 2 bars
        let inst = symbol_table.id("SYM_VWAP").unwrap();
        let vwap_data = [(10.0, 12.0, 8.0, 10.0, 100.0), (11.0, 14.0, 11.0, 11.0, 100.0)];
        for (i, &(o, h, l, c, v)) in vwap_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 14. ICHIMOKU: 60 bars
        let inst = symbol_table.id("SYM_ICHIMOKU").unwrap();
        for i in 1..=60 {
            let p = i as f64;
            let low = if p > 1.0 { p - 1.0 } else { 0.5 };
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p + 1.0, low, p, 100.0));
        }

        // 15. PSAR: 3 bars
        let inst = symbol_table.id("SYM_PSAR").unwrap();
        let psar_data =
            [(10.0, 11.0, 9.0, 10.0, 100.0), (10.0, 12.0, 9.5, 11.5, 100.0), (11.5, 13.0, 11.0, 12.5, 100.0)];
        for (i, &(o, h, l, c, v)) in psar_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 16. CCI: 30 bars
        let inst = symbol_table.id("SYM_CCI").unwrap();
        for i in 1..=30 {
            let p = (i % 5) as f64 + 10.0;
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p + 1.0, p - 1.0, p, 100.0));
        }

        // 17. WILLR: 3 bars
        let inst = symbol_table.id("SYM_WILLR").unwrap();
        let willr_data =
            [(10.0, 12.0, 8.0, 10.0, 100.0), (10.0, 13.0, 9.0, 12.0, 100.0), (12.0, 14.0, 10.0, 11.0, 100.0)];
        for (i, &(o, h, l, c, v)) in willr_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 18. DONCHIAN: 3 bars
        let inst = symbol_table.id("SYM_DONCHIAN").unwrap();
        let donch_data =
            [(10.0, 15.0, 5.0, 10.0, 100.0), (10.0, 16.0, 6.0, 11.0, 100.0), (11.0, 14.0, 7.0, 12.0, 100.0)];
        for (i, &(o, h, l, c, v)) in donch_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 19. KELTNER: 3 bars
        let inst = symbol_table.id("SYM_KELTNER").unwrap();
        let kelt_data =
            [(10.0, 12.0, 8.0, 10.0, 100.0), (10.0, 13.0, 9.0, 11.0, 100.0), (11.0, 14.0, 10.0, 12.0, 100.0)];
        for (i, &(o, h, l, c, v)) in kelt_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 20. CMF: 3 bars
        let inst = symbol_table.id("SYM_CMF").unwrap();
        let cmf_data =
            [(10.0, 12.0, 8.0, 11.0, 100.0), (11.0, 13.0, 9.0, 12.0, 100.0), (12.0, 14.0, 10.0, 13.0, 100.0)];
        for (i, &(o, h, l, c, v)) in cmf_data.iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, o, h, l, c, v));
        }

        // 21. MOM: 5 bars
        let inst = symbol_table.id("SYM_MOM").unwrap();
        for (i, &p) in [10.0, 12.0, 15.0, 14.0, 18.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 22. ROC: 4 bars
        let inst = symbol_table.id("SYM_ROC").unwrap();
        for (i, &p) in [100.0, 110.0, 120.0, 110.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 23. MFI: 20 bars
        let inst = symbol_table.id("SYM_MFI").unwrap();
        for i in 1..=20 {
            let p = (i as f64) * 2.0;
            all_bars.push(make_bar(base_ts + (i - 1) as i64 * day_ns, inst, p, p + 1.0, p - 1.0, p, 100.0));
        }

        // 24. STDDEV: 8 bars
        let inst = symbol_table.id("SYM_STDDEV").unwrap();
        for (i, &p) in [10.0, 12.0, 23.0, 23.0, 16.0, 23.0, 21.0, 16.0].iter().enumerate() {
            all_bars.push(make_bar(base_ts + i as i64 * day_ns, inst, p, p, p, p, 100.0));
        }

        // 25. TYPICAL / MEDIAN: 1 bar
        let inst = symbol_table.id("SYM_TYPICAL").unwrap();
        all_bars.push(make_bar(base_ts, inst, 10.0, 14.0, 8.0, 11.0, 100.0));

        // Warmup test symbol: 70 bars with non-trivial movement
        let inst = symbol_table.id("SYM_WARMUP").unwrap();
        for i in 0..70 {
            let p = 100.0 + (i as f64);
            all_bars.push(make_bar(base_ts + i * day_ns, inst, p, p + 2.0, p - 1.0, p + 0.5, 1000.0));
        }

        // Latency benchmark symbol: 10,000 bars
        let inst = symbol_table.id("SYM_BENCH").unwrap();
        for i in 0..10_000 {
            let p = 100.0 + ((i % 50) as f64);
            all_bars.push(make_bar(base_ts + i * day_ns, inst, p, p + 1.0, p - 1.0, p, 500.0));
        }

        // Sort bars by (ts_ns, instrument) as required by Dataset
        all_bars.sort_by_key(|b| (b.ts_ns, b.instrument));
        if let Some(err) = spectra_core::validate_bars(&all_bars) {
            panic!("validate_bars failed: {err}");
        }

        let id = compute_dataset_id(&all_bars, &symbol_table);
        let dataset = Dataset {
            info: DatasetInfo {
                id,
                bars_sha256: spectra_core::bars_sha256(&all_bars),
                ..DatasetInfo::default()
            },
            quality: spectra_data::dataset::assess_quality(
                &all_bars,
                &symbol_table,
                &spectra_data::CsvOptions::default(),
            ),
            bars: all_bars,
            symbols: symbol_table,
        };

        let data_dir = dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let checksum =
            DataStore::new(data_dir.clone()).expect("open data store").put(&dataset).expect("store dataset");

        let byte_size = i64::try_from(spectra_data::datastore::encode_dataset(&dataset).len()).unwrap();
        let version = spectra_db::datasets::register_version(
            &pool,
            &spectra_db::datasets::NewDatasetVersion {
                workspace_id: ws,
                dataset_name: "M35 Technical Indicators Fixture",
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
        .expect("register dataset version");

        Self { app, _pool: pool, dir, user, ws, version_id: version.id }
    }

    fn signed_get(&self, path_and_query: &str) -> Request<Body> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let user = self.user.to_string();
        let ws_str = self.ws.to_string();
        let req_id = spectra_db::new_id().to_string();
        let can = canonical("GET", path_and_query, &ts, &user, &ws_str, &req_id, b"");
        let sig = sign(SECRET.as_bytes(), &can);

        Request::builder()
            .method("GET")
            .uri(path_and_query)
            .header("X-Spectra-User", user)
            .header("X-Spectra-Workspace", ws_str)
            .header("X-Request-Id", req_id)
            .header("X-Spectra-Timestamp", ts)
            .header("X-Spectra-Signature", sig)
            .body(Body::empty())
            .unwrap()
    }

    async fn call_api(&self, path_and_query: &str) -> (StatusCode, Value) {
        let req = self.signed_get(path_and_query);
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn test_all_25_indicators_reference_fixtures_parity() {
    let pool = setup_pool().await;
    let fix = TestFixture::new(pool).await;
    let vid = fix.version_id;

    // 1. SMA: period=3 on SYM_SMA
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_SMA&dataset_version_id={vid}&timeframe=1Day&indicator=SMA&period=3")).await;
    assert_eq!(s, StatusCode::OK, "SMA error: {j}");
    assert!(j.get("series").is_none(), "single-output SMA has no `series`: {j}");
    let series = &j["values"];
    assert_eq!(series[0], Value::Null);
    assert_eq!(series[1], Value::Null);
    assert_eq!(series[2].as_f64().unwrap(), 11.0);
    assert_eq!(series[3].as_f64().unwrap(), 12.0);
    assert_eq!(series[4].as_f64().unwrap(), 13.0);
    assert_eq!(series[5].as_f64().unwrap(), 14.0);

    // 2. EMA: period=3 on SYM_EMA
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_EMA&dataset_version_id={vid}&timeframe=1Day&indicator=EMA&period=3")).await;
    assert_eq!(s, StatusCode::OK, "EMA error: {j}");
    let series = &j["values"];
    assert_eq!(series[2].as_f64().unwrap(), 11.0);
    assert_eq!(series[3].as_f64().unwrap(), 12.0);
    assert_eq!(series[4].as_f64().unwrap(), 13.0);

    // 3. WMA: period=3 on SYM_WMA
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_WMA&dataset_version_id={vid}&timeframe=1Day&indicator=WMA&period=3")).await;
    assert_eq!(s, StatusCode::OK, "WMA error: {j}");
    let series = &j["values"];
    let v2 = series[2].as_f64().unwrap();
    assert!((v2 - (14.0 / 6.0)).abs() < 1e-10);
    let v3 = series[3].as_f64().unwrap();
    assert!((v3 - (20.0 / 6.0)).abs() < 1e-10);

    // 4. DEMA: period=3 on SYM_DEMA
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_DEMA&dataset_version_id={vid}&timeframe=1Day&indicator=DEMA&period=3")).await;
    assert_eq!(s, StatusCode::OK, "DEMA error: {j}");
    let val = j["values"].as_array().unwrap().iter().rev().find_map(|x| x.as_f64()).unwrap();
    assert!(val > 10.0 && val <= 19.0);

    // 5. TEMA: period=3 on SYM_TEMA
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_TEMA&dataset_version_id={vid}&timeframe=1Day&indicator=TEMA&period=3")).await;
    assert_eq!(s, StatusCode::OK, "TEMA error: {j}");
    let last = j["values"].as_array().unwrap().last().unwrap().as_f64().unwrap();
    assert!((last - 20.0).abs() < 1.0);

    // 6. RSI: period=14 on SYM_RSI
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_RSI&dataset_version_id={vid}&timeframe=1Day&indicator=RSI&period=14")).await;
    assert_eq!(s, StatusCode::OK, "RSI error: {j}");
    assert_eq!(j["values"][13], Value::Null);
    let rsi_14 = j["values"][14].as_f64().unwrap();
    assert!(rsi_14 >= 65.0 && rsi_14 <= 75.0, "RSI: {rsi_14}");

    // 7. MACD: fast=12, slow=26, signal=9 on SYM_MACD
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_MACD&dataset_version_id={vid}&timeframe=1Day&indicator=MACD&fast=12&slow=26&signal=9")).await;
    assert_eq!(s, StatusCode::OK, "MACD error: {j}");
    assert!(j["series"]["macd"][35].is_number());
    assert!(j["series"]["signal"][35].is_number());
    assert!(j["series"]["histogram"][35].is_number());

    // 8. BB: period=5, mult=2.0 on SYM_BB
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_BB&dataset_version_id={vid}&timeframe=1Day&indicator=BB&period=5&mult=2.0")).await;
    assert_eq!(s, StatusCode::OK, "BB error: {j}");
    assert!(j.get("values").is_none(), "multi-output BB has no top-level `values`: {j}");
    let up = j["series"]["upper"][4].as_f64().unwrap();
    let mid = j["series"]["middle"][4].as_f64().unwrap();
    let low = j["series"]["lower"][4].as_f64().unwrap();
    let bw = j["series"]["bandwidth"][4].as_f64().unwrap();
    assert!(up > mid);
    assert!(low < mid);
    assert!(bw > 0.0);

    // 9. ATR: period=3 on SYM_ATR
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_ATR&dataset_version_id={vid}&timeframe=1Day&indicator=ATR&period=3")).await;
    assert_eq!(s, StatusCode::OK, "ATR error: {j}");
    assert!(j["values"][2].as_f64().unwrap() > 0.0);

    // 10. STOCH: k=3, d=2, slow_d=2 on SYM_STOCH
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_STOCH&dataset_version_id={vid}&timeframe=1Day&indicator=STOCH&k=3&d=2&slow_d=2")).await;
    assert_eq!(s, StatusCode::OK, "STOCH error: {j}");
    let pk = j["series"]["percent_k"][2].as_f64().unwrap();
    assert!(pk >= 0.0 && pk <= 100.0);

    // 11. ADX: period=14 on SYM_ADX
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_ADX&dataset_version_id={vid}&timeframe=1Day&indicator=ADX&period=14")).await;
    assert_eq!(s, StatusCode::OK, "ADX error: {j}");
    assert!(j["series"]["plus_di"][13].is_number());
    assert!(j["series"]["minus_di"][13].is_number());

    // 12. OBV: on SYM_OBV
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_OBV&dataset_version_id={vid}&timeframe=1Day&indicator=OBV"
        ))
        .await;
    assert_eq!(s, StatusCode::OK, "OBV error: {j}");
    assert_eq!(j["values"][0].as_f64().unwrap(), 100.0);
    assert_eq!(j["values"][1].as_f64().unwrap(), 150.0);
    assert_eq!(j["values"][2].as_f64().unwrap(), 120.0);

    // 13. VWAP: on SYM_VWAP
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_VWAP&dataset_version_id={vid}&timeframe=1Day&indicator=VWAP"
        ))
        .await;
    assert_eq!(s, StatusCode::OK, "VWAP error: {j}");
    assert_eq!(j["values"][0].as_f64().unwrap(), 10.0);
    assert_eq!(j["values"][1].as_f64().unwrap(), 11.0);

    // 14. ICHIMOKU: tenkan=9, kijun=26, span_b=52 on SYM_ICHIMOKU
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_ICHIMOKU&dataset_version_id={vid}&timeframe=1Day&indicator=ICHIMOKU&tenkan=9&kijun=26&span_b=52")).await;
    assert_eq!(s, StatusCode::OK, "ICHIMOKU error: {j}");
    assert!(j["series"]["tenkan_sen"][8].is_number());
    assert!(j["series"]["kijun_sen"][25].is_number());
    assert!(j["series"]["senkou_span_b"][51].is_number());

    // 15. PSAR: start=0.02, step=0.02, max=0.2 on SYM_PSAR
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_PSAR&dataset_version_id={vid}&timeframe=1Day&indicator=PSAR&start=0.02&step=0.02&max=0.2")).await;
    assert_eq!(s, StatusCode::OK, "PSAR error: {j}");
    assert!(j["values"][0].is_number());
    assert!(j["values"][1].is_number());

    // 16. CCI: period=14 on SYM_CCI
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_CCI&dataset_version_id={vid}&timeframe=1Day&indicator=CCI&period=14")).await;
    assert_eq!(s, StatusCode::OK, "CCI error: {j}");
    assert!(j["values"][13].is_number());

    // 17. WILLR: period=3 on SYM_WILLR
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_WILLR&dataset_version_id={vid}&timeframe=1Day&indicator=WILLR&period=3")).await;
    assert_eq!(s, StatusCode::OK, "WILLR error: {j}");
    let wr = j["values"][2].as_f64().unwrap();
    assert!(wr >= -100.0 && wr <= 0.0);

    // 18. DONCHIAN: period=3 on SYM_DONCHIAN
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_DONCHIAN&dataset_version_id={vid}&timeframe=1Day&indicator=DONCHIAN&period=3")).await;
    assert_eq!(s, StatusCode::OK, "DONCHIAN error: {j}");
    assert_eq!(j["series"]["upper"][2].as_f64().unwrap(), 16.0);
    assert_eq!(j["series"]["lower"][2].as_f64().unwrap(), 5.0);
    assert_eq!(j["series"]["middle"][2].as_f64().unwrap(), 10.5);

    // 19. KELTNER: period=3, mult=1.5 on SYM_KELTNER
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_KELTNER&dataset_version_id={vid}&timeframe=1Day&indicator=KELTNER&period=3&mult=1.5")).await;
    assert_eq!(s, StatusCode::OK, "KELTNER error: {j}");
    assert!(j["series"]["upper"][2].is_number());
    assert!(j["series"]["lower"][2].is_number());

    // 20. CMF: period=3 on SYM_CMF
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_CMF&dataset_version_id={vid}&timeframe=1Day&indicator=CMF&period=3")).await;
    assert_eq!(s, StatusCode::OK, "CMF error: {j}");
    let cmf = j["values"][2].as_f64().unwrap();
    assert!(cmf >= -1.0 && cmf <= 1.0);

    // 21. MOM: period=2 on SYM_MOM
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_MOM&dataset_version_id={vid}&timeframe=1Day&indicator=MOM&period=2")).await;
    assert_eq!(s, StatusCode::OK, "MOM error: {j}");
    assert_eq!(j["values"][0], Value::Null);
    assert_eq!(j["values"][1], Value::Null);
    assert_eq!(j["values"][2].as_f64().unwrap(), 5.0);
    assert_eq!(j["values"][3].as_f64().unwrap(), 2.0);
    assert_eq!(j["values"][4].as_f64().unwrap(), 3.0);

    // 22. ROC: period=2 on SYM_ROC
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_ROC&dataset_version_id={vid}&timeframe=1Day&indicator=ROC&period=2")).await;
    assert_eq!(s, StatusCode::OK, "ROC error: {j}");
    assert_eq!(j["values"][0], Value::Null);
    assert_eq!(j["values"][1], Value::Null);
    assert_eq!(j["values"][2].as_f64().unwrap(), 20.0);

    // 23. MFI: period=14 on SYM_MFI
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_MFI&dataset_version_id={vid}&timeframe=1Day&indicator=MFI&period=14")).await;
    assert_eq!(s, StatusCode::OK, "MFI error: {j}");
    let m = j["values"][14].as_f64().unwrap();
    assert!(m >= 0.0 && m <= 100.0);

    // 24. STDDEV: period=5 on SYM_STDDEV
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_STDDEV&dataset_version_id={vid}&timeframe=1Day&indicator=STDDEV&period=5")).await;
    assert_eq!(s, StatusCode::OK, "STDDEV error: {j}");
    assert!(j["values"][4].as_f64().unwrap() > 0.0);

    // 25. TYPICAL and MEDIAN: on SYM_TYPICAL
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_TYPICAL&dataset_version_id={vid}&timeframe=1Day&indicator=TYPICAL"
        ))
        .await;
    assert_eq!(s, StatusCode::OK, "TYPICAL error: {j}");
    assert_eq!(j["values"][0].as_f64().unwrap(), (14.0 + 8.0 + 11.0) / 3.0);

    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_TYPICAL&dataset_version_id={vid}&timeframe=1Day&indicator=MEDIAN"
        ))
        .await;
    assert_eq!(s, StatusCode::OK, "MEDIAN error: {j}");
    assert_eq!(j["values"][0].as_f64().unwrap(), (14.0 + 8.0) / 2.0);
}

#[tokio::test]
async fn test_validation_errors() {
    let pool = setup_pool().await;
    let fix = TestFixture::new(pool).await;
    let vid = fix.version_id;

    // 1. Unknown indicator returns 422 naming indicator
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_SMA&dataset_version_id={vid}&timeframe=1Day&indicator=UNKNOWN_XYZ"
        ))
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(j["code"], "invalid_request");
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs
        .iter()
        .any(|e| e["field"] == "indicator" && e["message"].as_str().unwrap().contains("unknown")));

    // 2. Period 0 returns 422 naming period
    let (s, j) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_SMA&dataset_version_id={vid}&timeframe=1Day&indicator=SMA&period=0")).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs
        .iter()
        .any(|e| e["field"] == "period" && e["message"].as_str().unwrap().contains("greater than 0")));

    // 3. Missing required parameter (e.g. symbol omitted) returns 422
    let (s, j) = fix
        .call_api(&format!("/api/v1/indicators?dataset_version_id={vid}&timeframe=1Day&indicator=SMA"))
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs
        .iter()
        .any(|e| e["field"] == "symbol" && e["message"].as_str().unwrap().contains("required")));

    // 4. Missing required dataset_version_id returns 422
    let (s, j) = fix.call_api("/api/v1/indicators?symbol=SYM_SMA&timeframe=1Day&indicator=SMA").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs.iter().any(|e| e["field"] == "dataset_version_id"));

    // 5. Incompatible timeframe returns 422
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_SMA&dataset_version_id={vid}&timeframe=1Min&indicator=SMA"
        ))
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs
        .iter()
        .any(|e| e["field"] == "timeframe" && e["message"].as_str().unwrap().contains("does not match")));

    // 6. "1ns" is a test-only timeframe: the production route refuses it as unsupported
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_SMA&dataset_version_id={vid}&timeframe=1ns&indicator=SMA"
        ))
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{j}");
    let errs = j["details"]["errors"].as_array().unwrap();
    assert!(errs.iter().any(
        |e| e["field"] == "timeframe" && e["message"].as_str().unwrap().contains("unsupported timeframe")
    ));
}

#[tokio::test]
async fn test_warmup_exact_contract() {
    let pool = setup_pool().await;
    let fix = TestFixture::new(pool).await;
    let vid = fix.version_id;

    // 1. SMA: default period=20 on SYM_WARMUP (70 bars)
    // First non-null index must be exactly 19
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_WARMUP&dataset_version_id={vid}&timeframe=1Day&indicator=SMA"
        ))
        .await;
    assert_eq!(s, StatusCode::OK);
    let sma = j["values"].as_array().unwrap();
    for i in 0..19 {
        assert_eq!(sma[i], Value::Null, "SMA warmup null expected at {i}");
    }
    assert!(sma[19].is_number(), "SMA first non-null expected at index 19");

    // 2. MACD: defaults (12, 26, 9) on SYM_WARMUP (70 bars)
    // MACD line from index 25 (26 bars); signal and histogram from index 33 (34 bars)
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_WARMUP&dataset_version_id={vid}&timeframe=1Day&indicator=MACD"
        ))
        .await;
    assert_eq!(s, StatusCode::OK);
    let macd_line = j["series"]["macd"].as_array().unwrap();
    let signal_line = j["series"]["signal"].as_array().unwrap();
    let hist = j["series"]["histogram"].as_array().unwrap();
    for i in 0..25 {
        assert_eq!(macd_line[i], Value::Null, "MACD line null expected at {i}");
    }
    assert!(macd_line[25].is_number(), "MACD line first value expected at index 25");
    for i in 0..33 {
        assert_eq!(signal_line[i], Value::Null, "Signal line null expected at {i}");
        assert_eq!(hist[i], Value::Null, "Histogram null expected at {i}");
    }
    assert!(signal_line[33].is_number(), "Signal first value expected at index 33");
    assert!(hist[33].is_number(), "Histogram first value expected at index 33");

    // 3. ADX: default period=14 on SYM_WARMUP (70 bars)
    // +DI and -DI from index 13; ADX from index 2n-2 = 26
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_WARMUP&dataset_version_id={vid}&timeframe=1Day&indicator=ADX"
        ))
        .await;
    assert_eq!(s, StatusCode::OK);
    let plus_di = j["series"]["plus_di"].as_array().unwrap();
    let minus_di = j["series"]["minus_di"].as_array().unwrap();
    let adx = j["series"]["adx"].as_array().unwrap();
    for i in 0..13 {
        assert_eq!(plus_di[i], Value::Null, "+DI null expected at {i}");
        assert_eq!(minus_di[i], Value::Null, "-DI null expected at {i}");
    }
    assert!(plus_di[13].is_number(), "+DI first value expected at index 13");
    assert!(minus_di[13].is_number(), "-DI first value expected at index 13");
    for i in 0..26 {
        assert_eq!(adx[i], Value::Null, "ADX null expected at {i}");
    }
    assert!(adx[26].is_number(), "ADX first value expected at index 26");

    // 4. Ichimoku: defaults (9, 26, 52) on SYM_WARMUP (70 bars)
    // Tenkan from index 8; Kijun and Span A from index 25; Span B from index 51; Chikou from index 0
    let (s, j) = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_WARMUP&dataset_version_id={vid}&timeframe=1Day&indicator=ICHIMOKU"
        ))
        .await;
    assert_eq!(s, StatusCode::OK);
    let tenkan = j["series"]["tenkan_sen"].as_array().unwrap();
    let kijun = j["series"]["kijun_sen"].as_array().unwrap();
    let span_a = j["series"]["senkou_span_a"].as_array().unwrap();
    let span_b = j["series"]["senkou_span_b"].as_array().unwrap();
    let chikou = j["series"]["chikou_span"].as_array().unwrap();

    for i in 0..8 {
        assert_eq!(tenkan[i], Value::Null, "Tenkan null expected at {i}");
    }
    assert!(tenkan[8].is_number(), "Tenkan first value expected at index 8");

    for i in 0..25 {
        assert_eq!(kijun[i], Value::Null, "Kijun null expected at {i}");
        assert_eq!(span_a[i], Value::Null, "Span A null expected at {i}");
    }
    assert!(kijun[25].is_number(), "Kijun first value expected at index 25");
    assert!(span_a[25].is_number(), "Span A first value expected at index 25");

    for i in 0..51 {
        assert_eq!(span_b[i], Value::Null, "Span B null expected at {i}");
    }
    assert!(span_b[51].is_number(), "Span B first value expected at index 51");

    assert!(chikou[0].is_number(), "Chikou first value expected at index 0");
}

/// Latency budget: p95 of one pass over 5 indicators on 10,000 bars.
const P95_BUDGET: std::time::Duration = std::time::Duration::from_millis(500);

#[tokio::test]
async fn test_latency_p95_benchmark_10k_bars() {
    let pool = setup_pool().await;
    let fix = TestFixture::new(pool).await;
    let vid = fix.version_id;

    // Warm-up run to seed any caches
    let _ = fix
        .call_api(&format!(
            "/api/v1/indicators?symbol=SYM_BENCH&dataset_version_id={vid}&timeframe=1Day&indicator=SMA"
        ))
        .await;

    // 5 indicators: SMA, EMA, RSI, BB, MACD
    let indicators = ["SMA", "EMA", "RSI", "BB", "MACD"];
    let iterations = 20;
    let mut durations = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t0 = Instant::now();
        for ind in &indicators {
            let (status, _) = fix.call_api(&format!("/api/v1/indicators?symbol=SYM_BENCH&dataset_version_id={vid}&timeframe=1Day&indicator={ind}")).await;
            assert_eq!(status, StatusCode::OK);
        }
        durations.push(t0.elapsed());
    }

    durations.sort();
    // 95th percentile index for 20 samples: index 19 (0-based)
    let p95_idx = ((iterations as f64 * 0.95).ceil() as usize).saturating_sub(1).min(iterations - 1);
    let p95 = durations[p95_idx];

    println!(
        "\n======================================================\n\
         MEASURED P95 LATENCY for 5 indicators on 10,000 bars:\n\
         P95: {:.3} ms ({:.3} ms per indicator average)\n\
         Min: {:.3} ms, Median: {:.3} ms, Max: {:.3} ms\n\
         ======================================================\n",
        p95.as_secs_f64() * 1000.0,
        (p95.as_secs_f64() * 1000.0) / 5.0,
        durations[0].as_secs_f64() * 1000.0,
        durations[iterations / 2].as_secs_f64() * 1000.0,
        durations[iterations - 1].as_secs_f64() * 1000.0
    );
    assert!(
        p95 <= P95_BUDGET,
        "p95 for 5 indicators on 10,000 bars is {:.3} ms, over the {} ms budget",
        p95.as_secs_f64() * 1000.0,
        P95_BUDGET.as_millis()
    );
}
