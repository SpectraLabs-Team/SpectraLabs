//! Workspace-scoped experiment registry HTTP endpoints.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use spectra_engine::{config_fingerprint, BacktestConfig, RunRecord};
use spectra_registry::registry::RunInput;
use spectra_registry::{ExperimentInfo, PgRegistry, RunArtifact, RunFilter, RunInfo, RunPage};
use spectra_research::ParamSet;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{error_response, json_response, AppState, Identity};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateExperiment {
    pub name: String,
    #[serde(default)]
    pub hypothesis: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub parent_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRegistryRun {
    /// OHLCV CSV with timestamp, symbol, open, high, low, close and volume columns.
    pub csv: String,
    /// Short SMA period; 1 through 999 and less than `slow`.
    #[serde(default = "default_fast")]
    pub fast: u16,
    /// Long SMA period; 2 through 1,000 and greater than `fast`.
    #[serde(default = "default_slow")]
    pub slow: u16,
    /// Optional note recorded with the run.
    #[serde(default)]
    pub note: String,
    /// Job priority, -10 through 10.
    #[serde(default)]
    pub priority: i32,
    /// `POST /api/v1/runs` only: the experiment to record the run under.
    /// Omitted: the workspace's default "Unsorted" experiment.
    #[serde(default)]
    pub experiment_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNote {
    /// The note, 1 through 2,000 characters.
    pub text: String,
}

fn default_fast() -> u16 {
    5
}
fn default_slow() -> u16 {
    20
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ListRuns {
    pub experiment_id: Option<String>,
    pub dataset_id: Option<String>,
    pub strategy: Option<String>,
    pub kind: Option<String>,
    pub min_sharpe: Option<f64>,
    #[serde(default)]
    pub only_trials: bool,
    /// `queued`, `running`, `succeeded`, `failed` or `cancelled`.
    pub status: Option<String>,
    /// `next_cursor` from the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 through 200 (default 50).
    pub limit: Option<u32>,
}

#[allow(clippy::result_large_err)] // route helpers return the response immediately
fn registry(st: &AppState, identity: &Identity) -> Result<PgRegistry, Response> {
    let Some(pool) = st.db.clone() else {
        return Err(error_response(503, "The experiment registry requires a database."));
    };
    let Some(workspace_id) = identity.workspace else {
        return Err(error_response(400, "This account has no active workspace."));
    };
    Ok(PgRegistry::new(pool, workspace_id))
}

fn fail(error: &spectra_registry::PgRegistryError) -> Response {
    tracing::error!(error = %error, "experiment registry request failed");
    error_response(503, "The experiment registry could not complete the request.")
}

fn experiment_json(item: &ExperimentInfo, counts: Option<(u64, u64)>) -> Value {
    let (run_count, succeeded_count) = counts.unwrap_or_default();
    json!({
        "run_count": run_count,
        "succeeded_count": succeeded_count,
        "id": item.id,
        "name": item.name,
        "hypothesis": item.hypothesis,
        "tags": item.tags,
        "parent_id": item.parent_id,
        "created_ts": item.created_ts,
        "notes": item.notes,
    })
}

fn run_json(item: RunInfo) -> Value {
    let params: Map<String, Value> =
        item.params.values.into_iter().map(|(key, value)| (key, json!(value))).collect();
    json!({
        "id": item.id,
        "experiment_id": item.experiment_id,
        "kind": item.kind,
        "status": item.status,
        "job_id": item.job_id,
        "error": item.error,
        "dataset_id": item.dataset_id,
        "dataset_bars_sha256": item.dataset_bars_sha256,
        "strategy": item.strategy,
        "strategy_sha256": item.strategy_sha256,
        "params": params,
        "config": serde_json::from_str::<Value>(&item.config.dump()).unwrap_or(Value::Null),
        "config_sha256": item.config_sha256,
        "result_sha256": item.result_sha256,
        "engine_version": item.engine_version,
        "seed": item.seed,
        "bar_count": item.bar_count,
        "metrics": {
            "valid": item.metrics.valid,
            "periods": item.metrics.periods,
            "total_return": item.metrics.total_return,
            "annualized_return": item.metrics.annualized_return,
            "annualized_volatility": item.metrics.annualized_volatility,
            "sharpe": item.metrics.sharpe,
            "max_drawdown": item.metrics.max_drawdown,
        },
        "sharpe_per_period": item.sharpe_per_period,
        "n_obs": item.n_obs,
        "counts_as_trial": item.counts_as_trial,
        "recorded_ts": item.recorded_ts,
        "notes": item.notes,
    })
}

#[utoipa::path(get, path = "/api/v1/experiments", tag = "registry", responses((status = 200, description = "Workspace experiments")))]
pub(crate) async fn list_experiments(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let counts = match registry.run_counts().await {
        Ok(counts) => counts,
        Err(error) => return fail(&error),
    };
    match registry.experiments().await {
        Ok(items) => json_response(
            200,
            json!({
                "items": items
                    .iter()
                    .map(|item| experiment_json(item, counts.get(&item.id).copied()))
                    .collect::<Vec<_>>()
            })
            .to_string(),
        ),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(post, path = "/api/v1/experiments", tag = "registry", request_body = CreateExperiment, responses((status = 201, description = "Created"), (status = 422, description = "Invalid request", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn create_experiment(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let request: CreateExperiment = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return error_response(422, format!("Invalid experiment request: {error}")),
    };
    if request.name.trim().is_empty() || request.name.len() > 200 || request.hypothesis.len() > 10_000 {
        return error_response(
            422,
            "Name must be 1–200 characters and hypothesis at most 10,000 characters.",
        );
    }
    if request.tags.len() > 50 || request.tags.iter().any(|tag| tag.trim().is_empty() || tag.len() > 100) {
        return error_response(
            422,
            "Experiments support up to 50 non-empty tags, each at most 100 characters.",
        );
    }
    let tags: Vec<&str> = request.tags.iter().map(String::as_str).collect();
    let created_by = Uuid::parse_str(&identity.user).ok();
    match registry
        .create_experiment(&request.name, &request.hypothesis, &tags, &request.parent_id, created_by)
        .await
    {
        Ok(id) => json_response(201, json!({ "id": id }).to_string()),
        Err(spectra_registry::PgRegistryError::Invalid(message)) => error_response(422, message),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/experiments/{id}", tag = "registry", params(("id" = String, Path)), responses((status = 200, description = "Experiment"), (status = 404, description = "Not found", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_experiment(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match registry.experiment(&id).await {
        Ok(Some(item)) => {
            let counts = match registry.run_counts().await {
                Ok(counts) => counts.get(&item.id).copied(),
                Err(error) => return fail(&error),
            };
            json_response(200, experiment_json(&item, counts).to_string())
        }
        Ok(None) => error_response(404, "Experiment not found."),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/experiments/{id}/lineage", tag = "registry", params(("id" = String, Path)), responses((status = 200, description = "Root-to-leaf experiment lineage")))]
pub(crate) async fn get_lineage(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match registry.lineage(&id).await {
        Ok(items) => json_response(200, json!({ "items": items }).to_string()),
        Err(spectra_registry::PgRegistryError::Invalid(_)) => error_response(404, "Experiment not found."),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(post, operation_id = "create_registry_run", path = "/api/v1/experiments/{id}/runs", tag = "registry", params(("id" = String, Path)), request_body = CreateRegistryRun, responses((status = 202, description = "Backtest queued"), (status = 422, description = "Invalid dataset or request", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn create_backtest(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(experiment_id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = match parse_backtest(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if request.experiment_id.as_deref().is_some_and(|id| id != experiment_id) {
        return error_response(422, "experiment_id in the body does not match the URL.");
    }
    queue_backtest(&registry, &identity, experiment_id, request).await
}

#[utoipa::path(post, path = "/api/v1/runs", tag = "registry", request_body = CreateRegistryRun, responses((status = 202, description = "Backtest queued; without experiment_id it is recorded under \"Unsorted\""), (status = 422, description = "Invalid dataset or request", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn create_run(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let mut request = match parse_backtest(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let experiment_id = match request.experiment_id.take().filter(|id| !id.trim().is_empty()) {
        Some(id) => id,
        None => match registry.default_experiment(Uuid::parse_str(&identity.user).ok()).await {
            Ok(id) => id,
            Err(error) => return fail(&error),
        },
    };
    queue_backtest(&registry, &identity, experiment_id, request).await
}

#[allow(clippy::result_large_err)] // route helpers return the response immediately
fn parse_backtest(body: Result<Bytes, BytesRejection>) -> Result<CreateRegistryRun, Response> {
    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the backtest request body."));
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| error_response(422, format!("Invalid backtest request: {error}")))
}

async fn queue_backtest(
    registry: &PgRegistry,
    identity: &Identity,
    experiment_id: String,
    request: CreateRegistryRun,
) -> Response {
    if request.csv.trim().is_empty() || request.csv.len() > 10_000_000 {
        return error_response(422, "Dataset CSV must be non-empty and no larger than 10 MB.");
    }
    if request.note.len() > 2_000 || !(-10..=10).contains(&request.priority) {
        return error_response(
            422,
            "Note must be at most 2,000 characters and priority must be between -10 and 10.",
        );
    }
    let payload = json!({
        "csv": request.csv,
        "experiment_id": experiment_id.clone(),
        "fast": request.fast,
        "slow": request.slow,
        "note": request.note.clone(),
    });
    let payload_text = payload.to_string();
    let validate = tokio::task::spawn_blocking(move || {
        let parsed = spectra_core::Json::parse(&payload_text).map_err(|error| (422, error.to_string()))?;
        crate::engine::validate_job("backtest", &parsed)?;
        let csv = parsed
            .at("csv")
            .and_then(spectra_core::Json::as_str)
            .map_err(|error| (422, error.to_string()))?;
        let dataset = spectra_data::load_csv(csv.as_bytes(), &spectra_data::CsvOptions::default())
            .map_err(|error| (422, format!("Dataset CSV is invalid: {error}")))?;
        Ok::<_, (u16, String)>((dataset.info.id, dataset.info.bars_sha256, dataset.info.bar_count))
    })
    .await;
    let (dataset_id, dataset_sha256, bar_count) = match validate {
        Ok(Ok(info)) => info,
        Ok(Err((status, message))) => return error_response(status, message),
        Err(_) => return error_response(500, "Could not validate the backtest dataset."),
    };
    let config = BacktestConfig::default();
    let fast = f64::from(request.fast);
    let slow = f64::from(request.slow);
    let strategy_sha256 = spectra_core::sha256_hex(b"spectra:builtin:sma-cross:v1");
    let input = RunInput {
        experiment_id: experiment_id.clone(),
        kind: "backtest".into(),
        dataset_id,
        strategy: "sma_cross".into(),
        strategy_sha256,
        params: ParamSet::from_pairs([("fast", fast), ("slow", slow)]),
        config,
        run: RunRecord {
            engine_version: spectra_core::ENGINE_VERSION.to_owned(),
            dataset_sha256,
            config_sha256: config_fingerprint(&config),
            result_sha256: "0".repeat(64),
            seed: config.seed,
            bar_count,
        },
        metrics: spectra_engine::Metrics::default(),
        returns: Vec::new(),
        counts_as_trial: true,
        note: request.note,
    };
    let new_job = spectra_db::jobs::NewJob {
        workspace_id: registry.workspace_id(),
        kind: "backtest".into(),
        payload,
        priority: request.priority,
        max_attempts: crate::jobs_api::MAX_ATTEMPTS,
        created_by: Uuid::parse_str(&identity.user).ok(),
    };
    match registry.enqueue_run(&input, &new_job).await {
        Ok((job, run_id)) => json_response(
            202,
            json!({
                "job": serde_json::to_value(crate::jobs_api::JobView::from(job)).unwrap_or(Value::Null),
                "run_id": run_id,
            })
            .to_string(),
        ),
        Err(spectra_registry::PgRegistryError::Invalid(message)) => error_response(422, message),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/runs", tag = "registry", params(ListRuns), responses((status = 200, description = "Workspace runs")))]
pub(crate) async fn list_runs(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Query(query): Query<ListRuns>,
) -> Response {
    if query.min_sharpe.is_some_and(|value| !value.is_finite()) {
        return error_response(422, "min_sharpe must be a finite number.");
    }
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let filter = RunFilter {
        experiment_id: query.experiment_id,
        dataset_id: query.dataset_id,
        strategy: query.strategy,
        kind: query.kind,
        min_sharpe: query.min_sharpe,
        only_trials: query.only_trials,
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return error_response(422, "limit must be between 1 and 200.");
    }
    let page = RunPage { status: query.status, cursor: query.cursor, limit };
    match registry.runs_page(&filter, &page).await {
        Ok((items, next)) => json_response(
            200,
            json!({ "items": items.into_iter().map(run_json).collect::<Vec<_>>(), "next_cursor": next })
                .to_string(),
        ),
        Err(spectra_registry::PgRegistryError::Invalid(message)) => error_response(422, message),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/runs/{id}", tag = "registry", params(("id" = String, Path)), responses((status = 200, description = "Run"), (status = 404, description = "Not found", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_run(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let item = match registry.run(&id).await {
        Ok(Some(item)) => item,
        Ok(None) => return error_response(404, "Run not found."),
        Err(error) => return fail(&error),
    };
    let artifacts = match registry.run_artifacts(&id).await {
        Ok(items) => items,
        Err(error) => return fail(&error),
    };
    let trials = match registry.trial_count(None).await {
        Ok(count) => count,
        Err(error) => return fail(&error),
    };
    let mut body = run_json(item);
    body["artifacts"] = Value::Array(artifacts.iter().map(artifact_json).collect());
    body["workspace_trial_count"] = json!(trials);
    json_response(200, body.to_string())
}

fn artifact_json(item: &RunArtifact) -> Value {
    json!({
        "id": item.id,
        "kind": item.kind,
        "sha256": item.sha256,
        "size_bytes": item.size_bytes,
        "content_type": item.content_type,
        "summary": serde_json::from_str::<Value>(&item.summary.dump()).unwrap_or(Value::Null),
    })
}

#[utoipa::path(post, path = "/api/v1/runs/{id}/notes", tag = "registry", params(("id" = String, Path)), request_body = CreateNote, responses((status = 201, description = "Note recorded"), (status = 404, description = "Not found", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn add_run_note(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let request: CreateNote = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return error_response(422, format!("Invalid note: {error}")),
    };
    if request.text.trim().is_empty() || request.text.len() > 2_000 {
        return error_response(422, "A note must be 1–2,000 characters.");
    }
    match registry.run(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, "Run not found."),
        Err(error) => return fail(&error),
    }
    match registry.add_note(&id, request.text.trim(), Uuid::parse_str(&identity.user).ok()).await {
        Ok(()) => json_response(201, json!({ "ok": true }).to_string()),
        Err(spectra_registry::PgRegistryError::Invalid(message)) => error_response(422, message),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/runs/{id}/artifacts/{kind}", tag = "registry", params(("id" = String, Path), ("kind" = String, Path)), responses((status = 200, description = "The artifact's bytes"), (status = 404, description = "Not found", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_run_artifact(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path((id, kind)): Path<(String, String)>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(store) = st.cfg.artifacts.clone() else {
        return error_response(503, "Artifact storage is not configured on this server.");
    };
    let artifact = match registry.run_artifacts(&id).await {
        Ok(items) => items.into_iter().rev().find(|item| item.kind == kind),
        Err(error) => return fail(&error),
    };
    let Some(artifact) = artifact else { return error_response(404, "Artifact not found.") };
    // The store re-hashes the bytes, so a corrupted file is an error, never a download.
    let bytes = match store.get(&artifact.sha256).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return error_response(404, "The artifact's bytes are no longer stored.")
        }
        Err(error) => {
            tracing::error!(error = %error, sha256 = %artifact.sha256, "could not read artifact");
            return error_response(503, "The artifact could not be read.");
        }
    };
    let extension = match artifact.content_type.as_str() {
        "application/json" => "json",
        "text/csv" => "csv",
        _ => "bin",
    };
    let safe_kind: String =
        kind.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(64).collect();
    let safe_id: String = id.chars().filter(|c| c.is_ascii_hexdigit() || *c == '-').take(64).collect();
    Response::builder()
        .status(200)
        .header("content-type", artifact.content_type.as_str())
        .header(
            "content-disposition",
            format!("attachment; filename=\"run-{safe_id}-{safe_kind}.{extension}\""),
        )
        .header("x-content-sha256", artifact.sha256.as_str())
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| error_response(500, "Could not build the artifact response."))
}

#[utoipa::path(get, path = "/api/v1/registry/verify", tag = "registry", responses((status = 200, description = "Registry chain verification")))]
pub(crate) async fn verify(State(st): State<AppState>, Extension(identity): Extension<Identity>) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match registry.verify().await {
        Ok(report) => json_response(200, json!({ "ok": report.ok, "events": report.events, "last_hash": report.last_hash, "detail": report.detail }).to_string()),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(get, path = "/api/v1/registry/export", tag = "registry", responses((status = 200, description = "Registry JSONL export")))]
pub(crate) async fn export_jsonl(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match registry.export_jsonl().await {
        Ok(text) => Response::builder()
            .status(200)
            .header("content-type", "application/x-ndjson; charset=utf-8")
            .body(axum::body::Body::from(text))
            .unwrap_or_else(|_| error_response(500, "Could not build the export response.")),
        Err(error) => fail(&error),
    }
}

#[utoipa::path(post, path = "/api/v1/registry/import", tag = "registry", responses((status = 200, description = "Imported event count"), (status = 409, description = "Workspace is not empty", body = crate::openapi::ErrorDoc), (status = 422, description = "Invalid or corrupt JSONL", body = crate::openapi::ErrorDoc)))]
pub(crate) async fn import_jsonl(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let registry = match registry(&st, &identity) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Ok(bytes) = body else {
        return error_response(400, "Could not read the registry import body.");
    };
    let Ok(input) = std::str::from_utf8(&bytes) else {
        return error_response(422, "Registry JSONL must be valid UTF-8.");
    };
    match registry.import_jsonl(input).await {
        Ok(events) => json_response(200, json!({ "events_imported": events }).to_string()),
        Err(spectra_registry::PgRegistryError::Invalid(message))
            if message == "registry import requires an empty workspace" =>
        {
            error_response(409, message)
        }
        Err(
            spectra_registry::PgRegistryError::Invalid(message)
            | spectra_registry::PgRegistryError::Integrity(message)
            | spectra_registry::PgRegistryError::Registry(spectra_core::Error::Registry(message)),
        ) => error_response(422, message),
        Err(error) => fail(&error),
    }
}
