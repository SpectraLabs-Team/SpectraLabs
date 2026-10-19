//! Structured logging with tracing and Prometheus metrics for observability (M20b step 11).

use metrics::{counter, histogram};
use tracing_subscriber::prelude::*;
use tracing_subscriber::fmt;

/// Initialize JSON structured logging with tracing and Prometheus metrics.
/// Logs include request_id and job_id context.
pub fn init_observability() {
    // Initialize structured logging
    let json_layer = fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .init();

    // Initialize Prometheus metrics recorder
    match metrics_exporter_prometheus::PrometheusBuilder::new().install_recorder() {
        Ok(_) => tracing::info!("prometheus metrics initialized"),
        Err(e) => tracing::warn!("failed to initialize prometheus: {}", e),
    }

    tracing::info!("observability initialized (tracing + prometheus)");
}

/// Metrics for request handling.
pub struct RequestMetrics;

impl RequestMetrics {
    /// Record request processing time (in seconds).
    pub fn request_duration(method: &str, path: &str, status: u16, duration_secs: f64) {
        histogram!("api_request_duration_seconds", "method" => method, "path" => path, "status" => status.to_string())
            .record(duration_secs);
        counter!("api_responses_total", "method" => method, "path" => path, "status" => status.to_string()).increment(1);
    }
}

/// Metrics for database operations.
pub struct DatabaseMetrics;

impl DatabaseMetrics {
    /// Record database pool stats: current connections, idle connections.
    pub fn pool_stats(current: u32, idle: u32) {
        let _ = metrics::gauge!("db_pool_connections", current as f64);
        let _ = metrics::gauge!("db_pool_idle", idle as f64);
    }

    /// Record a database query duration.
    pub fn query_duration(query_name: &str, duration_secs: f64) {
        histogram!("db_query_duration_seconds", "query" => query_name.to_string())
            .record(duration_secs);
    }
}

/// Metrics for job queue operations.
pub struct JobMetrics;

impl JobMetrics {
    /// Record job execution duration.
    pub fn job_duration(kind: &str, status: &str, duration_secs: f64) {
        histogram!("job_duration_seconds", "kind" => kind, "status" => status).record(duration_secs);
    }

    /// Record job completion.
    pub fn job_completed(kind: &str, status: &str) {
        counter!("jobs_completed_total", "kind" => kind, "status" => status).increment(1);
    }

    /// Record job claimed.
    pub fn job_claimed(kind: &str) {
        counter!("jobs_claimed_total", "kind" => kind).increment(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_initializes() {
        init_observability();
        tracing::info!("test log");
    }
}
