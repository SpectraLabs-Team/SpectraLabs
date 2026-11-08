//! M34a end to end: queue a fundamentals refresh through the API, run the job against a mock SEC
//! EDGAR server, refresh again after a restatement, and manage the refresh schedule.
//!
//! Needs a Postgres (`DATABASE_URL`); without one it passes without doing anything. Never calls the
//! real SEC: the mock serves the ticker map and Company Facts on a local port.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use spectra_api::auth::{canonical, sign};
use spectra_api::fundamentals::{
    run_refresh, sweep_due_schedules, EdgarConfig, RefreshOutcome, FUNDAMENTALS_REFRESH_JOB_KIND,
};
use spectra_api::{router, ApiConfig, Secret};
use spectra_data::edgar::known_at_ns;
use spectra_db::{connect, migrate, PgPool, PoolConfig};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const USER_AGENT: &str = "SpectraLabs test ops@example.com";

async fn pool() -> Option<PgPool> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let pool =
        connect(&PoolConfig::new(url)).await.expect("DATABASE_URL is set but the database is unreachable");
    migrate(&pool).await.expect("migrations");
    Some(pool)
}

/// Company Facts for CIK 1 (`EXA`). Version 1: Q1 FY2024 revenue 100 (A-Q1), Q2 revenue -5 (A-Q2,
/// quarantined: revenue cannot be negative) and total assets. Version 2 adds an amendment (A-Q1-AMEND,
/// filed 2024-03-15) restating Q1 revenue to 105.
fn company_facts(version: u8) -> String {
    let mut revenue = vec![
        json!({"start":"2023-10-01","end":"2023-12-30","val":100,"accn":"A-Q1","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}),
        json!({"start":"2023-12-31","end":"2024-03-30","val":-5,"accn":"A-Q2","fy":2024,"fp":"Q2","form":"10-Q","filed":"2024-05-03"}),
    ];
    if version >= 2 {
        revenue.push(json!({"start":"2023-10-01","end":"2023-12-30","val":105,"accn":"A-Q1-AMEND","fy":2024,"fp":"Q1","form":"10-Q/A","filed":"2024-03-15"}));
    }
    json!({
        "cik": 1,
        "entityName": "Example Inc.",
        "facts": {"us-gaap": {
            "Revenues": {"units": {"USD": revenue}},
            "Assets": {"units": {"USD": [
                {"end":"2023-12-30","val":500,"accn":"A-Q1","fy":2024,"fp":"Q1","form":"10-Q","filed":"2024-02-02"}
            ]}}
        }}
    })
    .to_string()
}

#[derive(Default)]
struct Mock {
    version: AtomicU8,
    user_agents: Mutex<Vec<String>>,
    request_times: Mutex<Vec<std::time::Instant>>,
}

async fn tickers(State(m): State<Arc<Mock>>, headers: HeaderMap) -> String {
    m.request_times.lock().unwrap().push(std::time::Instant::now());
    m.user_agents.lock().unwrap().push(headers["user-agent"].to_str().unwrap().to_string());
    json!({
        "0": {"cik_str": 1, "ticker": "EXA", "title": "Example Inc."},
        "1": {"cik_str": 2, "ticker": "GONE", "title": "Gone Corp"}
    })
    .to_string()
}

async fn facts(State(m): State<Arc<Mock>>, Path(file): Path<String>) -> Result<String, StatusCode> {
    m.request_times.lock().unwrap().push(std::time::Instant::now());
    match file.as_str() {
        "CIK0000000001.json" => Ok(company_facts(m.version.load(Ordering::SeqCst))),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

/// Serves the mock on a free local port; returns its config and state.
async fn mock_edgar() -> (EdgarConfig, Arc<Mock>) {
    let mock = Arc::new(Mock { version: AtomicU8::new(1), ..Mock::default() });
    let app = Router::new()
        .route("/files/company_tickers.json", get(tickers))
        .route("/api/xbrl/companyfacts/{file}", get(facts))
        .with_state(Arc::clone(&mock));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (EdgarConfig { user_agent: USER_AGENT.into(), data_url: base.clone(), www_url: base }, mock)
}

struct Fixture {
    app: Router,
    pool: PgPool,
    ws: Uuid,
    editor: Uuid,
    viewer: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let (ws, editor, viewer) = (spectra_db::new_id(), spectra_db::new_id(), spectra_db::new_id());
        spectra_db::identity::ensure_identity(&pool, Some(editor), ws).await.unwrap();
        spectra_db::identity::ensure_identity(&pool, Some(viewer), ws).await.unwrap();
        sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')")
            .bind(ws)
            .bind(viewer)
            .execute(&pool)
            .await
            .unwrap();
        let app = router(ApiConfig::new(Secret::new(SECRET)), Some(pool.clone()));
        Self { app, pool, ws, editor, viewer }
    }

    async fn call(&self, user: Uuid, method: &str, path: &str, body: &str) -> (u16, Value) {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let (user, ws, req_id) = (user.to_string(), self.ws.to_string(), spectra_db::new_id().to_string());
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
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn run(&self, job_id: Uuid, edgar: Result<&EdgarConfig, &str>) -> RefreshOutcome {
        let job = spectra_db::jobs::get(&self.pool, self.ws, job_id).await.unwrap().unwrap();
        let progress = Arc::new(AtomicU32::new(0));
        run_refresh(&self.pool, edgar, &job, Arc::new(AtomicBool::new(false)), progress).await
    }

    /// Removes the jobs this test queued. CI runs the SQL checks on the same database after the
    /// tests, and its queue check claims the highest-priority queued job: it must be its own.
    async fn clear_jobs(&self) {
        sqlx::query("DELETE FROM jobs WHERE workspace_id = $1")
            .bind(self.ws)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn revenue_q1(&self, as_of_ns: i64) -> Option<f64> {
        let as_of = chrono::DateTime::from_timestamp_nanos(as_of_ns);
        let rows = spectra_db::fundamentals::facts_as_of(&self.pool, self.ws, "EXA", as_of, Some("revenue"))
            .await
            .unwrap();
        rows.iter().find(|r| r.period == "2024-Q1").map(|r| r.value)
    }
}

fn done(outcome: RefreshOutcome) -> Value {
    match outcome {
        RefreshOutcome::Done(v) => v,
        other => panic!("expected a finished refresh, got {other:?}"),
    }
}

#[tokio::test]
async fn refresh_loads_edgar_and_a_second_refresh_adds_the_restatement() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;
    let (edgar, mock) = mock_edgar().await;

    // Viewers cannot refresh; editors queue a job. Symbols come back normalized.
    let body = json!({"symbols": [" exa", "GONE", "EXA"]}).to_string();
    let (status, err) = fix.call(fix.viewer, "POST", "/api/v1/fundamentals/refresh", &body).await;
    assert_eq!((status, err["code"].as_str()), (403, Some("viewer_read_only")), "{err}");
    let (status, queued) = fix.call(fix.editor, "POST", "/api/v1/fundamentals/refresh", &body).await;
    assert_eq!(status, 202, "{queued}");
    assert_eq!(queued["symbols"], json!(["EXA", "GONE"]));
    let job_id: Uuid = queued["job_id"].as_str().unwrap().parse().unwrap();

    // First run: EXA loads; GONE has no Company Facts and is reported, not fatal.
    let first = done(fix.run(job_id, Ok(&edgar)).await);
    assert_eq!((first["loaded"].as_u64(), first["failed"].as_u64()), (Some(1), Some(1)), "{first}");
    let exa = &first["symbols"][0];
    assert_eq!(
        (exa["accepted"].as_i64(), exa["new_rows"].as_i64(), exa["quarantined"].as_i64()),
        (Some(2), Some(2), Some(1))
    );
    assert!(first["symbols"][1]["error"].as_str().unwrap().contains("no Company Facts for GONE"));
    assert!(
        mock.user_agents.lock().unwrap().iter().all(|ua| ua == USER_AGENT),
        "SEC requires the User-Agent"
    );
    assert_eq!(fix.revenue_q1(i64::MAX).await, Some(100.0));

    // EDGAR now carries an amendment. A second refresh adds only the restatement.
    mock.version.store(2, Ordering::SeqCst);
    let (_, again) =
        fix.call(fix.editor, "POST", "/api/v1/fundamentals/refresh", r#"{"symbols":["EXA"]}"#).await;
    let second = done(fix.run(again["job_id"].as_str().unwrap().parse().unwrap(), Ok(&edgar)).await);
    assert_eq!(
        (second["symbols"][0]["accepted"].as_i64(), second["symbols"][0]["new_rows"].as_i64()),
        (Some(3), Some(1))
    );

    // The old value is kept: before the amendment was known, Q1 revenue is still 100.
    let amended = known_at_ns("2024-03-15").unwrap();
    assert_eq!(fix.revenue_q1(amended - 1).await, Some(100.0));
    assert_eq!(fix.revenue_q1(amended).await, Some(105.0));
    assert_eq!(fix.revenue_q1(known_at_ns("2024-02-02").unwrap() - 1).await, None);

    // Loads and quarantine are listed; the negative revenue is quarantined, not served.
    let (status, loads) = fix.call(fix.viewer, "GET", "/api/v1/fundamentals/loads?symbol=exa", "").await;
    assert_eq!(status, 200, "{loads}");
    let versions: Vec<&str> =
        loads["items"].as_array().unwrap().iter().map(|l| l["version_id"].as_str().unwrap()).collect();
    assert_eq!(versions.len(), 2);
    assert_ne!(versions[0], versions[1], "a new revision is a new version");
    let (_, quarantine) = fix.call(fix.viewer, "GET", "/api/v1/fundamentals/quarantine", "").await;
    let q = &quarantine["items"][0];
    assert_eq!(
        (q["metric"].as_str(), q["category"].as_str(), q["value"].as_f64()),
        (Some("revenue"), Some("IMPOSSIBLE_NEGATIVE"), Some(-5.0))
    );
    let (status, _) = fix.call(fix.viewer, "GET", "/api/v1/fundamentals/loads?limit=0", "").await;
    assert_eq!(status, 422);
    fix.clear_jobs().await;
}

#[tokio::test]
async fn refresh_without_sec_user_agent_fails_and_says_why() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;
    let (_, queued) =
        fix.call(fix.editor, "POST", "/api/v1/fundamentals/refresh", r#"{"symbols":["EXA"]}"#).await;
    let why = EdgarConfig::from_env().err();
    let outcome =
        fix.run(queued["job_id"].as_str().unwrap().parse().unwrap(), Err("SEC_USER_AGENT is not set")).await;
    match outcome {
        RefreshOutcome::Failed { message, retryable } => {
            assert!(message.contains("SEC_USER_AGENT"), "{message}");
            assert!(!retryable, "a missing setting will not fix itself on retry");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
    // from_env names the variable too (when the test environment has none set).
    if let Some(why) = why {
        assert!(why.contains("SEC_USER_AGENT"), "{why}");
    }
    fix.clear_jobs().await;
}

#[tokio::test]
async fn refresh_requests_are_validated() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;
    for body in [r#"{"symbols":[]}"#, r#"{"symbols":["AAPL;DROP"]}"#, r#"{"tickers":["AAPL"]}"#] {
        let (status, err) = fix.call(fix.editor, "POST", "/api/v1/fundamentals/refresh", body).await;
        assert_eq!(status, 422, "{body}: {err}");
    }
    fix.clear_jobs().await;
}

#[tokio::test]
async fn a_schedule_queues_refreshes_when_due() {
    let Some(pool) = pool().await else { return };
    let fix = Fixture::new(pool).await;
    let path = "/api/v1/fundamentals/schedule";

    let (status, _) = fix.call(fix.editor, "GET", path, "").await;
    assert_eq!(status, 404);
    let body = json!({"symbols": ["msft", "AAPL"], "interval_hours": 24}).to_string();
    let (status, _) = fix.call(fix.viewer, "PUT", path, &body).await;
    assert_eq!(status, 403);
    let (status, err) = fix.call(fix.editor, "PUT", path, r#"{"symbols":["AAPL"],"interval_hours":0}"#).await;
    assert_eq!(status, 422, "{err}");
    let (status, saved) = fix.call(fix.editor, "PUT", path, &body).await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(
        (saved["symbols"].clone(), saved["interval_hours"].as_i64()),
        (json!(["MSFT", "AAPL"]), Some(24))
    );
    let (_, read) = fix.call(fix.viewer, "GET", path, "").await;
    assert_eq!(read["symbols"], saved["symbols"]);

    // The first refresh is due now: a sweep queues it, with the schedule's symbols.
    sweep_due_schedules(&fix.pool).await;
    let payloads: Vec<Value> =
        sqlx::query_scalar("SELECT payload FROM jobs WHERE workspace_id = $1 AND kind = $2")
            .bind(fix.ws)
            .bind(FUNDAMENTALS_REFRESH_JOB_KIND)
            .fetch_all(&fix.pool)
            .await
            .unwrap();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["symbols"], json!(["MSFT", "AAPL"]));
    assert!(payloads[0]["schedule_id"].is_string());
    // Not due again for a day.
    sweep_due_schedules(&fix.pool).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE workspace_id = $1 AND kind = $2")
        .bind(fix.ws)
        .bind(FUNDAMENTALS_REFRESH_JOB_KIND)
        .fetch_one(&fix.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let (status, deleted) = fix.call(fix.editor, "DELETE", path, "").await;
    assert_eq!((status, deleted["deleted"].as_bool()), (200, Some(true)));
    let (status, _) = fix.call(fix.editor, "DELETE", path, "").await;
    assert_eq!(status, 404);
    fix.clear_jobs().await;
}

#[tokio::test]
async fn concurrent_refresh_jobs_respect_sec_rate_limit_spacing() {
    let Some(pool) = pool().await else { return };
    let fix1 = Fixture::new(pool.clone()).await;
    let fix2 = Fixture::new(pool).await;
    let (edgar, mock) = mock_edgar().await;

    // Queue a refresh job in workspace 1
    let (status1, queued1) = fix1
        .call(fix1.editor, "POST", "/api/v1/fundamentals/refresh", &json!({ "symbols": ["EXA"] }).to_string())
        .await;
    assert_eq!(status1, 202);
    let job_id1 = Uuid::parse_str(queued1["job_id"].as_str().unwrap()).unwrap();

    // Queue a refresh job in workspace 2
    let (status2, queued2) = fix2
        .call(fix2.editor, "POST", "/api/v1/fundamentals/refresh", &json!({ "symbols": ["EXA"] }).to_string())
        .await;
    assert_eq!(status2, 202);
    let job_id2 = Uuid::parse_str(queued2["job_id"].as_str().unwrap()).unwrap();

    // Clear request times recorded during setup/mock startup if any
    mock.request_times.lock().unwrap().clear();

    // Run both jobs concurrently
    let (res1, res2) = tokio::join!(fix1.run(job_id1, Ok(&edgar)), fix2.run(job_id2, Ok(&edgar)),);
    assert!(matches!(res1, RefreshOutcome::Done(_)));
    assert!(matches!(res2, RefreshOutcome::Done(_)));

    fix1.clear_jobs().await;
    fix2.clear_jobs().await;

    let mut times = mock.request_times.lock().unwrap().clone();
    // Two jobs each fetching ticker map + EXA Company Facts = 4 requests
    assert_eq!(times.len(), 4, "expected 4 requests to mock EDGAR across both jobs: {times:?}");
    times.sort();

    // Measure the spacing between consecutive requests
    for i in 1..times.len() {
        let gap = times[i].duration_since(times[i - 1]);
        // MIN_REQUEST_GAP is 125ms; allow 120ms to tolerate OS clock/scheduling jitter
        assert!(
            gap >= std::time::Duration::from_millis(120),
            "request {i} occurred {gap:?} after request {}, violating SEC 125ms rate limit",
            i - 1
        );
    }
}
