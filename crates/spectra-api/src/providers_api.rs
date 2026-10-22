//! Market data provider configs and refresh schedules API routes (M1b task 13).

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::lifecycle_api::DeletedItemView;
use crate::{error_response, invalid_fields, AppState, Identity};

const fn default_interval() -> i64 {
    86400
}

const fn default_lookback() -> i32 {
    5
}

const fn default_true() -> bool {
    true
}

// ------------------------------------------------------------- provider configs schemas

/// Request to create or update a provider configuration.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpsertProviderConfigRequest {
    /// Provider configuration name (e.g. "Alpaca Paper Equities").
    pub name: String,
    /// Provider kind ("fake", "alpaca").
    pub provider: String,
    /// Optional vault secret id for API credentials.
    pub secret_id: Option<Uuid>,
    /// Provider settings (feed, endpoints, etc.).
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

/// Provider configuration view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProviderConfigView {
    /// Config id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Provider kind.
    pub provider: String,
    /// Vault secret id, if configured.
    pub secret_id: Option<Uuid>,
    /// Settings.
    pub settings: serde_json::Value,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last updated.
    pub updated_at: DateTime<Utc>,
}

/// List of provider configurations.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProviderConfigListView {
    /// Config items.
    pub items: Vec<ProviderConfigView>,
}

// ------------------------------------------------------------- refresh schedule schemas

/// Request to create or update a dataset refresh schedule.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpsertRefreshScheduleRequest {
    /// Associated provider config id.
    pub provider_config_id: Uuid,
    /// Symbols to refresh.
    pub symbols: Vec<String>,
    /// Interval in seconds between refreshes (minimum 60s, default 86400s).
    #[serde(default = "default_interval")]
    pub interval_seconds: i64,
    /// Number of bars to refetch backward for revision overlap detection (default 5).
    #[serde(default = "default_lookback")]
    pub lookback_bars: i32,
    /// Whether schedule is enabled (default true).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Refresh schedule view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RefreshScheduleView {
    /// Schedule id.
    pub id: Uuid,
    /// Target dataset id.
    pub dataset_id: Uuid,
    /// Associated provider config id.
    pub provider_config_id: Uuid,
    /// Symbols.
    pub symbols: Vec<String>,
    /// Interval in seconds.
    pub interval_seconds: i64,
    /// Lookback bars.
    pub lookback_bars: i32,
    /// Enabled.
    pub enabled: bool,
    /// When last run.
    pub last_run_at: Option<DateTime<Utc>>,
    /// When next run is scheduled.
    pub next_run_at: DateTime<Utc>,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// When last updated.
    pub updated_at: DateTime<Utc>,
}

/// Request to trigger a manual refresh.
#[derive(Debug, Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct TriggerRefreshRequest {
    /// Optional provider config id override.
    pub provider_config_id: Option<Uuid>,
    /// Optional symbols override.
    pub symbols: Option<Vec<String>>,
    /// Optional lookback bars override.
    pub lookback_bars: Option<i32>,
}

/// Response returned when a refresh job is queued.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TriggerRefreshResponse {
    /// Queued job id.
    pub job_id: Uuid,
    /// Status ("queued").
    pub status: String,
}

// ------------------------------------------------------------- provider configs handlers

/// List provider configurations for the active workspace.
#[utoipa::path(get, path = "/api/v1/providers/configs", tag = "providers",
    responses((status = 200, body = ProviderConfigListView)))]
pub(crate) async fn list_provider_configs(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::providers::list_provider_configs(&pool, ws).await {
        Ok(rows) => to_json(
            200,
            &ProviderConfigListView {
                items: rows
                    .into_iter()
                    .map(|r| ProviderConfigView {
                        id: r.id,
                        name: r.name,
                        provider: r.provider,
                        secret_id: r.secret_id,
                        settings: r.settings,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Get a provider configuration by id.
#[utoipa::path(get, path = "/api/v1/providers/configs/{id}", tag = "providers",
    params(("id" = Uuid, Path, description = "Provider config id")),
    responses(
        (status = 200, body = ProviderConfigView),
        (status = 404, description = "Not found")
    ))]
pub(crate) async fn get_provider_config(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(cfg_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::providers::get_provider_config(&pool, ws, cfg_id).await {
        Ok(Some(r)) => to_json(
            200,
            &ProviderConfigView {
                id: r.id,
                name: r.name,
                provider: r.provider,
                secret_id: r.secret_id,
                settings: r.settings,
                created_at: r.created_at,
                updated_at: r.updated_at,
            },
        ),
        Ok(None) => error_response(404, "Provider configuration not found."),
        Err(e) => db_error(&e),
    }
}

/// Create or update a provider configuration (editor only).
#[utoipa::path(post, path = "/api/v1/providers/configs", tag = "providers",
    request_body = UpsertProviderConfigRequest,
    responses(
        (status = 200, body = ProviderConfigView),
        (status = 422, description = "Validation error"),
        (status = 403, description = "Forbidden")
    ))]
pub(crate) async fn upsert_provider_config(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) => return error_response(400, format!("invalid request body: {e}")),
    };
    let req: UpsertProviderConfigRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("malformed JSON request: {e}")),
    };

    let mut errors = Vec::new();
    if req.name.trim().is_empty() {
        errors.push(("name".into(), "name cannot be empty".into()));
    }
    if req.provider != "fake" && req.provider != "alpaca" {
        errors.push(("provider".into(), "provider must be 'fake' or 'alpaca'".into()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let user_id = Uuid::parse_str(&id.user).ok();
    let settings = req.settings.unwrap_or(serde_json::json!({}));

    match spectra_db::providers::upsert_provider_config(
        &pool,
        ws,
        &req.name,
        &req.provider,
        req.secret_id,
        &settings,
        user_id,
    )
    .await
    {
        Ok(r) => to_json(
            200,
            &ProviderConfigView {
                id: r.id,
                name: r.name,
                provider: r.provider,
                secret_id: r.secret_id,
                settings: r.settings,
                created_at: r.created_at,
                updated_at: r.updated_at,
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Delete a provider configuration (editor only).
#[utoipa::path(delete, path = "/api/v1/providers/configs/{id}", tag = "providers",
    params(("id" = Uuid, Path, description = "Provider config id")),
    responses(
        (status = 200, body = DeletedItemView),
        (status = 404, description = "Not found"),
        (status = 403, description = "Forbidden")
    ))]
pub(crate) async fn delete_provider_config(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(cfg_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::providers::delete_provider_config(&pool, ws, cfg_id).await {
        Ok(true) => to_json(200, &DeletedItemView { deleted: true }),
        Ok(false) => error_response(404, "Provider configuration not found."),
        Err(e) => db_error(&e),
    }
}

// ------------------------------------------------------------- refresh schedule handlers

/// Get the refresh schedule for a dataset.
#[utoipa::path(get, path = "/api/v1/datasets/{id}/refresh-schedule", tag = "datasets",
    params(("id" = Uuid, Path, description = "Dataset id")),
    responses(
        (status = 200, body = RefreshScheduleView),
        (status = 404, description = "Not found")
    ))]
pub(crate) async fn get_refresh_schedule(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(dataset_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::providers::get_refresh_schedule(&pool, ws, dataset_id).await {
        Ok(Some(r)) => to_json(
            200,
            &RefreshScheduleView {
                id: r.id,
                dataset_id: r.dataset_id,
                provider_config_id: r.provider_config_id,
                symbols: r.symbols,
                interval_seconds: r.interval_seconds,
                lookback_bars: r.lookback_bars,
                enabled: r.enabled,
                last_run_at: r.last_run_at,
                next_run_at: r.next_run_at,
                created_at: r.created_at,
                updated_at: r.updated_at,
            },
        ),
        Ok(None) => error_response(404, "Dataset refresh schedule not found."),
        Err(e) => db_error(&e),
    }
}

/// Create or update a dataset refresh schedule (editor only).
#[utoipa::path(post, path = "/api/v1/datasets/{id}/refresh-schedule", tag = "datasets",
    params(("id" = Uuid, Path, description = "Dataset id")),
    request_body = UpsertRefreshScheduleRequest,
    responses(
        (status = 200, body = RefreshScheduleView),
        (status = 422, description = "Validation error"),
        (status = 403, description = "Forbidden")
    ))]
pub(crate) async fn upsert_refresh_schedule(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(dataset_id): Path<Uuid>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let bytes = match body {
        Ok(b) => b,
        Err(e) => return error_response(400, format!("invalid request body: {e}")),
    };
    let req: UpsertRefreshScheduleRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("malformed JSON request: {e}")),
    };

    let mut errors = Vec::new();
    if req.symbols.is_empty() {
        errors.push(("symbols".into(), "symbols array cannot be empty".into()));
    }
    for (i, s) in req.symbols.iter().enumerate() {
        if s.trim().is_empty() {
            errors.push((format!("symbols[{i}]"), "symbol cannot be empty".into()));
        }
    }
    let interval = req.interval_seconds;
    if interval < 60 {
        errors.push(("interval_seconds".into(), "interval_seconds must be at least 60".into()));
    }
    let lookback = req.lookback_bars;
    if lookback < 0 {
        errors.push(("lookback_bars".into(), "lookback_bars cannot be negative".into()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let user_id = Uuid::parse_str(&id.user).ok();
    let enabled = req.enabled;

    match spectra_db::providers::upsert_refresh_schedule(
        &pool,
        ws,
        dataset_id,
        req.provider_config_id,
        &req.symbols,
        interval,
        lookback,
        enabled,
        user_id,
    )
    .await
    {
        Ok(r) => to_json(
            200,
            &RefreshScheduleView {
                id: r.id,
                dataset_id: r.dataset_id,
                provider_config_id: r.provider_config_id,
                symbols: r.symbols,
                interval_seconds: r.interval_seconds,
                lookback_bars: r.lookback_bars,
                enabled: r.enabled,
                last_run_at: r.last_run_at,
                next_run_at: r.next_run_at,
                created_at: r.created_at,
                updated_at: r.updated_at,
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Delete a dataset refresh schedule (editor only).
#[utoipa::path(delete, path = "/api/v1/datasets/{id}/refresh-schedule", tag = "datasets",
    params(("id" = Uuid, Path, description = "Dataset id")),
    responses(
        (status = 200, body = DeletedItemView),
        (status = 404, description = "Not found"),
        (status = 403, description = "Forbidden")
    ))]
pub(crate) async fn delete_refresh_schedule(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(dataset_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };

    let sched = match spectra_db::providers::get_refresh_schedule(&pool, ws, dataset_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, "Dataset refresh schedule not found."),
        Err(e) => return db_error(&e),
    };

    match spectra_db::providers::delete_refresh_schedule(&pool, ws, sched.id).await {
        Ok(true) => to_json(200, &DeletedItemView { deleted: true }),
        Ok(false) => error_response(404, "Dataset refresh schedule not found."),
        Err(e) => db_error(&e),
    }
}

/// Trigger a manual refresh for a dataset (editor only).
#[utoipa::path(post, path = "/api/v1/datasets/{id}/refresh", tag = "datasets",
    params(("id" = Uuid, Path, description = "Dataset id")),
    request_body = TriggerRefreshRequest,
    responses(
        (status = 202, body = TriggerRefreshResponse),
        (status = 404, description = "Dataset or schedule not found"),
        (status = 403, description = "Forbidden")
    ))]
pub(crate) async fn trigger_refresh(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(dataset_id): Path<Uuid>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };

    let req: TriggerRefreshRequest = match body {
        Ok(b) if !b.is_empty() => match serde_json::from_slice(&b) {
            Ok(r) => r,
            Err(e) => return error_response(422, format!("malformed JSON request: {e}")),
        },
        _ => TriggerRefreshRequest::default(),
    };

    // If provider_config_id or symbols omitted, resolve from schedule or dataset
    let (provider_config_id, symbols, lookback_bars) = match (
        req.provider_config_id,
        req.symbols,
        req.lookback_bars,
    ) {
        (Some(cfg_id), Some(syms), lb) => (cfg_id, syms, lb.unwrap_or(5)),
        (maybe_cfg, maybe_syms, lb) => {
            let sched = match spectra_db::providers::get_refresh_schedule(&pool, ws, dataset_id).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return error_response(
                                404,
                                "No refresh schedule configured for dataset; provider_config_id and symbols are required.",
                            );
                }
                Err(e) => return db_error(&e),
            };
            (
                maybe_cfg.unwrap_or(sched.provider_config_id),
                maybe_syms.unwrap_or(sched.symbols),
                lb.unwrap_or(sched.lookback_bars),
            )
        }
    };

    let user_id = Uuid::parse_str(&id.user).ok();
    let payload = serde_json::json!({
        "dataset_id": dataset_id,
        "provider_config_id": provider_config_id,
        "symbols": symbols,
        "lookback_bars": lookback_bars,
    });

    let new_job = spectra_db::jobs::NewJob {
        workspace_id: ws,
        kind: crate::providers::runner::PROVIDER_REFRESH_JOB_KIND.to_string(),
        payload,
        priority: 10,
        max_attempts: 3,
        created_by: user_id,
    };

    match spectra_db::jobs::enqueue(&pool, &new_job).await {
        Ok(job) => to_json(202, &TriggerRefreshResponse { job_id: job.id, status: "queued".to_string() }),
        Err(e) => db_error(&e),
    }
}
