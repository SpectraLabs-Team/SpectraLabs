//! Workspace scoped Strategy Survival Lab reports and presets.
use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, Extension, Path, State},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    auth::Identity,
    error_response,
    jobs_api::{self, to_json, JobView, MAX_ATTEMPTS},
    AppState,
};

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct StartSurvival {
    pub config: Option<Value>,
    pub preset_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct SavePreset {
    pub name: String,
    pub config: Value,
}

#[derive(Debug, Serialize)]
struct ReportRow {
    id: Uuid,
    run_id: Uuid,
    preset_id: Option<Uuid>,
    job_id: Option<Uuid>,
    status: String,
    config: Value,
    worst_verdict: Option<String>,
    verdicts: Value,
    results: Option<Value>,
    error: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn invalid(message: impl Into<String>) -> Response {
    error_response(422, message)
}

#[allow(clippy::result_large_err)]
fn parse_config(value: &Value) -> Result<spectra_research::SurvivalConfig, Response> {
    let encoded = serde_json::to_string(value).map_err(|error| invalid(error.to_string()))?;
    let json = spectra_core::Json::parse(&encoded).map_err(|error| invalid(error.to_string()))?;
    spectra_research::SurvivalConfig::from_json(&json).map_err(|error| invalid(error.to_string()))
}

#[allow(clippy::result_large_err)]
fn report(row: &sqlx::postgres::PgRow) -> Result<ReportRow, Response> {
    Ok(ReportRow {
        id: row.try_get("id").map_err(|_| error_response(500, "Could not read survival report."))?,
        run_id: row.try_get("run_id").map_err(|_| error_response(500, "Could not read survival report."))?,
        preset_id: row
            .try_get("preset_id")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
        job_id: row.try_get("job_id").map_err(|_| error_response(500, "Could not read survival report."))?,
        status: row.try_get("status").map_err(|_| error_response(500, "Could not read survival report."))?,
        config: row.try_get("config").map_err(|_| error_response(500, "Could not read survival report."))?,
        worst_verdict: row
            .try_get("worst_verdict")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
        verdicts: row
            .try_get("verdicts")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
        results: row
            .try_get("results")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
        error: row.try_get("error").map_err(|_| error_response(500, "Could not read survival report."))?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
        finished_at: row
            .try_get("finished_at")
            .map_err(|_| error_response(500, "Could not read survival report."))?,
    })
}

/// Queue an M6 survival report for a successful run.
#[utoipa::path(post, path = "/api/v1/runs/{id}/survival", tag = "survival", params(("id" = Uuid, Path)), request_body = StartSurvival, responses((status = 202, body = Value), (status = 404, body = crate::openapi::ErrorDoc), (status = 409, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn start(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    run_id: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(run_id)) = run_id else { return error_response(404, "Run not found.") };
    let Ok(bytes) = body else { return error_response(400, "Could not read survival request.") };
    let request: StartSurvival = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return invalid(format!("Invalid survival request: {error}")),
    };
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(context) => context,
        Err(error) => return error,
    };
    let saved_config = if let Some(preset_id) = request.preset_id {
        match sqlx::query_scalar::<_, Value>(
            "SELECT config FROM survival_presets WHERE workspace_id=$1 AND id=$2",
        )
        .bind(workspace)
        .bind(preset_id)
        .fetch_optional(&pool)
        .await
        {
            Ok(Some(config)) => Some(config),
            Ok(None) => return error_response(404, "Survival preset not found in this workspace."),
            Err(error) => {
                tracing::error!(%error, "survival preset lookup failed");
                return error_response(503, "The database is unavailable.");
            }
        }
    } else {
        None
    };
    let config = match request.config.or(saved_config) {
        Some(config) => config,
        None => match spectra_research::SurvivalConfig::default().to_json() {
            Ok(value) => serde_json::from_str(&crate::engine::to_text(&value)).unwrap_or(Value::Null),
            Err(error) => return invalid(error.to_string()),
        },
    };
    if let Err(error) = parse_config(&config) {
        return error;
    }
    match sqlx::query_scalar::<_, String>("SELECT status FROM runs WHERE workspace_id = $1 AND id = $2")
        .bind(workspace)
        .bind(run_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(status)) if status == "succeeded" => {}
        Ok(Some(_)) => return error_response(409, "Survival analysis requires a successful run."),
        Ok(None) => return error_response(404, "Run not found."),
        Err(error) => {
            tracing::error!(%error, "survival run lookup failed");
            return error_response(503, "The database is unavailable.");
        }
    }
    let report_id = Uuid::now_v7();
    if let Err(error) = sqlx::query("INSERT INTO survival_reports (id, workspace_id, run_id, preset_id, config, created_by) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(report_id).bind(workspace).bind(run_id).bind(request.preset_id).bind(&config).bind(Uuid::parse_str(&identity.user).ok())
        .execute(&pool).await {
        tracing::error!(%error, "survival report create failed");
        return error_response(503, "Could not save the survival report request.");
    }
    let payload = json!({ "report_id": report_id, "run_id": run_id, "config": config });
    let job = match spectra_db::jobs::enqueue(
        &pool,
        &spectra_db::jobs::NewJob {
            workspace_id: workspace,
            kind: "survival".into(),
            payload,
            priority: 0,
            max_attempts: MAX_ATTEMPTS,
            created_by: Uuid::parse_str(&identity.user).ok(),
        },
    )
    .await
    {
        Ok(job) => job,
        Err(error) => {
            tracing::error!(%error, "survival job enqueue failed");
            let _ = sqlx::query("UPDATE survival_reports SET status='failed', error='Could not queue survival analysis.', finished_at=now() WHERE id=$1 AND workspace_id=$2")
                .bind(report_id).bind(workspace).execute(&pool).await;
            return error_response(503, "Could not queue survival analysis.");
        }
    };
    match sqlx::query("UPDATE survival_reports SET job_id=$3 WHERE workspace_id=$1 AND id=$2")
        .bind(workspace)
        .bind(report_id)
        .bind(job.id)
        .execute(&pool)
        .await
    {
        Ok(_) => {
            let registry = spectra_registry::PgRegistry::new(pool.clone(), workspace);
            if let Err(error) = registry
                .record_survival_report_started(report_id, run_id, Uuid::parse_str(&identity.user).ok())
                .await
            {
                tracing::error!(%error, report_id = %report_id, "could not append survival start audit event");
            }
            to_json(202, &json!({ "report_id": report_id, "job": JobView::from(job) }))
        }
        Err(error) => {
            tracing::error!(%error, "survival report job link failed");
            error_response(503, "The job was queued but its report link could not be saved.")
        }
    }
}

/// List reports attached to one run.
#[utoipa::path(get, path = "/api/v1/runs/{id}/survival", tag = "survival", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<Value>), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn list_for_run(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    run_id: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let Ok(Path(run_id)) = run_id else { return error_response(404, "Run not found.") };
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let rows = match sqlx::query("SELECT * FROM survival_reports WHERE workspace_id = $1 AND run_id = $2 ORDER BY created_at DESC, id DESC")
        .bind(workspace).bind(run_id).fetch_all(&pool).await {
        Ok(rows) => rows, Err(error) => { tracing::error!(%error, "survival report list failed"); return error_response(503, "The database is unavailable."); }
    };
    let values = rows.iter().map(report).collect::<Result<Vec<_>, _>>();
    match values {
        Ok(v) => to_json(200, &v),
        Err(r) => r,
    }
}

/// Read a saved report.
#[utoipa::path(get, path = "/api/v1/survival/{id}", tag = "survival", params(("id" = Uuid, Path)), responses((status = 200, body = Value), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    report_id: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let Ok(Path(report_id)) = report_id else { return error_response(404, "Survival report not found.") };
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match sqlx::query("SELECT * FROM survival_reports WHERE workspace_id = $1 AND id = $2")
        .bind(workspace)
        .bind(report_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => match report(&row) {
            Ok(value) => to_json(200, &value),
            Err(r) => r,
        },
        Ok(None) => error_response(404, "Survival report not found."),
        Err(error) => {
            tracing::error!(%error, "survival report lookup failed");
            error_response(503, "The database is unavailable.")
        }
    }
}

/// Return the saved text report.
#[utoipa::path(get, path = "/api/v1/survival/{id}/text", tag = "survival", params(("id" = Uuid, Path)), responses((status = 200, description = "Text report"), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn text(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    report_id: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let Ok(Path(report_id)) = report_id else { return error_response(404, "Survival report not found.") };
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let hash = match sqlx::query_scalar::<_, Option<String>>("SELECT text_sha256 FROM survival_reports WHERE workspace_id = $1 AND id = $2 AND status = 'succeeded'")
        .bind(workspace).bind(report_id).fetch_optional(&pool).await {
        Ok(Some(Some(hash))) => hash,
        Ok(_) => return error_response(404, "Survival text report not found."),
        Err(error) => { tracing::error!(%error, "survival text lookup failed"); return error_response(503, "The database is unavailable."); }
    };
    let Some(store) = st.cfg.artifacts.as_ref() else {
        return error_response(503, "Artifact storage is unavailable.");
    };
    match store.get(&hash).await {
        Ok(bytes) => {
            ([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], axum::body::Body::from(bytes))
                .into_response()
        }
        Err(error) => {
            tracing::error!(%error, "survival text artifact read failed");
            error_response(503, "The survival text artifact is unavailable.")
        }
    }
}

/// List built-in and workspace survival presets.
#[utoipa::path(get, path = "/api/v1/survival/presets", tag = "survival", responses((status = 200, body = Vec<Value>)))]
pub(crate) async fn list_presets(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let rows = match sqlx::query("SELECT id, name, config, created_at, updated_at FROM survival_presets WHERE workspace_id = $1 ORDER BY name")
        .bind(workspace).fetch_all(&pool).await {
        Ok(rows) => rows,
        Err(error) => { tracing::error!(%error, "survival presets list failed"); return error_response(503, "The database is unavailable."); }
    };
    let mut items = [
        ("Default", spectra_research::SurvivalConfig::default_preset()),
        ("Strict", spectra_research::SurvivalConfig::strict_preset()),
        ("Lenient", spectra_research::SurvivalConfig::lenient_preset()),
    ]
    .into_iter()
    .filter_map(|(name, config)| {
        config.to_json().ok().and_then(|value| {
            serde_json::from_str::<Value>(&crate::engine::to_text(&value)).ok().map(
                |config| json!({ "id": Value::Null, "name": name, "config": config, "source": "builtin" }),
            )
        })
    })
    .collect::<Vec<_>>();
    items.extend(rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"), "name": r.get::<String, _>("name"), "config": r.get::<Value, _>("config"), "source": "workspace",
        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"), "updated_at": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
    })));
    to_json(200, &items)
}
/// Save a workspace preset.
#[utoipa::path(post, path = "/api/v1/survival/presets", tag = "survival", request_body = SavePreset, responses((status = 201, body = Value), (status = 422, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn create_preset(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read preset.") };
    let request: SavePreset = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return invalid(e.to_string()),
    };
    if request.name.trim().is_empty() || request.name.trim().len() > 80 {
        return invalid("name must be 1–80 characters.");
    }
    if let Err(error) = parse_config(&request.config) {
        return error;
    }
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let user = Uuid::parse_str(&identity.user).ok();
    match sqlx::query("INSERT INTO survival_presets (workspace_id, name, config, created_by) VALUES ($1, $2, $3, $4) ON CONFLICT (workspace_id, name) DO UPDATE SET config = EXCLUDED.config, updated_at = now() RETURNING id, name, config, created_at, updated_at")
        .bind(workspace).bind(request.name.trim()).bind(request.config).bind(user).fetch_one(&pool).await {
        Ok(row) => to_json(201, &json!({ "id": row.get::<Uuid, _>("id"), "name": row.get::<String, _>("name"), "config": row.get::<Value, _>("config"), "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"), "updated_at": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at") })),
        Err(error) => { tracing::error!(%error, "survival preset create failed"); error_response(503, "Could not save preset.") }
    }
}

/// Update a saved workspace preset.
#[utoipa::path(patch, path = "/api/v1/survival/presets/{id}", tag = "survival", params(("id" = Uuid, Path)), request_body = SavePreset, responses((status = 200, body = Value), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn update_preset(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    preset_id: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(preset_id)) = preset_id else { return error_response(404, "Preset not found.") };
    let Ok(bytes) = body else { return error_response(400, "Could not read preset.") };
    let request: SavePreset = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return invalid(e.to_string()),
    };
    if request.name.trim().is_empty() || request.name.trim().len() > 80 {
        return invalid("name must be 1–80 characters.");
    }
    if let Err(error) = parse_config(&request.config) {
        return error;
    }
    let (pool, workspace) = match jobs_api::context(&st, &identity) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match sqlx::query("UPDATE survival_presets SET name = $3, config = $4, updated_at = now() WHERE workspace_id = $1 AND id = $2 RETURNING id, name, config, created_at, updated_at")
        .bind(workspace).bind(preset_id).bind(request.name.trim()).bind(request.config).fetch_optional(&pool).await {
        Ok(Some(row)) => to_json(200, &json!({ "id": row.get::<Uuid, _>("id"), "name": row.get::<String, _>("name"), "config": row.get::<Value, _>("config"), "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"), "updated_at": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at") })),
        Ok(None) => error_response(404, "Preset not found."),
        Err(error) => { tracing::error!(%error, "survival preset update failed"); error_response(503, "Could not update preset.") }
    }
}
