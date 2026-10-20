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

/// Initialize JSON structured logging with tracing and Prometheus metrics.
/// Safe to call more than once (later calls change nothing).
pub fn init_observability() {
    let json_layer =
        fmt::layer().json().with_target(true).with_thread_ids(true).with_file(true).with_line_number(true);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Fails only if a subscriber is already set, which is fine.
    let _ = tracing_subscriber::registry().with(env_filter).with(json_layer).try_init();

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
    pub fn pool_stats(current: u32, idle: u32) {
        metrics::gauge!("db_pool_connections").set(f64::from(current));
        metrics::gauge!("db_pool_idle").set(f64::from(idle));
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
        let text = render_metrics().expect("recorder installed");
        assert!(text.contains("jobs_claimed_total"), "{text}");
    }
}
