//! Spectra Labs HTTP API: the Rust replacement for the FastAPI backend. It
//! calls the risk engine directly and serves the routes the Next.js dashboard
//! uses. Only the Next.js proxy talks to it; see [`auth`].
//!
//! | route | auth | |
//! |---|---|---|
//! | `GET /`, `GET /health` | none | liveness: the process is up |
//! | `GET /ready` | none | readiness: the database answers (live workers are reported, not required) |
//! | `GET /metrics` | none | Prometheus text format |
//! | `GET /api/v1/openapi.json` | none | the OpenAPI description |
//! | `POST /api/v1/risk` | signed | `{portfolio_csv, metric, confidence, simulations[, seed, backend, horizon_days]}` |
//! | `POST /api/v1/benchmark` | signed | `{portfolio_csv, simulations, repetitions, warmups}` |
//! | `POST /api/v1/stress` | signed | `{portfolio_csv, scenario[, simulations, seed]}` |
//! | `GET /api/v1/metrics` | signed | which metric names are computable |
//! | `GET /api/v1/sample-portfolio` | signed | `{csv}` |
//! | `GET /api/v1/telemetry/{cpu,workers,gpu}` | signed | host readings |
//! | `/api/v1/alpaca/*` | signed | 501: not ported yet |
//! | `POST /api/v1/jobs`, `GET /api/v1/jobs[/{id}]`, `POST /api/v1/jobs/{id}/cancel` | signed | queued work; see [`jobs_api`] |
//!
//! Every engine route is also served at its old `/api/...` path (the jobs
//! routes are `/api/v1` only). Those
//! aliases are temporary: remove them once nothing calls them.
//!
//! **Conventions** (docs/api-conventions.md): errors are
//! `{"code", "message", "details", "request_id"}` (plus a deprecated `detail`
//! copy of `message` while the old paths exist); every response carries
//! `X-Request-Id`, taken from the proxy or generated as a UUIDv7.
//!
//! The server is [axum] on a multi-threaded tokio runtime. Engine work is
//! CPU-bound and synchronous, so each compute request runs on tokio's blocking
//! pool, at most [`MAX_CONCURRENT_COMPUTE`] at a time; further compute
//! requests get 503 rather than queueing. [`serve`] shuts down gracefully.
//!
//! Configuration is read once at startup: see [`config`].

pub mod auth;
pub mod config;
pub mod engine;
pub mod identity;
pub mod jobs_api;
pub mod observability;
pub mod openapi;
pub mod telemetry;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, MethodRouter};
use axum::Router;
use spectra_core::{Json, ENGINE_VERSION};
use spectra_db::PgPool;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

pub use auth::Identity;
pub use config::{Config, ConfigError, Secret};
pub use engine::{to_text, Reply, BENCHMARK_PATH_BUDGET};

/// The sample portfolio served at `/api/v1/sample-portfolio`.
pub const SAMPLE_PORTFOLIO: &str = include_str!("../../../examples/sample_portfolio.csv");

/// Largest accepted request body (the dashboard sends portfolio CSVs up to 10 MB).
pub const MAX_BODY: usize = 12 * 1024 * 1024;

/// Engine computations (risk, stress, benchmark, telemetry sampling) run at
/// once; further ones get 503.
pub const MAX_CONCURRENT_COMPUTE: usize = 32;

/// How long `/ready` waits for the database.
pub const READY_TIMEOUT: Duration = Duration::from_secs(2);

/// A worker counts as alive in `/ready` if it sent a heartbeat this recently
/// (workers beat every few seconds).
pub const WORKER_ALIVE_WITHIN: Duration = Duration::from_secs(30);

/// Router settings.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Origins allowed by CORS. `*` allows any origin.
    pub cors_origins: Vec<String>,
    /// Verifies the proxy's request signatures.
    pub engine_secret: Secret,
}

impl ApiConfig {
    /// Default CORS origins (`http://localhost:3000`) and the given secret.
    pub fn new(engine_secret: Secret) -> Self {
        Self { cors_origins: vec!["http://localhost:3000".into()], engine_secret }
    }

    fn allows(&self, origin: &str) -> bool {
        self.cors_origins.iter().any(|o| o == origin || o == "*")
    }
}

impl From<&Config> for ApiConfig {
    fn from(c: &Config) -> Self {
        Self { cors_origins: c.cors_origins.clone(), engine_secret: c.engine_secret.clone() }
    }
}

/// The request id, in each request's extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

#[derive(Debug, Clone)]
struct AppState {
    cfg: Arc<ApiConfig>,
    compute: Arc<Semaphore>,
    db: Option<PgPool>,
}

// ---------------------------------------------------------------------------
// Responses and the error format
// ---------------------------------------------------------------------------

/// An error waiting to be rendered. Handlers and middleware attach it to an
/// empty response; [`request_id`] renders it once the request id is known, so
/// every error, from any layer, has the same shape.
#[derive(Debug, Clone)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

/// The machine-readable `code` for a status.
pub fn error_code(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        401 => "unauthenticated",
        403 => "forbidden",
        404 => "not_found",
        405 => "method_not_allowed",
        408 => "timeout",
        409 => "conflict",
        413 => "payload_too_large",
        422 => "invalid_request",
        429 => "rate_limited",
        501 => "not_implemented",
        503 => "unavailable",
        _ if status >= 500 => "internal",
        _ => "error",
    }
}

fn json_response(status: u16, text: String) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, [(header::CONTENT_TYPE, "application/json")], text).into_response()
}

fn error_response(status: u16, message: impl Into<String>) -> Response {
    let mut resp = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR).into_response();
    resp.extensions_mut().insert(ErrorBody { code: error_code(status), message: message.into() });
    resp
}

/// An error with a specific `code` rather than the status's default (for
/// example `no_workspace`).
fn error_with_code(status: u16, code: &'static str, message: impl Into<String>) -> Response {
    let mut resp = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR).into_response();
    resp.extensions_mut().insert(ErrorBody { code, message: message.into() });
    resp
}

fn render_error(resp: Response, err: ErrorBody, request_id: &str) -> Response {
    let (mut parts, _) = resp.into_parts();
    let body = Json::object()
        .with("code", Json::string(err.code))
        .with("message", Json::string(err.message.clone()))
        .with("details", Json::Null)
        .with("request_id", Json::string(request_id))
        // Deprecated: the old `{"detail"}` shape, kept while the /api/... aliases exist.
        .with("detail", Json::string(err.message));
    parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(body.dump()))
}

fn respond(reply: Reply) -> Response {
    match reply {
        Ok(j) => json_response(200, to_text(&j)),
        Err((code, msg)) => error_response(code, msg),
    }
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

fn valid_request_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

/// Takes `X-Request-Id` from the proxy (or makes a UUIDv7), puts it in the
/// request's extensions and on the response, and renders pending errors.
async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(auth::headers::REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .filter(|v| valid_request_id(v))
        .map_or_else(|| Uuid::now_v7().to_string(), str::to_string);
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut resp = next.run(req).await;
    if let Some(err) = resp.extensions_mut().remove::<ErrorBody>() {
        resp = render_error(resp, err, &id);
    }
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert(auth::headers::REQUEST_ID, v);
    }
    resp
}

/// Rejects a declared `Content-Length` over [`MAX_BODY`] before reading any of
/// the body. (The body limit on the extractor only trips as bytes arrive, so a
/// client that declares a huge body and sends nothing would otherwise hang.)
async fn reject_oversized(req: Request, next: Next) -> Response {
    let declared = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_BODY as u64) {
        return error_response(413, "Request body exceeds the 12 MB limit.");
    }
    next.run(req).await
}

/// Records each request's latency and status for `/metrics`. The path label
/// is the matched route pattern (`/api/v1/jobs/{id}`), never the raw path, so
/// ids cannot blow up the number of series.
async fn track_metrics(req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |m| m.as_str().to_owned());
    let start = Instant::now();
    let resp = next.run(req).await;
    observability::RequestMetrics::request_duration(
        &method,
        &path,
        resp.status().as_u16(),
        start.elapsed().as_secs_f64(),
    );
    let request_id =
        resp.headers().get(auth::headers::REQUEST_ID).and_then(|v| v.to_str().ok()).unwrap_or("missing");
    tracing::info!(
        request_id,
        method,
        route = path,
        status = resp.status().as_u16(),
        duration_ms = start.elapsed().as_secs_f64() * 1000.0,
        "http request completed"
    );
    resp
}

/// CORS: headers only for configured origins, a 403 for preflights from any
/// other origin, and 204 for every preflight that passes.
async fn cors(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req.headers().get(header::ORIGIN).cloned();
    let allowed = origin.as_ref().and_then(|o| o.to_str().ok()).is_some_and(|o| st.cfg.allows(o));
    let mut resp = if req.method() == Method::OPTIONS {
        if origin.is_some() && !allowed {
            error_response(403, "Origin not allowed.")
        } else {
            StatusCode::NO_CONTENT.into_response()
        }
    } else {
        next.run(req).await
    };
    if let (true, Some(origin)) = (allowed, origin) {
        let h = resp.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        h.append(header::VARY, HeaderValue::from_static("Origin"));
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Content-Type"));
        h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    resp
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Verifies the proxy's HMAC signature (see [`auth`]) and puts the caller's
/// [`Identity`] in the request's extensions. Reads the body to hash it, then
/// hands the same bytes on.
async fn require_signature(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY).await else {
        return error_response(400, "The request body is too large or was not received in full.");
    };
    let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok());
    let signed = auth::SignedRequest {
        method: parts.method.as_str(),
        path_and_query: parts.uri.path_and_query().map_or_else(|| parts.uri.path(), |p| p.as_str()),
        user: header(auth::headers::USER),
        workspace: header(auth::headers::WORKSPACE),
        request_id: header(auth::headers::REQUEST_ID),
        timestamp: header(auth::headers::TIMESTAMP),
        signature: header(auth::headers::SIGNATURE),
        body: &bytes,
    };
    match auth::verify(st.cfg.engine_secret.expose(), &signed, now_secs()) {
        Ok(identity) => {
            let mut req = Request::from_parts(parts, Body::from(bytes));
            req.extensions_mut().insert(identity);
            next.run(req).await
        }
        Err(reason) => error_response(401, reason),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Parses a request body that must be a JSON object.
fn body_json(body: Result<Bytes, BytesRejection>) -> Result<Json, (u16, String)> {
    let bytes = body.map_err(|r| match r.status() {
        StatusCode::PAYLOAD_TOO_LARGE => (413, "Request body exceeds the 12 MB limit.".to_string()),
        s => (s.as_u16(), r.body_text()),
    })?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| engine::unprocessable("Request body must be UTF-8 JSON."))?;
    let j = Json::parse(text)
        .map_err(|e| engine::unprocessable(format!("Request body is not valid JSON: {e}")))?;
    if j.entries().is_err() {
        return Err(engine::unprocessable("Request body must be a JSON object."));
    }
    Ok(j)
}

/// Runs synchronous engine work on the blocking pool, bounded by the compute
/// semaphore so a burst of requests cannot pile up unbounded CPU work.
async fn compute(st: &AppState, work: impl FnOnce() -> Reply + Send + 'static) -> Response {
    let Ok(permit) = Arc::clone(&st.compute).try_acquire_owned() else {
        return error_response(503, "Server busy; try again.");
    };
    let joined = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await;
    match joined {
        Ok(reply) => respond(reply),
        Err(_) => error_response(500, "The engine failed while handling this request."),
    }
}

/// A POST route: parse the JSON body, then run `route` on the blocking pool.
async fn post_json(
    st: &AppState,
    body: Result<Bytes, BytesRejection>,
    route: fn(&Json) -> Reply,
) -> Response {
    match body_json(body) {
        Ok(j) => compute(st, move || route(&j)).await,
        Err((code, msg)) => error_response(code, msg),
    }
}

#[allow(clippy::unused_async)] // axum handlers are async fns
async fn root() -> Response {
    respond(Ok(Json::object()
        .with("message", Json::string("Spectra Labs API"))
        .with("version", Json::string(ENGINE_VERSION))))
}

/// Liveness.
#[utoipa::path(get, path = "/health", tag = "probes", responses((status = 200, description = "The process is up")))]
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn health() -> Response {
    respond(Ok(Json::object()
        .with("status", Json::string("healthy"))
        .with("engine", Json::string("spectra-risk"))
        .with("engine_version", Json::string(ENGINE_VERSION))
        .with("cli_exists", Json::boolean(true))))
}

/// Readiness: 200 when both the database and at least one worker answer,
/// otherwise 503. Compose starts the API and worker together after migrations.
#[utoipa::path(get, path = "/ready", tag = "probes", responses(
    (status = 200, description = "The database and a worker are available"),
    (status = 503, description = "The database or worker is unavailable or not configured"),
))]
async fn ready(State(st): State<AppState>) -> Response {
    let (database, worker) = match &st.db {
        None => ("not_configured", "not_configured"),
        Some(pool) => match tokio::time::timeout(READY_TIMEOUT, spectra_db::ping(pool)).await {
            Ok(Ok(())) => {
                let alive = tokio::time::timeout(
                    READY_TIMEOUT,
                    spectra_db::jobs::workers_alive(pool, WORKER_ALIVE_WITHIN),
                )
                .await;
                let worker = match alive {
                    Ok(Ok(n)) if n > 0 => "ok",
                    Ok(Ok(_)) => "none_alive",
                    _ => "unavailable",
                };
                ("ok", worker)
            }
            _ => ("unavailable", "unavailable"),
        },
    };
    let ok = database == "ok" && worker == "ok";
    let body = Json::object().with("status", Json::string(if ok { "ready" } else { "not_ready" })).with(
        "checks",
        Json::object().with("database", Json::string(database)).with("worker", Json::string(worker)),
    );
    json_response(if ok { 200 } else { 503 }, body.dump())
}

/// Prometheus metrics (request latency by route and status, job counts and
/// durations). Unsigned like `/health`: keep the port private.
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn prometheus_metrics() -> Response {
    match observability::render_metrics() {
        Some(text) => {
            (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], text)
                .into_response()
        }
        None => error_response(503, "Metrics are not initialised in this process."),
    }
}

/// The OpenAPI description, for generating clients.
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn openapi_json() -> Response {
    json_response(200, openapi::spec_json())
}

/// A risk metric for a portfolio.
#[utoipa::path(post, path = "/api/v1/risk", tag = "engine", request_body = crate::openapi::RiskRequest,
    responses((status = 200, description = "Risk report"), (status = 422, body = crate::openapi::ErrorDoc)))]
async fn risk_handler(State(st): State<AppState>, body: Result<Bytes, BytesRejection>) -> Response {
    post_json(&st, body, engine::risk).await
}

/// Benchmark the engine on a portfolio.
#[utoipa::path(post, path = "/api/v1/benchmark", tag = "engine", request_body = crate::openapi::BenchmarkRequest,
    responses((status = 200, description = "Benchmark report"), (status = 422, body = crate::openapi::ErrorDoc)))]
async fn benchmark_handler(State(st): State<AppState>, body: Result<Bytes, BytesRejection>) -> Response {
    post_json(&st, body, engine::benchmark_route).await
}

/// Run a stress scenario on a portfolio.
#[utoipa::path(post, path = "/api/v1/stress", tag = "engine", request_body = crate::openapi::StressRequest,
    responses((status = 200, description = "Stress report"), (status = 422, body = crate::openapi::ErrorDoc)))]
async fn stress_handler(State(st): State<AppState>, body: Result<Bytes, BytesRejection>) -> Response {
    post_json(&st, body, engine::stress).await
}

/// Which risk metric names are computable (not the Prometheus metrics).
#[utoipa::path(get, path = "/api/v1/metrics", tag = "engine", responses((status = 200, description = "Metric names")))]
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn metrics_handler() -> Response {
    respond(Ok(engine::metrics_route()))
}

/// A sample portfolio CSV.
#[utoipa::path(get, path = "/api/v1/sample-portfolio", tag = "engine", responses((status = 200, description = "`{csv}`")))]
#[allow(clippy::unused_async)] // axum handlers are async fns
async fn sample_portfolio() -> Response {
    respond(Ok(Json::object().with("csv", Json::string(SAMPLE_PORTFOLIO))))
}

// Telemetry samples /proc over a 100 ms window (and may run nvidia-smi), so it
// runs on the blocking pool too.
async fn telemetry_cpu(State(st): State<AppState>) -> Response {
    compute(&st, || Ok(telemetry::cpu())).await
}

async fn telemetry_workers(State(st): State<AppState>) -> Response {
    compute(&st, || Ok(telemetry::workers())).await
}

async fn telemetry_gpu(State(st): State<AppState>) -> Response {
    compute(&st, || Ok(telemetry::gpu())).await
}

#[allow(clippy::unused_async)] // axum handlers are async fns
async fn alpaca() -> Response {
    error_response(
        501,
        "The Alpaca brokerage proxy is not available in the Rust API yet: it needs an HTTPS client, which \
         has not been added.",
    )
}

#[allow(clippy::unused_async)] // axum handlers are async fns
async fn not_found() -> Response {
    error_response(404, "Not Found")
}

#[allow(clippy::unused_async)] // axum handlers are async fns
async fn method_not_allowed() -> Response {
    error_response(405, "Method not allowed.")
}

/// A route that answers 405 to other methods.
fn only(route: MethodRouter<AppState>) -> MethodRouter<AppState> {
    route.fallback(method_not_allowed)
}

/// The engine routes under `prefix` (`/api/v1`, or the old `/api`).
fn engine_routes(prefix: &str) -> Router<AppState> {
    Router::new()
        .route(&format!("{prefix}/risk"), only(post(risk_handler)))
        .route(&format!("{prefix}/benchmark"), only(post(benchmark_handler)))
        .route(&format!("{prefix}/stress"), only(post(stress_handler)))
        .route(&format!("{prefix}/metrics"), only(get(metrics_handler)))
        .route(&format!("{prefix}/sample-portfolio"), only(get(sample_portfolio)))
        .route(&format!("{prefix}/telemetry/cpu"), only(get(telemetry_cpu)))
        .route(&format!("{prefix}/telemetry/workers"), only(get(telemetry_workers)))
        .route(&format!("{prefix}/telemetry/gpu"), only(get(telemetry_gpu)))
        .route(&format!("{prefix}/alpaca"), any(alpaca))
        .route(&format!("{prefix}/alpaca/{{*rest}}"), any(alpaca))
}

/// The queued-job routes (`/api/v1` only; there was never an `/api` version).
fn job_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/jobs", only(get(jobs_api::list_jobs).post(jobs_api::create_job)))
        .route("/api/v1/jobs/{id}", only(get(jobs_api::get_job)))
        .route("/api/v1/jobs/{id}/cancel", only(post(jobs_api::cancel_job)))
}

/// The application. `db` is checked by `/ready`; pass `None` where there is
/// no database (unit tests), and `/ready` reports 503.
pub fn router(cfg: ApiConfig, db: Option<PgPool>) -> Router {
    let st = AppState { cfg: Arc::new(cfg), compute: Arc::new(Semaphore::new(MAX_CONCURRENT_COMPUTE)), db };
    let signed = engine_routes("/api/v1")
        .merge(engine_routes("/api"))
        .merge(job_routes())
        .route_layer(middleware::from_fn_with_state(st.clone(), require_signature));
    Router::new()
        .route("/", only(get(root)))
        .route("/health", only(get(health)))
        .route("/ready", only(get(ready)))
        .route("/metrics", only(get(prometheus_metrics)))
        .route("/api/v1/openapi.json", only(get(openapi_json)))
        .merge(signed)
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(reject_oversized))
        .layer(middleware::from_fn_with_state(st.clone(), cors))
        .layer(middleware::from_fn(request_id))
        .layer(middleware::from_fn(track_metrics))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .with_state(st)
}

/// Serves [`router`] on `listener` until `shutdown` resolves, then stops
/// accepting connections and waits for in-flight requests to finish.
///
/// # Errors
/// An I/O error from the listener.
pub async fn serve(
    listener: TcpListener,
    cfg: ApiConfig,
    db: Option<PgPool>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let sampler = db.clone().map(|pool| {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                observability::DatabaseMetrics::pool_stats(pool.size(), pool.num_idle());
                match spectra_db::jobs::queue_depth(&pool).await {
                    Ok((queued, running)) => observability::DatabaseMetrics::queue_depth(queued, running),
                    Err(error) => tracing::warn!(%error, "could not sample job queue depth"),
                }
            }
        })
    });
    let result = axum::serve(listener, router(cfg, db)).with_graceful_shutdown(shutdown).await;
    if let Some(sampler) = sampler {
        sampler.abort();
    }
    result
}

/// Resolves on Ctrl-C, or on SIGTERM on Unix (what `docker stop` sends).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
