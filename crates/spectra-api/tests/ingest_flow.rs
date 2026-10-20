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
        let dir = std::env::temp_dir().join(format!("spectra-ingest-{}", spectra_db::new_id().simple()));
        let store = ArtifactBackend::Local(ArtifactStore::new(dir.join("artifacts")));
        let mut cfg = ApiConfig::new(Secret::new(SECRET));
        cfg.artifacts = Some(store.clone());
        cfg.artifact_quota_bytes = quota;
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
        sqlx::query("DELETE FROM audit_events WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await
            .unwrap();
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
