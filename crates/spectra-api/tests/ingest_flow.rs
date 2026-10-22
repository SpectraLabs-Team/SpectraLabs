//! M1 tasks 4-5 end to end: a signed upload, an ingest job, and a backtest
//! on the result, for a user the engine has never seen. Needs a Postgres to
//! own: set `DATABASE_URL` (CI runs a Postgres service). Without it the tests
//! pass without doing anything.

use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::Request;
use axum::Router;
use http_body_util::BodyExt;
use spectra_api::auth::{canonical, canonical_with_body_hash, sha256_hex, sign};
use spectra_api::ingest::{ensure_local, run_ingest, IngestEnv, IngestOutcome};
use spectra_api::{router, ApiConfig, Secret};
use spectra_core::Json;
use spectra_db::artifacts::{ArtifactBackend, ArtifactStore};
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";

/// Two symbols with a symbol column, so the command line's alias matching
/// and the Studio's explicit settings read the same file the same way.
const CSV: &str = "Date,Ticker,Open,High,Low,Close,Volume\n\
2024-01-02,SPY,470.10,472,469,471.5,1000\n2024-01-02,QQQ,400,401,399,400.5,500\n\
2024-01-03,SPY,471.5,473,470,472.25,900\n2024-01-03,QQQ,400.5,402,400,401,450\n\
2024-01-04,SPY,472.25,474,471,473,950\n2024-01-04,QQQ,401,403,400.5,402.5,480\n\
2024-01-05,SPY,473,475,472,474.5,1100\n2024-01-05,QQQ,402.5,404,402,403,510\n";

const SETTINGS: &str = r#"{"delimiter":",","time_format":"iso8601","utc_offset_minutes":0,
    "columns":{"timestamp":"Date","open":"Open","high":"High","low":"Low","close":"Close",
               "volume":"Volume","symbol":"Ticker"}}"#;

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
    fn new(pool: PgPool, quota: i64) -> Self {
        Self::with_limit(pool, quota, 1 << 30)
    }

    fn with_limit(pool: PgPool, quota: i64, max_upload: u64) -> Self {
        let dir = std::env::temp_dir().join(format!("spectra-ingest-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.artifact_quota_bytes = quota;
        cfg.max_upload_bytes = max_upload;
        let app = router(cfg, Some(pool.clone()));
        Self { app, pool, store, dir, user: spectra_db::new_id(), ws: spectra_db::new_id() }
    }

    fn env(&self) -> IngestEnv {
        IngestEnv {
            pool: self.pool.clone(),
            store: self.store.clone(),
            data_dir: self.dir.join("datasets"),
            quota: 1 << 30,
        }
    }

    fn headers(&self, method: &str, path: &str, body_hash: &str) -> Vec<(&'static str, String)> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws) = (self.user.to_string(), self.ws.to_string());
        let sig = sign(
            SECRET.as_bytes(),
            &canonical_with_body_hash(method, path, &ts, &user, &ws, "req-ingest", body_hash),
        );
        vec![
            ("x-spectra-user", user),
            ("x-spectra-workspace", ws),
            ("x-request-id", "req-ingest".into()),
            ("x-spectra-timestamp", ts),
            ("x-spectra-signature", sig),
        ]
    }

    /// A JSON request signed as the proxy signs it.
    fn json(&self, method: &str, path: &str, body: &str) -> Request<Body> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws) = (self.user.to_string(), self.ws.to_string());
        let sig =
            sign(SECRET.as_bytes(), &canonical(method, path, &ts, &user, &ws, "req-ingest", body.as_bytes()));
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-spectra-user", user)
            .header("x-spectra-workspace", ws)
            .header("x-request-id", "req-ingest")
            .header("x-spectra-timestamp", ts)
            .header("x-spectra-signature", sig)
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// An upload as the proxy's upload route sends it.
    fn upload_request(&self, bytes: &[u8], declared_hash: &str, filename: &str) -> Request<Body> {
        let path = format!("/api/v1/uploads?filename={filename}");
        let mut b = Request::builder()
            .method("POST")
            .uri(&path)
            .header("content-type", "application/octet-stream")
            .header("content-length", bytes.len().to_string())
            .header("x-spectra-content-sha256", declared_hash);
        for (k, v) in self.headers("POST", &path, declared_hash) {
            b = b.header(k, v);
        }
        b.body(Body::from(bytes.to_vec())).unwrap()
    }

    /// Raw bytes, signed as the dashboard's chunk route signs them.
    fn raw(&self, method: &str, path: &str, body: &[u8]) -> Request<Body> {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws) = (self.user.to_string(), self.ws.to_string());
        let sig = sign(SECRET.as_bytes(), &canonical(method, path, &ts, &user, &ws, "req-chunk", body));
        Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/octet-stream")
            .header("x-spectra-user", user)
            .header("x-spectra-workspace", ws)
            .header("x-request-id", "req-chunk")
            .header("x-spectra-timestamp", ts)
            .header("x-spectra-signature", sig)
            .body(Body::from(body.to_vec()))
            .unwrap()
    }

    async fn call(&self, req: Request<Body>) -> (u16, Json) {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, Json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap_or(Json::Null))
    }

    async fn upload(&self, csv: &str) -> String {
        let (status, j) =
            self.call(self.upload_request(csv.as_bytes(), &sha256_hex(csv.as_bytes()), "prices.csv")).await;
        assert_eq!(status, 201, "{}", j.dump());
        j.at("id").unwrap().as_str().unwrap().to_string()
    }

    /// Queues an ingest through the API and runs the job's code on it, as a worker would.
    async fn ingest(&self, upload_id: &str, name: &str, settings: &str, cancel: bool) -> IngestOutcome {
        let body = format!(r#"{{"upload_id":"{upload_id}","dataset_name":"{name}","settings":{settings}}}"#);
        let (status, j) = self.call(self.json("POST", "/api/v1/datasets/ingest", &body)).await;
        assert_eq!(status, 202, "{}", j.dump());
        assert_eq!(j.at("kind").unwrap().as_str().unwrap(), "ingest");
        let job_id = Uuid::parse_str(j.at("id").unwrap().as_str().unwrap()).unwrap();
        let job = spectra_db::jobs::get(&self.pool, self.ws, job_id).await.unwrap().unwrap();
        let progress = Arc::new(AtomicU32::new(0));
        run_ingest(&self.env(), &job, Arc::new(AtomicBool::new(cancel)), progress).await
    }

    async fn clean_up(self) {
        sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(self.ws).execute(&self.pool).await.unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1").bind(self.user).execute(&self.pool).await.unwrap();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn registered(o: IngestOutcome) -> serde_json::Value {
    match o {
        IngestOutcome::Registered(r) => r,
        other => panic!("expected a registered version, got {other:?}"),
    }
}

#[tokio::test]
async fn upload_ingest_and_backtest_without_the_command_line() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 1 << 30);

    // Upload, then ingest: version 1.
    let upload = f.upload(CSV).await;
    let first = registered(f.ingest(&upload, "spy-qqq daily", SETTINGS, false).await);
    assert_eq!((first["version"].as_i64(), first["existed"].as_bool()), (Some(1), Some(false)));
    assert_eq!(first["rows"].as_u64(), Some(8));
    assert_eq!(first["quality_report"]["symbols"], serde_json::json!(["QQQ", "SPY"]));

    // The same CSV through the command line gives the same dataset id.
    let cli = spectra_data::load_csv(CSV.as_bytes(), &spectra_data::CsvOptions::default()).unwrap();
    assert_eq!(
        first["checksum"].as_str(),
        Some(cli.info.id.as_str()),
        "CLI and upload agree on the dataset id"
    );

    // Uploading identical content again creates no new version.
    let again = f.upload(CSV).await;
    assert_ne!(again, upload, "a new upload row");
    let same = registered(f.ingest(&again, "spy-qqq daily", SETTINGS, false).await);
    assert_eq!(same["dataset_version_id"], first["dataset_version_id"]);
    assert_eq!(same["existed"].as_bool(), Some(true));

    // Different content is the next version.
    let more = format!("{CSV}2024-01-08,SPY,474.5,476,474,475,1000\n2024-01-08,QQQ,403,405,402,404,520\n");
    let second = registered(f.ingest(&f.upload(&more).await, "spy-qqq daily", SETTINGS, false).await);
    assert_eq!(second["version"].as_i64(), Some(2));

    // The version is in the artifact store; a worker with an empty local store copies it before a backtest.
    let version_id = Uuid::parse_str(first["dataset_version_id"].as_str().unwrap()).unwrap();
    let v = spectra_db::datasets::resolve_version(&f.pool, f.ws, version_id).await.unwrap().unwrap();
    let blob = v.blob_sha256.clone().expect("stored with the artifacts");
    let fresh = f.dir.join("another-worker");
    ensure_local(&f.store, &fresh, &v.checksum, Some(&blob)).await.unwrap();
    assert!(spectra_data::DataStore::new(&fresh).unwrap().contains(&v.checksum));
    let body = format!(r#"{{"dataset_version_id":"{version_id}","strategy":"buy_and_hold"}}"#);
    let (status, j) = f.call(f.json("POST", "/api/v1/backtests", &body)).await;
    assert_eq!(status, 202, "{}", j.dump());

    // The upload is kept as the version's source, and both steps are in the audit log.
    let source: serde_json::Value = sqlx::query_scalar("SELECT source FROM dataset_versions WHERE id = $1")
        .bind(version_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(source["filename"], "prices.csv");
    assert_eq!(source["settings"]["columns"]["symbol"], "Ticker");
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_events WHERE workspace_id = $1 ORDER BY id")
            .bind(f.ws)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(actions.iter().filter(|a| *a == "upload.created").count(), 3);
    assert!(actions.contains(&"dataset.version_created".to_string()));
    assert!(actions.contains(&"dataset.version_unchanged".to_string()));
    f.clean_up().await;
}

#[tokio::test]
async fn a_rejected_file_fails_its_job_with_every_bad_row_reported() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 1 << 30);
    let bad = format!("{CSV}2024-01-08,SPY,abc,1,1,1,1\n2024-01-09,SPY,1,1,1,1,-5\n");
    let upload = f.upload(&bad).await;
    match f.ingest(&upload, "bad", SETTINGS, false).await {
        IngestOutcome::Rejected { message, result } => {
            assert!(message.contains("line 10 [bad_number]"), "{message}");
            assert_eq!(
                result["quality_report"]["rows_rejected"].as_u64(),
                Some(2),
                "both bad rows: {result}"
            );
            assert_eq!(result["quality_report"]["rejected_by_category"]["negative_volume"].as_u64(), Some(1));
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
    // With "skip bad rows" the same upload loads, the two rows counted.
    let skip = SETTINGS.replacen('{', r#"{"skip_bad_rows":true,"#, 1);
    let r = registered(f.ingest(&upload, "bad", &skip, false).await);
    assert_eq!(r["quality_report"]["rows_rejected"].as_u64(), Some(2));
    assert_eq!(r["rows"].as_u64(), Some(8));

    // Cancelled before the end: nothing registered.
    let other = f.upload(&format!("{CSV}2024-01-10,SPY,1,2,0.5,1.5,1\n")).await;
    assert!(matches!(f.ingest(&other, "cancelled", SETTINGS, true).await, IngestOutcome::Cancelled));
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM datasets WHERE workspace_id = $1 AND name = 'cancelled'")
            .bind(f.ws)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
    f.clean_up().await;
}

#[tokio::test]
async fn uploads_are_checked_and_scoped_to_their_workspace() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool.clone(), 1 << 30);
    let bytes = CSV.as_bytes();

    // The body must match the signed hash; the signature must match the request.
    let (status, j) = f.call(f.upload_request(bytes, &sha256_hex(b"something else"), "x.csv")).await;
    assert_eq!(status, 400, "{}", j.dump());
    let mut forged = f.upload_request(bytes, &sha256_hex(bytes), "x.csv");
    forged.headers_mut().insert("x-spectra-content-sha256", sha256_hex(b"other").parse().unwrap());
    assert_eq!(f.call(forged).await.0, 401, "the declared hash is signed");
    let mut no_hash = f.upload_request(bytes, &sha256_hex(bytes), "x.csv");
    no_hash.headers_mut().remove("x-spectra-content-sha256");
    assert_eq!(f.call(no_hash).await.0, 400);
    let mut huge = f.upload_request(bytes, &sha256_hex(bytes), "x.csv");
    huge.headers_mut().insert("content-length", (200u64 << 20).to_string().parse().unwrap());
    assert_eq!(f.call(huge).await.0, 413);

    // A good upload; a path in the file name is dropped.
    let (status, j) = f.call(f.upload_request(bytes, &sha256_hex(bytes), "..%2F..%2Fprices.csv")).await;
    assert_eq!(status, 201, "{}", j.dump());
    assert_eq!(j.at("filename").unwrap().as_str().unwrap(), "prices.csv");
    let upload = j.at("id").unwrap().as_str().unwrap().to_string();

    // Every problem with an ingest request at once.
    let body = format!(
        r#"{{"upload_id":"{}","dataset_name":" ","settings":{{"delimiter":","}}}}"#,
        spectra_db::new_id()
    );
    let (status, j) = f.call(f.json("POST", "/api/v1/datasets/ingest", &body)).await;
    assert_eq!(status, 422, "{}", j.dump());
    let fields: Vec<String> = j
        .at("details")
        .unwrap()
        .at("errors")
        .unwrap()
        .items()
        .unwrap()
        .iter()
        .map(|e| e.at("field").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        fields,
        [
            "dataset_name",
            "settings.time_format",
            "settings.utc_offset_minutes",
            "settings.columns",
            "upload_id"
        ]
    );

    // Another workspace can't ingest this workspace's upload.
    let other = Fixture::new(pool, 1 << 30);
    let body = format!(r#"{{"upload_id":"{upload}","dataset_name":"x","settings":{SETTINGS}}}"#);
    let (status, j) = other.call(other.json("POST", "/api/v1/datasets/ingest", &body)).await;
    assert_eq!(status, 422, "{}", j.dump());
    assert!(j.dump().contains("upload_id"));
    // Nor queue one through the generic jobs route.
    let raw = format!(r#"{{"kind":"ingest","payload":{{"upload_id":"{upload}"}}}}"#);
    assert_eq!(f.call(f.json("POST", "/api/v1/jobs", &raw)).await.0, 422);
    other.clean_up().await;
    f.clean_up().await;
}

#[tokio::test]
async fn uploads_count_against_the_storage_quota() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 100);
    let (status, j) = f.call(f.upload_request(CSV.as_bytes(), &sha256_hex(CSV.as_bytes()), "big.csv")).await;
    assert_eq!(status, 413, "{}", j.dump());
    assert_eq!(j.at("code").unwrap().as_str().unwrap(), "quota_exceeded");
    f.clean_up().await;
}

// ---------------------------------------------------------------------------
// M1 task 6: browsing, charting, renaming and deleting datasets
// ---------------------------------------------------------------------------

fn field_names(j: &Json) -> Vec<String> {
    j.at("details")
        .and_then(|d| d.at("errors"))
        .and_then(Json::items)
        .map(|e| e.iter().map(|x| x.at("field").unwrap().as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn datasets_can_be_browsed_charted_renamed_and_deleted() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool.clone(), 1 << 30);
    let v1 = registered(f.ingest(&f.upload(CSV).await, "spy-qqq daily", SETTINGS, false).await);
    let more = format!("{CSV}2024-01-08,SPY,474.5,476,474,475,1000\n2024-01-08,QQQ,403,405,402,404,520\n");
    let v2 = registered(f.ingest(&f.upload(&more).await, "spy-qqq daily", SETTINGS, false).await);
    let (v1_id, v2_id) =
        (v1["dataset_version_id"].as_str().unwrap(), v2["dataset_version_id"].as_str().unwrap());

    // The plain list is unchanged; ?view=grouped adds one entry per dataset.
    let (status, j) = f.call(f.json("GET", "/api/v1/datasets", "")).await;
    assert_eq!(status, 200);
    assert!(j.find("datasets").is_none(), "opt-in only");
    assert_eq!(j.at("items").unwrap().items().unwrap().len(), 2);
    let (status, j) = f.call(f.json("GET", "/api/v1/datasets?view=grouped", "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    let d = &j.at("datasets").unwrap().items().unwrap()[0];
    assert_eq!(d.at("name").unwrap().as_str().unwrap(), "spy-qqq daily");
    assert_eq!(d.at("version_count").unwrap().as_int().unwrap(), 2);
    assert_eq!(d.at("latest_version").unwrap().as_int().unwrap(), 2);
    assert_eq!(d.at("row_count").unwrap().as_int().unwrap(), 10);
    assert_eq!(d.at("first_ts").unwrap().as_str().unwrap(), "2024-01-02T00:00:00Z");
    assert_eq!(d.at("last_ts").unwrap().as_str().unwrap(), "2024-01-08T00:00:00Z");
    let dataset_id = d.at("id").unwrap().as_str().unwrap().to_string();
    assert_eq!(f.call(f.json("GET", "/api/v1/datasets?view=flat", "")).await.0, 422);

    // The dataset with its history, and one version in full.
    let (status, j) = f.call(f.json("GET", &format!("/api/v1/datasets/{dataset_id}"), "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    let versions = j.at("versions").unwrap().items().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].at("version").unwrap().as_int().unwrap(), 2);
    assert_eq!(versions[0].at("filename").unwrap().as_str().unwrap(), "prices.csv");
    let (status, j) = f.call(f.json("GET", &format!("/api/v1/datasets/versions/{v1_id}"), "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    let typed: spectra_api::datasets_api::VersionDetailView = serde_json::from_str(&j.dump()).unwrap();
    assert!(typed.chartable);
    assert_eq!(typed.quality_report["symbol_stats"][0]["bars"], 4);
    assert_eq!(typed.source.unwrap()["settings"]["time_format"], "iso8601");

    // Chart data: per symbol, reduced, within a range; every bad parameter at once.
    let (status, j) = f
        .call(f.json("GET", &format!("/api/v1/datasets/versions/{v2_id}/bars?symbol=SPY&points=10"), ""))
        .await;
    assert_eq!(status, 200, "{}", j.dump());
    let typed: spectra_api::datasets_api::BarsView = serde_json::from_str(&j.dump()).unwrap();
    assert_eq!((typed.symbol.as_str(), typed.total_bars, typed.points.len()), ("SPY", 5, 5));
    assert_eq!(typed.symbols, ["QQQ", "SPY"]);
    assert!((typed.points[0].open - 470.1).abs() < 1e-9);
    assert_eq!(typed.points[0].t, 1_704_153_600_000);
    let (_, j) = f
        .call(f.json(
            "GET",
            &format!("/api/v1/datasets/versions/{v2_id}/bars?from=2024-01-03&to=2024-01-04"),
            "",
        ))
        .await;
    assert_eq!(j.at("symbol").unwrap().as_str().unwrap(), "QQQ", "default: the first symbol");
    assert_eq!(j.at("total_bars").unwrap().as_int().unwrap(), 2, "dates are inclusive");
    let (status, j) = f
        .call(f.json(
            "GET",
            &format!("/api/v1/datasets/versions/{v2_id}/bars?from=yesterday&to=x&points=1"),
            "",
        ))
        .await;
    assert_eq!(status, 422);
    assert_eq!(field_names(&j), ["from", "to", "points"]);
    let (status, j) =
        f.call(f.json("GET", &format!("/api/v1/datasets/versions/{v2_id}/bars?symbol=AAPL"), "")).await;
    assert_eq!(status, 422);
    assert!(j.dump().contains("QQQ, SPY"));

    // Rename: every problem at once, then a taken name, then a real change.
    let (status, j) = f
        .call(f.json(
            "PATCH",
            &format!("/api/v1/datasets/{dataset_id}"),
            &format!(r#"{{"name":" ","description":"{}"}}"#, "x".repeat(2001)),
        ))
        .await;
    assert_eq!(status, 422);
    assert_eq!(field_names(&j), ["name", "description"]);
    registered(
        f.ingest(&f.upload(&format!("{CSV}2024-01-09,SPY,1,2,0.5,1.5,1\n")).await, "other", SETTINGS, false)
            .await,
    );
    let (status, j) =
        f.call(f.json("PATCH", &format!("/api/v1/datasets/{dataset_id}"), r#"{"name":"other"}"#)).await;
    assert_eq!(status, 409, "{}", j.dump());
    let (status, j) = f
        .call(f.json(
            "PATCH",
            &format!("/api/v1/datasets/{dataset_id}"),
            r#"{"name":"SPY + QQQ","description":"Daily bars"}"#,
        ))
        .await;
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(
        (j.at("name").unwrap().as_str().unwrap(), j.at("description").unwrap().as_str().unwrap()),
        ("SPY + QQQ", "Daily bars")
    );

    // Runs that used a version; such a version can't be deleted.
    let job: Uuid = sqlx::query_scalar(
        "INSERT INTO jobs (workspace_id, kind, status, attempts, started_at, finished_at) \
         VALUES ($1, 'backtest', 'succeeded', 1, now(), now()) RETURNING id",
    )
    .bind(f.ws)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO runs (workspace_id, job_id, dataset_version_id, status, finished_at, params, metrics) \
         VALUES ($1, $2, $3, 'succeeded', now(), '{\"strategy\":\"sma_cross\"}', '{\"metrics\":{\"total_return\":0.05}}')",
    )
    .bind(f.ws)
    .bind(job)
    .bind(Uuid::parse_str(v1_id).unwrap())
    .execute(&f.pool)
    .await
    .unwrap();
    let (status, j) = f.call(f.json("GET", &format!("/api/v1/datasets/versions/{v1_id}/runs"), "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    let run = &j.at("items").unwrap().items().unwrap()[0];
    assert_eq!(run.at("job_id").unwrap().as_str().unwrap(), job.to_string());
    assert_eq!(run.at("strategy").unwrap().as_str().unwrap(), "sma_cross");
    let (status, j) = f.call(f.json("DELETE", &format!("/api/v1/datasets/versions/{v1_id}"), "")).await;
    assert_eq!((status, j.at("code").unwrap().as_str().unwrap()), (409, "in_use"));

    // An unused version goes, with its file (nothing else uses it); the dataset stays.
    let v2_blob = spectra_db::datasets::get_version(&f.pool, f.ws, Uuid::parse_str(v2_id).unwrap())
        .await
        .unwrap()
        .unwrap()
        .blob_sha256
        .unwrap();
    let (status, j) = f.call(f.json("DELETE", &format!("/api/v1/datasets/versions/{v2_id}"), "")).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert!(!j.at("dataset_deleted").unwrap().as_bool().unwrap());
    assert_eq!(f.store.get(&v2_blob).await.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    assert_eq!(f.call(f.json("GET", &format!("/api/v1/datasets/versions/{v2_id}"), "")).await.0, 404);

    // Another workspace sees none of it.
    let other = Fixture::new(pool, 1 << 30);
    for path in [
        format!("/api/v1/datasets/{dataset_id}"),
        format!("/api/v1/datasets/versions/{v1_id}"),
        format!("/api/v1/datasets/versions/{v1_id}/runs"),
        format!("/api/v1/datasets/versions/{v1_id}/bars"),
    ] {
        assert_eq!(other.call(other.json("GET", &path, "")).await.0, 404, "{path}");
    }
    assert_eq!(
        other.call(other.json("DELETE", &format!("/api/v1/datasets/versions/{v1_id}"), "")).await.0,
        404
    );
    assert_eq!(
        other
            .call(other.json("PATCH", &format!("/api/v1/datasets/{dataset_id}"), r#"{"name":"mine"}"#))
            .await
            .0,
        404
    );
    let (_, j) = other.call(other.json("GET", "/api/v1/datasets?view=grouped", "")).await;
    assert!(j.at("datasets").unwrap().items().unwrap().is_empty());
    other.clean_up().await;

    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_events WHERE workspace_id = $1")
        .bind(f.ws)
        .fetch_all(&f.pool)
        .await
        .unwrap();
    assert!(
        actions.contains(&"dataset.updated".to_string())
            && actions.contains(&"dataset.version_deleted".to_string())
    );
    sqlx::query("DELETE FROM runs WHERE job_id = $1").bind(job).execute(&f.pool).await.unwrap();
    f.clean_up().await;
}

// ---------------------------------------------------------------------------
// M1b task 9: resumable uploads, and ingest streaming from the store
// ---------------------------------------------------------------------------

fn code(j: &Json) -> String {
    j.at("code").and_then(Json::as_str).unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_resumable_upload_survives_retries_and_ingests_like_any_upload() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 1 << 30);
    let data = CSV.as_bytes();
    let (status, j) = f
        .call(f.json(
            "POST",
            "/api/v1/uploads/sessions",
            &format!(r#"{{"filename":"C:\\x\\big.csv","size":{}}}"#, data.len()),
        ))
        .await;
    assert_eq!(status, 201, "{}", j.dump());
    assert_eq!(j.at("filename").unwrap().as_str().unwrap(), "big.csv", "no path kept");
    assert_eq!(j.at("received").unwrap().as_int().unwrap(), 0);
    let sid = j.at("id").unwrap().as_str().unwrap().to_string();
    let chunk = |offset: usize, part: &[u8]| {
        f.raw("PUT", &format!("/api/v1/uploads/sessions/{sid}/chunk?offset={offset}"), part)
    };

    // Three chunks; the first is sent twice (its answer "lost"): the retry is told where to go on.
    let (a, b) = (100, 250);
    assert_eq!(f.call(chunk(0, &data[..a])).await.0, 200);
    let (status, j) = f.call(chunk(0, &data[..a])).await;
    assert_eq!((status, code(&j).as_str()), (409, "offset_mismatch"), "{}", j.dump());
    assert_eq!(j.at("details").unwrap().at("received").unwrap().as_int().unwrap(), a as i64);
    // Completing early says how far it got.
    let (status, j) = f.call(f.json("POST", &format!("/api/v1/uploads/sessions/{sid}/complete"), "")).await;
    assert_eq!((status, code(&j).as_str()), (409, "incomplete"));
    // A "reload": ask where it stands, then continue from there.
    let (_, j) = f.call(f.json("GET", &format!("/api/v1/uploads/sessions/{sid}"), "")).await;
    let at = j.at("received").unwrap().as_int().unwrap() as usize;
    assert_eq!(at, a);
    assert_eq!(f.call(chunk(at, &data[at..b])).await.0, 200);
    let (status, _) = f.call(chunk(b, &[data, b"x"].concat()[b..])).await;
    assert_eq!(status, 422, "past the declared size");
    let (status, j) = f.call(chunk(b, &data[b..])).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(j.at("received").unwrap().as_int().unwrap() as usize, data.len());

    // Complete; a retry answers with the same upload.
    let (status, j) = f.call(f.json("POST", &format!("/api/v1/uploads/sessions/{sid}/complete"), "")).await;
    assert_eq!(status, 201, "{}", j.dump());
    assert_eq!(j.at("sha256").unwrap().as_str().unwrap(), sha256_hex(data));
    let upload = j.at("id").unwrap().as_str().unwrap().to_string();
    let (status, j) = f.call(f.json("POST", &format!("/api/v1/uploads/sessions/{sid}/complete"), "")).await;
    assert_eq!((status, j.at("id").unwrap().as_str().unwrap()), (200, upload.as_str()));
    assert_eq!(code(&f.call(chunk(data.len(), b"x")).await.1), "already_complete");

    // It ingests (streamed from the store) to the same dataset id as the command line.
    let r = registered(f.ingest(&upload, "resumable", SETTINGS, false).await);
    let cli = spectra_data::load_csv(data, &spectra_data::CsvOptions::default()).unwrap();
    assert_eq!(r["checksum"].as_str(), Some(cli.info.id.as_str()));
    f.clean_up().await;
}

#[tokio::test]
async fn resumable_uploads_are_limited_scoped_and_can_be_abandoned() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::with_limit(pool.clone(), 1 << 30, 2048);
    let (_, j) = f.call(f.json("GET", "/api/v1/uploads/limits", "")).await;
    assert_eq!(j.at("max_bytes").unwrap().as_int().unwrap(), 2048);
    assert_eq!(j.at("chunk_bytes").unwrap().as_int().unwrap(), 8 << 20);
    let create = |size: usize| {
        f.json("POST", "/api/v1/uploads/sessions", &format!(r#"{{"filename":"a.csv","size":{size}}}"#))
    };
    assert_eq!(f.call(create(4096)).await.0, 413, "over the configured limit");
    let (status, j) = f.call(create(0)).await;
    assert_eq!(status, 422, "{}", j.dump());

    // Five open at most.
    let mut ids = Vec::new();
    for _ in 0..5 {
        let (status, j) = f.call(create(1000)).await;
        assert_eq!(status, 201, "{}", j.dump());
        ids.push(j.at("id").unwrap().as_str().unwrap().to_string());
    }
    let (status, j) = f.call(create(1000)).await;
    assert_eq!((status, code(&j).as_str()), (409, "too_many_uploads"));

    // Another workspace sees none of them.
    let other = Fixture::new(pool.clone(), 1 << 30);
    let first = &ids[0];
    assert_eq!(other.call(other.json("GET", &format!("/api/v1/uploads/sessions/{first}"), "")).await.0, 404);
    assert_eq!(
        other
            .call(other.raw("PUT", &format!("/api/v1/uploads/sessions/{first}/chunk?offset=0"), b"x"))
            .await
            .0,
        404
    );
    assert_eq!(
        other.call(other.json("POST", &format!("/api/v1/uploads/sessions/{first}/complete"), "")).await.0,
        404
    );
    assert_eq!(
        other.call(other.json("DELETE", &format!("/api/v1/uploads/sessions/{first}"), "")).await.0,
        404
    );

    // Abandon one: its staging file goes, and a slot frees up.
    assert_eq!(
        f.call(f.raw("PUT", &format!("/api/v1/uploads/sessions/{first}/chunk?offset=0"), b"date,open"))
            .await
            .0,
        200
    );
    let staging =
        f.store.staging_dir().join(format!("session-{}.part", Uuid::parse_str(first).unwrap().simple()));
    assert!(staging.exists());
    assert_eq!(f.call(f.json("DELETE", &format!("/api/v1/uploads/sessions/{first}"), "")).await.0, 200);
    assert!(!staging.exists());
    assert_eq!(f.call(f.json("GET", &format!("/api/v1/uploads/sessions/{first}"), "")).await.0, 404);
    assert_eq!(f.call(create(1000)).await.0, 201);

    // Idle ones expire with their files.
    let second = Uuid::parse_str(&ids[1]).unwrap();
    assert_eq!(
        f.call(f.raw("PUT", &format!("/api/v1/uploads/sessions/{second}/chunk?offset=0"), b"abc")).await.0,
        200
    );
    sqlx::query("UPDATE upload_sessions SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(second)
        .execute(&f.pool)
        .await
        .unwrap();
    let removed = spectra_api::sessions_api::sweep_expired(&f.pool, &f.store.staging_dir()).await.unwrap();
    assert!(removed >= 1);
    assert!(!f.store.staging_dir().join(format!("session-{}.part", second.simple())).exists());

    // The quota is checked when starting.
    let small = Fixture::new(pool, 100);
    let (status, j) = small
        .call(small.json("POST", "/api/v1/uploads/sessions", r#"{"filename":"a.csv","size":1000}"#))
        .await;
    assert_eq!((status, code(&j).as_str()), (413, "quota_exceeded"));
    small.clean_up().await;
    other.clean_up().await;
    f.clean_up().await;
}

#[tokio::test]
async fn a_stored_upload_that_no_longer_matches_its_hash_is_refused() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 1 << 30);
    let upload = f.upload(CSV).await;
    let u =
        spectra_db::uploads::get(&f.pool, f.ws, Uuid::parse_str(&upload).unwrap()).await.unwrap().unwrap();
    // The fixture's local store lives under dir/artifacts.
    let path = f.dir.join("artifacts").join(ArtifactStore::relative_path(&u.sha256));
    // Corrupt in a way that still parses, and in a way that doesn't: both are "corrupt".
    for tampered in [CSV.replace("1000", "1001"), CSV.replace("471.5", "999.5")] {
        std::fs::write(&path, tampered).unwrap();
        match f.ingest(&upload, "corrupt", SETTINGS, false).await {
            IngestOutcome::Failed { message, retryable } => {
                assert!(message.contains("corrupt"), "{message}");
                assert!(!retryable);
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }
    f.clean_up().await;
}

#[tokio::test]
async fn parquet_ingest_and_export_roundtrip() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool.clone(), 1 << 30);

    // 1. Ingest CSV file
    let csv_upload = f.upload(CSV).await;
    let csv_outcome = f.ingest(&csv_upload, "bench", SETTINGS, false).await;
    let csv_reg = registered(csv_outcome);
    let csv_checksum = csv_reg.get("checksum").unwrap().as_str().unwrap();
    let version_id = csv_reg.get("dataset_version_id").unwrap().as_str().unwrap();

    // 2. Export as Parquet
    let (status, resp) = {
        let req = f.json("GET", &format!("/api/v1/datasets/versions/{version_id}/export?format=parquet"), "");
        let r = f.app.clone().oneshot(req).await.unwrap();
        let s = r.status().as_u16();
        let b = r.into_body().collect().await.unwrap().to_bytes();
        (s, b)
    };
    assert_eq!(status, 200);
    assert!(resp.starts_with(b"PAR1"));

    // 3. Re-upload exported Parquet and ingest it
    let pq_hash = sha256_hex(&resp);
    let (upload_status, upload_j) = f.call(f.upload_request(&resp, &pq_hash, "export.parquet")).await;
    assert_eq!(upload_status, 201);
    let pq_upload_id = upload_j.at("id").unwrap().as_str().unwrap();

    let pq_settings = r#"{"delimiter":",","time_format":"iso8601","utc_offset_minutes":0,
        "columns":{"timestamp":"timestamp","open":"open","high":"high","low":"low","close":"close",
                   "volume":"volume","symbol":"symbol"}}"#;
    let pq_outcome = f.ingest(pq_upload_id, "bench-pq", pq_settings, false).await;
    let pq_reg = registered(pq_outcome);
    let pq_checksum = pq_reg.get("checksum").unwrap().as_str().unwrap();

    // Identity parity: CSV and Parquet with the same bars give the exact same dataset id
    assert_eq!(csv_checksum, pq_checksum);

    // 4. Cross-workspace access to export is refused (404)
    let other = Fixture::new(pool, 1 << 30);
    let other_req =
        other.json("GET", &format!("/api/v1/datasets/versions/{version_id}/export?format=parquet"), "");
    let other_resp = other.app.clone().oneshot(other_req).await.unwrap();
    assert_eq!(other_resp.status().as_u16(), 404);

    // 5. Bad format query parameter is rejected with 422
    let bad_fmt_req = f.json("GET", &format!("/api/v1/datasets/versions/{version_id}/export?format=csv"), "");
    let bad_fmt_resp = f.app.clone().oneshot(bad_fmt_req).await.unwrap();
    assert_eq!(bad_fmt_resp.status().as_u16(), 422);

    other.clean_up().await;
    f.clean_up().await;
}

#[tokio::test]
async fn parquet_ingest_rejects_floats_and_excessive_decimals() {
    let Some(pool) = pool().await else { return };
    let f = Fixture::new(pool, 1 << 30);

    // Create a Parquet with a Float64 column
    let schema = std::sync::Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("Date", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("Open", arrow_schema::DataType::Float64, false),
        arrow_schema::Field::new("High", arrow_schema::DataType::Float64, false),
        arrow_schema::Field::new("Low", arrow_schema::DataType::Float64, false),
        arrow_schema::Field::new("Close", arrow_schema::DataType::Float64, false),
        arrow_schema::Field::new("Volume", arrow_schema::DataType::Int64, false),
        arrow_schema::Field::new("Ticker", arrow_schema::DataType::Utf8, false),
    ]));
    let batch = arrow_array::RecordBatch::try_new(
        schema,
        vec![
            std::sync::Arc::new(arrow_array::StringArray::from(vec!["2024-01-02"])),
            std::sync::Arc::new(arrow_array::Float64Array::from(vec![185.50])),
            std::sync::Arc::new(arrow_array::Float64Array::from(vec![186.00])),
            std::sync::Arc::new(arrow_array::Float64Array::from(vec![185.00])),
            std::sync::Arc::new(arrow_array::Float64Array::from(vec![185.75])),
            std::sync::Arc::new(arrow_array::Int64Array::from(vec![10000])),
            std::sync::Arc::new(arrow_array::StringArray::from(vec!["SPY"])),
        ],
    )
    .unwrap();

    let mut buf = Vec::new();
    let props = parquet::file::properties::WriterProperties::builder().build();
    let mut writer = parquet::arrow::ArrowWriter::try_new(&mut buf, batch.schema(), Some(props)).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();

    let hash = sha256_hex(&buf);
    let (status, j) = f.call(f.upload_request(&buf, &hash, "floats.parquet")).await;
    assert_eq!(status, 201);
    let upload_id = j.at("id").unwrap().as_str().unwrap();

    match f.ingest(upload_id, "bad-float", SETTINGS, false).await {
        IngestOutcome::Rejected { message, result } => {
            assert!(message.contains("floating point numbers are not allowed"), "{message}");
            assert_eq!(result.get("rejected"), Some(&serde_json::Value::Bool(true)));
            let qr = result.get("quality_report").unwrap();
            assert_eq!(qr.get("rows_rejected"), Some(&serde_json::json!(1)));
        }
        other => panic!("expected IngestOutcome::Rejected, got {other:?}"),
    }

    f.clean_up().await;
}
