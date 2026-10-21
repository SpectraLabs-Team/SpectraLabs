//! The OpenAPI description of the engine API, served at
//! `GET /api/v1/openapi.json` (generate a TypeScript client from it later,
//! M20 step 13).

use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

/// Every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDoc {
    /// Machine-readable: `invalid_request`, `unauthenticated`, `not_found`, `no_workspace`, ...
    pub code: String,
    /// For people.
    pub message: String,
    /// Extra structured information, or null.
    #[schema(value_type = Option<Object>)]
    pub details: Option<serde_json::Value>,
    /// Matches the `X-Request-Id` response header.
    pub request_id: String,
    /// Deprecated copy of `message`, removed with the `/api/...` aliases.
    pub detail: String,
}

/// `POST /api/v1/risk` body.
#[derive(Debug, Serialize, ToSchema)]
pub struct RiskRequest {
    /// `symbol,quantity,price[,volatility]` rows; up to 10 MB.
    pub portfolio_csv: String,
    /// Metric name (`var`, `cvar`, ...; see `/api/v1/metrics`). Default `var`.
    pub metric: Option<String>,
    /// 0.5..1, default 0.99.
    pub confidence: Option<f64>,
    /// 1,000..100,000,000, default 1,000,000.
    pub simulations: Option<u64>,
    /// RNG seed.
    pub seed: Option<u64>,
    /// `auto`, `cpu` or `parallel`.
    pub backend: Option<String>,
    /// Horizon in trading days, up to 7,560. Default 252.
    pub horizon_days: Option<f64>,
}

/// `POST /api/v1/benchmark` body (also the `benchmark` job payload).
#[derive(Debug, Serialize, ToSchema)]
pub struct BenchmarkRequest {
    /// Portfolio CSV.
    pub portfolio_csv: String,
    /// Paths per run, default 100,000.
    pub simulations: Option<u64>,
    /// 1..30, default 5.
    pub repetitions: Option<u32>,
    /// 0..5, default 1.
    pub warmups: Option<u32>,
}

/// `POST /api/v1/stress` body (also the `stress` job payload).
#[derive(Debug, Serialize, ToSchema)]
pub struct StressRequest {
    /// Portfolio CSV.
    pub portfolio_csv: String,
    /// Scenario name, default `market-crash`.
    pub scenario: Option<String>,
    /// Default 1,000,000.
    pub simulations: Option<u64>,
    /// RNG seed.
    pub seed: Option<u64>,
}

#[derive(Debug, OpenApi)]
#[openapi(
    info(
        title = "Spectra Labs engine API",
        version = "1",
        description = "Reached only through the dashboard's server. Every /api route needs the HMAC headers \
                       X-Spectra-User, X-Spectra-Workspace, X-Request-Id, X-Spectra-Timestamp and \
                       X-Spectra-Signature (see crates/spectra-api/src/auth.rs). Timestamps are UTC RFC 3339; \
                       ids are UUIDv7; lists page with an opaque cursor."
    ),
    paths(
        crate::health,
        crate::ready,
        crate::risk_handler,
        crate::benchmark_handler,
        crate::stress_handler,
        crate::metrics_handler,
        crate::sample_portfolio,
        crate::jobs_api::create_job,
        crate::jobs_api::list_jobs,
        crate::jobs_api::get_job,
        crate::jobs_api::cancel_job,
        crate::registry_api::list_experiments,
        crate::registry_api::create_experiment,
        crate::registry_api::get_experiment,
        crate::registry_api::get_lineage,
        crate::registry_api::create_backtest,
        crate::registry_api::list_runs,
        crate::registry_api::create_run,
        crate::registry_api::get_run,
        crate::registry_api::add_run_note,
        crate::registry_api::get_run_artifact,
        crate::registry_api::verify,
        crate::registry_api::export_jsonl,
        crate::registry_api::import_jsonl,
        crate::backtests_api::create_backtest,
        crate::backtests_api::list_backtests,
        crate::backtests_api::get_backtest,
        crate::backtests_api::cancel_backtest,
        crate::backtests_api::backtest_series,
        crate::backtests_api::backtest_artifact,
        crate::backtests_api::list_datasets,
        crate::backtests_api::builtin_strategies,
        crate::datasets_api::preview,
        crate::uploads_api::upload,
        crate::datasets_api::ingest,
        crate::datasets_api::get_dataset,
        crate::datasets_api::update_dataset,
        crate::datasets_api::get_version,
        crate::datasets_api::delete_version,
        crate::datasets_api::version_runs,
        crate::datasets_api::bars,
        crate::datasets_api::list_calendars,
        crate::sessions_api::limits,
        crate::sessions_api::create,
        crate::sessions_api::get,
        crate::sessions_api::chunk,
        crate::sessions_api::complete,
        crate::sessions_api::abort,
        crate::survival_api::start,
        crate::survival_api::list_for_run,
        crate::survival_api::get,
        crate::survival_api::text,
        crate::survival_api::list_presets,
        crate::survival_api::create_preset,
        crate::survival_api::update_preset,
    ),
    components(schemas(
        ErrorDoc,
        RiskRequest,
        BenchmarkRequest,
        StressRequest,
        crate::jobs_api::CreateJob,
        crate::jobs_api::JobView,
        crate::jobs_api::JobPage,
        crate::registry_api::CreateExperiment,
        crate::registry_api::CreateRegistryRun,
        crate::registry_api::CreateNote,
        crate::backtests_api::CreateBacktest,
        crate::backtests_api::BacktestView,
        crate::backtests_api::BacktestListItem,
        crate::backtests_api::BacktestPage,
        crate::backtests_api::DatasetVersionView,
        crate::backtests_api::DatasetList,
        crate::datasets_api::PreviewRequest,
        crate::datasets_api::IngestSettings,
        crate::datasets_api::IngestColumns,
        crate::datasets_api::QualityThresholds,
        crate::datasets_api::PreviewResponse,
        crate::datasets_api::Suggestions,
        crate::datasets_api::SuggestedColumns,
        crate::datasets_api::Preview,
        crate::datasets_api::PreviewRow,
        crate::datasets_api::QualityReport,
        crate::datasets_api::RowIssue,
        crate::datasets_api::SymbolStats,
        crate::uploads_api::UploadView,
        crate::datasets_api::IngestRequest,
        crate::datasets_api::DatasetSummaryView,
        crate::datasets_api::DatasetDetailView,
        crate::datasets_api::VersionEntryView,
        crate::datasets_api::VersionDetailView,
        crate::datasets_api::VersionRunView,
        crate::datasets_api::VersionRunList,
        crate::datasets_api::UpdateDataset,
        crate::datasets_api::DeletedVersionView,
        crate::datasets_api::BarsView,
        crate::datasets_api::BarPoint,
        crate::datasets_api::CalendarView,
        crate::datasets_api::CalendarsResponse,
        crate::sessions_api::UploadLimits,
        crate::sessions_api::CreateSession,
        crate::sessions_api::SessionView,
        crate::sessions_api::ChunkView,
        crate::survival_api::StartSurvival,
        crate::survival_api::SavePreset,
    )),
    tags(
        (name = "probes", description = "Liveness and readiness"),
        (name = "engine", description = "Synchronous engine calls"),
        (name = "jobs", description = "Queued engine work, run by spectra-worker"),
        (name = "registry", description = "Workspace-scoped experiment and run registry"),
        (name = "backtests", description = "Backtests: jobs of kind backtest, and the built-in strategies"),
        (name = "datasets", description = "Data Studio: ingest settings, previews and datasets"),
        (name = "survival", description = "Strategy Survival Lab reports and presets"),
    )
)]
pub struct ApiDoc;

/// The spec as JSON text.
pub fn spec_json() -> String {
    ApiDoc::openapi().to_json().unwrap_or_else(|_| "{}".to_string())
}

/// The spec as indented JSON (what `just api-client` commits, so diffs stay readable).
pub fn spec_pretty_json() -> String {
    ApiDoc::openapi().to_pretty_json().unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_spec_lists_every_documented_route() {
        let spec = super::spec_json();
        for path in [
            "/health",
            "/ready",
            "/api/v1/risk",
            "/api/v1/jobs",
            "/api/v1/jobs/{id}",
            "/api/v1/jobs/{id}/cancel",
            "/api/v1/experiments",
            "/api/v1/experiments/{id}",
            "/api/v1/experiments/{id}/lineage",
            "/api/v1/experiments/{id}/runs",
            "/api/v1/runs",
            "/api/v1/runs/{id}",
            "/api/v1/runs/{id}/notes",
            "/api/v1/runs/{id}/artifacts/{kind}",
            "/api/v1/registry/verify",
            "/api/v1/registry/export",
            "/api/v1/registry/import",
            "/api/v1/backtests",
            "/api/v1/backtests/{id}",
            "/api/v1/backtests/{id}/cancel",
            "/api/v1/backtests/{id}/series",
            "/api/v1/backtests/{id}/artifacts/{kind}",
            "/api/v1/datasets",
            "/api/v1/strategies/builtin",
            "/api/v1/datasets/preview",
            "/api/v1/uploads",
            "/api/v1/datasets/ingest",
            "/api/v1/datasets/{id}",
            "/api/v1/datasets/versions/{id}",
            "/api/v1/datasets/versions/{id}/runs",
            "/api/v1/datasets/versions/{id}/bars",
            "/api/v1/uploads/limits",
            "/api/v1/uploads/sessions",
            "/api/v1/uploads/sessions/{id}",
            "/api/v1/uploads/sessions/{id}/chunk",
            "/api/v1/uploads/sessions/{id}/complete",
        ] {
            assert!(spec.contains(&format!("\"{path}\"")), "missing {path}");
        }
    }
}
