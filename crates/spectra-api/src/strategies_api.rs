//! `/api/v1/strategies` (M2 Strategy Studio): saved strategies, versions, drafts, parameter sets, and diffs.
//!
//! A saved strategy is a named record in the caller's workspace. Each version
//! is a built-in strategy or DSL script with a full, validated parameter set, stored once and
//! never changed: saving the same code and parameters again returns
//! the existing version. A backtest can name a version with
//! `strategy_version_id` instead of `strategy` and `params`; [`apply_version`]
//! turns it into the strategy and parameters it stands for, so the worker runs
//! exactly what was saved and records the version on the run.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_core::{sha256_hex, Json};
use spectra_db::strategies::{self, NewVersion, ParameterSet, Strategy, StrategyVersion};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{engine, error_response, error_with_code, invalid_fields, AppState, Identity};

/// Longest strategy name, in characters.
const MAX_NAME: usize = 200;
/// Longest description, in characters.
const MAX_DESCRIPTION: usize = 2000;
const NOT_FOUND: &str = "No such strategy.";
const VERSION_NOT_FOUND: &str = "No such strategy version.";

/// Check if the caller has write permissions (viewer is read-only).
#[allow(clippy::result_large_err)]
async fn ensure_can_edit(pool: &spectra_db::PgPool, ws: Uuid, user_id: Option<Uuid>) -> Result<(), Response> {
    if let Some(uid) = user_id {
        let role: Option<(String,)> =
            sqlx::query_as("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
                .bind(ws)
                .bind(uid)
                .fetch_optional(pool)
                .await
                .map_err(|e| db_error(&e))?;

        if let Some((r,)) = role {
            if r == "viewer" {
                return Err(error_with_code(
                    403,
                    "viewer_read_only",
                    "Viewers have read-only access to strategies and versions.",
                ));
            }
        }
    }
    Ok(())
}

/// `POST /api/v1/strategies` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStrategy {
    /// Unique in the workspace; at most 200 characters.
    pub name: String,
    /// Free text; at most 2000 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// Save version 1 at once: a built-in strategy name.
    #[serde(default)]
    pub builtin: Option<String>,
    /// Or save version 1 from DSL code.
    #[serde(default)]
    pub code: Option<String>,
    /// Parameters of version 1; missing ones take their defaults.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
}

/// `POST /api/v1/strategies/{id}/versions` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveVersion {
    /// A built-in strategy name.
    #[serde(default)]
    pub builtin: Option<String>,
    /// Or user-authored DSL strategy code.
    #[serde(default)]
    pub code: Option<String>,
    /// Every parameter; missing ones take their defaults.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
}

/// `PUT /api/v1/strategies/{id}/draft` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveDraftRequest {
    /// The draft DSL code.
    pub code: String,
    /// Parameters object.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub parameters: Option<serde_json::Value>,
}

/// `POST /api/v1/strategies/{id}/validate` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateCodeRequest {
    pub code: String,
}

/// Validation result with line-level diagnostics.
#[derive(Debug, Serialize, ToSchema)]
pub struct ValidateCodeResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<CodeErrorView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamSpecView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CodeErrorView {
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParamSpecView {
    pub name: String,
    pub kind: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub description: String,
}

/// `POST /api/v1/strategies/{id}/versions/{version_id}/parameter_sets` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParamSetRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[schema(value_type = Object)]
    pub parameters: serde_json::Value,
}

/// Strategy Diff query params.
#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    pub from: i32,
    pub to: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyDiffView {
    pub strategy_id: Uuid,
    pub from_version: i32,
    pub to_version: i32,
    pub code_diff: Vec<DiffLine>,
    pub parameter_diff: ParameterDiff,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DiffLine {
    pub kind: String,
    pub line_from: Option<usize>,
    pub line_to: Option<usize>,
    pub text: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParameterDiff {
    pub changed: HashMap<String, ParamChange>,
    pub added: HashMap<String, serde_json::Value>,
    pub removed: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParamChange {
    pub from: serde_json::Value,
    pub to: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ParameterSetView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_version_id: Uuid,
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

impl From<ParameterSet> for ParameterSetView {
    fn from(p: ParameterSet) -> Self {
        Self {
            id: p.id,
            workspace_id: p.workspace_id,
            strategy_version_id: p.strategy_version_id,
            name: p.name,
            description: p.description,
            parameters: p.parameters,
            created_at: p.created_at,
            created_by: p.created_by,
        }
    }
}

/// The outcome of [`save_strategy_version`].
#[derive(Debug, Serialize, ToSchema)]
pub struct SavedVersion {
    pub created: bool,
    pub version: StrategyVersionView,
}

/// A saved strategy summary for lists and cards.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyView {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version_count: i64,
    pub latest_version_id: Option<Uuid>,
    pub latest_version: Option<i32>,
    pub latest_builtin: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<Strategy> for StrategyView {
    fn from(s: Strategy) -> Self {
        Self {
            id: s.id,
            name: s.name,
            description: s.description,
            version_count: s.version_count,
            latest_version_id: s.latest_version_id,
            latest_version: s.latest_version,
            latest_builtin: s.latest_builtin,
            created_by: s.created_by,
            created_at: s.created_at,
        }
    }
}

/// One immutable version of a saved strategy.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyVersionView {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub strategy_name: String,
    pub version: i32,
    pub builtin: String,
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
    pub strategy_sha256: String,
    pub config_hash: String,
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<StrategyVersion> for StrategyVersionView {
    fn from(v: StrategyVersion) -> Self {
        let current = is_current(&v);
        Self {
            id: v.id,
            strategy_id: v.strategy_id,
            strategy_name: v.strategy_name,
            version: v.version,
            builtin: v.builtin,
            params: v.parameters,
            strategy_sha256: v.source_hash,
            config_hash: v.config_hash,
            current,
            code: v.code,
            created_by: v.created_by,
            created_at: v.created_at,
        }
    }
}

/// A saved strategy with every version, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyDetail {
    pub strategy: StrategyView,
    pub versions: Vec<StrategyVersionView>,
}

/// The workspace's saved strategies.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyList {
    pub items: Vec<StrategyView>,
}

fn is_current(v: &StrategyVersion) -> bool {
    if v.code.is_some() || v.builtin == "dsl" {
        return true;
    }
    spectra_backtest::find_builtin(&v.builtin).is_some_and(|b| b.source_sha256() == v.source_hash)
}

/// Checks `builtin` and `params` and prepares `NewVersion`.
#[allow(clippy::result_large_err)]
fn prepare_builtin_version(
    builtin: &str,
    params: Option<&serde_json::Value>,
    created_by: Option<Uuid>,
) -> Result<NewVersion, Response> {
    let Some(b) = spectra_backtest::find_builtin(builtin.trim()) else {
        let names: Vec<&str> = spectra_backtest::builtin_strategies().iter().map(|s| s.name).collect();
        return Err(invalid_fields(&[(
            "builtin".into(),
            format!("unknown strategy {builtin:?}; built-in strategies are {}", names.join(", ")),
        )]));
    };
    let doc = match params {
        None => None,
        Some(p) => match Json::parse(&p.to_string()) {
            Ok(j) => Some(j),
            Err(e) => return Err(invalid_fields(&[("params".into(), e.to_string())])),
        },
    };
    let resolved = b.resolve_params_json(doc.as_ref()).map_err(|e| {
        invalid_fields(&e.errors.into_iter().map(|f| (f.field, f.message)).collect::<Vec<_>>())
    })?;
    let mut parameters = serde_json::Map::new();
    for spec in b.params {
        let v = resolved.values.iter().find(|(k, _)| k == spec.name).map_or(spec.default, |(_, v)| *v);
        #[allow(clippy::cast_possible_truncation)]
        let value = match spec.kind {
            spectra_backtest::ParamKind::Int => serde_json::json!(v as i64),
            spectra_backtest::ParamKind::Bool => serde_json::json!(v != 0.0),
            spectra_backtest::ParamKind::Float => serde_json::json!(v),
        };
        parameters.insert(spec.name.to_string(), value);
    }
    let schema = b
        .to_json()
        .ok()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&engine::to_text(&j)).ok())
        .and_then(|j| j.get("params").cloned())
        .unwrap_or(serde_json::Value::Null);
    Ok(NewVersion {
        builtin: b.name.to_string(),
        source_hash: b.source_sha256(),
        config_hash: sha256_hex(resolved.canonical().as_bytes()),
        parameter_schema: serde_json::json!({ "params": schema }),
        parameters: serde_json::Value::Object(parameters),
        code: None,
        created_by,
    })
}

/// Checks DSL code and prepares `NewVersion`.
#[allow(clippy::result_large_err)]
fn prepare_dsl_version(
    code: &str,
    params: Option<&serde_json::Value>,
    created_by: Option<Uuid>,
) -> Result<NewVersion, Response> {
    let program = match spectra_engine::dsl::parse_and_validate(code) {
        Ok(p) => p,
        Err(e) => {
            return Err(error_with_code(
                422,
                "invalid_strategy_code",
                format!("{}:{}: [{}] {}", e.line, e.col, e.code, e.message),
            ));
        }
    };

    let mut schema_props = serde_json::Map::new();
    let mut parameters = serde_json::Map::new();

    for p in &program.params {
        let mut prop = serde_json::Map::new();
        prop.insert("type".into(), serde_json::json!(p.kind.as_str()));
        prop.insert("default".into(), serde_json::json!(p.default));
        prop.insert("min".into(), serde_json::json!(p.min));
        prop.insert("max".into(), serde_json::json!(p.max));
        prop.insert("description".into(), serde_json::json!(p.description));
        schema_props.insert(p.name.clone(), serde_json::Value::Object(prop));

        let val = if let Some(overrides) = params.and_then(|v| v.as_object()) {
            if let Some(user_val) = overrides.get(&p.name) {
                user_val.clone()
            } else {
                match p.kind {
                    spectra_engine::dsl::ParamType::Int => serde_json::json!(p.default as i64),
                    spectra_engine::dsl::ParamType::Bool => serde_json::json!(p.default != 0.0),
                    spectra_engine::dsl::ParamType::Float => serde_json::json!(p.default),
                }
            }
        } else {
            match p.kind {
                spectra_engine::dsl::ParamType::Int => serde_json::json!(p.default as i64),
                spectra_engine::dsl::ParamType::Bool => serde_json::json!(p.default != 0.0),
                spectra_engine::dsl::ParamType::Float => serde_json::json!(p.default),
            }
        };
        parameters.insert(p.name.clone(), val);
    }

    let param_obj = serde_json::Value::Object(parameters);
    let canonical = serde_json::to_string(&param_obj).unwrap_or_default();

    Ok(NewVersion {
        builtin: "dsl".to_string(),
        source_hash: sha256_hex(code.as_bytes()),
        config_hash: sha256_hex(canonical.as_bytes()),
        parameter_schema: serde_json::Value::Object(schema_props),
        parameters: param_obj,
        code: Some(code.to_string()),
        created_by,
    })
}

/// Resolves a backtest payload's `strategy_version_id`.
#[allow(clippy::result_large_err)]
pub(crate) async fn apply_version(
    pool: &spectra_db::PgPool,
    workspace: Uuid,
    payload: &mut serde_json::Value,
) -> Result<(), Response> {
    let given = match payload.get("strategy_version_id") {
        None | Some(serde_json::Value::Null) => return Ok(()),
        Some(v) => v.clone(),
    };
    let Some(id) = given.as_str().and_then(|s| Uuid::parse_str(s.trim()).ok()) else {
        return Err(invalid_fields(&[("strategy_version_id".into(), "must be a UUID".into())]));
    };
    let mixed: Vec<(String, String)> = ["strategy", "params", "strategies"]
        .into_iter()
        .filter(|k| payload.get(*k).is_some_and(|v| !v.is_null()))
        .map(|k| (k.to_string(), "leave out when strategy_version_id is given".to_string()))
        .collect();
    if !mixed.is_empty() {
        return Err(invalid_fields(&mixed));
    }
    let version = match strategies::get_version(pool, workspace, id).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            return Err(invalid_fields(&[(
                "strategy_version_id".into(),
                "no such strategy version in this workspace".into(),
            )]))
        }
        Err(e) => return Err(db_error(&e)),
    };
    if !is_current(&version) {
        return Err(error_with_code(
            409,
            "strategy_version_stale",
            format!(
                "{} v{} was saved with a different version of the {} code; save a new version to run it.",
                version.strategy_name, version.version, version.builtin
            ),
        ));
    }
    payload["strategy_version_id"] = serde_json::Value::String(version.id.to_string());
    payload["strategy"] = serde_json::Value::String(version.builtin);
    if let Some(code) = version.code {
        payload["dsl_code"] = serde_json::Value::String(code);
    }
    payload["params"] = version.parameters;
    Ok(())
}

fn user_of(id: &Identity) -> Option<Uuid> {
    Uuid::parse_str(&id.user).ok()
}

/// Create a saved strategy, and optionally its first version.
#[utoipa::path(post, path = "/api/v1/strategies", tag = "strategies", request_body = CreateStrategy,
    responses(
        (status = 201, description = "Created", body = StrategyDetail),
        (status = 409, description = "The workspace has a strategy of that name (code `strategy_exists`)", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every problem with the name, built-in or parameters", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_strategy(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateStrategy = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid strategy request: {e}")),
    };
    let name = req.name.trim();
    let description = req.description.as_deref().unwrap_or("").trim();
    let mut errors = Vec::new();
    if name.is_empty() {
        errors.push(("name".to_string(), "must not be empty".to_string()));
    } else if name.chars().count() > MAX_NAME {
        errors.push(("name".to_string(), format!("must be at most {MAX_NAME} characters")));
    }
    if description.chars().count() > MAX_DESCRIPTION {
        errors.push(("description".to_string(), format!("must be at most {MAX_DESCRIPTION} characters")));
    }
    if req.params.is_some() && req.builtin.is_none() && req.code.is_none() {
        errors.push(("params".to_string(), "give builtin or code with params".to_string()));
    }
    if req.builtin.is_some() && req.code.is_some() {
        errors.push(("code".to_string(), "specify either builtin or code, not both".to_string()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let first = if let Some(code) = req.code.as_deref() {
        match prepare_dsl_version(code, req.params.as_ref(), user_of(&id)) {
            Ok(v) => Some(v),
            Err(r) => return r,
        }
    } else if let Some(b) = req.builtin.as_deref() {
        match prepare_builtin_version(b, req.params.as_ref(), user_of(&id)) {
            Ok(v) => Some(v),
            Err(r) => return r,
        }
    } else {
        None
    };

    let created = match strategies::create(&pool, ws, name, description, user_of(&id)).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return error_with_code(
                409,
                "strategy_exists",
                format!("This workspace has a strategy called {name:?}."),
            )
        }
        Err(e) => return db_error(&e),
    };
    if let Some(v) = first {
        if let Err(e) = strategies::save_version(&pool, ws, created.id, &v).await {
            return db_error(&e);
        }
    }
    detail(&pool, ws, created.id, 201).await
}

async fn detail(pool: &spectra_db::PgPool, ws: Uuid, strategy_id: Uuid, status: u16) -> Response {
    let strategy = match strategies::get(pool, ws, strategy_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, NOT_FOUND),
        Err(e) => return db_error(&e),
    };
    match strategies::versions(pool, ws, strategy_id).await {
        Ok(vs) => to_json(
            status,
            &StrategyDetail {
                strategy: strategy.into(),
                versions: vs.into_iter().map(StrategyVersionView::from).collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// The workspace's saved strategies.
#[utoipa::path(get, path = "/api/v1/strategies", tag = "strategies",
    responses((status = 200, body = StrategyList)))]
pub(crate) async fn list_strategies(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match strategies::list(&pool, ws, 200).await {
        Ok(ss) => to_json(200, &StrategyList { items: ss.into_iter().map(StrategyView::from).collect() }),
        Err(e) => db_error(&e),
    }
}

/// The saved strategy with its versions.
#[utoipa::path(get, path = "/api/v1/strategies/{id}", tag = "strategies",
    params(("id" = Uuid, Path, description = "Strategy id")),
    responses((status = 200, body = StrategyDetail), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_strategy(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    strategy_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(strategy_id)) = strategy_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    detail(&pool, ws, strategy_id, 200).await
}

/// Save a version: a built-in or DSL code with parameters. Saving the same code and
/// parameters again returns the existing version (200, `created: false`).
#[utoipa::path(post, path = "/api/v1/strategies/{id}/versions", tag = "strategies", request_body = SaveVersion,
    params(("id" = Uuid, Path, description = "Strategy id")),
    responses(
        (status = 201, description = "A new version", body = SavedVersion),
        (status = 200, description = "Saved before; the existing version", body = SavedVersion),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 422, description = "Every problem with the built-in or parameters", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn save_strategy_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    strategy_id: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(strategy_id)) = strategy_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: SaveVersion = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid version request: {e}")),
    };
    if req.builtin.is_none() && req.code.is_none() {
        return invalid_fields(&[("builtin".into(), "provide either builtin or code".into())]);
    }
    if req.builtin.is_some() && req.code.is_some() {
        return invalid_fields(&[("code".into(), "provide either builtin or code, not both".into())]);
    }

    let new = if let Some(code) = req.code.as_deref() {
        match prepare_dsl_version(code, req.params.as_ref(), user_of(&id)) {
            Ok(v) => v,
            Err(r) => return r,
        }
    } else {
        match prepare_builtin_version(req.builtin.as_deref().unwrap_or(""), req.params.as_ref(), user_of(&id))
        {
            Ok(v) => v,
            Err(r) => return r,
        }
    };

    match strategies::save_version(&pool, ws, strategy_id, &new).await {
        Ok(Some((v, created))) => {
            to_json(if created { 201 } else { 200 }, &SavedVersion { created, version: v.into() })
        }
        Ok(None) => error_response(404, NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

/// One strategy version by id (what a backtest's `strategy_version_id` names).
#[utoipa::path(get, path = "/api/v1/strategies/versions/{id}", tag = "strategies",
    params(("id" = Uuid, Path, description = "Strategy version id")),
    responses((status = 200, body = StrategyVersionView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_strategy_version(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    version_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(version_id)) = version_id else { return error_response(404, VERSION_NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match strategies::get_version(&pool, ws, version_id).await {
        Ok(Some(v)) => to_json(200, &StrategyVersionView::from(v)),
        Ok(None) => error_response(404, VERSION_NOT_FOUND),
        Err(e) => db_error(&e),
    }
}

/// Live validation of DSL strategy code for the editor.
pub(crate) async fn validate_strategy_code(body: Result<Bytes, BytesRejection>) -> Response {
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: ValidateCodeRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid validation request: {e}")),
    };

    match spectra_engine::dsl::parse_and_validate(&req.code) {
        Ok(program) => {
            let params = program
                .params
                .into_iter()
                .map(|p| ParamSpecView {
                    name: p.name,
                    kind: p.kind.as_str().to_string(),
                    default: p.default,
                    min: p.min,
                    max: p.max,
                    description: p.description,
                })
                .collect();
            to_json(200, &ValidateCodeResponse { valid: true, errors: Vec::new(), params })
        }
        Err(e) => {
            let errors = vec![CodeErrorView {
                line: e.line,
                col: e.col,
                message: e.message,
                code: e.code.to_string(),
            }];
            to_json(200, &ValidateCodeResponse { valid: false, errors, params: Vec::new() })
        }
    }
}

/// Fetch active draft for strategy.
pub(crate) async fn get_strategy_draft(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    strategy_id: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(strategy_id)) = strategy_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match strategies::get_draft(&pool, ws, strategy_id).await {
        Ok(Some(d)) => to_json(
            200,
            &serde_json::json!({
                "strategy_id": d.strategy_id,
                "workspace_id": d.workspace_id,
                "code": d.code,
                "parameters": d.parameters,
                "updated_at": d.updated_at,
                "updated_by": d.updated_by,
            }),
        ),
        Ok(None) => to_json(
            200,
            &serde_json::json!({
                "strategy_id": strategy_id,
                "workspace_id": ws,
                "code": "",
                "parameters": {},
                "updated_at": null,
                "updated_by": null,
            }),
        ),
        Err(e) => db_error(&e),
    }
}

/// Autosave mutable draft for strategy.
pub(crate) async fn save_strategy_draft(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    strategy_id: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(strategy_id)) = strategy_id else { return error_response(404, NOT_FOUND) };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: SaveDraftRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid draft request: {e}")),
    };

    let params = req.parameters.unwrap_or_else(|| serde_json::json!({}));
    match strategies::save_draft(&pool, ws, strategy_id, &req.code, &params, user_of(&id)).await {
        Ok(d) => to_json(
            200,
            &serde_json::json!({
                "strategy_id": d.strategy_id,
                "workspace_id": d.workspace_id,
                "code": d.code,
                "parameters": d.parameters,
                "updated_at": d.updated_at,
                "updated_by": d.updated_by,
            }),
        ),
        Err(e) => db_error(&e),
    }
}

/// Create a named parameter set for a strategy version.
pub(crate) async fn create_parameter_set(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path((_strategy_id, version_id)): Path<(Uuid, Uuid)>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, user_of(&id)).await {
        return r;
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateParamSetRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid parameter set request: {e}")),
    };

    let name = req.name.trim();
    if name.is_empty() {
        return invalid_fields(&[("name".into(), "name must not be empty".into())]);
    }
    let desc = req.description.as_deref().unwrap_or("").trim();

    match strategies::create_parameter_set(&pool, ws, version_id, name, desc, &req.parameters, user_of(&id))
        .await
    {
        Ok(ps) => to_json(201, &ParameterSetView::from(ps)),
        Err(e) => db_error(&e),
    }
}

/// List parameter sets for a version.
pub(crate) async fn list_parameter_sets(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path((_strategy_id, version_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match strategies::list_parameter_sets(&pool, ws, version_id).await {
        Ok(pss) => {
            let views: Vec<ParameterSetView> = pss.into_iter().map(ParameterSetView::from).collect();
            to_json(200, &serde_json::json!({ "parameter_sets": views }))
        }
        Err(e) => db_error(&e),
    }
}

/// Fetch a parameter set by id.
pub(crate) async fn get_parameter_set(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(id_param): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match strategies::get_parameter_set(&pool, ws, id_param).await {
        Ok(Some(ps)) => to_json(200, &ParameterSetView::from(ps)),
        Ok(None) => error_response(404, "No such parameter set."),
        Err(e) => db_error(&e),
    }
}

/// Compare two versions of a strategy (text diff and parameter diff).
pub(crate) async fn diff_strategy_versions(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(strategy_id): Path<Uuid>,
    query: Result<Query<DiffQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return invalid_fields(&[(
            "from".into(),
            "required query parameters 'from' and 'to' (version numbers)".into(),
        )]);
    };

    let all_versions = match strategies::versions(&pool, ws, strategy_id).await {
        Ok(vs) => vs,
        Err(e) => return db_error(&e),
    };

    let v_from = all_versions.iter().find(|v| v.version == q.from);
    let v_to = all_versions.iter().find(|v| v.version == q.to);

    let (Some(vf), Some(vt)) = (v_from, v_to) else {
        return error_response(404, "One or both specified version numbers not found.");
    };

    let code_from = vf.code.as_deref().unwrap_or(&vf.builtin);
    let code_to = vt.code.as_deref().unwrap_or(&vt.builtin);

    let code_diff = compute_code_diff(code_from, code_to);
    let parameter_diff = compute_param_diff(&vf.parameters, &vt.parameters);

    to_json(
        200,
        &StrategyDiffView { strategy_id, from_version: q.from, to_version: q.to, code_diff, parameter_diff },
    )
}

fn compute_code_diff(from_code: &str, to_code: &str) -> Vec<DiffLine> {
    let from_lines: Vec<&str> = from_code.lines().collect();
    let to_lines: Vec<&str> = to_code.lines().collect();
    let mut out = Vec::new();

    let (mut i, mut j) = (0, 0);
    while i < from_lines.len() && j < to_lines.len() {
        if from_lines[i] == to_lines[j] {
            out.push(DiffLine {
                kind: "unchanged".into(),
                line_from: Some(i + 1),
                line_to: Some(j + 1),
                text: from_lines[i].to_string(),
            });
            i += 1;
            j += 1;
        } else {
            let next_to = to_lines[j..].iter().position(|&l| l == from_lines[i]);
            let next_from = from_lines[i..].iter().position(|&l| l == to_lines[j]);
            match (next_to, next_from) {
                (Some(tj), Some(fi)) => {
                    if tj <= fi {
                        for k in 0..tj {
                            out.push(DiffLine {
                                kind: "added".into(),
                                line_from: None,
                                line_to: Some(j + k + 1),
                                text: to_lines[j + k].to_string(),
                            });
                        }
                        j += tj;
                    } else {
                        for k in 0..fi {
                            out.push(DiffLine {
                                kind: "removed".into(),
                                line_from: Some(i + k + 1),
                                line_to: None,
                                text: from_lines[i + k].to_string(),
                            });
                        }
                        i += fi;
                    }
                }
                (Some(tj), None) => {
                    for k in 0..tj {
                        out.push(DiffLine {
                            kind: "added".into(),
                            line_from: None,
                            line_to: Some(j + k + 1),
                            text: to_lines[j + k].to_string(),
                        });
                    }
                    j += tj;
                }
                (None, Some(fi)) => {
                    for k in 0..fi {
                        out.push(DiffLine {
                            kind: "removed".into(),
                            line_from: Some(i + k + 1),
                            line_to: None,
                            text: from_lines[i + k].to_string(),
                        });
                    }
                    i += fi;
                }
                (None, None) => {
                    out.push(DiffLine {
                        kind: "removed".into(),
                        line_from: Some(i + 1),
                        line_to: None,
                        text: from_lines[i].to_string(),
                    });
                    out.push(DiffLine {
                        kind: "added".into(),
                        line_from: None,
                        line_to: Some(j + 1),
                        text: to_lines[j].to_string(),
                    });
                    i += 1;
                    j += 1;
                }
            }
        }
    }
    while i < from_lines.len() {
        out.push(DiffLine {
            kind: "removed".into(),
            line_from: Some(i + 1),
            line_to: None,
            text: from_lines[i].to_string(),
        });
        i += 1;
    }
    while j < to_lines.len() {
        out.push(DiffLine {
            kind: "added".into(),
            line_from: None,
            line_to: Some(j + 1),
            text: to_lines[j].to_string(),
        });
        j += 1;
    }
    out
}

fn compute_param_diff(from_params: &serde_json::Value, to_params: &serde_json::Value) -> ParameterDiff {
    let empty_map = serde_json::Map::new();
    let m1 = from_params.as_object().unwrap_or(&empty_map);
    let m2 = to_params.as_object().unwrap_or(&empty_map);

    let mut changed = HashMap::new();
    let mut added = HashMap::new();
    let mut removed = HashMap::new();

    for (k, v1) in m1 {
        if let Some(v2) = m2.get(k) {
            if v1 != v2 {
                changed.insert(k.clone(), ParamChange { from: v1.clone(), to: v2.clone() });
            }
        } else {
            removed.insert(k.clone(), v1.clone());
        }
    }
    for (k, v2) in m2 {
        if !m1.contains_key(k) {
            added.insert(k.clone(), v2.clone());
        }
    }
    ParameterDiff { changed, added, removed }
}
