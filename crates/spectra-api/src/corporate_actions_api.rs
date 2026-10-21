//! Corporate actions routes (M1b task 11): manage split and dividend action sets.

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
use crate::{error_response, invalid_fields, AppState, Identity};

fn default_one() -> i32 {
    1
}

/// A corporate action item in an upload or creation request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CorporateActionItem {
    /// Instrument ticker symbol.
    pub symbol: String,
    /// Ex-action timestamp (RFC 3339 or YYYY-MM-DD).
    pub ex_date: String,
    /// "split" or "dividend".
    pub action_kind: String,
    /// For splits: original ratio (e.g. 1 in 2:1). Default: 1.
    #[serde(default = "default_one")]
    pub split_from: i32,
    /// For splits: new ratio (e.g. 2 in 2:1). Default: 1.
    #[serde(default = "default_one")]
    pub split_to: i32,
    /// For cash dividends: dividend amount per share (e.g. "0.205").
    #[serde(default)]
    pub dividend_amount: Option<String>,
}

/// `POST /api/v1/corporate-actions` request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCorporateActionSetRequest {
    /// Action set name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Corporate actions list.
    #[serde(default)]
    pub actions: Vec<CorporateActionItem>,
    /// Optional CSV text containing actions with headers:
    /// `symbol,ex_date,action_kind,split_from,split_to,dividend_amount`.
    #[serde(default)]
    pub csv: Option<String>,
}

/// A corporate action view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CorporateActionView {
    /// Action id.
    pub id: Uuid,
    /// Instrument symbol.
    pub symbol: String,
    /// Ex-action date.
    pub ex_date: DateTime<Utc>,
    /// "split" or "dividend".
    pub action_kind: String,
    /// Split from.
    pub split_from: i32,
    /// Split to.
    pub split_to: i32,
    /// Cash dividend amount per share.
    pub dividend_amount: String,
    /// Recorded at.
    pub created_at: DateTime<Utc>,
}

/// Summary of a corporate action set.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CorporateActionSetSummaryView {
    /// Set id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Action count.
    pub action_count: i64,
}

/// List of corporate action sets.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CorporateActionSetListView {
    /// Items.
    pub items: Vec<CorporateActionSetSummaryView>,
}

/// Detail view of a corporate action set.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CorporateActionSetDetailView {
    /// Set id.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Created at.
    pub created_at: DateTime<Utc>,
    /// Actions in the set.
    pub actions: Vec<CorporateActionView>,
}

/// Result of deleting a corporate action set.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeletedActionSetView {
    /// True when deleted.
    pub deleted: bool,
}

const NO_SET: &str = "No such corporate action set.";

/// List corporate action sets for the active workspace.
#[utoipa::path(get, path = "/api/v1/corporate-actions", tag = "corporate-actions",
    responses(
        (status = 200, body = CorporateActionSetListView),
    ))]
pub(crate) async fn list_corporate_action_sets(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::corporate_actions::list_action_sets(&pool, ws).await {
        Ok(sets) => to_json(
            200,
            &CorporateActionSetListView {
                items: sets
                    .into_iter()
                    .map(|s| CorporateActionSetSummaryView {
                        id: s.id,
                        name: s.name,
                        description: s.description,
                        created_at: s.created_at,
                        action_count: s.action_count,
                    })
                    .collect(),
            },
        ),
        Err(e) => db_error(&e),
    }
}

fn parse_ex_date(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc());
    }
    None
}

/// Create a corporate action set and its actions.
#[utoipa::path(post, path = "/api/v1/corporate-actions", tag = "corporate-actions",
    request_body = CreateCorporateActionSetRequest,
    responses(
        (status = 201, body = CorporateActionSetDetailView),
        (status = 409, description = "Name already in use", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Validation errors", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn create_corporate_action_set(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let req: CreateCorporateActionSetRequest = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid corporate actions request: {e}")),
    };

    let mut errors: Vec<(String, String)> = Vec::new();
    let name = req.name.trim();
    if name.is_empty() {
        errors.push(("name".to_string(), "Name cannot be empty.".to_string()));
    } else if name.len() > 200 {
        errors.push(("name".to_string(), "Name must be at most 200 characters.".to_string()));
    }

    let mut actions = req.actions.clone();

    // If CSV provided, parse CSV rows
    if let Some(csv_text) = &req.csv {
        let lines: Vec<&str> = csv_text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            let mut start_idx = 0;
            let header = lines[0].to_ascii_lowercase();
            if header.contains("symbol") {
                start_idx = 1;
            }
            for (idx, line) in lines[start_idx..].iter().enumerate() {
                let parts: Vec<&str> = line.split(',').map(str::trim).collect();
                if parts.len() < 3 {
                    errors.push((
                        format!("csv_line_{}", idx + start_idx + 1),
                        "Expected at least: symbol,ex_date,action_kind".to_string(),
                    ));
                    continue;
                }
                let sym = parts[0].to_string();
                let ex = parts[1].to_string();
                let kind = parts[2].to_ascii_lowercase();
                let split_from = parts.get(3).and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
                let split_to = parts.get(4).and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
                let dividend_amount = parts.get(5).map(|v| (*v).to_string());
                actions.push(CorporateActionItem {
                    symbol: sym,
                    ex_date: ex,
                    action_kind: kind,
                    split_from,
                    split_to,
                    dividend_amount,
                });
            }
        }
    }

    // Validate actions
    let mut validated_actions: Vec<(String, DateTime<Utc>, String, i32, i32, String)> = Vec::new();
    for (i, act) in actions.iter().enumerate() {
        let sym = act.symbol.trim().to_uppercase();
        if sym.is_empty() {
            errors.push((format!("actions[{i}].symbol"), "Symbol cannot be empty.".to_string()));
        }
        let ex_dt = if let Some(dt) = parse_ex_date(&act.ex_date) {
            dt
        } else {
            errors.push((
                format!("actions[{i}].ex_date"),
                "Invalid date: use YYYY-MM-DD or RFC 3339.".to_string(),
            ));
            Utc::now()
        };
        let kind = act.action_kind.trim().to_ascii_lowercase();
        if kind != "split" && kind != "dividend" {
            errors.push((format!("actions[{i}].action_kind"), "Must be 'split' or 'dividend'.".to_string()));
        }
        let split_from = act.split_from;
        let split_to = act.split_to;
        if kind == "split" {
            if split_from <= 0 {
                errors.push((
                    format!("actions[{i}].split_from"),
                    "split_from must be greater than zero.".to_string(),
                ));
            }
            if split_to <= 0 {
                errors.push((
                    format!("actions[{i}].split_to"),
                    "split_to must be greater than zero.".to_string(),
                ));
            }
        }
        let div_amt_str = act.dividend_amount.as_deref().unwrap_or("0").trim();
        if kind == "dividend" {
            match spectra_data::parse_fixed(div_amt_str) {
                Some(raw) if raw > 0 => {}
                _ => {
                    errors.push((
                        format!("actions[{i}].dividend_amount"),
                        "dividend_amount must be a positive decimal number.".to_string(),
                    ));
                }
            }
        }

        validated_actions.push((sym, ex_dt, kind, split_from, split_to, div_amt_str.to_string()));
    }

    if !errors.is_empty() {
        return invalid_fields(&errors);
    }

    let description = req.description.as_deref().unwrap_or("").trim();
    let user_id = Uuid::parse_str(&id.user).ok();
    let set_row =
        match spectra_db::corporate_actions::create_action_set(&pool, ws, name, description, user_id).await {
            Ok(s) => s,
            Err(e) => {
                if let spectra_db::Error::Database(ref d) = e {
                    if d.code().as_deref() == Some("23505") {
                        return crate::error_with_code(
                            409,
                            "conflict",
                            "A corporate action set with that name already exists.",
                        );
                    }
                }
                return db_error(&e);
            }
        };

    let mut created_actions = Vec::new();
    for (sym, ex_dt, kind, split_from, split_to, div_amt) in validated_actions {
        let aid = match spectra_db::corporate_actions::insert_action(
            &pool, ws, set_row.id, &sym, ex_dt, &kind, split_from, split_to, &div_amt,
        )
        .await
        {
            Ok(aid) => aid,
            Err(e) => return db_error(&e),
        };
        created_actions.push(CorporateActionView {
            id: aid,
            symbol: sym,
            ex_date: ex_dt,
            action_kind: kind,
            split_from,
            split_to,
            dividend_amount: div_amt,
            created_at: Utc::now(),
        });
    }

    to_json(
        201,
        &CorporateActionSetDetailView {
            id: set_row.id,
            name: set_row.name,
            description: set_row.description,
            created_at: set_row.created_at,
            actions: created_actions,
        },
    )
}

/// Fetch a corporate action set and all its events.
#[utoipa::path(get, path = "/api/v1/corporate-actions/{id}", tag = "corporate-actions",
    params(("id" = Uuid, Path, description = "Set id")),
    responses(
        (status = 200, body = CorporateActionSetDetailView),
        (status = 404, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn get_corporate_action_set(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(set_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let set_row = match spectra_db::corporate_actions::get_action_set(&pool, ws, set_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return error_response(404, NO_SET),
        Err(e) => return db_error(&e),
    };
    let actions = match spectra_db::corporate_actions::list_actions_by_set(&pool, ws, set_id).await {
        Ok(acts) => acts,
        Err(e) => return db_error(&e),
    };

    to_json(
        200,
        &CorporateActionSetDetailView {
            id: set_row.id,
            name: set_row.name,
            description: set_row.description,
            created_at: set_row.created_at,
            actions: actions
                .into_iter()
                .map(|a| CorporateActionView {
                    id: a.id,
                    symbol: a.symbol,
                    ex_date: a.ex_date,
                    action_kind: a.action_kind,
                    split_from: a.split_from,
                    split_to: a.split_to,
                    dividend_amount: a.dividend_amount,
                    created_at: a.created_at,
                })
                .collect(),
        },
    )
}

/// Delete a corporate action set.
#[utoipa::path(delete, path = "/api/v1/corporate-actions/{id}", tag = "corporate-actions",
    params(("id" = Uuid, Path, description = "Set id")),
    responses(
        (status = 200, body = DeletedActionSetView),
        (status = 404, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn delete_corporate_action_set(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    Path(set_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match spectra_db::corporate_actions::delete_action_set(&pool, ws, set_id).await {
        Ok(true) => to_json(200, &DeletedActionSetView { deleted: true }),
        Ok(false) => error_response(404, NO_SET),
        Err(e) => db_error(&e),
    }
}
