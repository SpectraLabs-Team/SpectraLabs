//! Structured logging with tracing and Prometheus metrics for observability (M20b step 11).

use std::sync::OnceLock;

use metrics::{counter, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Histogram buckets in seconds, from fast API calls to long benchmark jobs.
/// With buckets set, histograms export as `_bucket` series, so
/// `histogram_quantile` works across API and worker replicas.
const BUCKETS: &[f64] =
    &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0];

/// The installed Prometheus recorder, rendered by `GET /metrics`.
static PROMETHEUS: OnceLock<PrometheusHandle> = OnceLock::new();
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();

/// Initialize JSON structured logging with tracing and Prometheus metrics.
/// Safe to call more than once (later calls change nothing).
pub fn init_observability() {
    let json_layer =
        fmt::layer().json().with_target(true).with_thread_ids(true).with_file(true).with_line_number(true);
    let _ = SENTRY_GUARD.get_or_init(|| {
        let dsn = std::env::var("SENTRY_DSN").ok().filter(|s| !s.trim().is_empty());
        let mut opts = sentry::ClientOptions::default();
        opts.dsn = dsn.and_then(|d| d.parse().ok());
        opts.release = sentry::release_name!();
        sentry::init(sentry::apply_defaults(opts))
    });
    let sentry_layer = sentry::integrations::tracing::layer().event_filter(|metadata| {
        if *metadata.level() == tracing::Level::ERROR {
            sentry::integrations::tracing::EventFilter::Event
        } else {
            sentry::integrations::tracing::EventFilter::Breadcrumb
        }
    });

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Fails only if a subscriber is already set, which is fine.
    let _ = tracing_subscriber::registry().with(env_filter).with(json_layer).with(sentry_layer).try_init();

    if PROMETHEUS.get().is_none() {
        let builder = match PrometheusBuilder::new().set_buckets(BUCKETS) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("histogram buckets rejected, exporting summaries: {e}");
                PrometheusBuilder::new()
            }
        };
        match builder.install_recorder() {
            Ok(handle) => {
                let _ = PROMETHEUS.set(handle);
                tracing::info!("prometheus metrics initialized");
            }
            Err(e) => tracing::warn!("failed to initialize prometheus: {}", e),
        }
    }
}

/// The current metrics in the Prometheus text format, or `None` before
/// [`init_observability`] has run.
pub fn render_metrics() -> Option<String> {
    PROMETHEUS.get().map(PrometheusHandle::render)
}

/// Returns true if Sentry is currently initialized and enabled with a valid DSN.
pub fn is_sentry_enabled() -> bool {
    sentry::Hub::current().client().is_some_and(|c| c.is_enabled())
}

/// Sentry delivery status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SentryStatus {
    pub enabled: bool,
    pub dsn_configured: bool,
    pub release: Option<String>,
}

/// Return current Sentry status.
pub fn sentry_status() -> SentryStatus {
    let dsn_configured = std::env::var("SENTRY_DSN").is_ok_and(|s| !s.trim().is_empty());
    SentryStatus {
        enabled: is_sentry_enabled(),
        dsn_configured,
        release: sentry::release_name!().map(Into::into),
    }
}

/// Verification result of sending an event to Sentry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SentryVerifyOutcome {
    pub enabled: bool,
    pub event_id: Option<String>,
    pub message: String,
    pub delivered: bool,
}

/// Sends a test verification message to Sentry and flushes the client.
pub fn verify_sentry_delivery(custom_message: Option<&str>) -> SentryVerifyOutcome {
    let dsn_configured = std::env::var("SENTRY_DSN").is_ok_and(|s| !s.trim().is_empty());
    if !is_sentry_enabled() {
        return SentryVerifyOutcome {
            enabled: false,
            event_id: None,
            message: if dsn_configured {
                "SENTRY_DSN was set but could not be initialized or parsed.".into()
            } else {
                "SENTRY_DSN is not configured; error reporting is disabled.".into()
            },
            delivered: false,
        };
    }

    let msg = custom_message.unwrap_or("Spectra Labs Sentry delivery verification test event");
    let event_id = sentry::capture_message(msg, sentry::Level::Info);
    let delivered = if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(2)))
    } else {
        false
    };

    SentryVerifyOutcome {
        enabled: true,
        event_id: Some(event_id.to_string()),
        message: format!("Verification event successfully dispatched: {msg}"),
        delivered,
    }
}

/// Metrics for request handling.
#[derive(Debug)]
pub struct RequestMetrics;

impl RequestMetrics {
    /// Record request processing time (in seconds).
    pub fn request_duration(method: &str, path: &str, status: u16, duration_secs: f64) {
        histogram!("api_request_duration_seconds", "method" => method.to_string(), "path" => path.to_string(), "status" => status.to_string())
            .record(duration_secs);
        counter!("api_responses_total", "method" => method.to_string(), "path" => path.to_string(), "status" => status.to_string())
            .increment(1);
    }
}

/// Metrics for database operations.
#[derive(Debug)]
pub struct DatabaseMetrics;

impl DatabaseMetrics {
    /// Record database pool stats: current connections, idle connections.
    pub fn pool_stats(current: u32, idle: usize) {
        metrics::gauge!("db_pool_connections").set(f64::from(current));
        metrics::gauge!("db_pool_idle").set(f64::from(u32::try_from(idle).unwrap_or(u32::MAX)));
    }

    /// Record current queue depth without workspace or job ID labels.
    pub fn queue_depth(queued: i64, running: i64) {
        metrics::gauge!("jobs_queue_depth", "status" => "queued").set(queued as f64);
        metrics::gauge!("jobs_queue_depth", "status" => "running").set(running as f64);
    }

    /// Record a database query duration.
    pub fn query_duration(query_name: &str, duration_secs: f64) {
        histogram!("db_query_duration_seconds", "query" => query_name.to_string()).record(duration_secs);
    }
}

/// Metrics for job queue operations.
#[derive(Debug)]
pub struct JobMetrics;

impl JobMetrics {
    /// Record job execution duration.
    pub fn job_duration(kind: &str, status: &str, duration_secs: f64) {
        histogram!("job_duration_seconds", "kind" => kind.to_string(), "status" => status.to_string())
            .record(duration_secs);
    }

    /// Record job completion.
    pub fn job_completed(kind: &str, status: &str) {
        counter!("jobs_completed_total", "kind" => kind.to_string(), "status" => status.to_string())
            .increment(1);
    }

    /// Record job claimed.
    pub fn job_claimed(kind: &str) {
        counter!("jobs_claimed_total", "kind" => kind.to_string()).increment(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent_and_metrics_render() {
        init_observability();
        init_observability();
        JobMetrics::job_claimed("risk");
        DatabaseMetrics::pool_stats(10, 4);
        DatabaseMetrics::queue_depth(3, 2);
        let text = render_metrics().expect("recorder installed");
        assert!(text.contains("jobs_claimed_total"), "{text}");
        assert!(text.contains("db_pool_connections"), "{text}");
        assert!(text.contains("jobs_queue_depth"), "{text}");
    }
}
