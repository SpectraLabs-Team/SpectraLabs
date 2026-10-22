//! Instrument lifecycles and universes API routes (M1b task 12):
//! symbol changes, delistings, and survivorship-free point-in-time universe resolution.

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, error_with_code, invalid_fields, AppState, Identity};

// ------------------------------------------------------------- helper schemas

/// Result of a delete operation.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeletedItemView {
    /// True when the item was deleted.
    pub deleted: bool,
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }
    None
}

// ------------------------------------------------------------- symbol changes

/// `POST /api/v1/symbol-changes` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSymbolChangeRequest {
    /// Previous ticker symbol (e.g. "FB").
    pub old_symbol: String,
    /// New ticker symbol (e.g. "META").
    pub new_symbol: String,
    /// Effective timestamp or date (RFC 3339 or YYYY-MM-DD).
    pub effective_date: String,
    /// Optional notes or reason for change.
    #[serde(default)]
    pub notes: Option<String>,
}

/// A symbol change view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SymbolChangeView {
    /// Change id.
    pub id: Uuid,
    /// Previous symbol.
    pub old_symbol: String,
    /// New symbol.
    pub new_symbol: String,
    /// Effective date.
    pub effective_date: DateTime<Utc>,
    /// Notes.
    pub notes: String,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// List of symbol changes.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SymbolChangeListView {
    /// Items.
    pub items: Vec<SymbolChangeView>,
}

/// List all symbol changes for the active workspace.
#[utoipa::path(get, path = "/api/v1/symbol-changes", tag = "lifecycles",
    responses((status = 200, body = SymbolChangeListView)))]
pub(crate) async fn list_symbol_changes(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::list_symbol_changes(&pool, ws).await {
        Ok(rows) => to_json(
            200,
            &SymbolChangeListView {
                items: rows
                    .into_iter()
                    .map(|r| SymbolChangeView {
                        id: r.id,
                        old_symbol: r.old_symbol,
                        new_symbol: r.new_symbol,
                        effective_date: r.effective_date,
                        notes: r.notes,
                        created_at: r.created_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Record a symbol change.
#[utoipa::path(post, path = "/api/v1/symbol-changes", tag = "lifecycles",
    request_body = CreateSymbolChangeRequest,
    responses(
        (status = 201, body = SymbolChangeView),
        (status = 409, description = "Conflict", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Validation errors", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_symbol_change(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateSymbolChangeRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid symbol change request: {e}")),
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let old_symbol = req.old_symbol.trim().to_ascii_uppercase();
    if old_symbol.is_empty() {
        errors.push(("old_symbol".to_string(), "old_symbol cannot be empty.".to_string()));
    } else if old_symbol.len() > 32 {
        errors.push(("old_symbol".to_string(), "old_symbol must be at most 32 characters.".to_string()));
    }

    let new_symbol = req.new_symbol.trim().to_ascii_uppercase();
    if new_symbol.is_empty() {
        errors.push(("new_symbol".to_string(), "new_symbol cannot be empty.".to_string()));
    } else if new_symbol.len() > 32 {
        errors.push(("new_symbol".to_string(), "new_symbol must be at most 32 characters.".to_string()));
    }

    if !old_symbol.is_empty() && old_symbol == new_symbol {
        errors.push(("new_symbol".to_string(), "new_symbol cannot be the same as old_symbol.".to_string()));
    }

    let eff_date = if let Some(dt) = parse_timestamp(&req.effective_date) {
        dt
    } else {
        errors.push((
            "effective_date".to_string(),
            "effective_date must be an RFC 3339 timestamp or YYYY-MM-DD date.".to_string(),
        ));
        Utc::now()
    };

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let notes = req.notes.as_deref().unwrap_or("").trim();
    match spectra_db::lifecycle::create_symbol_change(&pool, ws, &old_symbol, &new_symbol, eff_date, notes)
        .await
    {
        Ok(r) => to_json(
            201,
            &SymbolChangeView {
                id: r.id,
                old_symbol: r.old_symbol,
                new_symbol: r.new_symbol,
                effective_date: r.effective_date,
                notes: r.notes,
                created_at: r.created_at,
            },
        ),
        Err(e) => {
            if let spectra_db::Error::Database(ref d) = e {
                if d.code().as_deref() == Some("23505") {
                    return error_with_code(
                        409,
                        "conflict",
                        "A symbol change for that old_symbol and effective_date already exists.",
                    );
                }
            }
            db_error(&e)
        }
    }
}

/// Delete a symbol change.
#[utoipa::path(delete, path = "/api/v1/symbol-changes/{id}", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Symbol change id")),
    responses(
        (status = 200, body = DeletedItemView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_symbol_change(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(cid): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::delete_symbol_change(&pool, ws, cid).await {
        Ok(true) => to_json(200, &DeletedItemView { deleted: true }),
        Ok(false) => error_response(404, "No such symbol change."),
        Err(e) => db_error(&e),
    }
}

// ---------------------------------------------------------------- delistings

/// `POST /api/v1/delistings` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateDelistingRequest {
    /// Instrument ticker symbol.
    pub symbol: String,
    /// Delisted timestamp or date (RFC 3339 or YYYY-MM-DD).
    pub delisted_at: String,
    /// Optional reason or notes.
    #[serde(default)]
    pub reason: Option<String>,
}

/// A delisting view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DelistingView {
    /// Delisting record id.
    pub id: Uuid,
    /// Symbol.
    pub symbol: String,
    /// Delisted timestamp.
    pub delisted_at: DateTime<Utc>,
    /// Reason.
    pub reason: String,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// List of delistings.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DelistingListView {
    /// Items.
    pub items: Vec<DelistingView>,
}

/// List all instrument delistings for the active workspace.
#[utoipa::path(get, path = "/api/v1/delistings", tag = "lifecycles",
    responses((status = 200, body = DelistingListView)))]
pub(crate) async fn list_delistings(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::list_delistings(&pool, ws).await {
        Ok(rows) => to_json(
            200,
            &DelistingListView {
                items: rows
                    .into_iter()
                    .map(|r| DelistingView {
                        id: r.id,
                        symbol: r.symbol,
                        delisted_at: r.delisted_at,
                        reason: r.reason,
                        created_at: r.created_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Record an instrument delisting.
#[utoipa::path(post, path = "/api/v1/delistings", tag = "lifecycles",
    request_body = CreateDelistingRequest,
    responses(
        (status = 201, body = DelistingView),
        (status = 409, description = "Conflict", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Validation errors", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_delisting(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateDelistingRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid delisting request: {e}")),
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let symbol = req.symbol.trim().to_ascii_uppercase();
    if symbol.is_empty() {
        errors.push(("symbol".to_string(), "symbol cannot be empty.".to_string()));
    } else if symbol.len() > 32 {
        errors.push(("symbol".to_string(), "symbol must be at most 32 characters.".to_string()));
    }

    let delisted_at = if let Some(dt) = parse_timestamp(&req.delisted_at) {
        dt
    } else {
        errors.push((
            "delisted_at".to_string(),
            "delisted_at must be an RFC 3339 timestamp or YYYY-MM-DD date.".to_string(),
        ));
        Utc::now()
    };

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let reason = req.reason.as_deref().unwrap_or("").trim();
    match spectra_db::lifecycle::create_delisting(&pool, ws, &symbol, delisted_at, reason).await {
        Ok(r) => to_json(
            201,
            &DelistingView {
                id: r.id,
                symbol: r.symbol,
                delisted_at: r.delisted_at,
                reason: r.reason,
                created_at: r.created_at,
            },
        ),
        Err(e) => {
            if let spectra_db::Error::Database(ref d) = e {
                if d.code().as_deref() == Some("23505") {
                    return error_with_code(
                        409,
                        "conflict",
                        "A delisting record for that symbol and date already exists.",
                    );
                }
            }
            db_error(&e)
        }
    }
}

/// Delete a delisting record.
#[utoipa::path(delete, path = "/api/v1/delistings/{id}", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Delisting record id")),
    responses(
        (status = 200, body = DeletedItemView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_delisting(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(did): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::delete_delisting(&pool, ws, did).await {
        Ok(true) => to_json(200, &DeletedItemView { deleted: true }),
        Ok(false) => error_response(404, "No such delisting record."),
        Err(e) => db_error(&e),
    }
}

// ---------------------------------------------------------------- universes

/// `POST /api/v1/universes` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUniverseRequest {
    /// Universe name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional initial member symbols.
    #[serde(default)]
    pub symbols: Vec<String>,
}

/// Universe summary view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseSummaryView {
    /// Universe id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
    /// Member count.
    pub member_count: i64,
}

/// List of universes.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseListView {
    /// Items.
    pub items: Vec<UniverseSummaryView>,
}

/// Universe detail view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseDetailView {
    /// Universe id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// `POST /api/v1/universes/{id}/members` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddUniverseMemberRequest {
    /// Member symbol.
    pub symbol: String,
    /// When added to the universe (RFC 3339 or YYYY-MM-DD; defaults to now).
    #[serde(default)]
    pub added_at: Option<String>,
}

/// Member row view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseMemberView {
    /// Record id.
    pub id: Uuid,
    /// Symbol.
    pub symbol: String,
    /// Added at.
    pub added_at: DateTime<Utc>,
    /// Removed at.
    pub removed_at: Option<DateTime<Utc>>,
    /// When recorded.
    pub created_at: DateTime<Utc>,
}

/// List of universe members.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseMemberListView {
    /// Items.
    pub items: Vec<UniverseMemberView>,
}

/// Query parameters for point-in-time universe resolution.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UniverseAsOfQuery {
    /// Point-in-time snapshot timestamp or date (RFC 3339 or YYYY-MM-DD). Defaults to now.
    pub as_of: Option<String>,
}

/// A resolved universe constituent.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ResolvedUniverseMemberView {
    /// Original constituent symbol.
    pub symbol: String,
    /// Resolved ticker symbol as of date D.
    pub resolved_symbol: String,
    /// True if active on date D (not delisted prior to D).
    pub is_active: bool,
    /// When delisted, if delisted at all.
    pub delisted_at: Option<DateTime<Utc>>,
}

/// Point-in-time resolved universe view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UniverseAsOfView {
    /// Universe id.
    pub universe_id: Uuid,
    /// Snapshot timestamp.
    pub as_of: DateTime<Utc>,
    /// Constituents.
    pub items: Vec<ResolvedUniverseMemberView>,
}

const NO_UNIVERSE: &str = "No such universe.";

/// List universes in the active workspace.
#[utoipa::path(get, path = "/api/v1/universes", tag = "lifecycles",
    responses((status = 200, body = UniverseListView)))]
pub(crate) async fn list_universes(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::list_universes(&pool, ws).await {
        Ok(rows) => to_json(
            200,
            &UniverseListView {
                items: rows
                    .into_iter()
                    .map(|r| UniverseSummaryView {
                        id: r.id,
                        name: r.name,
                        description: r.description,
                        created_at: r.created_at,
                        member_count: r.member_count,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Create a new universe.
#[utoipa::path(post, path = "/api/v1/universes", tag = "lifecycles",
    request_body = CreateUniverseRequest,
    responses(
        (status = 201, body = UniverseDetailView),
        (status = 409, description = "Conflict", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Validation errors", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_universe(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateUniverseRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid universe request: {e}")),
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let name = req.name.trim();
    if name.is_empty() {
        errors.push(("name".to_string(), "name cannot be empty.".to_string()));
    } else if name.len() > 100 {
        errors.push(("name".to_string(), "name must be at most 100 characters.".to_string()));
    }

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let description = req.description.as_deref().unwrap_or("").trim();
    let detail = match spectra_db::lifecycle::create_universe(&pool, ws, name, description).await {
        Ok(u) => u,
        Err(e) => {
            if let spectra_db::Error::Database(ref d) = e {
                if d.code().as_deref() == Some("23505") {
                    return error_with_code(409, "conflict", "A universe with that name already exists.");
                }
            }
            return db_error(&e);
        }
    };

    // Add any initial member symbols
    let epoch = DateTime::from_timestamp(0, 0).unwrap();
    for sym in &req.symbols {
        let trimmed = sym.trim().to_ascii_uppercase();
        if !trimmed.is_empty() {
            let _ = spectra_db::lifecycle::add_universe_member(&pool, ws, detail.id, &trimmed, epoch).await;
        }
    }

    to_json(
        201,
        &UniverseDetailView {
            id: detail.id,
            name: detail.name,
            description: detail.description,
            created_at: detail.created_at,
        },
    )
}

/// Fetch a universe by id.
#[utoipa::path(get, path = "/api/v1/universes/{id}", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Universe id")),
    responses(
        (status = 200, body = UniverseDetailView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn get_universe(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(uid): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::get_universe(&pool, ws, uid).await {
        Ok(Some(u)) => to_json(
            200,
            &UniverseDetailView {
                id: u.id,
                name: u.name,
                description: u.description,
                created_at: u.created_at,
            },
        ),
        Ok(None) => error_response(404, NO_UNIVERSE),
        Err(e) => db_error(&e),
    }
}

/// Delete a universe.
#[utoipa::path(delete, path = "/api/v1/universes/{id}", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Universe id")),
    responses(
        (status = 200, body = DeletedItemView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_universe(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(uid): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::delete_universe(&pool, ws, uid).await {
        Ok(true) => to_json(200, &DeletedItemView { deleted: true }),
        Ok(false) => error_response(404, NO_UNIVERSE),
        Err(e) => db_error(&e),
    }
}

/// List constituent members of a universe.
#[utoipa::path(get, path = "/api/v1/universes/{id}/members", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Universe id")),
    responses(
        (status = 200, body = UniverseMemberListView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn list_universe_members(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(uid): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::get_universe(&pool, ws, uid).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, NO_UNIVERSE),
        Err(e) => return db_error(&e),
    }
    match spectra_db::lifecycle::list_universe_members(&pool, ws, uid).await {
        Ok(rows) => to_json(
            200,
            &UniverseMemberListView {
                items: rows
                    .into_iter()
                    .map(|r| UniverseMemberView {
                        id: r.id,
                        symbol: r.symbol,
                        added_at: r.added_at,
                        removed_at: r.removed_at,
                        created_at: r.created_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

/// Add a member symbol to a universe.
#[utoipa::path(post, path = "/api/v1/universes/{id}/members", tag = "lifecycles",
    params(("id" = Uuid, Path, description = "Universe id")),
    request_body = AddUniverseMemberRequest,
    responses(
        (status = 201, body = UniverseMemberView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Validation errors", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn add_universe_member(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(uid): Path<Uuid>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::get_universe(&pool, ws, uid).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, NO_UNIVERSE),
        Err(e) => return db_error(&e),
    }
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: AddUniverseMemberRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid add member request: {e}")),
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let symbol = req.symbol.trim().to_ascii_uppercase();
    if symbol.is_empty() {
        errors.push(("symbol".to_string(), "symbol cannot be empty.".to_string()));
    } else if symbol.len() > 32 {
        errors.push(("symbol".to_string(), "symbol must be at most 32 characters.".to_string()));
    }

    let added_at = match req.added_at.as_deref() {
        Some(s) => {
            if let Some(dt) = parse_timestamp(s) {
                dt
            } else {
                errors.push((
                    "added_at".to_string(),
                    "added_at must be an RFC 3339 timestamp or YYYY-MM-DD date.".to_string(),
                ));
                DateTime::from_timestamp(0, 0).unwrap()
            }
        }
        None => DateTime::from_timestamp(0, 0).unwrap(),
    };

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    match spectra_db::lifecycle::add_universe_member(&pool, ws, uid, &symbol, added_at).await {
        Ok(mid) => to_json(
            201,
            &UniverseMemberView { id: mid, symbol, added_at, removed_at: None, created_at: Utc::now() },
        ),
        Err(e) => db_error(&e),
    }
}

/// Point-in-time resolution of universe members as of date `as_of`.
#[utoipa::path(get, path = "/api/v1/universes/{id}/as-of", tag = "lifecycles",
    params(
        ("id" = Uuid, Path, description = "Universe id"),
        UniverseAsOfQuery,
    ),
    responses(
        (status = 200, body = UniverseAsOfView),
        (status = 404, description = "Not found", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid timestamp", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn resolve_universe_as_of(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(uid): Path<Uuid>,
    Query(q): Query<UniverseAsOfQuery>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::lifecycle::get_universe(&pool, ws, uid).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, NO_UNIVERSE),
        Err(e) => return db_error(&e),
    }

    let as_of = match q.as_of.as_deref() {
        Some(s) => match parse_timestamp(s) {
            Some(dt) => dt,
            None => {
                return invalid_fields(&[(
                    "as_of".to_string(),
                    "as_of must be an RFC 3339 timestamp or YYYY-MM-DD date.".to_string(),
                )])
            }
        },
        None => Utc::now(),
    };

    match spectra_db::lifecycle::resolve_universe_as_of(&pool, ws, uid, as_of).await {
        Ok(members) => to_json(
            200,
            &UniverseAsOfView {
                universe_id: uid,
                as_of,
                items: members
                    .into_iter()
                    .map(|m| ResolvedUniverseMemberView {
                        symbol: m.symbol,
                        resolved_symbol: m.resolved_symbol,
                        is_active: m.is_active,
                        delisted_at: m.delisted_at,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}
