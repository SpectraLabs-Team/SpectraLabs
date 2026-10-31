//! Per-user saved state endpoints (M25.1): portfolio, paper account, risk
//! parameters, sandbox, watchlist, sandbox orders, and the one-time import.
//!
//! The user and workspace come from the signed identity, never from the request
//! body. Every route checks workspace membership. Viewers can read but not write.

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::Extension;
use serde::Deserialize;
use serde_json::{json, Map, Value as JsonValue};
use spectra_db::user_state::{self, PutOutcome, StateKey, StateValue};
use spectra_db::PgPool;
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, error_with_code, AppState, Identity};

/// Resolve the caller, check workspace membership, and return the pool, user and workspace.
#[allow(clippy::result_large_err)]
async fn scope(st: &AppState, id: &Identity, write: bool) -> Result<(PgPool, Uuid, Uuid), Response> {
    let (pool, ws) = context(st, id)?;
    let Ok(user) = Uuid::parse_str(&id.user) else {
        return Err(error_response(401, "Sign in to use saved state."));
    };
    let role: Option<(String,)> =
        sqlx::query_as("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
            .bind(ws)
            .bind(user)
            .fetch_optional(&pool)
            .await
            .map_err(|e| db_error(&e))?;
    let Some((role,)) = role else {
        return Err(error_with_code(403, "not_a_member", "You are not a member of this workspace."));
    };
    if write && role == "viewer" {
        return Err(error_with_code(
            403,
            "viewer_read_only",
            "Viewers have read-only access to saved state.",
        ));
    }
    Ok((pool, user, ws))
}

fn state_view(key: StateKey, row: Option<StateValue>) -> JsonValue {
    match row {
        Some(v) => json!({"value": v.payload, "version": v.version, "updated_at": v.updated_at}),
        None => json!({"value": key.empty(), "version": 0, "updated_at": null}),
    }
}

/// `GET /api/v1/me/state`: every per-user key for the signed-in user in the active workspace.
pub(crate) async fn get_all(State(st): State<AppState>, Extension(id): Extension<Identity>) -> Response {
    let (pool, user, ws) = match scope(&st, &id, false).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let mut out = Map::new();
    for key in StateKey::ALL {
        match user_state::get(&pool, user, ws, key).await {
            Ok(row) => {
                out.insert(key.name().to_string(), state_view(key, row));
            }
            Err(e) => return db_error(&e),
        }
    }
    to_json(200, &json!({"workspace_id": ws, "state": out}))
}

/// `PUT /api/v1/me/state/{key}` body: `{"expected_version": n, "value": ...}`.
#[derive(Deserialize)]
pub(crate) struct PutState {
    expected_version: i32,
    value: JsonValue,
}

/// Write one key. A stale `expected_version` returns 409 with the current value.
pub(crate) async fn put_state(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    seg: Result<Path<String>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(name)) = seg else { return error_response(404, "No such saved state.") };
    let Some(key) = StateKey::parse(&name) else {
        return error_response(404, "No such saved state.");
    };
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: PutState = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid request: {e}")),
    };
    if req.expected_version < 0 {
        return error_response(422, "expected_version must be 0 or more.");
    }
    let shape_ok = if key.expects_object() { req.value.is_object() } else { req.value.is_array() };
    if !shape_ok {
        let want = if key.expects_object() { "a JSON object" } else { "a JSON array" };
        return error_response(422, format!("Expected {want} for this saved state."));
    }

    match user_state::put(&pool, user, ws, key, req.expected_version, &req.value).await {
        Ok(PutOutcome::Saved(row)) => to_json(
            200,
            &json!({
                "saved": true,
                "key": key.name(),
                "value": row.payload,
                "version": row.version,
                "updated_at": row.updated_at,
            }),
        ),
        Ok(PutOutcome::Conflict(current)) => {
            let view = state_view(key, current);
            to_json(
                409,
                &json!({
                    "message": "This saved state changed on another device. Reload and try again.",
                    "code": "version_conflict",
                    "current": view.clone(),
                    "details": { "current": view },
                }),
            )
        }
        Err(e) => db_error(&e),
    }
}

/// `GET /api/v1/me/sandbox/orders`: sandbox orders, oldest first.
pub(crate) async fn list_sandbox_orders(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, user, ws) = match scope(&st, &id, false).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match user_state::list_sandbox_orders(&pool, user, ws).await {
        Ok(rows) => {
            let orders: Vec<JsonValue> = rows
                .iter()
                .rev()
                .map(|o| json!({"id": o.id, "payload": o.payload, "created_at": o.created_at}))
                .collect();
            to_json(200, &json!({"orders": orders}))
        }
        Err(e) => db_error(&e),
    }
}

/// `POST /api/v1/me/sandbox/orders`: append one sandbox order (a JSON object).
pub(crate) async fn append_sandbox_order(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let payload: JsonValue = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return error_response(422, format!("Invalid order: {e}")),
    };
    if !payload.is_object() {
        return error_response(422, "An order must be a JSON object.");
    }
    match user_state::append_sandbox_order(&pool, user, ws, &payload).await {
        Ok(o) => to_json(201, &json!({"id": o.id, "payload": o.payload, "created_at": o.created_at})),
        Err(e) => db_error(&e),
    }
}

/// `PATCH /api/v1/me/sandbox/orders/{id}` body: the order's new JSON object (e.g. a status change).
pub(crate) async fn update_sandbox_order(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    seg: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(order_id)) = seg else { return error_response(404, "No such sandbox order.") };
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let payload: JsonValue = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return error_response(422, format!("Invalid order: {e}")),
    };
    if !payload.is_object() {
        return error_response(422, "An order must be a JSON object.");
    }
    match user_state::update_sandbox_order(&pool, user, ws, order_id, &payload).await {
        Ok(Some(o)) => to_json(200, &json!({"id": o.id, "payload": o.payload, "created_at": o.created_at})),
        Ok(None) => error_response(404, "No such sandbox order."),
        Err(e) => db_error(&e),
    }
}

/// `DELETE /api/v1/me/sandbox/orders`: delete all of the user's sandbox orders (sandbox reset).
pub(crate) async fn clear_sandbox_orders(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match user_state::clear_sandbox_orders(&pool, user, ws).await {
        Ok(n) => to_json(200, &json!({"cleared": n})),
        Err(e) => db_error(&e),
    }
}

/// `POST /api/v1/me/sandbox/transact` body: one trade, applied atomically.
#[derive(Deserialize)]
pub(crate) struct TransactRequest {
    expected_version: i32,
    cash: f64,
    positions: JsonValue,
    order: JsonValue,
}

/// Record a sandbox trade (order, cash and positions) in one transaction. A stale version returns 409
/// and writes nothing.
pub(crate) async fn transact_sandbox(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: TransactRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid trade: {e}")),
    };
    if req.expected_version < 0 {
        return error_response(422, "expected_version must be 0 or more.");
    }
    if !req.cash.is_finite() {
        return error_response(422, "cash must be a finite number.");
    }
    if !req.positions.is_array() {
        return error_response(422, "positions must be a JSON array.");
    }
    if !req.order.is_object() {
        return error_response(422, "An order must be a JSON object.");
    }

    match user_state::transact_sandbox(
        &pool,
        user,
        ws,
        req.expected_version,
        req.cash,
        &req.positions,
        &req.order,
    )
    .await
    {
        Ok(user_state::TransactOutcome::Saved { state, order }) => to_json(
            200,
            &json!({
                "saved": true,
                "version": state.version,
                "updated_at": state.updated_at,
                "value": state.payload,
                "order": {"id": order.id, "payload": order.payload, "created_at": order.created_at},
            }),
        ),
        Ok(user_state::TransactOutcome::Conflict(current)) => {
            let view = state_view(StateKey::Sandbox, current);
            to_json(
                409,
                &json!({
                    "message": "The sandbox changed on another device. Reload and try again.",
                    "code": "version_conflict",
                    "current": view.clone(),
                    "details": { "current": view },
                }),
            )
        }
        Err(e) => db_error(&e),
    }
}

/// `POST /api/v1/me/state/import` body: `{"import_key": "...", "data": {"portfolio": ..., ...}}`.
#[derive(Deserialize)]
pub(crate) struct ImportRequest {
    import_key: String,
    data: Map<String, JsonValue>,
}

/// One-time import of browser-stored state. Server data wins; a repeat key is a no-op.
pub(crate) async fn import(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, user, ws) = match scope(&st, &id, true).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: ImportRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid import request: {e}")),
    };
    let key = req.import_key.trim();
    if key.is_empty() || key.len() > 128 {
        return error_response(422, "import_key must be 1 to 128 characters.");
    }
    match user_state::import(&pool, user, ws, key, &req.data).await {
        Ok(r) => to_json(200, &json!({"noop": r.noop, "imported": r.imported, "skipped": r.skipped})),
        Err(e) => db_error(&e),
    }
}
