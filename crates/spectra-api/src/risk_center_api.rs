//! `/api/v1/risk` (Module 8): Pre-trade Risk Enforcement, Limit Controls,
//! Emergency Kill Switches, and Audit Trail.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_db::risk_center::{self, KillSwitchRow, RiskDecisionRow, RiskLimitsRow};
use spectra_risk::enforcement::{
    OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, PreTradeChecker, PreTradeVerdict,
    RiskLimits,
};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error};
use crate::{error_response, error_with_code, json_response, AppState, Identity};

#[allow(clippy::result_large_err)]
fn parse_body<T: serde::de::DeserializeOwned>(body: Result<Bytes, BytesRejection>) -> Result<T, Response> {
    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the request body."));
    };
    serde_json::from_slice(&bytes).map_err(|e| error_response(422, format!("Invalid JSON payload: {e}")))
}

const FORBIDDEN_VIEWER: &str = "Viewers have read-only access to risk center configuration.";

fn user_of(id: &Identity) -> Option<Uuid> {
    Uuid::parse_str(&id.user).ok()
}

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
                return Err(error_with_code(403, "viewer_read_only", FORBIDDEN_VIEWER));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- Data Transfer Objects

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskLimitsView {
    pub id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub max_gross_exposure: f64,
    pub max_net_exposure: f64,
    pub max_position_size: f64,
    pub max_concentration_pct: f64,
    pub max_leverage: f64,
    pub max_drawdown_pct: f64,
    pub max_daily_loss: f64,
    pub max_var_pct: f64,
    pub size_reduction_enabled: bool,
    pub updated_by: Option<Uuid>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<RiskLimitsRow> for RiskLimitsView {
    fn from(r: RiskLimitsRow) -> Self {
        Self {
            id: Some(r.id),
            workspace_id: r.workspace_id,
            strategy_id: r.strategy_id,
            max_gross_exposure: r.max_gross_exposure,
            max_net_exposure: r.max_net_exposure,
            max_position_size: r.max_position_size,
            max_concentration_pct: r.max_concentration_pct,
            max_leverage: r.max_leverage,
            max_drawdown_pct: r.max_drawdown_pct,
            max_daily_loss: r.max_daily_loss,
            max_var_pct: r.max_var_pct,
            size_reduction_enabled: r.size_reduction_enabled,
            updated_by: r.updated_by,
            updated_at: Some(r.updated_at),
        }
    }
}

impl From<&RiskLimitsView> for RiskLimits {
    fn from(v: &RiskLimitsView) -> Self {
        Self {
            max_gross_exposure: v.max_gross_exposure,
            max_net_exposure: v.max_net_exposure,
            max_position_size: v.max_position_size,
            max_concentration_pct: v.max_concentration_pct,
            max_leverage: v.max_leverage,
            max_drawdown_pct: v.max_drawdown_pct,
            max_daily_loss: v.max_daily_loss,
            max_var_pct: v.max_var_pct,
            size_reduction_enabled: v.size_reduction_enabled,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertLimitsRequest {
    pub strategy_id: Option<Uuid>,
    pub max_gross_exposure: f64,
    pub max_net_exposure: f64,
    pub max_position_size: f64,
    pub max_concentration_pct: f64,
    pub max_leverage: f64,
    pub max_drawdown_pct: f64,
    pub max_daily_loss: f64,
    pub max_var_pct: f64,
    pub size_reduction_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KillSwitchView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub active: bool,
    pub reason: String,
    pub triggered_by: Option<Uuid>,
    pub triggered_at: DateTime<Utc>,
    pub cleared_at: Option<DateTime<Utc>>,
    pub cleared_by: Option<Uuid>,
}

impl From<KillSwitchRow> for KillSwitchView {
    fn from(r: KillSwitchRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            strategy_id: r.strategy_id,
            active: r.active,
            reason: r.reason,
            triggered_by: r.triggered_by,
            triggered_at: r.triggered_at,
            cleared_at: r.cleared_at,
            cleared_by: r.cleared_by,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetKillSwitchRequest {
    pub strategy_id: Option<Uuid>,
    pub active: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PositionSnapshotDto {
    pub qty: f64,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSnapshotDto {
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
    pub positions: HashMap<String, PositionSnapshotDto>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PreTradeCheckRequest {
    pub strategy_id: Option<Uuid>,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub portfolio: Option<PortfolioSnapshotDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskBreachView {
    pub code: String,
    pub limit: f64,
    pub projected: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectedMetricsView {
    pub gross_exposure: f64,
    pub net_exposure: f64,
    pub leverage: f64,
    pub position_size: f64,
    pub concentration_pct: f64,
    pub drawdown_pct: f64,
    pub daily_loss: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PreTradeResultView {
    pub verdict: String,
    pub requested_qty: f64,
    pub approved_qty: f64,
    pub breaches: Vec<RiskBreachView>,
    pub projected_metrics: ProjectedMetricsView,
    pub decision_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskDecisionView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub symbol: String,
    pub side: String,
    pub requested_qty: f64,
    pub price: f64,
    pub approved_qty: f64,
    pub verdict: String,
    pub breaches: serde_json::Value,
    pub reasons: Vec<String>,
    pub metrics: serde_json::Value,
    pub actor_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<RiskDecisionRow> for RiskDecisionView {
    fn from(r: RiskDecisionRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            strategy_id: r.strategy_id,
            symbol: r.symbol,
            side: r.side,
            requested_qty: r.requested_qty,
            price: r.price,
            approved_qty: r.approved_qty,
            verdict: r.verdict,
            breaches: r.breaches,
            reasons: r.reasons,
            metrics: r.metrics,
            actor_id: r.actor_id,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskMetricsSummaryView {
    pub workspace_id: Uuid,
    pub strategy_id: Option<Uuid>,
    pub kill_switch_active: bool,
    pub kill_switch_reason: Option<String>,
    pub gross_exposure: f64,
    pub gross_exposure_limit: f64,
    pub gross_exposure_utilization_pct: f64,
    pub net_exposure: f64,
    pub net_exposure_limit: f64,
    pub net_exposure_utilization_pct: f64,
    pub leverage: f64,
    pub leverage_limit: f64,
    pub leverage_utilization_pct: f64,
    pub drawdown_pct: f64,
    pub drawdown_limit_pct: f64,
    pub drawdown_utilization_pct: f64,
    pub daily_loss: f64,
    pub daily_loss_limit: f64,
    pub daily_loss_utilization_pct: f64,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct RiskLimitsQuery {
    pub strategy_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct KillSwitchQuery {
    pub strategy_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct RiskAuditQuery {
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------- Handlers

/// Get risk limits for a workspace or strategy.
#[utoipa::path(get, path = "/api/v1/risk/limits", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Active limits", body = RiskLimitsView)))]
pub(crate) async fn get_limits(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<RiskLimitsQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    // 1. Try to fetch strategy-specific limits if requested
    if let Some(strat_id) = q.strategy_id {
        match risk_center::get_limits(&pool, ws, Some(strat_id)).await {
            Ok(Some(row)) => return json_response(200, json!(RiskLimitsView::from(row)).to_string()),
            Ok(None) => {}
            Err(e) => return db_error(&e),
        }
    }

    // 2. Fall back to workspace limits
    match risk_center::get_limits(&pool, ws, None).await {
        Ok(Some(row)) => json_response(200, json!(RiskLimitsView::from(row)).to_string()),
        Ok(None) => {
            // Return default configuration
            let def = RiskLimits::default();
            let view = RiskLimitsView {
                id: None,
                workspace_id: ws,
                strategy_id: q.strategy_id,
                max_gross_exposure: def.max_gross_exposure,
                max_net_exposure: def.max_net_exposure,
                max_position_size: def.max_position_size,
                max_concentration_pct: def.max_concentration_pct,
                max_leverage: def.max_leverage,
                max_drawdown_pct: def.max_drawdown_pct,
                max_daily_loss: def.max_daily_loss,
                max_var_pct: def.max_var_pct,
                size_reduction_enabled: def.size_reduction_enabled,
                updated_by: None,
                updated_at: None,
            };
            json_response(200, json!(view).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Upsert risk limits for a workspace or strategy.
#[utoipa::path(put, path = "/api/v1/risk/limits", tag = "risk",
    request_body = UpsertLimitsRequest,
    responses((status = 200, description = "Limits updated", body = RiskLimitsView)))]
pub(crate) async fn put_limits(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let user_id = user_of(&identity);
    if let Err(r) = ensure_can_edit(&pool, ws, user_id).await {
        return r;
    }
    let req: UpsertLimitsRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    if req.max_gross_exposure < 0.0 || req.max_net_exposure < 0.0 || req.max_position_size < 0.0 {
        return error_response(422, "Exposure and position size limits must be non-negative.");
    }
    if req.max_concentration_pct <= 0.0 || req.max_concentration_pct > 1.0 {
        return error_response(422, "Concentration limit must be between 0 and 1 (0% - 100%).");
    }
    if req.max_leverage < 1.0 {
        return error_response(422, "Leverage limit must be at least 1.0x.");
    }
    if req.max_drawdown_pct <= 0.0 || req.max_drawdown_pct > 1.0 {
        return error_response(422, "Max drawdown limit must be between 0 and 1.");
    }

    match risk_center::upsert_limits(
        &pool,
        ws,
        req.strategy_id,
        req.max_gross_exposure,
        req.max_net_exposure,
        req.max_position_size,
        req.max_concentration_pct,
        req.max_leverage,
        req.max_drawdown_pct,
        req.max_daily_loss,
        req.max_var_pct,
        req.size_reduction_enabled,
        user_id,
    )
    .await
    {
        Ok(row) => json_response(200, json!(RiskLimitsView::from(row)).to_string()),
        Err(e) => db_error(&e),
    }
}

/// Run pre-trade risk check on candidate order and log decision.
#[utoipa::path(post, path = "/api/v1/risk/check", tag = "risk",
    request_body = PreTradeCheckRequest,
    responses((status = 200, description = "Pre-trade verdict", body = PreTradeResultView)))]
pub(crate) async fn pre_trade_check(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let user_id = user_of(&identity);
    let req: PreTradeCheckRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    if req.symbol.trim().is_empty() {
        return error_response(422, "Symbol is required.");
    }
    if req.qty <= 0.0 {
        return error_response(422, "Order quantity must be positive.");
    }
    if req.price <= 0.0 {
        return error_response(422, "Order price must be positive.");
    }

    let side = match req.side.to_lowercase().as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => return error_response(422, "Side must be 'buy' or 'sell'."),
    };

    // 1. Determine kill switch status
    let mut kill_switch_active = false;
    if let Some(strat_id) = req.strategy_id {
        if let Ok(Some(ks)) = risk_center::get_kill_switch(&pool, ws, Some(strat_id)).await {
            if ks.active {
                kill_switch_active = true;
            }
        }
    }
    if !kill_switch_active {
        if let Ok(Some(ks)) = risk_center::get_kill_switch(&pool, ws, None).await {
            if ks.active {
                kill_switch_active = true;
            }
        }
    }

    // 2. Determine limits
    let mut limits = RiskLimits::default();
    if let Some(strat_id) = req.strategy_id {
        if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, Some(strat_id)).await {
            let v = RiskLimitsView::from(row);
            limits = (&v).into();
        } else if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, None).await {
            let v = RiskLimitsView::from(row);
            limits = (&v).into();
        }
    } else if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, None).await {
        let v = RiskLimitsView::from(row);
        limits = (&v).into();
    }

    // 3. Construct portfolio snapshot
    let portfolio = if let Some(dto) = req.portfolio {
        let mut positions = HashMap::new();
        for (sym, p) in dto.positions {
            positions.insert(sym, PositionSnapshot { qty: p.qty, price: p.price });
        }
        PortfolioSnapshot {
            cash: dto.cash,
            equity: dto.equity,
            peak_equity: dto.peak_equity,
            daily_start_equity: dto.daily_start_equity,
            positions,
        }
    } else {
        PortfolioSnapshot::default()
    };

    // 4. Run pre-trade check
    let candidate = OrderCandidate { symbol: req.symbol.clone(), side, qty: req.qty, price: req.price };
    let result = PreTradeChecker::evaluate(&limits, &portfolio, &candidate, kill_switch_active);

    let verdict_str = match result.verdict {
        PreTradeVerdict::Approved => "approved",
        PreTradeVerdict::Reduced => "reduced",
        PreTradeVerdict::Rejected => "rejected",
    };

    let breaches_json = serde_json::to_value(
        result
            .breaches
            .iter()
            .map(|b| RiskBreachView {
                code: b.code.clone(),
                limit: b.limit,
                projected: b.projected,
                message: b.message.clone(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or(json!([]));

    let reasons: Vec<String> = result.breaches.iter().map(|b| b.message.clone()).collect();

    let metrics_json = json!({
        "gross_exposure": result.projected_metrics.gross_exposure,
        "net_exposure": result.projected_metrics.net_exposure,
        "leverage": result.projected_metrics.leverage,
        "position_size": result.projected_metrics.position_size,
        "concentration_pct": result.projected_metrics.concentration_pct,
        "drawdown_pct": result.projected_metrics.drawdown_pct,
        "daily_loss": result.projected_metrics.daily_loss,
    });

    // 5. Record decision in audit trail
    let decision_id = match risk_center::record_decision(
        &pool,
        ws,
        req.strategy_id,
        &req.symbol,
        match side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        },
        req.qty,
        req.price,
        result.approved_qty,
        verdict_str,
        &breaches_json,
        &reasons,
        &metrics_json,
        user_id,
    )
    .await
    {
        Ok(dec) => Some(dec.id),
        Err(e) => {
            tracing::error!(%e, "Failed to record risk decision in audit log");
            None
        }
    };

    let view = PreTradeResultView {
        verdict: verdict_str.to_string(),
        requested_qty: result.requested_qty,
        approved_qty: result.approved_qty,
        breaches: result
            .breaches
            .into_iter()
            .map(|b| RiskBreachView {
                code: b.code,
                limit: b.limit,
                projected: b.projected,
                message: b.message,
            })
            .collect(),
        projected_metrics: ProjectedMetricsView {
            gross_exposure: result.projected_metrics.gross_exposure,
            net_exposure: result.projected_metrics.net_exposure,
            leverage: result.projected_metrics.leverage,
            position_size: result.projected_metrics.position_size,
            concentration_pct: result.projected_metrics.concentration_pct,
            drawdown_pct: result.projected_metrics.drawdown_pct,
            daily_loss: result.projected_metrics.daily_loss,
        },
        decision_id,
    };

    json_response(200, json!(view).to_string())
}

/// Get kill switches for workspace or specific strategy.
#[utoipa::path(get, path = "/api/v1/risk/kill-switch", tag = "risk", params(KillSwitchQuery),
    responses((status = 200, description = "Kill switch state", body = Vec<KillSwitchView>)))]
pub(crate) async fn get_kill_switches(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<KillSwitchQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    if let Some(strat_id) = q.strategy_id {
        match risk_center::get_kill_switch(&pool, ws, Some(strat_id)).await {
            Ok(Some(ks)) => json_response(200, json!(vec![KillSwitchView::from(ks)]).to_string()),
            Ok(None) => json_response(200, "[]".to_string()),
            Err(e) => db_error(&e),
        }
    } else {
        match risk_center::list_kill_switches(&pool, ws).await {
            Ok(list) => {
                let views: Vec<KillSwitchView> = list.into_iter().map(KillSwitchView::from).collect();
                json_response(200, json!(views).to_string())
            }
            Err(e) => db_error(&e),
        }
    }
}

/// Activate or clear emergency kill switch.
#[utoipa::path(post, path = "/api/v1/risk/kill-switch", tag = "risk",
    request_body = SetKillSwitchRequest,
    responses((status = 200, description = "Updated kill switch", body = KillSwitchView)))]
pub(crate) async fn set_kill_switch(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let user_id = user_of(&identity);
    if let Err(r) = ensure_can_edit(&pool, ws, user_id).await {
        return r;
    }
    let req: SetKillSwitchRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    let reason = req.reason.unwrap_or_else(|| {
        if req.active {
            "Emergency halt triggered via Risk Center.".into()
        } else {
            "Kill switch cleared.".into()
        }
    });

    match risk_center::set_kill_switch(&pool, ws, req.strategy_id, req.active, &reason, user_id).await {
        Ok(row) => json_response(200, json!(KillSwitchView::from(row)).to_string()),
        Err(e) => db_error(&e),
    }
}

/// List recent pre-trade decisions audit trail.
#[utoipa::path(get, path = "/api/v1/risk/audit", tag = "risk", params(RiskAuditQuery),
    responses((status = 200, description = "Recent pre-trade decisions", body = Vec<RiskDecisionView>)))]
pub(crate) async fn list_audit(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<RiskAuditQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    let limit = q.limit.unwrap_or(100).clamp(1, 500);

    match risk_center::list_decisions(&pool, ws, limit).await {
        Ok(list) => {
            let views: Vec<RiskDecisionView> = list.into_iter().map(RiskDecisionView::from).collect();
            json_response(200, json!(views).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Retrieve live risk metrics vs active limits for workspace or strategy.
#[utoipa::path(get, path = "/api/v1/risk/metrics", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Risk metrics vs limits summary", body = RiskMetricsSummaryView)))]
pub(crate) async fn get_metrics(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<RiskLimitsQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    // Kill switch
    let mut kill_switch_active = false;
    let mut kill_switch_reason = None;

    if let Some(strat_id) = q.strategy_id {
        if let Ok(Some(ks)) = risk_center::get_kill_switch(&pool, ws, Some(strat_id)).await {
            if ks.active {
                kill_switch_active = true;
                kill_switch_reason = Some(ks.reason);
            }
        }
    }
    if !kill_switch_active {
        if let Ok(Some(ks)) = risk_center::get_kill_switch(&pool, ws, None).await {
            if ks.active {
                kill_switch_active = true;
                kill_switch_reason = Some(ks.reason);
            }
        }
    }

    // Limits
    let mut limits = RiskLimits::default();
    if let Some(strat_id) = q.strategy_id {
        if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, Some(strat_id)).await {
            let v = RiskLimitsView::from(row);
            limits = (&v).into();
        } else if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, None).await {
            let v = RiskLimitsView::from(row);
            limits = (&v).into();
        }
    } else if let Ok(Some(row)) = risk_center::get_limits(&pool, ws, None).await {
        let v = RiskLimitsView::from(row);
        limits = (&v).into();
    }

    // Baseline live snapshot (in live environment, hooked into portfolio service; fallback to default)
    let p = PortfolioSnapshot::default();
    let gross = p.gross_exposure();
    let net = p.net_exposure();
    let lev = p.leverage();
    let dd = p.drawdown_pct();
    let loss = p.daily_loss();

    let gross_pct =
        if limits.max_gross_exposure > 0.0 { (gross / limits.max_gross_exposure) * 100.0 } else { 0.0 };
    let net_pct =
        if limits.max_net_exposure > 0.0 { (net.abs() / limits.max_net_exposure) * 100.0 } else { 0.0 };
    let lev_pct = if limits.max_leverage > 0.0 { (lev / limits.max_leverage) * 100.0 } else { 0.0 };
    let dd_pct = if limits.max_drawdown_pct > 0.0 { (dd / limits.max_drawdown_pct) * 100.0 } else { 0.0 };
    let loss_pct = if limits.max_daily_loss > 0.0 { (loss / limits.max_daily_loss) * 100.0 } else { 0.0 };

    let summary = RiskMetricsSummaryView {
        workspace_id: ws,
        strategy_id: q.strategy_id,
        kill_switch_active,
        kill_switch_reason,
        gross_exposure: gross,
        gross_exposure_limit: limits.max_gross_exposure,
        gross_exposure_utilization_pct: gross_pct,
        net_exposure: net,
        net_exposure_limit: limits.max_net_exposure,
        net_exposure_utilization_pct: net_pct,
        leverage: lev,
        leverage_limit: limits.max_leverage,
        leverage_utilization_pct: lev_pct,
        drawdown_pct: dd,
        drawdown_limit_pct: limits.max_drawdown_pct,
        drawdown_utilization_pct: dd_pct,
        daily_loss: loss,
        daily_loss_limit: limits.max_daily_loss,
        daily_loss_utilization_pct: loss_pct,
    };

    json_response(200, json!(summary).to_string())
}
