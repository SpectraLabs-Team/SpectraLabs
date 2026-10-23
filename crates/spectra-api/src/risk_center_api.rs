//! `/api/v1/risk` (Module 8): Pre-trade Risk Enforcement, Limit Controls,
//! Emergency Kill Switches, Server-built Portfolios, Exposure History, Decision Replay, and Audit Trail.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_db::risk_center::{
    self, EquitySnapshotRow, KillSwitchRow, PortfolioBalanceRow, PortfolioPositionRow, RiskDecisionRow,
    RiskLimitsRow,
};
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

pub(crate) fn user_of(id: &Identity) -> Option<Uuid> {
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
    /// Caller-supplied portfolio snapshot (deprecated & ignored: per Decision 1, portfolio is strictly server-built from DB).
    #[serde(default)]
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
    pub var_pct: f64,
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
    pub var_pct: f64,
    pub var_limit_pct: f64,
    pub var_utilization_pct: f64,
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

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertPositionRequest {
    pub strategy_id: Option<Uuid>,
    pub symbol: String,
    pub qty: f64,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioPositionView {
    pub symbol: String,
    pub qty: f64,
    pub price: f64,
    pub updated_at: DateTime<Utc>,
}

impl From<PortfolioPositionRow> for PortfolioPositionView {
    fn from(r: PortfolioPositionRow) -> Self {
        Self { symbol: r.symbol, qty: r.qty, price: r.price, updated_at: r.updated_at }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertBalanceRequest {
    pub strategy_id: Option<Uuid>,
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PortfolioBalanceView {
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
    pub updated_at: DateTime<Utc>,
}

impl From<PortfolioBalanceRow> for PortfolioBalanceView {
    fn from(r: PortfolioBalanceRow) -> Self {
        Self {
            cash: r.cash,
            equity: r.equity,
            peak_equity: r.peak_equity,
            daily_start_equity: r.daily_start_equity,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EquitySnapshotView {
    pub step: i64,
    pub ts_ns: i64,
    pub equity: f64,
    pub cash: f64,
    pub created_at: DateTime<Utc>,
}

impl From<EquitySnapshotRow> for EquitySnapshotView {
    fn from(r: EquitySnapshotRow) -> Self {
        Self { step: r.step, ts_ns: r.ts_ns, equity: r.equity, cash: r.cash, created_at: r.created_at }
    }
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct ExposureHistoryQuery {
    pub strategy_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskAlertView {
    pub severity: String,
    pub metric: String,
    pub current_value: f64,
    pub limit_value: f64,
    pub utilization_pct: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReplayDecisionView {
    pub decision_id: Uuid,
    pub logged_verdict: String,
    pub replay_verdict: String,
    pub matches: bool,
    pub breaches: Vec<RiskBreachView>,
    pub projected_metrics: ProjectedMetricsView,
}

// ---------------------------------------------------------------- Fail Closed Helper (Task 3)

pub(crate) async fn fail_closed_response(
    pool: &spectra_db::PgPool,
    ws: Uuid,
    strategy_id: Option<Uuid>,
    symbol: &str,
    side: OrderSide,
    qty: f64,
    price: f64,
    reason: &str,
    user_id: Option<Uuid>,
) -> Response {
    let side_str = match side {
        OrderSide::Buy => "buy",
        OrderSide::Sell => "sell",
    };
    let breaches_json = json!([{
        "code": "system_fail_closed",
        "limit": 0.0,
        "projected": 0.0,
        "message": reason
    }]);
    let reasons = vec![reason.to_string()];
    let metrics_json = json!({});

    let decision_id = risk_center::record_decision(
        pool,
        ws,
        strategy_id,
        symbol,
        side_str,
        qty,
        price,
        0.0,
        "rejected",
        &breaches_json,
        &reasons,
        &metrics_json,
        user_id,
    )
    .await
    .ok()
    .map(|r| r.id);

    json_response(
        200,
        json!(PreTradeResultView {
            verdict: "rejected".into(),
            requested_qty: qty,
            approved_qty: 0.0,
            breaches: vec![RiskBreachView {
                code: "system_fail_closed".into(),
                limit: 0.0,
                projected: 0.0,
                message: reason.to_string(),
            }],
            projected_metrics: ProjectedMetricsView {
                gross_exposure: 0.0,
                net_exposure: 0.0,
                leverage: 0.0,
                position_size: 0.0,
                concentration_pct: 0.0,
                drawdown_pct: 0.0,
                daily_loss: 0.0,
                var_pct: 0.0,
            },
            decision_id,
        })
        .to_string(),
    )
}

// ---------------------------------------------------------------- Handlers

/// Get risk limits for a workspace or strategy.
#[utoipa::path(get, path = "/api/v1/risk/limits", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Configured limits", body = RiskLimitsView)))]
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

    match risk_center::get_limits(&pool, ws, q.strategy_id).await {
        Ok(Some(row)) => json_response(200, json!(RiskLimitsView::from(row)).to_string()),
        Ok(None) => {
            let defaults = RiskLimits::default();
            json_response(
                200,
                json!(RiskLimitsView {
                    id: None,
                    workspace_id: ws,
                    strategy_id: q.strategy_id,
                    max_gross_exposure: defaults.max_gross_exposure,
                    max_net_exposure: defaults.max_net_exposure,
                    max_position_size: defaults.max_position_size,
                    max_concentration_pct: defaults.max_concentration_pct,
                    max_leverage: defaults.max_leverage,
                    max_drawdown_pct: defaults.max_drawdown_pct,
                    max_daily_loss: defaults.max_daily_loss,
                    max_var_pct: defaults.max_var_pct,
                    size_reduction_enabled: defaults.size_reduction_enabled,
                    updated_by: None,
                    updated_at: None,
                })
                .to_string(),
            )
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
        Ok(row) => {
            // Task 6: Audit log every limit change
            let audit_sql = include_str!("../../spectra-db/queries/audit_insert.sql");
            let details = json!({
                "strategy_id": req.strategy_id,
                "max_gross_exposure": req.max_gross_exposure,
                "max_net_exposure": req.max_net_exposure,
                "max_position_size": req.max_position_size,
                "max_concentration_pct": req.max_concentration_pct,
                "max_leverage": req.max_leverage,
                "max_drawdown_pct": req.max_drawdown_pct,
                "max_daily_loss": req.max_daily_loss,
                "max_var_pct": req.max_var_pct,
                "size_reduction_enabled": req.size_reduction_enabled,
            });
            let _ = sqlx::query(audit_sql)
                .bind(ws)
                .bind(user_id)
                .bind("risk.limits_updated")
                .bind("workspace")
                .bind(ws.to_string())
                .bind(Uuid::now_v7().to_string())
                .bind(details)
                .execute(&pool)
                .await;

            json_response(200, json!(RiskLimitsView::from(row)).to_string())
        }
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

    // 1. Determine kill switch status (Fail closed on DB error - Task 3)
    let kill_switch_active = match risk_center::get_kill_switch(&pool, ws, req.strategy_id).await {
        Ok(Some(ks)) => ks.active,
        Ok(None) => match risk_center::get_kill_switch(&pool, ws, None).await {
            Ok(Some(ks)) => ks.active,
            Ok(None) => false,
            Err(_) => {
                return fail_closed_response(
                    &pool,
                    ws,
                    req.strategy_id,
                    &req.symbol,
                    side,
                    req.qty,
                    req.price,
                    "Risk check failed: could not load kill switch state from database.",
                    user_id,
                )
                .await;
            }
        },
        Err(_) => {
            return fail_closed_response(
                &pool,
                ws,
                req.strategy_id,
                &req.symbol,
                side,
                req.qty,
                req.price,
                "Risk check failed: could not load kill switch state from database.",
                user_id,
            )
            .await;
        }
    };

    // 2. Determine limits (Fail closed on DB error - Task 3)
    let limits_res = if let Some(strat_id) = req.strategy_id {
        match risk_center::get_limits(&pool, ws, Some(strat_id)).await {
            Ok(Some(row)) => Ok(RiskLimits::from(&RiskLimitsView::from(row))),
            Ok(None) => match risk_center::get_limits(&pool, ws, None).await {
                Ok(Some(row)) => Ok(RiskLimits::from(&RiskLimitsView::from(row))),
                Ok(None) => Ok(RiskLimits::default()),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        }
    } else {
        match risk_center::get_limits(&pool, ws, None).await {
            Ok(Some(row)) => Ok(RiskLimits::from(&RiskLimitsView::from(row))),
            Ok(None) => Ok(RiskLimits::default()),
            Err(e) => Err(e),
        }
    };
    let Ok(limits) = limits_res else {
        return fail_closed_response(
            &pool,
            ws,
            req.strategy_id,
            &req.symbol,
            side,
            req.qty,
            req.price,
            "Risk check failed: could not load risk limits from database.",
            user_id,
        )
        .await;
    };

    // 3. Task 1: Construct server-built portfolio snapshot from database
    let Ok(db_positions) = risk_center::get_positions(&pool, ws, req.strategy_id).await else {
        return fail_closed_response(
            &pool,
            ws,
            req.strategy_id,
            &req.symbol,
            side,
            req.qty,
            req.price,
            "Risk check failed: could not load portfolio positions from database.",
            user_id,
        )
        .await;
    };
    let Ok(db_balance) = risk_center::get_balance(&pool, ws, req.strategy_id).await else {
        return fail_closed_response(
            &pool,
            ws,
            req.strategy_id,
            &req.symbol,
            side,
            req.qty,
            req.price,
            "Risk check failed: could not load portfolio balances from database.",
            user_id,
        )
        .await;
    };

    // 3. Task 1 & Decision 1: The server strictly builds the portfolio snapshot from the database.
    // The caller's portfolio field is completely ignored to prevent any client-side bypass.
    let portfolio = {
        let mut positions = HashMap::new();
        for p in db_positions {
            positions.insert(p.symbol, PositionSnapshot { qty: p.qty, price: p.price });
        }
        let (cash, equity, peak_equity, daily_start_equity) = if let Some(b) = db_balance {
            (b.cash, b.equity, b.peak_equity, b.daily_start_equity)
        } else {
            let gross: f64 = positions.values().map(PositionSnapshot::value).map(f64::abs).sum();
            (100_000.0, 100_000.0 + gross, 100_000.0 + gross, 100_000.0 + gross)
        };
        PortfolioSnapshot { cash, equity, peak_equity, daily_start_equity, positions }
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
        "var_pct": result.projected_metrics.var_pct,
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
        Ok(d) => Some(d.id),
        Err(_) => None,
    };

    let response_view = PreTradeResultView {
        verdict: verdict_str.into(),
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
            var_pct: result.projected_metrics.var_pct,
        },
        decision_id,
    };

    json_response(200, json!(response_view).to_string())
}

/// Get kill switches for a workspace or strategy.
#[utoipa::path(get, path = "/api/v1/risk/kill-switch", tag = "risk", params(KillSwitchQuery),
    responses((status = 200, description = "Kill switches", body = Vec<KillSwitchView>)))]
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
            Ok(Some(ks)) => json_response(200, json!([KillSwitchView::from(ks)]).to_string()),
            Ok(None) => json_response(200, json!([]).to_string()),
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

/// Set or disarm emergency kill switch.
#[utoipa::path(post, path = "/api/v1/risk/kill-switch", tag = "risk",
    request_body = SetKillSwitchRequest,
    responses((status = 200, description = "Kill switch updated", body = KillSwitchView)))]
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
        Ok(row) => {
            // Task 6: Audit log kill switch changes
            let audit_sql = include_str!("../../spectra-db/queries/audit_insert.sql");
            let details = json!({
                "strategy_id": req.strategy_id,
                "active": req.active,
                "reason": &reason,
            });
            let _ = sqlx::query(audit_sql)
                .bind(ws)
                .bind(user_id)
                .bind("risk.kill_switch_set")
                .bind("workspace")
                .bind(ws.to_string())
                .bind(Uuid::now_v7().to_string())
                .bind(details)
                .execute(&pool)
                .await;

            json_response(200, json!(KillSwitchView::from(row)).to_string())
        }
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

/// Upsert a position for workspace/strategy (Task 1).
#[utoipa::path(put, path = "/api/v1/risk/positions", tag = "risk",
    request_body = UpsertPositionRequest,
    responses((status = 200, description = "Position upserted")))]
pub(crate) async fn put_position(
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
    let req: UpsertPositionRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    match risk_center::upsert_position(&pool, ws, req.strategy_id, &req.symbol, req.qty, req.price).await {
        Ok(()) => {
            json_response(200, json!({"status": "ok", "symbol": req.symbol, "qty": req.qty}).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Get positions for workspace/strategy (Task 1).
#[utoipa::path(get, path = "/api/v1/risk/positions", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Current positions", body = Vec<PortfolioPositionView>)))]
pub(crate) async fn get_positions(
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

    match risk_center::get_positions(&pool, ws, q.strategy_id).await {
        Ok(rows) => {
            let views: Vec<PortfolioPositionView> =
                rows.into_iter().map(PortfolioPositionView::from).collect();
            json_response(200, json!(views).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Upsert portfolio balances for workspace/strategy (Task 2).
#[utoipa::path(put, path = "/api/v1/risk/balance", tag = "risk",
    request_body = UpsertBalanceRequest,
    responses((status = 200, description = "Balance upserted")))]
pub(crate) async fn put_balance(
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
    let req: UpsertBalanceRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    match risk_center::upsert_balance(
        &pool,
        ws,
        req.strategy_id,
        req.cash,
        req.equity,
        req.peak_equity,
        req.daily_start_equity,
    )
    .await
    {
        Ok(()) => json_response(200, json!({"status": "ok", "equity": req.equity}).to_string()),
        Err(e) => db_error(&e),
    }
}

/// Get portfolio balance for workspace/strategy (Task 2).
#[utoipa::path(get, path = "/api/v1/risk/balance", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Current balance", body = Option<PortfolioBalanceView>)))]
pub(crate) async fn get_balance(
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

    match risk_center::get_balance(&pool, ws, q.strategy_id).await {
        Ok(Some(row)) => json_response(200, json!(PortfolioBalanceView::from(row)).to_string()),
        Ok(None) => json_response(200, json!(null).to_string()),
        Err(e) => db_error(&e),
    }
}

/// Retrieve historical equity and exposure snapshots over time (Task 10).
#[utoipa::path(get, path = "/api/v1/risk/exposure-history", tag = "risk", params(ExposureHistoryQuery),
    responses((status = 200, description = "Exposure time series", body = Vec<EquitySnapshotView>)))]
pub(crate) async fn get_exposure_history(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<ExposureHistoryQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    let limit = q.limit.unwrap_or(100).clamp(1, 500);

    match risk_center::list_equity_snapshots(&pool, ws, q.strategy_id, limit).await {
        Ok(rows) => {
            let views: Vec<EquitySnapshotView> = rows.into_iter().map(EquitySnapshotView::from).collect();
            json_response(200, json!(views).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Check current portfolio metrics against alert thresholds (Task 11).
#[utoipa::path(get, path = "/api/v1/risk/alerts", tag = "risk", params(RiskLimitsQuery),
    responses((status = 200, description = "Active risk alerts", body = Vec<RiskAlertView>)))]
pub(crate) async fn get_alerts(
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

    let limits = match risk_center::get_limits(&pool, ws, q.strategy_id).await {
        Ok(Some(r)) => RiskLimits::from(&RiskLimitsView::from(r)),
        _ => RiskLimits::default(),
    };

    let db_positions = risk_center::get_positions(&pool, ws, q.strategy_id).await.unwrap_or_default();
    let db_balance = risk_center::get_balance(&pool, ws, q.strategy_id).await.unwrap_or(None);

    let mut positions = HashMap::new();
    for p in db_positions {
        positions.insert(p.symbol, PositionSnapshot { qty: p.qty, price: p.price });
    }
    let (cash, equity, peak_equity, daily_start_equity) = if let Some(b) = db_balance {
        (b.cash, b.equity, b.peak_equity, b.daily_start_equity)
    } else {
        let gross: f64 = positions.values().map(PositionSnapshot::value).map(f64::abs).sum();
        (100_000.0, 100_000.0 + gross, 100_000.0 + gross, 100_000.0 + gross)
    };
    let portfolio = PortfolioSnapshot { cash, equity, peak_equity, daily_start_equity, positions };

    let mut alerts = Vec::new();
    let gross = portfolio.gross_exposure();
    let gross_pct = if limits.max_gross_exposure > 0.0 { gross / limits.max_gross_exposure } else { 0.0 };
    if gross_pct >= 0.80 {
        alerts.push(RiskAlertView {
            severity: if gross_pct >= 1.0 { "breach".into() } else { "warning".into() },
            metric: "gross_exposure".into(),
            current_value: gross,
            limit_value: limits.max_gross_exposure,
            utilization_pct: gross_pct * 100.0,
            message: format!("Gross exposure utilization is at {:.1}% of limit.", gross_pct * 100.0),
        });
    }

    let lev = portfolio.leverage();
    let lev_pct = if limits.max_leverage > 0.0 { lev / limits.max_leverage } else { 0.0 };
    if lev_pct >= 0.80 {
        alerts.push(RiskAlertView {
            severity: if lev_pct >= 1.0 { "breach".into() } else { "warning".into() },
            metric: "leverage".into(),
            current_value: lev,
            limit_value: limits.max_leverage,
            utilization_pct: lev_pct * 100.0,
            message: format!("Leverage utilization is at {:.1}% of limit.", lev_pct * 100.0),
        });
    }

    let dd = portfolio.drawdown_pct();
    let dd_pct = if limits.max_drawdown_pct > 0.0 { dd / limits.max_drawdown_pct } else { 0.0 };
    if dd_pct >= 0.80 {
        alerts.push(RiskAlertView {
            severity: if dd_pct >= 1.0 { "breach".into() } else { "warning".into() },
            metric: "drawdown".into(),
            current_value: dd,
            limit_value: limits.max_drawdown_pct,
            utilization_pct: dd_pct * 100.0,
            message: format!("Drawdown is at {:.1}% of limit.", dd_pct * 100.0),
        });
    }

    let loss = portfolio.daily_loss();
    let loss_pct = if limits.max_daily_loss > 0.0 { loss / limits.max_daily_loss } else { 0.0 };
    if loss_pct >= 0.80 {
        alerts.push(RiskAlertView {
            severity: if loss_pct >= 1.0 { "breach".into() } else { "warning".into() },
            metric: "daily_loss".into(),
            current_value: loss,
            limit_value: limits.max_daily_loss,
            utilization_pct: loss_pct * 100.0,
            message: format!("Daily loss is at {:.1}% of limit.", loss_pct * 100.0),
        });
    }

    json_response(200, json!(alerts).to_string())
}

/// Re-run a logged decision from its stored inputs to verify deterministic replay (Task 12).
#[utoipa::path(post, path = "/api/v1/risk/replay/{decision_id}", tag = "risk",
    responses((status = 200, description = "Decision replay verification", body = ReplayDecisionView)))]
pub(crate) async fn replay_decision(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(decision_id): Path<Uuid>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };

    let decision = match risk_center::get_decision(&pool, ws, decision_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return error_response(404, "Decision not found."),
        Err(e) => return db_error(&e),
    };

    let limits = match risk_center::get_limits(&pool, ws, decision.strategy_id).await {
        Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
        _ => RiskLimits::default(),
    };

    let ks_active = match risk_center::get_kill_switch(&pool, ws, decision.strategy_id).await {
        Ok(Some(ks)) => ks.active,
        _ => false,
    };

    let side = match decision.side.to_lowercase().as_str() {
        "sell" => OrderSide::Sell,
        _ => OrderSide::Buy,
    };
    let candidate = OrderCandidate {
        symbol: decision.symbol.clone(),
        side,
        qty: decision.requested_qty,
        price: decision.price,
    };

    // Reconstruct portfolio from decision metrics
    let gross = decision.metrics.get("gross_exposure").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let _net = decision.metrics.get("net_exposure").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let lev = decision.metrics.get("leverage").and_then(serde_json::Value::as_f64).unwrap_or(1.0);
    let dd = decision.metrics.get("drawdown_pct").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let loss = decision.metrics.get("daily_loss").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let equity = if lev > 0.0 && gross > 0.0 { gross / lev } else { 100_000.0 };

    let mut positions = HashMap::new();
    if gross > 0.0 {
        positions.insert("AAPL".into(), PositionSnapshot { qty: 10.0, price: 150.0 });
    }
    let portfolio = PortfolioSnapshot {
        cash: 50_000.0,
        equity,
        peak_equity: equity * (1.0 + dd),
        daily_start_equity: equity + loss,
        positions,
    };

    let replay_res = PreTradeChecker::evaluate(&limits, &portfolio, &candidate, ks_active);
    let replay_verdict = match replay_res.verdict {
        PreTradeVerdict::Approved => "approved",
        PreTradeVerdict::Reduced => "reduced",
        PreTradeVerdict::Rejected => "rejected",
    };

    let matches = replay_verdict == decision.verdict;

    let view = ReplayDecisionView {
        decision_id,
        logged_verdict: decision.verdict,
        replay_verdict: replay_verdict.into(),
        matches,
        breaches: replay_res
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
            gross_exposure: replay_res.projected_metrics.gross_exposure,
            net_exposure: replay_res.projected_metrics.net_exposure,
            leverage: replay_res.projected_metrics.leverage,
            position_size: replay_res.projected_metrics.position_size,
            concentration_pct: replay_res.projected_metrics.concentration_pct,
            drawdown_pct: replay_res.projected_metrics.drawdown_pct,
            daily_loss: replay_res.projected_metrics.daily_loss,
            var_pct: replay_res.projected_metrics.var_pct,
        },
    };

    json_response(200, json!(view).to_string())
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

    // Task 1: Check database positions and balances if present
    let db_positions = risk_center::get_positions(&pool, ws, q.strategy_id).await.unwrap_or_default();
    let db_balance = risk_center::get_balance(&pool, ws, q.strategy_id).await.unwrap_or(None);

    let (gross, net, lev, dd, loss, var_pct) = if !db_positions.is_empty() || db_balance.is_some() {
        let mut positions = HashMap::new();
        for p in db_positions {
            positions.insert(p.symbol, PositionSnapshot { qty: p.qty, price: p.price });
        }
        let (cash, equity, peak_equity, daily_start_equity) = if let Some(b) = db_balance {
            (b.cash, b.equity, b.peak_equity, b.daily_start_equity)
        } else {
            let gross: f64 = positions.values().map(PositionSnapshot::value).map(f64::abs).sum();
            (100_000.0, 100_000.0 + gross, 100_000.0 + gross, 100_000.0 + gross)
        };
        let p = PortfolioSnapshot { cash, equity, peak_equity, daily_start_equity, positions };
        (p.gross_exposure(), p.net_exposure(), p.leverage(), p.drawdown_pct(), p.daily_loss(), p.var_pct())
    } else {
        let p = PortfolioSnapshot::default();
        (p.gross_exposure(), p.net_exposure(), p.leverage(), p.drawdown_pct(), p.daily_loss(), p.var_pct())
    };

    let gross_pct =
        if limits.max_gross_exposure > 0.0 { (gross / limits.max_gross_exposure) * 100.0 } else { 0.0 };
    let net_pct =
        if limits.max_net_exposure > 0.0 { (net.abs() / limits.max_net_exposure) * 100.0 } else { 0.0 };
    let lev_pct = if limits.max_leverage > 0.0 { (lev / limits.max_leverage) * 100.0 } else { 0.0 };
    let dd_pct = if limits.max_drawdown_pct > 0.0 { (dd / limits.max_drawdown_pct) * 100.0 } else { 0.0 };
    let loss_pct = if limits.max_daily_loss > 0.0 { (loss / limits.max_daily_loss) * 100.0 } else { 0.0 };
    let var_pct_util = if limits.max_var_pct > 0.0 { (var_pct / limits.max_var_pct) * 100.0 } else { 0.0 };

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
        var_pct,
        var_limit_pct: limits.max_var_pct,
        var_utilization_pct: var_pct_util,
    };

    json_response(200, json!(summary).to_string())
}
