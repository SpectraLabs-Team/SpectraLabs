//! `/api/v1/strategies` (M2a tasks 3 and 4): saved strategies and their versions.
//!
//! A saved strategy is a named record in the caller's workspace. Each version
//! is a built-in strategy with a full, validated parameter set, stored once and
//! never changed: saving the same built-in code and parameters again returns
//! the existing version. A backtest can name a version with
//! `strategy_version_id` instead of `strategy` and `params`; [`apply_version`]
//! turns it into the built-in and parameters it stands for, so the worker runs
//! exactly what was saved and records the version on the run.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use spectra_core::{sha256_hex, Json};
use spectra_db::strategies::{self, NewVersion, Strategy, StrategyVersion};
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

/// `POST /api/v1/strategies` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStrategy {
    /// Unique in the workspace; at most 200 characters.
    pub name: String,
    /// Free text; at most 2000 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// Save version 1 at once: a built-in strategy name (see
    /// `GET /api/v1/strategies/builtin`).
    #[serde(default)]
    pub builtin: Option<String>,
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
    pub builtin: String,
    /// Parameters; missing ones take their defaults.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
}

/// A saved strategy.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyView {
    /// Strategy id (UUIDv7).
    pub id: Uuid,
    /// Name, unique in the workspace.
    pub name: String,
    /// Free text.
    pub description: String,
    /// Who created it.
    pub created_by: Option<Uuid>,
    /// Created at (UTC).
    pub created_at: DateTime<Utc>,
    /// Number of versions.
    pub version_count: i64,
    /// The newest version's id.
    pub latest_version_id: Option<Uuid>,
    /// The newest version's number.
    pub latest_version: Option<i32>,
    /// The newest version's built-in strategy.
    pub latest_builtin: Option<String>,
}

impl From<Strategy> for StrategyView {
    fn from(s: Strategy) -> Self {
        Self {
            id: s.id,
            name: s.name,
            description: s.description,
            created_by: s.created_by,
            created_at: s.created_at,
            version_count: s.version_count,
            latest_version_id: s.latest_version_id,
            latest_version: s.latest_version,
            latest_builtin: s.latest_builtin,
        }
    }
}

/// One immutable version of a saved strategy.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyVersionView {
    /// Version id: a backtest's `strategy_version_id`.
    pub id: Uuid,
    /// The strategy it belongs to.
    pub strategy_id: Uuid,
    /// That strategy's name.
    pub strategy_name: String,
    /// 1, 2, ... within the strategy.
    pub version: i32,
    /// Built-in strategy name.
    pub builtin: String,
    /// Every parameter, defaults filled in.
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
    /// SHA-256 of the built-in's source when saved (the runs' `strategy_sha256`).
    pub strategy_sha256: String,
    /// SHA-256 of the canonical parameter text.
    pub config_hash: String,
    /// True while the built-in's code is unchanged, so the version can still run.
    pub current: bool,
    /// Who saved it.
    pub created_by: Option<Uuid>,
    /// Saved at (UTC).
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
            created_by: v.created_by,
            created_at: v.created_at,
        }
    }
}

/// A saved strategy with every version, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyDetail {
    /// The strategy.
    pub strategy: StrategyView,
    /// Its versions, newest first.
    pub versions: Vec<StrategyVersionView>,
}

/// The workspace's saved strategies.
#[derive(Debug, Serialize, ToSchema)]
pub struct StrategyList {
    /// Newest first, at most 500.
    pub items: Vec<StrategyView>,
}

/// The result of saving a version.
#[derive(Debug, Serialize, ToSchema)]
pub struct SavedVersion {
    /// False when this code and these parameters were saved before; the
    /// existing version is returned.
    pub created: bool,
    /// The version.
    pub version: StrategyVersionView,
}

/// True when the version's built-in still exists with the same source.
fn is_current(v: &StrategyVersion) -> bool {
    spectra_backtest::find_builtin(&v.builtin).is_some_and(|b| b.source_sha256() == v.source_hash)
}

/// Checks `builtin` and `params` exactly as a backtest would, and returns the
/// version to store: every parameter typed as its schema says, the source hash,
/// and the hash of the canonical parameter text.
#[allow(clippy::result_large_err)] // the error is the finished response
fn prepare_version(
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
        #[allow(clippy::cast_possible_truncation)] // whole numbers within the spec's bounds
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
        created_by,
    })
}

/// Resolves a backtest payload's `strategy_version_id` (if any) into the
/// built-in and parameters it stands for, in place. A payload that also names
/// `strategy`, `params` or `strategies` is refused, so a run recorded against a
/// version always ran that version.
///
/// # Errors
/// 422 for a malformed or unknown version or a mixed payload; 409
/// (`strategy_version_stale`) when the built-in's code has changed since the
/// version was saved; 503 for the database.
#[allow(clippy::result_large_err)] // the error is the finished response
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
    payload["params"] = version.parameters;
    Ok(())
}

fn user_of(id: &Identity) -> Option<Uuid> {
    Uuid::parse_str(&id.user).ok()
}

/// Create a saved strategy, and with `builtin` its first version.
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
    if req.params.is_some() && req.builtin.is_none() {
        errors.push(("params".to_string(), "give builtin with params".to_string()));
    }
    if !errors.is_empty() {
        return invalid_fields(&errors);
    }
    // Check the first version before creating anything, so a bad one leaves no empty strategy.
    let first = match req.builtin.as_deref() {
        Some(b) => match prepare_version(b, req.params.as_ref(), user_of(&id)) {
            Ok(v) => Some(v),
            Err(r) => return r,
        },
        None => None,
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

/// The strategy `strategy_id` with its versions, as a response.
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

/// The workspace's saved strategies, newest first.
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
    match strategies::list(&pool, ws, 500).await {
        Ok(rows) => to_json(200, &StrategyList { items: rows.into_iter().map(StrategyView::from).collect() }),
        Err(e) => db_error(&e),
    }
}

/// One saved strategy with every version, newest first.
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

/// Save a version: a built-in with parameters. Saving the same code and
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
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: SaveVersion = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid version request: {e}")),
    };
    let new = match prepare_version(&req.builtin, req.params.as_ref(), user_of(&id)) {
        Ok(v) => v,
        Err(r) => return r,
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
