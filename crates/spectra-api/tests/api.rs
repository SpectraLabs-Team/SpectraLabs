//! API tests: routing, validation and the JSON shapes the dashboard reads,
//! plus round trips over a real socket and a graceful shutdown.

use std::fmt::Write as FmtWrite;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{HeaderMap, Request};
use http_body_util::BodyExt;
use spectra_api::auth::{canonical, sign};
use spectra_api::{router, serve, to_text, ApiConfig, Secret, SAMPLE_PORTFOLIO};
use spectra_core::Json;
use tower::ServiceExt;

const SECRET: &str = "test-secret-test-secret-test-secret!";
const WORKSPACE: &str = "0192f3a4-0000-7000-8000-000000000001";

fn cfg() -> ApiConfig {
    ApiConfig::new(Secret::new(SECRET))
}

fn now() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string()
}

/// The identity headers and signature the Next.js proxy adds.
fn signature_headers(method: &str, path: &str, body: &[u8]) -> Vec<(&'static str, String)> {
    let ts = now();
    let sig = sign(
        SECRET.as_bytes(),
        &canonical(method, path, &ts, "alice@example.com", WORKSPACE, "req-test-1", body),
    );
    vec![
        ("x-spectra-user", "alice@example.com".into()),
        ("x-spectra-workspace", WORKSPACE.into()),
        ("x-request-id", "req-test-1".into()),
        ("x-spectra-timestamp", ts),
        ("x-spectra-signature", sig),
    ]
}

fn signed(method: &str, path: &str, body: Vec<u8>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(path).header("content-type", "application/json");
    for (k, v) in signature_headers(method, path, &body) {
        b = b.header(k, v);
    }
    b.body(Body::from(body)).unwrap()
}

fn post(path: &str, body: &Json) -> Request<Body> {
    post_raw(path, body.dump())
}

fn post_raw(path: &str, body: impl Into<Vec<u8>>) -> Request<Body> {
    signed("POST", path, body.into())
}

fn get(path: &str) -> Request<Body> {
    signed("GET", path, Vec::new())
}

fn unsigned(method: &str, path: &str) -> Request<Body> {
    Request::builder().method(method).uri(path).body(Body::empty()).unwrap()
}

async fn send(cfg: ApiConfig, req: Request<Body>) -> (u16, HeaderMap, Json) {
    let resp = router(cfg, None).oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = std::str::from_utf8(&bytes).ok().and_then(|t| Json::parse(t).ok()).unwrap_or(Json::Null);
    (status, headers, json)
}

async fn call(req: Request<Body>) -> (u16, Json) {
    let (status, _, json) = send(cfg(), req).await;
    (status, json)
}

fn body(metric: &str, sims: i64) -> Json {
    Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("metric", Json::string(metric))
        .with("confidence", Json::number(0.99).unwrap())
        .with("simulations", Json::integer(sims))
}

#[tokio::test]
async fn risk_returns_the_report_shape_the_dashboard_reads() {
    let (status, j) = call(post("/api/v1/risk", &body("var", 20_000))).await;
    assert_eq!(status, 200, "{}", j.dump());
    for k in [
        "metric",
        "value",
        "confidence",
        "simulations_run",
        "seed",
        "backend",
        "library_version",
        "git_commit",
        "total_ms",
        "compute_ms",
    ] {
        assert!(j.find(k).is_some(), "missing {k}");
    }
    assert_eq!(j.at("metric").unwrap().as_str().unwrap(), "VaR");
    assert_eq!(j.at("simulations_run").unwrap().as_int().unwrap(), 20_000);
    // Same request, same answer.
    let (_, again) = call(post("/api/v1/risk", &body("var", 20_000))).await;
    assert_eq!(j.at("value").unwrap().as_double().unwrap(), again.at("value").unwrap().as_double().unwrap());
}

#[tokio::test]
async fn risk_validation_matches_the_python_backend() {
    let cases = [
        (body("var", 500), 422, "between 1,000 and 100,000,000"),
        (body("var", 20_000).with("confidence", Json::number(0.4).unwrap()), 422, "between 0.5 and 1.0"),
        (body("v@r", 20_000), 422, "Metric name is invalid"),
        (body("var", 20_000).with("portfolio_csv", Json::string("  ")), 422, "Portfolio CSV is required"),
        (body("sharperatio", 20_000), 422, "time-series metric"),
        (body("nonsense", 20_000), 422, "Unknown metric"),
        (body("var", 20_000).with("simulations", Json::string("many")), 422, "must be an integer"),
    ];
    for (b, code, text) in cases {
        let (status, j) = call(post("/api/v1/risk", &b)).await;
        assert_eq!(status, code, "{}", j.dump());
        assert!(j.at("detail").unwrap().as_str().unwrap().contains(text), "{}", j.dump());
    }
    let (status, _) =
        call(post("/api/v1/risk", &body("var", 20_000).with("simulations", Json::string("20000")))).await;
    assert_eq!(status, 200, "numeric strings are accepted, as int() accepted them");
}

#[tokio::test]
async fn benchmark_and_stress() {
    let b = Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("simulations", Json::integer(5_000))
        .with("repetitions", Json::integer(2))
        .with("warmups", Json::integer(0));
    let (status, j) = call(post("/api/v1/benchmark", &b)).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(j.at("benchmarks").unwrap().items().unwrap().len(), 3);
    let too_big = b.clone().with("simulations", Json::integer(2_000_000));
    assert_eq!(call(post("/api/v1/benchmark", &too_big)).await.0, 422);

    let s = Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("scenario", Json::string("market-crash"))
        .with("simulations", Json::integer(5_000));
    let (status, j) = call(post("/api/v1/stress", &s)).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert!(j.at("scenario_loss").unwrap().as_double().unwrap() > 0.0);
    let bad = s.with("scenario", Json::string("meteor"));
    assert_eq!(call(post("/api/v1/stress", &bad)).await.0, 422);
}

#[tokio::test]
async fn info_routes_and_errors() {
    assert_eq!(call(get("/health")).await.1.at("status").unwrap().as_str().unwrap(), "healthy");
    assert_eq!(
        call(get("/api/v1/sample-portfolio")).await.1.at("csv").unwrap().as_str().unwrap(),
        SAMPLE_PORTFOLIO
    );
    assert!(call(get("/api/v1/telemetry/cpu")).await.1.find("cpu_threads").is_some());
    assert!(call(get("/api/v1/telemetry/workers")).await.1.find("workers").is_some());
    assert!(call(get("/api/v1/telemetry/gpu")).await.1.find("available").is_some());
    assert!(call(get("/api/v1/metrics")).await.1.items().unwrap().len() > 20);
    assert_eq!(call(get("/api/v1/alpaca/account")).await.0, 501);

    let (status, j) = call(get("/api/v1/risk")).await;
    assert_eq!(status, 405);
    assert_eq!(j.at("detail").unwrap().as_str().unwrap(), "Method not allowed.");
    let (status, j) = call(get("/nowhere")).await;
    assert_eq!(status, 404);
    assert_eq!(j.at("detail").unwrap().as_str().unwrap(), "Not Found");

    for bad in ["{", "[1,2]", "\u{0}"] {
        assert_eq!(call(post_raw("/api/v1/risk", bad)).await.0, 422, "{bad}");
    }
    assert_eq!(call(post_raw("/api/v1/risk", vec![0xff_u8, 0xfe])).await.0, 422, "non-UTF-8 body");
}

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    for req in [get("/health"), get("/nowhere"), post("/api/v1/risk", &body("var", 500))] {
        let (_, h, _) = send(cfg(), req).await;
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(h.get("cache-control").unwrap(), "no-store");
        assert_eq!(h.get("content-type").unwrap(), "application/json");
    }
}

#[tokio::test]
async fn cors_allows_only_configured_origins() {
    let preflight = |o: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/risk")
            .header("origin", o)
            .body(Body::empty())
            .unwrap()
    };
    let (status, h, _) = send(cfg(), preflight("http://localhost:3000")).await;
    assert_eq!(status, 204);
    assert_eq!(h.get("access-control-allow-origin").unwrap(), "http://localhost:3000");
    assert_eq!(h.get("access-control-allow-methods").unwrap(), "GET, POST, OPTIONS");

    let (status, h, _) = send(cfg(), preflight("https://evil.example")).await;
    assert_eq!(status, 403);
    assert!(!h.keys().any(|k| k.as_str().starts_with("access-control")));

    // Simple requests: headers for allowed origins only, and the request still runs either way.
    let with_origin =
        |o: &str| Request::builder().uri("/health").header("origin", o).body(Body::empty()).unwrap();
    let (status, h, _) = send(cfg(), with_origin("http://localhost:3000")).await;
    assert_eq!(status, 200);
    assert!(h.get("access-control-allow-origin").is_some());
    let (status, h, _) = send(cfg(), with_origin("https://evil.example")).await;
    assert_eq!(status, 200);
    assert!(h.get("access-control-allow-origin").is_none());

    let any = ApiConfig { cors_origins: vec!["*".into()], ..cfg() };
    let (status, h, _) = send(any, preflight("https://anywhere.example")).await;
    assert_eq!(status, 204);
    assert_eq!(h.get("access-control-allow-origin").unwrap(), "https://anywhere.example");
}

#[test]
fn text_output_uses_the_shortest_round_trip_numbers() {
    let j = Json::object().with("c", Json::number(0.99).unwrap()).with("n", Json::integer(3));
    assert_eq!(to_text(&j), r#"{"c":0.99,"n":3}"#);
    assert_eq!(Json::parse(&to_text(&j)).unwrap(), j);
}

/// Sends raw bytes and reads until the server closes the connection.
fn roundtrip(addr: SocketAddr, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s.write_all(raw).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out); // keeps whatever arrived before an error
    String::from_utf8_lossy(&out).into_owned()
}

fn risk_request(metric: &str, sims: i64) -> Vec<u8> {
    let b = body(metric, sims).dump();
    let mut sig = String::new();
    for (k, v) in signature_headers("POST", "/api/v1/risk", b.as_bytes()) {
        writeln!(&mut sig, "{k}: {v}\r").unwrap();
    }
    format!(
        "POST /api/v1/risk HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\n\
         {sig}Content-Length: {}\r\n\r\n{b}",
        b.len()
    )
    .into_bytes()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serves_over_a_real_socket_and_rejects_bad_requests() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(listener, cfg(), None, async {
        let _ = stopped.await;
    }));

    let ok =
        tokio::task::spawn_blocking(move || roundtrip(addr, &risk_request("cvar", 5_000))).await.unwrap();
    assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"), "{ok}");
    assert!(ok.contains("\"metric\":\"CVaR\""), "{ok}");

    let big = tokio::task::spawn_blocking(move || {
        roundtrip(
            addr,
            b"POST /api/v1/risk HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 999999999\r\n\r\n",
        )
    })
    .await
    .unwrap();
    assert!(big.starts_with("HTTP/1.1 413"), "{big}");
    assert!(big.contains("12 MB"), "{big}");

    let garbage = tokio::task::spawn_blocking(move || roundtrip(addr, b"HELLO\r\n\r\n")).await.unwrap();
    assert!(garbage.starts_with("HTTP/1.1 400"), "{garbage}");

    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_lets_in_flight_requests_finish() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(listener, cfg(), None, async {
        let _ = stopped.await;
    }));

    // A request large enough to still be running when the shutdown signal arrives.
    let in_flight = tokio::task::spawn_blocking(move || roundtrip(addr, &risk_request("var", 2_000_000)));
    tokio::time::sleep(Duration::from_millis(200)).await;
    stop.send(()).unwrap();

    let resp = in_flight.await.unwrap();
    assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "in-flight request was cut off: {resp}");
    tokio::time::timeout(Duration::from_secs(60), server)
        .await
        .expect("server did not stop")
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect(addr).is_err(), "server still accepting after shutdown");
}

#[tokio::test]
async fn unsigned_or_forged_requests_are_refused() {
    for path in ["/api/v1/risk", "/api/risk", "/api/v1/sample-portfolio", "/api/v1/telemetry/cpu"] {
        let method = if path.ends_with("risk") { "POST" } else { "GET" };
        let (status, j) = call(unsigned(method, path)).await;
        assert_eq!(status, 401, "{path}: {}", j.dump());
        assert_eq!(j.at("code").unwrap().as_str().unwrap(), "unauthenticated");
    }
    // A valid signature for one body does not cover another.
    let mut req = post("/api/v1/risk", &body("var", 20_000));
    *req.body_mut() = Body::from(body("var", 30_000).dump());
    assert_eq!(call(req).await.0, 401);
    // Nor does it cover another path.
    let mut req = get("/api/v1/metrics");
    *req.uri_mut() = "/api/v1/sample-portfolio".parse().unwrap();
    assert_eq!(call(req).await.0, 401);
    // Nor a different secret.
    let other = ApiConfig::new(Secret::new("a-different-secret-a-different-secret"));
    assert_eq!(send(other, get("/api/v1/metrics")).await.0, 401);
    // Liveness and readiness probes need no signature.
    assert_eq!(call(unsigned("GET", "/health")).await.0, 200);
}

#[tokio::test]
async fn errors_share_one_shape_with_the_request_id() {
    let (status, h, j) = send(cfg(), post("/api/v1/risk", &body("var", 500))).await;
    assert_eq!(status, 422);
    assert_eq!(j.at("code").unwrap().as_str().unwrap(), "invalid_request");
    assert!(j.at("message").unwrap().as_str().unwrap().contains("between 1,000"));
    assert!(j.at("details").unwrap().is_null());
    assert_eq!(j.at("request_id").unwrap().as_str().unwrap(), "req-test-1");
    assert_eq!(h.get("x-request-id").unwrap(), "req-test-1");
    // The deprecated copy the current dashboard reads.
    assert_eq!(j.at("detail").unwrap().as_str().unwrap(), j.at("message").unwrap().as_str().unwrap());

    // Without an X-Request-Id, the API makes a UUIDv7 and returns it.
    let (status, h, j) = send(cfg(), unsigned("GET", "/nowhere")).await;
    assert_eq!(status, 404);
    let id = h.get("x-request-id").unwrap().to_str().unwrap().to_string();
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "7", "{id}");
    assert_eq!(j.at("request_id").unwrap().as_str().unwrap(), id);

    // An unusable incoming id (spaces, too long) is replaced, not echoed.
    for bad in ["bad id".to_string(), "x".repeat(200)] {
        let req = Request::builder()
            .uri("/health")
            .header("x-request-id", bad.as_str())
            .body(Body::empty())
            .unwrap();
        let (_, h, _) = send(cfg(), req).await;
        assert_ne!(h.get("x-request-id").unwrap(), bad.as_str());
    }
}

#[tokio::test]
async fn old_paths_still_work_as_aliases() {
    let (status, j) = call(post("/api/risk", &body("var", 20_000))).await;
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(call(get("/api/sample-portfolio")).await.0, 200);
}

#[tokio::test]
async fn ready_reports_503_without_a_database() {
    let (status, j) = call(unsigned("GET", "/ready")).await;
    assert_eq!(status, 503);
    assert_eq!(j.at("status").unwrap().as_str().unwrap(), "not_ready");
    assert_eq!(j.at("checks").unwrap().at("database").unwrap().as_str().unwrap(), "not_configured");
}

#[tokio::test]
async fn ready_reports_the_worker_check() {
    let (_, j) = call(unsigned("GET", "/ready")).await;
    assert_eq!(j.at("checks").unwrap().at("worker").unwrap().as_str().unwrap(), "not_configured");
}

#[tokio::test]
async fn job_routes_need_a_signature() {
    for (method, path) in [
        ("GET", "/api/v1/jobs"),
        ("POST", "/api/v1/jobs"),
        ("GET", "/api/v1/jobs/0192f3a4-0000-7000-8000-000000000002"),
        ("POST", "/api/v1/jobs/0192f3a4-0000-7000-8000-000000000002/cancel"),
    ] {
        assert_eq!(call(unsigned(method, path)).await.0, 401, "{method} {path}");
    }
}

#[tokio::test]
async fn job_routes_report_503_without_a_database() {
    let job = Json::object()
        .with("kind", Json::string("risk"))
        .with("payload", Json::object().with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO)));
    let (status, j) = call(post("/api/v1/jobs", &job)).await;
    assert_eq!(status, 503, "{}", j.dump());
    assert_eq!(j.at("code").unwrap().as_str().unwrap(), "unavailable");
    assert_eq!(call(get("/api/v1/jobs")).await.0, 503);
    assert_eq!(call(get("/api/v1/jobs/0192f3a4-0000-7000-8000-000000000002")).await.0, 503);
}

#[tokio::test]
async fn backtest_routes_need_a_signature() {
    for (method, path) in [
        ("POST", "/api/v1/backtests"),
        ("GET", "/api/v1/backtests/0192f3a4-0000-7000-8000-000000000002"),
        ("POST", "/api/v1/backtests/0192f3a4-0000-7000-8000-000000000002/cancel"),
        ("GET", "/api/v1/backtests/0192f3a4-0000-7000-8000-000000000002/series"),
        ("GET", "/api/v1/strategies/builtin"),
    ] {
        assert_eq!(call(unsigned(method, path)).await.0, 401, "{method} {path}");
    }
}

#[tokio::test]
async fn backtest_routes_report_503_without_a_database() {
    let body = Json::object()
        .with("dataset_version_id", Json::string("0192f3a4-0000-7000-8000-000000000003"))
        .with("strategy", Json::string("sma_cross"));
    let (status, j) = call(post("/api/v1/backtests", &body)).await;
    assert_eq!(status, 503, "{}", j.dump());
    assert_eq!(call(get("/api/v1/backtests/0192f3a4-0000-7000-8000-000000000002")).await.0, 503);
}

#[tokio::test]
async fn backtest_requests_are_checked_before_the_database() {
    // Unknown fields and a missing dataset are refused before anything is queued.
    let extra = Json::object().with("strategy", Json::string("sma_cross")).with("dataset", Json::integer(1));
    assert_eq!(call(post("/api/v1/backtests", &extra)).await.0, 422);
    let (status, j) =
        call(get("/api/v1/backtests/0192f3a4-0000-7000-8000-000000000002/series?kind=pnl")).await;
    assert_eq!(status, 422, "{}", j.dump());
}

#[tokio::test]
async fn builtin_strategies_describe_the_backtest_form() {
    let (status, j) = call(get("/api/v1/strategies/builtin")).await;
    assert_eq!(status, 200, "{}", j.dump());
    let names: Vec<String> = j
        .at("strategies")
        .unwrap()
        .items()
        .unwrap()
        .iter()
        .map(|s| s.at("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["buy_and_hold", "equal_weight", "sma_cross", "zscore_reversion"]);
    assert!(j.at("config_schema").unwrap().at("properties").unwrap().at("slippage_bps").is_ok());
    assert_eq!(j.at("presets").unwrap().items().unwrap().len(), 3);
}

#[tokio::test]
async fn job_routes_are_not_served_at_the_old_paths() {
    assert_eq!(call(get("/api/jobs")).await.0, 404);
}

#[tokio::test]
async fn openapi_spec_is_served_unsigned() {
    let (status, j) = call(unsigned("GET", "/api/v1/openapi.json")).await;
    assert_eq!(status, 200);
    let paths = j.at("paths").unwrap();
    for path in [
        "/api/v1/jobs",
        "/api/v1/jobs/{id}",
        "/api/v1/jobs/{id}/cancel",
        "/api/v1/backtests",
        "/api/v1/backtests/{id}/series",
        "/api/v1/strategies/builtin",
        "/api/v1/risk",
        "/ready",
    ] {
        assert!(paths.at(path).is_ok(), "missing {path}");
    }
}

#[tokio::test]
async fn prometheus_metrics_are_served_unsigned() {
    spectra_api::observability::init_observability();
    // One request first, so there is a series to render.
    let _ = call(unsigned("GET", "/health")).await;
    let (status, headers, _) = send(cfg(), unsigned("GET", "/metrics")).await;
    assert_eq!(status, 200);
    assert!(headers.get("content-type").unwrap().to_str().unwrap().starts_with("text/plain"));
}
