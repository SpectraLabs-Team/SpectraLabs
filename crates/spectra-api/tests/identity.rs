//! Caller identity on the API: with ENGINE_SHARED_SECRET set, only signed
//! requests from the dashboard proxy get through, and computations need the
//! editor role in the caller's active workspace.

use spectra_api::identity::{hmac_sha256, unix_now, Secret};
use spectra_api::{handle, ApiConfig, Request, SAMPLE_PORTFOLIO};
use spectra_core::Json;

const SECRET: &str = "test-engine-secret-xxxxxxxxxxxxxxxxxxxxxxxxxxxx";

fn b64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// What the Next.js proxy sends (web/frontend/lib/server/engineIdentity.ts).
fn identity(role: &str, method: &str, path: &str) -> String {
    let iat = unix_now();
    let payload = Json::object()
        .with("v", Json::integer(1))
        .with("uid", Json::string("user-1"))
        .with("sid", Json::string("session-1"))
        .with("wid", Json::string("workspace-1"))
        .with("wrole", Json::string(role))
        .with("prole", Json::string("user"))
        .with("m", Json::string(method))
        .with("p", Json::string(path.trim_start_matches('/')))
        .with("iat", Json::integer(iat))
        .with("exp", Json::integer(iat + 60))
        .dump();
    let p = b64url(payload.as_bytes());
    format!("{p}.{}", b64url(&hmac_sha256(SECRET.as_bytes(), p.as_bytes())))
}

fn cfg() -> ApiConfig {
    ApiConfig { shared_secret: Secret::new(SECRET), ..ApiConfig::default() }
}

fn req(method: &str, path: &str, header: Option<String>) -> Request {
    let body = if method == "POST" {
        Json::object()
            .with("portfolio_csv", Json::string(SAMPLE_PORTFOLIO))
            .with("metric", Json::string("var"))
            .with("simulations", Json::integer(5_000))
            .dump()
            .into_bytes()
    } else {
        Vec::new()
    };
    Request {
        method: method.into(),
        path: path.into(),
        headers: header.map(|h| vec![("x-spectra-identity".into(), h)]).unwrap_or_default(),
        body,
        ..Request::default()
    }
}

fn status(c: &ApiConfig, r: &Request) -> u16 {
    handle(c, r).status
}

#[test]
fn without_a_secret_the_api_is_open_as_before() {
    assert_eq!(status(&ApiConfig::default(), &req("GET", "/api/sample-portfolio", None)), 200);
}

#[test]
fn health_stays_public() {
    assert_eq!(status(&cfg(), &req("GET", "/health", None)), 200);
    assert_eq!(status(&cfg(), &req("GET", "/", None)), 200);
}

#[test]
fn unsigned_or_mis_signed_requests_are_401() {
    let c = cfg();
    assert_eq!(status(&c, &req("GET", "/api/sample-portfolio", None)), 401);
    assert_eq!(status(&c, &req("POST", "/api/risk", None)), 401);
    assert_eq!(status(&c, &req("GET", "/api/sample-portfolio", Some("garbage".into()))), 401);
    // A valid header for another route can't be replayed here.
    let for_metrics = identity("owner", "GET", "/api/metrics");
    assert_eq!(status(&c, &req("GET", "/api/sample-portfolio", Some(for_metrics))), 401);
    let resp = handle(&c, &req("GET", "/api/telemetry/cpu", None));
    assert!(std::str::from_utf8(&resp.body).unwrap().contains("detail"));
}

#[test]
fn viewers_read_editors_and_owners_compute() {
    let c = cfg();
    let get = |role| {
        status(&c, &req("GET", "/api/sample-portfolio", Some(identity(role, "GET", "/api/sample-portfolio"))))
    };
    assert_eq!(get("viewer"), 200);
    let risk = |role| status(&c, &req("POST", "/api/risk", Some(identity(role, "POST", "/api/risk"))));
    assert_eq!(risk("viewer"), 403);
    assert_eq!(risk("editor"), 200);
    assert_eq!(risk("owner"), 200);
}

#[test]
fn options_preflight_is_not_blocked() {
    assert_eq!(status(&cfg(), &req("OPTIONS", "/api/risk", None)), 204);
}
