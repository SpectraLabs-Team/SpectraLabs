//! M36: Chart drawings, symbol comparison, and saved layout templates API.
//!
//! Exposes REST endpoints for:
//! 1. Chart drawings (trendlines, levels, Fibonacci, text) with soft delete and 200-drawing limits.
//! 2. Symbol comparison rebased to 100 on the first bar of the full dataset with timestamp alignment.
//! 3. Saved chart templates with workspace sharing, 50-template limit, and retired version alerts.

#![allow(clippy::result_large_err, clippy::single_match_else)]

use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use spectra_core::Bar;
use spectra_db::chart_drawings::{
    create_drawing as db_create_drawing, create_template as db_create_template,
    delete_template as db_delete_template, get_template as db_get_template, list_active_drawings,
    list_templates as db_list_templates, soft_delete_drawing as db_soft_delete_drawing,
    update_drawing as db_update_drawing, DrawingError, NewDrawing, NewTemplate, TemplateError,
};
use spectra_db::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::datasets_api::load_for_chart;
use crate::indicators_api::check_timeframe_pub;
use crate::jobs_api::{context, db_error, to_json};
use crate::{error_response, error_with_code, invalid_fields, AppState, Identity};

/// Resolves the caller, verifies workspace membership, and enforces edit permissions for writes.
#[allow(clippy::result_large_err)]
async fn scope(st: &AppState, id: &Identity, write: bool) -> Result<(PgPool, Uuid, Uuid), Response> {
    let (pool, ws) = context(st, id)?;
    let Ok(user) = Uuid::parse_str(&id.user) else {
        return Err(error_response(401, "Sign in to use chart features."));
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
            "Viewers have read-only access to drawings and templates.",
        ));
    }
    Ok((pool, user, ws))
}

/// A stored drawing view returned to the client.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DrawingView {
    pub id: Uuid,
    pub chart_id: String,
    pub dataset_version_id: Option<String>,
    pub tool: String,
    pub points: JsonValue,
    pub style: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request payload to create a new drawing.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateDrawingPayload {
    pub tool: String,
    pub points: JsonValue,
    #[serde(default)]
    pub style: JsonValue,
    pub dataset_version_id: Option<String>,
}

/// Request payload to update an existing drawing.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateDrawingPayload {
    pub points: Option<JsonValue>,
    pub style: Option<JsonValue>,
}

/// A stored template view returned to the client.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TemplateView {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
    pub indicators: JsonValue,
    pub drawings: JsonValue,
    pub comparisons: JsonValue,
    pub dataset_versions: JsonValue,
    pub retired_versions: Vec<String>,
    pub retired_dataset_versions: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request payload to create a new template.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTemplatePayload {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: i32,
    #[serde(default)]
    pub indicators: JsonValue,
    #[serde(default)]
    pub drawings: JsonValue,
    #[serde(default)]
    pub comparisons: JsonValue,
    #[serde(default)]
    pub dataset_versions: JsonValue,
}

fn default_version() -> i32 {
    1
}

/// Rebased comparison series for one symbol.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompareSymbolSeries {
    pub symbol: String,
    pub base_price: f64,
    pub values: Vec<Option<f64>>,
}

/// Output response for symbol comparison.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompareResponse {
    pub primary: CompareSymbolSeries,
    pub secondary: CompareSymbolSeries,
    pub base_symbol: String,
    pub compare_symbols: Vec<String>,
    pub series: BTreeMap<String, Vec<Option<f64>>>,
    pub timestamps: Vec<i64>,
    pub count: usize,
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Drawing Endpoints
// ---------------------------------------------------------------------------

/// Lists active drawings for a chart.
#[utoipa::path(
    get,
    path = "/api/v1/charts/{chart_id}/drawings",
    params(
        ("chart_id" = String, Path, description = "Identifier of the chart, e.g. 'AAPL:1d'")
    ),
    responses(
        (status = 200, description = "List of active chart drawings", body = inline(Vec<DrawingView>)),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Not a workspace member")
    ),
    tag = "charts"
)]
pub(crate) async fn list_drawings(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(chart_id): Path<String>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, false).await?;
    let rows = list_active_drawings(&pool, ws, &chart_id).await.map_err(|e| db_error(&e))?;
    let items: Vec<DrawingView> = rows
        .into_iter()
        .map(|r| DrawingView {
            id: r.id,
            chart_id: r.chart_id,
            dataset_version_id: r.dataset_version_id,
            tool: r.tool,
            points: r.points,
            style: r.style,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();
    Ok(to_json(200, &json!({ "items": items })))
}

/// Creates a new drawing on a chart.
#[utoipa::path(
    post,
    path = "/api/v1/charts/{chart_id}/drawings",
    params(
        ("chart_id" = String, Path, description = "Identifier of the chart, e.g. 'AAPL:1d'")
    ),
    request_body = CreateDrawingPayload,
    responses(
        (status = 201, description = "Drawing created successfully", body = DrawingView),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Viewer cannot create drawings"),
        (status = 422, description = "Validation error or limit exceeded")
    ),
    tag = "charts"
)]
pub(crate) async fn create_drawing(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(chart_id): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, Response> {
    let (pool, user, ws) = scope(&st, &id, true).await?;

    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the request body."));
    };
    let payload: CreateDrawingPayload = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return Err(error_response(422, format!("Invalid JSON payload: {e}"))),
    };

    let new_drawing = NewDrawing {
        chart_id,
        dataset_version_id: payload.dataset_version_id,
        tool: payload.tool,
        points: payload.points,
        style: payload.style,
    };

    let created = db_create_drawing(&pool, ws, user, &new_drawing).await.map_err(|err| match err {
        DrawingError::LimitExceeded => error_with_code(
            422,
            "drawing_limit_exceeded",
            "Chart has reached the maximum drawing limit of 200.",
        ),
        DrawingError::PayloadTooLarge => {
            error_with_code(422, "payload_too_large", "Drawing size exceeds maximum allowed of 64KB.")
        }
        DrawingError::InvalidTool(tool) => {
            invalid_fields(&[("tool".into(), format!("Invalid drawing tool '{tool}'"))])
        }
        DrawingError::Db(e) => db_error(&e),
        DrawingError::Serialization(e) => error_response(422, format!("Invalid JSON payload: {e}")),
    })?;

    let view = DrawingView {
        id: created.id,
        chart_id: created.chart_id,
        dataset_version_id: created.dataset_version_id,
        tool: created.tool,
        points: created.points,
        style: created.style,
        created_at: created.created_at,
        updated_at: created.updated_at,
    };

    Ok(to_json(201, &view))
}

/// Updates an existing drawing.
#[utoipa::path(
    put,
    path = "/api/v1/drawings/{id}",
    params(
        ("id" = Uuid, Path, description = "Drawing ID")
    ),
    request_body = UpdateDrawingPayload,
    responses(
        (status = 200, description = "Drawing updated successfully", body = DrawingView),
        (status = 403, description = "Viewer cannot update drawings"),
        (status = 404, description = "Drawing not found")
    ),
    tag = "charts"
)]
pub(crate) async fn update_drawing(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(drawing_id): Path<Uuid>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, true).await?;

    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the request body."));
    };
    let payload: UpdateDrawingPayload = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return Err(error_response(422, format!("Invalid JSON payload: {e}"))),
    };

    let updated = db_update_drawing(&pool, ws, drawing_id, payload.points, payload.style)
        .await
        .map_err(|e| error_response(422, e.to_string()))?;

    let Some(r) = updated else {
        return Err(error_response(404, "Drawing not found or has been deleted."));
    };

    let view = DrawingView {
        id: r.id,
        chart_id: r.chart_id,
        dataset_version_id: r.dataset_version_id,
        tool: r.tool,
        points: r.points,
        style: r.style,
        created_at: r.created_at,
        updated_at: r.updated_at,
    };

    Ok(to_json(200, &view))
}

/// Soft deletes a drawing.
#[utoipa::path(
    delete,
    path = "/api/v1/drawings/{id}",
    params(
        ("id" = Uuid, Path, description = "Drawing ID")
    ),
    responses(
        (status = 200, description = "Drawing soft-deleted successfully"),
        (status = 403, description = "Viewer cannot delete drawings"),
        (status = 404, description = "Drawing not found")
    ),
    tag = "charts"
)]
pub(crate) async fn delete_drawing(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(drawing_id): Path<Uuid>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, true).await?;

    let deleted = db_soft_delete_drawing(&pool, ws, drawing_id).await.map_err(|e| db_error(&e))?;
    if !deleted {
        return Err(error_response(404, "Drawing not found or already deleted."));
    }

    Ok(to_json(200, &json!({ "deleted": true })))
}

// ---------------------------------------------------------------------------
// Symbol Comparison Endpoint
// ---------------------------------------------------------------------------

/// Compares two symbols by overlaying a secondary symbol rebased to 100 on the first dataset bar.
#[utoipa::path(
    get,
    path = "/api/v1/charts/compare",
    params(
        ("primary" = String, Query, description = "Primary ticker symbol, e.g. 'AAPL'"),
        ("secondary" = String, Query, description = "Secondary ticker symbol to compare, e.g. 'MSFT'"),
        ("timeframe" = String, Query, description = "Native bar timeframe, e.g. '1d'"),
        ("dataset_version_id" = Uuid, Query, description = "Dataset version ID containing the symbols")
    ),
    responses(
        (status = 200, description = "Rebased comparison series with gap alignment", body = CompareResponse),
        (status = 404, description = "Symbol or dataset version not found"),
        (status = 422, description = "Timeframe mismatch or invalid parameter")
    ),
    tag = "charts"
)]
pub(crate) async fn compare_symbols(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Result<Response, Response> {
    let mut errors = Vec::new();

    let primary = match params
        .get("primary")
        .or_else(|| params.get("base_symbol"))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_uppercase(),
        None => {
            errors.push(("primary".to_string(), "primary symbol is required".to_string()));
            String::new()
        }
    };

    let secondary_raw = match params
        .get("secondary")
        .or_else(|| params.get("compare_symbols"))
        .or_else(|| params.get("compare_symbol"))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => {
            errors.push(("secondary".to_string(), "secondary symbol is required".to_string()));
            String::new()
        }
    };

    let timeframe = match params.get("timeframe").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            errors.push(("timeframe".to_string(), "timeframe is required".to_string()));
            String::new()
        }
    };

    let version_id = match params
        .get("dataset_version_id")
        .or_else(|| params.get("dataset_version"))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(s) => match Uuid::parse_str(s) {
            Ok(u) => Some(u),
            Err(_) => {
                errors.push(("dataset_version_id".to_string(), "invalid UUID format".to_string()));
                None
            }
        },
        None => {
            errors.push(("dataset_version_id".to_string(), "dataset_version_id is required".to_string()));
            None
        }
    };

    if !errors.is_empty() {
        return Err(invalid_fields(&errors));
    }

    let version_id = version_id.unwrap();
    let (pool, _user, ws) = scope(&st, &id, false).await?;

    // Check version resolution & sealed holdouts
    let access = spectra_db::datasets::Access { purpose: "compare", user: Uuid::parse_str(&id.user).ok() };
    let data = match spectra_db::datasets::resolve_version(&pool, ws, version_id, access).await {
        Ok(spectra_db::datasets::Resolved::Found(d)) => d,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            tracing::warn!(%version_id, "direct read attempt on sealed holdout refused");
            return Err(crate::optimization_api::sealed_response(&holdout, "comparing chart symbols"));
        }
        Ok(spectra_db::datasets::Resolved::Missing) => {
            return Err(error_response(404, "The dataset version no longer exists."));
        }
        Err(e) => return Err(db_error(&e)),
    };

    // Load dataset bars via existing chart loader
    let dataset = match load_for_chart(&st, &data.checksum, data.blob_sha256.as_deref()).await {
        Ok(d) => d,
        Err(r) => return Err(r),
    };

    // Check primary symbol membership
    let Some(primary_inst) = dataset.symbols.id(&primary) else {
        return Err(invalid_fields(&[(
            "primary".to_string(),
            format!(
                "primary symbol '{primary}' not in this dataset version (available: {})",
                dataset.symbols.symbols.join(", ")
            ),
        )]));
    };

    let mut primary_bars: Vec<Bar> =
        dataset.bars.iter().filter(|b| b.instrument == primary_inst).copied().collect();
    primary_bars.sort_by_key(|b| b.ts_ns);

    if primary_bars.is_empty() {
        return Err(invalid_fields(&[(
            "primary".to_string(),
            format!("no bars found for primary symbol '{primary}' in this dataset"),
        )]));
    }

    // Secondary symbols list (comma-separated or single)
    let sec_symbols: Vec<String> =
        secondary_raw.split(',').map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty()).collect();

    if sec_symbols.is_empty() {
        return Err(invalid_fields(&[("secondary".to_string(), "secondary symbol is required".to_string())]));
    }

    let mut secondary_series_data = Vec::new();
    for sec_sym in &sec_symbols {
        let Some(sec_inst) = dataset.symbols.id(sec_sym) else {
            return Err(invalid_fields(&[(
                "secondary".to_string(),
                format!(
                    "secondary symbol '{sec_sym}' not in this dataset version (available: {})",
                    dataset.symbols.symbols.join(", ")
                ),
            )]));
        };
        let mut sec_bars: Vec<Bar> =
            dataset.bars.iter().filter(|b| b.instrument == sec_inst).copied().collect();
        sec_bars.sort_by_key(|b| b.ts_ns);
        secondary_series_data.push((sec_sym.clone(), sec_bars));
    }

    // Validate timeframe matches native interval
    let primary_deltas: Vec<f64> =
        primary_bars.windows(2).map(|w| (w[1].ts_ns - w[0].ts_ns) as f64).filter(|&d| d > 0.0).collect();
    let median_ns = if !primary_deltas.is_empty() {
        crate::indicators_api::upper_median_pub(primary_deltas) as i64
    } else {
        dataset.quality.symbols.iter().find(|s| s.symbol == primary).map_or(0, |s| s.median_interval_ns)
    };

    if let Err(tf_err) = check_timeframe_pub(&timeframe, median_ns) {
        return Err(error_with_code(
            422,
            "timeframe_mismatch",
            format!("timeframe '{timeframe}' does not match native bar interval: {tf_err}"),
        ));
    }

    // Rebase base bar: First bar of the full dataset (Decision 2 recommendation)
    let primary_base_price = primary_bars[0].close.to_f64();

    // Map timestamps to bars
    let primary_map: BTreeMap<i64, f64> = primary_bars.iter().map(|b| (b.ts_ns, b.close.to_f64())).collect();

    // Union of timestamps sorted across primary and all secondary symbols
    let mut all_ts = BTreeSet::new();
    for b in &primary_bars {
        all_ts.insert(b.ts_ns);
    }
    for (_, sec_bars) in &secondary_series_data {
        for b in sec_bars {
            all_ts.insert(b.ts_ns);
        }
    }
    let timestamps: Vec<i64> = all_ts.into_iter().collect();

    // Rebase to 100.0: gap on missing bars
    let mut primary_rebased = Vec::with_capacity(timestamps.len());
    for &ts in &timestamps {
        let p_val = primary_map.get(&ts).map(|&p| (p / primary_base_price) * 100.0);
        primary_rebased.push(p_val);
    }

    let mut series_map = BTreeMap::new();
    series_map.insert(primary.clone(), primary_rebased.clone());

    let mut first_secondary_series = None;
    let mut messages = Vec::new();

    for (sec_sym, sec_bars) in &secondary_series_data {
        let sec_base_price = sec_bars.first().map_or(1.0, |b| b.close.to_f64());
        if sec_bars.is_empty() {
            messages.push(format!("No data available for symbol '{sec_sym}' in the selected dataset"));
        }
        let sec_map: BTreeMap<i64, f64> = sec_bars.iter().map(|b| (b.ts_ns, b.close.to_f64())).collect();

        let mut sec_rebased = Vec::with_capacity(timestamps.len());
        for &ts in &timestamps {
            let s_val = sec_map.get(&ts).map(|&p| (p / sec_base_price) * 100.0);
            sec_rebased.push(s_val);
        }

        let symbol_series = CompareSymbolSeries {
            symbol: sec_sym.clone(),
            base_price: sec_base_price,
            values: sec_rebased.clone(),
        };

        if first_secondary_series.is_none() {
            first_secondary_series = Some(symbol_series);
        }
        series_map.insert(sec_sym.clone(), sec_rebased);
    }

    let secondary = first_secondary_series.unwrap_or_else(|| CompareSymbolSeries {
        symbol: String::new(),
        base_price: 1.0,
        values: vec![None; timestamps.len()],
    });

    let message = if messages.is_empty() { None } else { Some(messages.join("; ")) };

    let resp = CompareResponse {
        primary: CompareSymbolSeries {
            symbol: primary.clone(),
            base_price: primary_base_price,
            values: primary_rebased,
        },
        secondary,
        base_symbol: primary,
        compare_symbols: sec_symbols,
        series: series_map,
        count: timestamps.len(),
        timestamps,
        message,
    };

    Ok(to_json(200, &resp))
}

// ---------------------------------------------------------------------------
// Template Endpoints
// ---------------------------------------------------------------------------

/// Lists saved chart templates for the workspace.
#[utoipa::path(
    get,
    path = "/api/v1/chart-templates",
    responses(
        (status = 200, description = "List of saved chart templates", body = inline(Vec<TemplateView>)),
        (status = 401, description = "Not authenticated")
    ),
    tag = "charts"
)]
pub(crate) async fn list_templates(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, false).await?;
    let rows = db_list_templates(&pool, ws).await.map_err(|e| db_error(&e))?;

    let items: Vec<TemplateView> = rows
        .into_iter()
        .map(|r| TemplateView {
            id: r.id,
            name: r.name,
            version: r.version,
            indicators: r.indicators,
            drawings: r.drawings,
            comparisons: r.comparisons,
            dataset_versions: r.dataset_versions,
            retired_versions: Vec::new(),
            retired_dataset_versions: Vec::new(),
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();

    Ok(to_json(200, &json!({ "items": items })))
}

/// Creates a new saved chart template.
#[utoipa::path(
    post,
    path = "/api/v1/chart-templates",
    request_body = CreateTemplatePayload,
    responses(
        (status = 201, description = "Template created successfully", body = TemplateView),
        (status = 403, description = "Viewer cannot create templates"),
        (status = 422, description = "Workspace limit of 50 templates reached or invalid name")
    ),
    tag = "charts"
)]
pub(crate) async fn create_template(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, Response> {
    let (pool, user, ws) = scope(&st, &id, true).await?;

    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the request body."));
    };
    let payload: CreateTemplatePayload = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => return Err(error_response(422, format!("Invalid JSON payload: {e}"))),
    };

    let new_tmpl = NewTemplate {
        name: payload.name,
        version: payload.version,
        indicators: payload.indicators,
        drawings: payload.drawings,
        comparisons: payload.comparisons,
        dataset_versions: payload.dataset_versions,
    };

    let created = db_create_template(&pool, ws, user, &new_tmpl).await.map_err(|err| match err {
        TemplateError::LimitExceeded => error_with_code(
            422,
            "template_limit_exceeded",
            "Workspace has reached the maximum template limit of 50.",
        ),
        TemplateError::InvalidName => invalid_fields(&[(
            "name".into(),
            "Template name cannot be empty or longer than 100 characters".into(),
        )]),
        TemplateError::Db(e) => db_error(&e),
    })?;

    let view = TemplateView {
        id: created.id,
        name: created.name,
        version: created.version,
        indicators: created.indicators,
        drawings: created.drawings,
        comparisons: created.comparisons,
        dataset_versions: created.dataset_versions,
        retired_versions: Vec::new(),
        retired_dataset_versions: Vec::new(),
        created_at: created.created_at,
        updated_at: created.updated_at,
    };

    Ok(to_json(201, &view))
}

/// Retrieves a saved chart template by ID.
#[utoipa::path(
    get,
    path = "/api/v1/chart-templates/{id}",
    params(
        ("id" = Uuid, Path, description = "Template ID")
    ),
    responses(
        (status = 200, description = "Template details", body = TemplateView),
        (status = 404, description = "Template not found")
    ),
    tag = "charts"
)]
pub(crate) async fn get_template(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(tmpl_id): Path<Uuid>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, false).await?;

    let tmpl = db_get_template(&pool, ws, tmpl_id).await.map_err(|e| db_error(&e))?;
    let Some(r) = tmpl else {
        return Err(error_response(404, "Template not found."));
    };

    // Check if recorded dataset versions are missing or retired in workspace
    let mut retired_versions = Vec::new();
    if let Some(map) = r.dataset_versions.as_object() {
        for (_k, v) in map {
            if let Some(v_str) = v.as_str() {
                if let Ok(ver_uuid) = Uuid::parse_str(v_str) {
                    let exists: Option<(Uuid,)> =
                        sqlx::query_as("SELECT id FROM dataset_versions WHERE id = $1 AND workspace_id = $2")
                            .bind(ver_uuid)
                            .bind(ws)
                            .fetch_optional(&pool)
                            .await
                            .unwrap_or(None);

                    if exists.is_none() {
                        retired_versions.push(v_str.to_string());
                    }
                }
            }
        }
    }

    let view = TemplateView {
        id: r.id,
        name: r.name,
        version: r.version,
        indicators: r.indicators,
        drawings: r.drawings,
        comparisons: r.comparisons,
        dataset_versions: r.dataset_versions,
        retired_dataset_versions: retired_versions.clone(),
        retired_versions,
        created_at: r.created_at,
        updated_at: r.updated_at,
    };

    Ok(to_json(200, &view))
}

/// Deletes a saved chart template.
#[utoipa::path(
    delete,
    path = "/api/v1/chart-templates/{id}",
    params(
        ("id" = Uuid, Path, description = "Template ID")
    ),
    responses(
        (status = 200, description = "Template deleted successfully"),
        (status = 403, description = "Viewer cannot delete templates"),
        (status = 404, description = "Template not found")
    ),
    tag = "charts"
)]
pub(crate) async fn delete_template(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(tmpl_id): Path<Uuid>,
) -> Result<Response, Response> {
    let (pool, _user, ws) = scope(&st, &id, true).await?;

    let deleted = db_delete_template(&pool, ws, tmpl_id).await.map_err(|e| db_error(&e))?;
    if !deleted {
        return Err(error_response(404, "Template not found."));
    }

    Ok(to_json(200, &json!({ "deleted": true })))
}
