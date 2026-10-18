//! API tests: routing, validation and the JSON shapes the dashboard reads,
//! plus one round trip over a real socket.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use spectra_api::{handle, http, to_text, ApiConfig, Request, SAMPLE_PORTFOLIO};
use spectra_core::Json;

fn post(path: &str, body: &Json) -> Request {
    Request { method: "POST".into(), path: path.into(), body: body.dump().into_bytes(), ..Request::default() }
}

fn get(path: &str) -> Request {
    Request { method: "GET".into(), path: path.into(), ..Request::default() }
}

fn call(req: &Request) -> (u16, Json) {
    let r = handle(&ApiConfig::default(), req);
    (r.status, Json::parse(std::str::from_utf8(&r.body).unwrap()).unwrap_or(Json::Null))
}

fn body(metric: &str, sims: i64) -> Json {
    Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("metric", Json::string(metric))
        .with("confidence", Json::number(0.99).unwrap())
        .with("simulations", Json::integer(sims))
}

#[test]
fn risk_returns_the_report_shape_the_dashboard_reads() {
    let (status, j) = call(&post("/api/risk", &body("var", 20_000)));
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
    let (_, again) = call(&post("/api/risk", &body("var", 20_000)));
    assert_eq!(j.at("value").unwrap().as_double().unwrap(), again.at("value").unwrap().as_double().unwrap());
}

#[test]
fn risk_validation_matches_the_python_backend() {
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
        let (status, j) = call(&post("/api/risk", &b));
        assert_eq!(status, code, "{}", j.dump());
        assert!(j.at("detail").unwrap().as_str().unwrap().contains(text), "{}", j.dump());
    }
    let (status, _) =
        call(&post("/api/risk", &body("var", 20_000).with("simulations", Json::string("20000"))));
    assert_eq!(status, 200, "numeric strings are accepted, as int() accepted them");
}

#[test]
fn benchmark_and_stress() {
    let b = Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("simulations", Json::integer(5_000))
        .with("repetitions", Json::integer(2))
        .with("warmups", Json::integer(0));
    let (status, j) = call(&post("/api/benchmark", &b));
    assert_eq!(status, 200, "{}", j.dump());
    assert_eq!(j.at("benchmarks").unwrap().items().unwrap().len(), 3);
    let too_big = b.clone().with("simulations", Json::integer(2_000_000));
    assert_eq!(call(&post("/api/benchmark", &too_big)).0, 422);

    let s = Json::object()
        .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
        .with("scenario", Json::string("market-crash"))
        .with("simulations", Json::integer(5_000));
    let (status, j) = call(&post("/api/stress", &s));
    assert_eq!(status, 200, "{}", j.dump());
    assert!(j.at("scenario_loss").unwrap().as_double().unwrap() > 0.0);
    let bad = s.with("scenario", Json::string("meteor"));
    assert_eq!(call(&post("/api/stress", &bad)).0, 422);
}

#[test]
fn info_routes_and_errors() {
    assert_eq!(call(&get("/health")).1.at("status").unwrap().as_str().unwrap(), "healthy");
    assert_eq!(call(&get("/api/sample-portfolio")).1.at("csv").unwrap().as_str().unwrap(), SAMPLE_PORTFOLIO);
    assert!(call(&get("/api/telemetry/cpu")).1.find("cpu_threads").is_some());
    assert!(call(&get("/api/telemetry/workers")).1.find("workers").is_some());
    assert!(call(&get("/api/telemetry/gpu")).1.find("available").is_some());
    assert!(call(&get("/api/metrics")).1.items().unwrap().len() > 20);
    assert_eq!(call(&get("/api/alpaca/account")).0, 501);
    assert_eq!(call(&get("/api/risk")).0, 405);
    assert_eq!(call(&get("/nowhere")).0, 404);
    let bad_json = Request { body: b"{".to_vec(), ..post("/api/risk", &Json::Null) };
    assert_eq!(call(&bad_json).0, 422);
}

#[test]
fn cors_allows_only_configured_origins() {
    let cfg = ApiConfig::default();
    let with_origin = |o: &str| Request {
        method: "OPTIONS".into(),
        path: "/api/risk".into(),
        headers: vec![("origin".into(), o.into())],
        ..Request::default()
    };
    let ok = handle(&cfg, &with_origin("http://localhost:3000"));
    assert_eq!(ok.status, 204);
    assert!(ok
        .headers
        .iter()
        .any(|(k, v)| k == "Access-Control-Allow-Origin" && v == "http://localhost:3000"));
    let denied = handle(&cfg, &with_origin("https://evil.example"));
    assert_eq!(denied.status, 403);
    assert!(!denied.headers.iter().any(|(k, _)| k.starts_with("Access-Control")));
}

#[test]
fn text_output_uses_the_shortest_round_trip_numbers() {
    let j = Json::object().with("c", Json::number(0.99).unwrap()).with("n", Json::integer(3));
    assert_eq!(to_text(&j), r#"{"c":0.99,"n":3}"#);
    assert_eq!(Json::parse(&to_text(&j)).unwrap(), j);
}

#[test]
fn serves_over_a_real_socket_and_rejects_oversized_bodies() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = ApiConfig::default();
    std::thread::spawn(move || http::serve(&listener, Arc::new(move |r| handle(&cfg, r))));

    let roundtrip = |raw: &[u8]| -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(raw).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let b = body("cvar", 5_000).dump();
    let resp = roundtrip(
        format!("POST /api/risk HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len())
            .as_bytes(),
    );
    assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "{resp}");
    assert!(resp.contains("\"metric\":\"CVaR\""));
    let big = roundtrip(b"POST /api/risk HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n");
    assert!(big.starts_with("HTTP/1.1 413"), "{big}");
    let garbage = roundtrip(b"HELLO\r\n\r\n");
    assert!(garbage.starts_with("HTTP/1.1 400"), "{garbage}");
}
