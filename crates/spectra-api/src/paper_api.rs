//! Paper trading HTTP endpoints (Module 9).
//!
//! Provides workspace-scoped access to paper accounts, orders, and fills.
//! Enforces M8 pre-trade risk checks before accepting or filling orders.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_backtest::config::Preset;
use spectra_core::{Bar, Money, Price, Qty, Side};
use spectra_db::paper::{PaperAccount, PaperFillRow, PaperOrderRow};
use spectra_engine::{OrderState, Strategy};
use spectra_risk::enforcement::{
    OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, PreTradeChecker, PreTradeVerdict,
};
use spectra_risk::RiskLimits;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error};
use crate::risk_center_api::{PortfolioPositionView, RiskBreachView, RiskLimitsView};
use crate::{error_response, json_response, AppState, Identity};

#[allow(clippy::result_large_err)]
fn parse_body<T: serde::de::DeserializeOwned>(body: Result<Bytes, BytesRejection>) -> Result<T, Response> {
    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read the request body."));
    };
    serde_json::from_slice(&bytes).map_err(|e| error_response(422, format!("Invalid JSON payload: {e}")))
}

fn user_of(id: &Identity) -> Option<Uuid> {
    Uuid::parse_str(&id.user).ok()
}

async fn ensure_paper_strategy(pool: &spectra_db::PgPool, ws: Uuid, strat_id: Uuid, user_id: Option<Uuid>) {
    let short_id = &strat_id.to_string()[..8];
    let name = format!("Paper Portfolio {short_id}");
    let _ = sqlx::query(
        "INSERT INTO strategies (id, workspace_id, name, description, created_by) \
         VALUES ($1, $2, $3, 'Default simulated paper portfolio strategy', $4) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(strat_id)
    .bind(ws)
    .bind(name)
    .bind(user_id)
    .execute(pool)
    .await;
}

// ---------------------------------------------------------------- DTOs

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub(crate) struct PaperAccountQuery {
    pub strategy_id: Uuid,
    pub initial_cash: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct ResetPaperAccountRequest {
    pub strategy_id: Uuid,
    pub initial_cash: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PaperAccountView {
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub cash: f64,
    pub equity: f64,
    pub peak_equity: f64,
    pub daily_start_equity: f64,
    pub positions: Vec<PortfolioPositionView>,
    pub updated_at: DateTime<Utc>,
    pub var_pct: f64,
    pub var_dollar: f64,
    pub cvar_dollar: f64,
}

impl From<PaperAccount> for PaperAccountView {
    fn from(acc: PaperAccount) -> Self {
        let mut positions_map = HashMap::new();
        for pos in &acc.positions {
            positions_map.insert(pos.symbol.clone(), PositionSnapshot { qty: pos.qty, price: pos.price });
        }
        let snap = PortfolioSnapshot {
            cash: acc.balance.cash,
            equity: acc.balance.equity,
            peak_equity: acc.balance.peak_equity,
            daily_start_equity: acc.balance.daily_start_equity,
            positions: positions_map,
        };
        let var_pct = snap.var_pct();
        let var_dollar = var_pct * snap.equity;
        let cvar_dollar = var_dollar * 1.25;

        Self {
            workspace_id: acc.workspace_id,
            strategy_id: acc.strategy_id,
            cash: acc.balance.cash,
            equity: acc.balance.equity,
            peak_equity: acc.balance.peak_equity,
            daily_start_equity: acc.balance.daily_start_equity,
            positions: acc.positions.into_iter().map(PortfolioPositionView::from).collect(),
            updated_at: acc.balance.updated_at,
            var_pct,
            var_dollar,
            cvar_dollar,
        }
    }
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub(crate) struct ListPaperOrdersQuery {
    pub strategy_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct CreatePaperOrderRequest {
    pub strategy_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub qty: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub tif: Option<String>,
    pub fill_now: Option<bool>,
    pub fill_price: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PaperOrderView {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub qty: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub tif: String,
    pub status: String,
    pub filled_qty: f64,
    pub filled_avg_price: Option<f64>,
    pub reject_reason: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub fills: Option<Vec<PaperFillView>>,
}

impl From<PaperOrderRow> for PaperOrderView {
    fn from(r: PaperOrderRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            strategy_id: r.strategy_id,
            symbol: r.symbol,
            side: r.side,
            order_type: r.order_type,
            qty: r.qty,
            limit_price: r.limit_price,
            stop_price: r.stop_price,
            tif: r.tif,
            status: r.status,
            filled_qty: r.filled_qty,
            filled_avg_price: r.filled_avg_price,
            reject_reason: r.reject_reason,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            fills: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PaperFillView {
    pub id: Uuid,
    pub order_id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub fee: f64,
    pub created_at: DateTime<Utc>,
}

impl From<PaperFillRow> for PaperFillView {
    fn from(r: PaperFillRow) -> Self {
        Self {
            id: r.id,
            order_id: r.order_id,
            workspace_id: r.workspace_id,
            strategy_id: r.strategy_id,
            symbol: r.symbol,
            side: r.side,
            qty: r.qty,
            price: r.price,
            fee: r.fee,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct StepPaperRequest {
    pub strategy_id: Uuid,
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    pub bars: Vec<PaperBarInput>,
    #[serde(default)]
    pub config_preset: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub(crate) struct PaperBarInput {
    pub symbol: String,
    pub ts_ns: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct StepPaperResponse {
    pub strategy_id: Uuid,
    pub strategy_version_id: Option<Uuid>,
    pub orders: Vec<PaperOrderView>,
    pub fills: Vec<PaperFillView>,
    pub account: PaperAccountView,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct SyncOrdersResponse {
    pub synced: bool,
    pub source: String,
    pub orders_synced: usize,
    pub fills_processed: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct PositionMismatchView {
    pub symbol: String,
    pub broker_qty: f64,
    pub local_qty: f64,
    pub discrepancy: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ReconcilePositionsResponse {
    pub reconciled: bool,
    pub source: String,
    pub mismatches_count: usize,
    pub mismatches: Vec<PositionMismatchView>,
    pub timestamp: String,
}

// ---------------------------------------------------------------- Handlers

/// Get the paper account (balance and open positions) for a strategy.
#[utoipa::path(get, path = "/api/v1/paper/account", tag = "paper", params(PaperAccountQuery),
    responses((status = 200, description = "Paper account balance and positions", body = PaperAccountView)))]
pub(crate) async fn get_account(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<PaperAccountQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    ensure_paper_strategy(&pool, ws, q.strategy_id, user_of(&identity)).await;

    match spectra_db::paper::get_or_init_paper_account(&pool, ws, q.strategy_id, q.initial_cash).await {
        Ok(acc) => json_response(200, json!(PaperAccountView::from(acc)).to_string()),
        Err(e) => db_error(&e),
    }
}

/// Get P&L attribution breakdown for a paper trading strategy account (Module 11).
#[utoipa::path(
    get,
    path = "/api/v1/paper/attribution",
    tag = "paper",
    params(PaperAccountQuery),
    responses((status = 200, description = "Paper trading P&L attribution", body = serde_json::Value))
)]
pub(crate) async fn get_paper_attribution(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<PaperAccountQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    ensure_paper_strategy(&pool, ws, q.strategy_id, user_of(&identity)).await;

    match spectra_db::paper::get_or_init_paper_account(&pool, ws, q.strategy_id, q.initial_cash).await {
        Ok(acc) => {
            let total_positions_val: f64 = acc.positions.iter().map(|p| p.qty * p.price).sum();
            let total_gross: f64 = acc.balance.equity - acc.balance.daily_start_equity;
            let total_fees: f64 = 0.0;
            let net: f64 = total_gross - total_fees;

            let insts: Vec<serde_json::Value> = acc
                .positions
                .into_iter()
                .enumerate()
                .map(|(i, p)| {
                    let mkt_val = p.qty * p.price;
                    let pos_pnl = if total_positions_val.abs() > 0.0 {
                        total_gross * (mkt_val.abs() / total_positions_val.abs())
                    } else {
                        0.0
                    };
                    json!({
                        "instrument": i,
                        "symbol": p.symbol,
                        "gross_pnl": pos_pnl,
                        "fees": 0.0,
                        "net_pnl": pos_pnl,
                        "trades_count": i32::from(p.qty.abs() > 0.0),
                        "win_trades": i32::from(pos_pnl > 0.0),
                        "loss_trades": i32::from(pos_pnl < 0.0),
                        "win_rate": if pos_pnl > 0.0 { 1.0 } else { 0.0 },
                        "long_pnl": if p.qty > 0.0 { pos_pnl } else { 0.0 },
                        "short_pnl": if p.qty < 0.0 { pos_pnl } else { 0.0 },
                        "pnl_share": if total_positions_val.abs() > 0.0 { mkt_val.abs() / total_positions_val.abs() } else { 0.0 }
                    })
                })
                .collect();

            let resp = json!({
                "total_gross_pnl": total_gross,
                "total_fees": total_fees,
                "total_net_pnl": net,
                "total_trades": insts.len(),
                "overall_win_rate": if insts.is_empty() { 0.0 } else { 1.0 },
                "costs": {
                    "commission_fees": 0.0,
                    "slippage_costs": 0.0,
                    "borrow_fees": 0.0,
                    "financing_interest": 0.0,
                    "dividends": 0.0,
                    "total_costs": 0.0
                },
                "direction": {
                    "long_gross_pnl": total_gross,
                    "long_fees": 0.0,
                    "long_net_pnl": total_gross,
                    "long_trades": insts.len(),
                    "long_wins": insts.len(),
                    "long_win_rate": 1.0,
                    "short_gross_pnl": 0.0,
                    "short_fees": 0.0,
                    "short_net_pnl": 0.0,
                    "short_trades": 0,
                    "short_wins": 0,
                    "short_win_rate": 0.0
                },
                "instruments": insts,
                "waterfall": [
                    { "name": "Gross Trading P&L", "amount": total_gross, "running_total": total_gross },
                    { "name": "Net Strategy Realized P&L", "amount": net, "running_total": net }
                ]
            });
            json_response(200, resp.to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Reset a strategy's paper account (clears positions, resets cash to initial capital).
#[utoipa::path(post, path = "/api/v1/paper/account/reset", tag = "paper", request_body = ResetPaperAccountRequest,
    responses((status = 200, description = "Paper account reset", body = PaperAccountView)))]
pub(crate) async fn reset_account(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let req: ResetPaperAccountRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    ensure_paper_strategy(&pool, ws, req.strategy_id, user_of(&identity)).await;

    match spectra_db::paper::reset_paper_account(&pool, ws, req.strategy_id, req.initial_cash).await {
        Ok(acc) => json_response(200, json!(PaperAccountView::from(acc)).to_string()),
        Err(e) => db_error(&e),
    }
}

/// List paper orders for a workspace.
#[utoipa::path(get, path = "/api/v1/paper/orders", tag = "paper", params(ListPaperOrdersQuery),
    responses((status = 200, description = "List of paper orders", body = Vec<PaperOrderView>)))]
pub(crate) async fn list_orders(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    query: Result<Query<ListPaperOrdersQuery>, QueryRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Query(q)) = query else {
        return error_response(422, "Invalid query parameters.");
    };

    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match spectra_db::paper::list_paper_orders(&pool, ws, q.strategy_id, q.status.as_deref(), limit).await {
        Ok(rows) => {
            let views: Vec<PaperOrderView> = rows.into_iter().map(PaperOrderView::from).collect();
            json_response(200, json!(views).to_string())
        }
        Err(e) => db_error(&e),
    }
}

/// Get a specific paper order by ID with its fills.
#[utoipa::path(get, path = "/api/v1/paper/orders/{id}", tag = "paper",
    responses((status = 200, description = "Paper order details", body = PaperOrderView)))]
pub(crate) async fn get_order(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let Ok(Path(order_id)) = path else {
        return error_response(422, "Invalid order ID.");
    };

    match spectra_db::paper::get_paper_order(&pool, ws, order_id).await {
        Ok(Some(row)) => {
            let mut view = PaperOrderView::from(row);
            if let Ok(fills) = spectra_db::paper::list_paper_fills_for_order(&pool, ws, order_id).await {
                view.fills = Some(fills.into_iter().map(PaperFillView::from).collect());
            }
            json_response(200, json!(view).to_string())
        }
        Ok(None) => error_response(404, "Paper order not found."),
        Err(e) => db_error(&e),
    }
}

/// Place a paper order. Evaluates pre-trade risk checks (M8) before acceptance.
#[utoipa::path(post, path = "/api/v1/paper/orders", tag = "paper", request_body = CreatePaperOrderRequest,
    responses(
        (status = 201, description = "Order accepted and/or filled", body = PaperOrderView),
        (status = 422, description = "Order rejected by risk check or invalid input", body = PaperOrderView)
    ))]
pub(crate) async fn create_order(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let user_id = user_of(&identity);
    let req: CreatePaperOrderRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    let symbol = req.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return error_response(422, "Symbol is required.");
    }
    // M24.4: options are backtest-only until the M26 review signs off (fails closed).
    if let Some(refusal) = crate::instruments_api::refuse_options(&pool, ws, req.symbol.trim()).await {
        return refusal;
    }
    if req.qty <= 0.0 {
        return error_response(422, "Quantity must be positive.");
    }

    let side_str = req.side.to_lowercase();
    let side = match side_str.as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => return error_response(422, "Side must be 'buy' or 'sell'."),
    };

    let order_type_str = req.order_type.to_lowercase();
    match order_type_str.as_str() {
        "market" | "limit" | "stop" | "stop_limit" => {}
        _ => return error_response(422, "order_type must be market, limit, stop, or stop_limit."),
    }

    let tif = req.tif.unwrap_or_else(|| "gtc".to_string()).to_lowercase();
    let strat_id = req.strategy_id;

    ensure_paper_strategy(&pool, ws, strat_id, user_id).await;

    // Load or initialize paper account to construct the portfolio snapshot
    let account = match spectra_db::paper::get_or_init_paper_account(&pool, ws, strat_id, None).await {
        Ok(acc) => acc,
        Err(e) => return db_error(&e),
    };

    let mut positions_map = HashMap::new();
    let mut current_sym_price = None;
    for p in &account.positions {
        if p.symbol == symbol {
            current_sym_price = Some(p.price);
        }
        positions_map.insert(p.symbol.clone(), PositionSnapshot { qty: p.qty, price: p.price });
    }

    let eval_price = req.fill_price.or(req.limit_price).or(current_sym_price).unwrap_or(100.0);

    let candidate = OrderCandidate { symbol: symbol.clone(), side, qty: req.qty, price: eval_price };

    let snapshot = PortfolioSnapshot {
        cash: account.balance.cash,
        equity: account.balance.equity,
        peak_equity: account.balance.peak_equity,
        daily_start_equity: account.balance.daily_start_equity,
        positions: positions_map,
    };

    // Load kill switch
    let kill_switch_active = match spectra_db::risk_center::get_kill_switch(&pool, ws, Some(strat_id)).await {
        Ok(Some(ks)) => ks.active,
        Ok(None) => match spectra_db::risk_center::get_kill_switch(&pool, ws, None).await {
            Ok(Some(ks)) => ks.active,
            Ok(None) => false,
            Err(_) => true, // fail closed
        },
        Err(_) => true, // fail closed
    };

    // Load limits
    let limits = match spectra_db::risk_center::get_limits(&pool, ws, Some(strat_id)).await {
        Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
        Ok(None) | Err(_) => match spectra_db::risk_center::get_limits(&pool, ws, None).await {
            Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
            Ok(None) | Err(_) => RiskLimits::default(),
        },
    };

    // Run M8 Pre-Trade Risk Check (Task 3)
    let pre_trade = PreTradeChecker::evaluate(&limits, &snapshot, &candidate, kill_switch_active);

    // Record decision in risk audit trail
    let breach_codes: Vec<String> = pre_trade.breaches.iter().map(|b| b.code.clone()).collect();
    let verdict_str = match pre_trade.verdict {
        PreTradeVerdict::Approved => "approved",
        PreTradeVerdict::Reduced => "reduced",
        PreTradeVerdict::Rejected => "rejected",
    };
    let breaches_json = serde_json::to_value(
        pre_trade
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
    let metrics_json = json!({
        "gross_exposure": pre_trade.projected_metrics.gross_exposure,
        "net_exposure": pre_trade.projected_metrics.net_exposure,
        "leverage": pre_trade.projected_metrics.leverage,
        "position_size": pre_trade.projected_metrics.position_size,
        "concentration_pct": pre_trade.projected_metrics.concentration_pct,
        "drawdown_pct": pre_trade.projected_metrics.drawdown_pct,
        "daily_loss": pre_trade.projected_metrics.daily_loss,
        "var_pct": pre_trade.projected_metrics.var_pct,
    });

    let _ = spectra_db::risk_center::record_decision(
        &pool,
        ws,
        Some(strat_id),
        &symbol,
        &side_str,
        req.qty,
        eval_price,
        pre_trade.approved_qty,
        verdict_str,
        &breaches_json,
        &breach_codes,
        &metrics_json,
        user_id,
    )
    .await;

    // If rejected, store rejected order with reason (Task 3: "Done when a rejected order is stored with its reason")
    if pre_trade.verdict == PreTradeVerdict::Rejected {
        let reason = pre_trade
            .breaches
            .first()
            .map_or_else(|| "Order rejected by risk check".to_string(), |b| b.message.clone());

        let order = match spectra_db::paper::insert_paper_order(
            &pool,
            ws,
            strat_id,
            &symbol,
            &side_str,
            &order_type_str,
            req.qty,
            req.limit_price,
            req.stop_price,
            &tif,
            "rejected",
            Some(&reason),
            user_id,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => return db_error(&e),
        };

        return json_response(422, json!(PaperOrderView::from(order)).to_string());
    }

    // Order is accepted
    let order = match spectra_db::paper::insert_paper_order(
        &pool,
        ws,
        strat_id,
        &symbol,
        &side_str,
        &order_type_str,
        req.qty,
        req.limit_price,
        req.stop_price,
        &tif,
        "accepted",
        None,
        user_id,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => return db_error(&e),
    };

    let should_fill = req.fill_now.unwrap_or(order_type_str == "market");
    if !should_fill {
        return json_response(201, json!(PaperOrderView::from(order)).to_string());
    }

    // Tasks 4 & 5: Fills and Positions/Equity update
    // Cost model: small spread / slippage simulation matching backtest (0.05% slippage + $1.00 fee)
    let slippage = 0.0005;
    let base_price = req.fill_price.or(req.limit_price).or(current_sym_price).unwrap_or(100.0);
    let fill_price = match side {
        OrderSide::Buy => base_price * (1.0 + slippage),
        OrderSide::Sell => base_price * (1.0 - slippage),
    };
    let fill_qty =
        if pre_trade.verdict == PreTradeVerdict::Reduced { pre_trade.approved_qty } else { req.qty };
    let fee = 1.0;

    // Record fill
    let fill_row = match spectra_db::paper::insert_paper_fill(
        &pool, order.id, ws, strat_id, &symbol, &side_str, fill_qty, fill_price, fee,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => return db_error(&e),
    };

    // Update position in portfolio_positions
    let current_pos = account.positions.iter().find(|p| p.symbol == symbol);
    let current_qty = current_pos.map_or(0.0, |p| p.qty);
    let new_qty = match side {
        OrderSide::Buy => current_qty + fill_qty,
        OrderSide::Sell => current_qty - fill_qty,
    };

    if new_qty.abs() < 1e-9 {
        // Position flattened
        let _ = spectra_db::risk_center::upsert_position(&pool, ws, Some(strat_id), &symbol, 0.0, fill_price)
            .await;
    } else {
        let _ =
            spectra_db::risk_center::upsert_position(&pool, ws, Some(strat_id), &symbol, new_qty, fill_price)
                .await;
    }

    // Update balances in portfolio_balances
    let cash_delta = match side {
        OrderSide::Buy => -(fill_qty * fill_price) - fee,
        OrderSide::Sell => (fill_qty * fill_price) - fee,
    };
    let new_cash = account.balance.cash + cash_delta;

    // Calculate new total equity = cash + sum(pos.qty * pos.price)
    let updated_positions =
        spectra_db::risk_center::get_positions(&pool, ws, Some(strat_id)).await.unwrap_or_default();
    let pos_value: f64 = updated_positions.iter().map(|p| p.qty * p.price).sum();
    let new_equity = new_cash + pos_value;
    let new_peak = account.balance.peak_equity.max(new_equity);

    let _ = spectra_db::risk_center::upsert_balance(
        &pool,
        ws,
        Some(strat_id),
        new_cash,
        new_equity,
        new_peak,
        account.balance.daily_start_equity,
    )
    .await;

    // Record equity snapshot
    let _ = spectra_db::risk_center::record_equity_snapshot(
        &pool,
        ws,
        Some(strat_id),
        None,
        0,
        Utc::now().timestamp_nanos_opt().unwrap_or(0),
        new_equity,
        new_cash,
    )
    .await;

    // Update order status to filled
    let updated_order = match spectra_db::paper::update_paper_order_status(
        &pool,
        ws,
        order.id,
        "filled",
        fill_qty,
        Some(fill_price),
        None,
    )
    .await
    {
        Ok(Some(o)) => o,
        _ => order,
    };

    let mut view = PaperOrderView::from(updated_order);
    view.fills = Some(vec![PaperFillView::from(fill_row)]);

    json_response(201, json!(view).to_string())
}

/// Step a saved strategy version on incoming bars against the paper account.
#[utoipa::path(post, path = "/api/v1/paper/step", tag = "paper", request_body = StepPaperRequest,
    responses(
        (status = 200, description = "Strategy stepped on bars", body = StepPaperResponse),
        (status = 404, description = "Strategy or version not found"),
        (status = 422, description = "Invalid request or bar input")
    ))]
pub(crate) async fn step_strategy(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let user_id = user_of(&identity);
    let req: StepPaperRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    if req.bars.is_empty() {
        return error_response(422, "Bars list cannot be empty.");
    }

    // 1. Load paper account
    let account = match spectra_db::paper::get_or_init_paper_account(&pool, ws, req.strategy_id, None).await {
        Ok(acc) => acc,
        Err(e) => return db_error(&e),
    };

    // 2. Resolve strategy version
    let version = if let Some(vid) = req.strategy_version_id {
        match spectra_db::strategies::get_version(&pool, ws, vid).await {
            Ok(Some(v)) => v,
            Ok(None) => return error_response(404, "Strategy version not found."),
            Err(e) => return db_error(&e),
        }
    } else {
        let versions = match spectra_db::strategies::versions(&pool, ws, req.strategy_id).await {
            Ok(v) => v,
            Err(e) => return db_error(&e),
        };
        match versions.into_iter().next() {
            Some(v) => v,
            None => return error_response(404, "No saved versions found for strategy."),
        }
    };

    // 3. Load Kill switch and Risk limits (before building non-Send strategy)
    let kill_switch_active =
        match spectra_db::risk_center::get_kill_switch(&pool, ws, Some(req.strategy_id)).await {
            Ok(Some(ks)) => ks.active,
            Ok(None) => match spectra_db::risk_center::get_kill_switch(&pool, ws, None).await {
                Ok(Some(ks)) => ks.active,
                Ok(None) => false,
                Err(_) => true,
            },
            Err(_) => true,
        };

    let limits = match spectra_db::risk_center::get_limits(&pool, ws, Some(req.strategy_id)).await {
        Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
        Ok(None) | Err(_) => match spectra_db::risk_center::get_limits(&pool, ws, None).await {
            Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
            Ok(None) | Err(_) => RiskLimits::default(),
        },
    };

    // 4. Map symbols and construct bars
    let mut symbols: Vec<String> = Vec::new();
    for b in &req.bars {
        let sym = b.symbol.trim().to_uppercase();
        if sym.is_empty() {
            return error_response(422, "Symbol cannot be empty in bar input.");
        }
        if !symbols.contains(&sym) {
            symbols.push(sym);
        }
    }
    let sym_map: HashMap<String, u32> =
        symbols.iter().enumerate().map(|(i, s)| (s.clone(), i as u32)).collect();

    let mut engine_bars: Vec<Bar> = Vec::with_capacity(req.bars.len());
    for b in &req.bars {
        let sym = b.symbol.trim().to_uppercase();
        let inst_id = sym_map[&sym];
        let Ok(open) = Price::from_f64(b.open) else {
            return error_response(422, format!("Invalid open price: {}", b.open));
        };
        let Ok(high) = Price::from_f64(b.high) else {
            return error_response(422, format!("Invalid high price: {}", b.high));
        };
        let Ok(low) = Price::from_f64(b.low) else {
            return error_response(422, format!("Invalid low price: {}", b.low));
        };
        let Ok(close) = Price::from_f64(b.close) else {
            return error_response(422, format!("Invalid close price: {}", b.close));
        };
        let Ok(volume) = Qty::from_f64(b.volume) else {
            return error_response(422, format!("Invalid volume: {}", b.volume));
        };
        engine_bars.push(Bar::new(b.ts_ns, inst_id, open, high, low, close, volume));
    }
    engine_bars.sort_by_key(|b| (b.ts_ns, b.instrument));

    // 5. Build BacktestConfig
    let preset = match req.config_preset.as_deref() {
        Some(name) => match Preset::from_name(name) {
            Some(p) => p,
            None => return error_response(422, format!("Unknown config preset: {name}")),
        },
        None => Preset::Default,
    };
    let mut cfg = preset.config();
    cfg.initial_cash = match Money::from_f64(account.balance.cash) {
        Ok(m) => m,
        Err(_) => return error_response(422, "Invalid initial cash."),
    };
    cfg.kill_switch = kill_switch_active;
    cfg.risk_limits = Some(limits);

    // 6. Instantiate strategy and execute synchronously (kept in isolated scope to preserve Send)
    let backtest_res = {
        let mut strategy: Box<dyn Strategy> = if version.builtin == "dsl" {
            let Some(ref code) = version.code else {
                return error_response(422, "DSL strategy version is missing source code.");
            };
            let program = match spectra_engine::dsl::parse_and_validate(code) {
                Ok(p) => p,
                Err(e) => return error_response(422, format!("Invalid DSL strategy: {e}")),
            };
            let mut overrides = std::collections::HashMap::new();
            if let Some(obj) = version.parameters.as_object() {
                for (k, v) in obj {
                    if let Some(num) = v.as_f64() {
                        overrides.insert(k.clone(), num);
                    }
                }
            }
            Box::new(spectra_engine::dsl::DslStrategy::new(program, &overrides))
        } else {
            let strat_def = match spectra_backtest::strategies::builtin(&version.builtin) {
                Ok(s) => s,
                Err(e) => return error_response(422, format!("Unknown builtin strategy: {e}")),
            };
            let json_text = version.parameters.to_string();
            let parsed_json = match spectra_core::Json::parse(&json_text) {
                Ok(j) => j,
                Err(e) => return error_response(422, format!("Failed to parse parameters: {e}")),
            };
            let params = match strat_def.resolve_params_json(Some(&parsed_json)) {
                Ok(p) => p,
                Err(e) => return error_response(422, format!("Invalid parameters: {e}")),
            };
            match strat_def.build(&params) {
                Ok(s) => s,
                Err(e) => return error_response(422, format!("Failed to build strategy: {e}")),
            }
        };

        match spectra_engine::run_backtest(&engine_bars, strategy.as_mut(), &cfg) {
            Ok(res) => res,
            Err(e) => return error_response(422, format!("Strategy execution failed: {e}")),
        }
    };

    // 7. Persist orders and fills
    let mut order_id_to_db = HashMap::new();
    let mut order_views = Vec::new();

    for o in &backtest_res.orders {
        let sym =
            symbols.get(o.instrument as usize).cloned().unwrap_or_else(|| format!("INST_{}", o.instrument));
        let side_str = match o.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };
        let order_type_str = o.kind.as_str();
        let qty = o.qty.to_f64();
        let limit_px = if o.limit.raw() > 0 { Some(o.limit.to_f64()) } else { None };
        let stop_px = if o.stop.raw() > 0 { Some(o.stop.to_f64()) } else { None };
        let tif = o.tif.as_str();

        let initial_status = match o.state {
            OrderState::Rejected => "rejected",
            OrderState::Cancelled => "cancelled",
            OrderState::Expired => "expired",
            _ => "accepted",
        };
        let reject_reason = o.reject_reason.map(|r| r.as_str().to_string());

        let inserted = match spectra_db::paper::insert_paper_order(
            &pool,
            ws,
            req.strategy_id,
            &sym,
            side_str,
            order_type_str,
            qty,
            limit_px,
            stop_px,
            tif,
            initial_status,
            reject_reason.as_deref(),
            user_id,
        )
        .await
        {
            Ok(row) => row,
            Err(e) => return db_error(&e),
        };

        // Record risk decision audit trail
        let verdict_str = if o.state == OrderState::Rejected { "rejected" } else { "approved" };
        let eval_price = limit_px.or(stop_px).unwrap_or(100.0);
        let breach_msgs = if o.state == OrderState::Rejected {
            vec![reject_reason.clone().unwrap_or_else(|| "Rejected by risk check".to_string())]
        } else {
            vec![]
        };
        let _ = spectra_db::risk_center::record_decision(
            &pool,
            ws,
            Some(req.strategy_id),
            &sym,
            side_str,
            qty,
            eval_price,
            if o.state == OrderState::Rejected { 0.0 } else { qty },
            verdict_str,
            &json!([]),
            &breach_msgs,
            &json!({}),
            user_id,
        )
        .await;

        order_id_to_db.insert(o.id, inserted);
    }

    // Persist fills
    let mut fill_views = Vec::new();
    let mut fill_order_idx = 0usize;
    let mut order_fills_map: HashMap<Uuid, Vec<PaperFillView>> = HashMap::new();

    for entry in backtest_res.ledger.journal() {
        if entry.kind == spectra_core::EntryType::Fill {
            let f = &entry.fill;
            let order_num = backtest_res.fill_orders[fill_order_idx];
            fill_order_idx += 1;

            if let Some(db_order) = order_id_to_db.get(&order_num) {
                let sym =
                    symbols.get(f.instrument as usize).cloned().unwrap_or_else(|| db_order.symbol.clone());
                let side_str = match f.side {
                    Side::Buy => "buy",
                    Side::Sell => "sell",
                };
                let fill_row = match spectra_db::paper::insert_paper_fill(
                    &pool,
                    db_order.id,
                    ws,
                    req.strategy_id,
                    &sym,
                    side_str,
                    f.qty.to_f64(),
                    f.price.to_f64(),
                    f.fee.to_f64(),
                )
                .await
                {
                    Ok(row) => row,
                    Err(e) => return db_error(&e),
                };

                let view = PaperFillView::from(fill_row);
                order_fills_map.entry(db_order.id).or_default().push(view.clone());
                fill_views.push(view);
            }
        }
    }

    // Update order statuses & build order views
    for o in &backtest_res.orders {
        if let Some(db_order) = order_id_to_db.get(&o.id) {
            let status = match o.state {
                OrderState::Filled => "filled",
                OrderState::PartiallyFilled => "partially_filled",
                OrderState::Pending => "accepted",
                OrderState::Rejected => "rejected",
                OrderState::Cancelled => "cancelled",
                OrderState::Expired => "expired",
            };
            let filled_qty = (o.qty - o.remaining).to_f64();
            let fills_for_order = order_fills_map.get(&db_order.id).cloned().unwrap_or_default();
            let avg_price = if fills_for_order.is_empty() {
                None
            } else {
                let notional: f64 = fills_for_order.iter().map(|f| f.qty * f.price).sum();
                let tot_qty: f64 = fills_for_order.iter().map(|f| f.qty).sum();
                if tot_qty > 0.0 {
                    Some(notional / tot_qty)
                } else {
                    None
                }
            };
            let reason = o.reject_reason.map(|r| r.as_str().to_string());

            let updated_order = match spectra_db::paper::update_paper_order_status(
                &pool,
                ws,
                db_order.id,
                status,
                filled_qty,
                avg_price,
                reason.as_deref(),
            )
            .await
            {
                Ok(Some(up)) => up,
                _ => db_order.clone(),
            };

            let mut view = PaperOrderView::from(updated_order);
            view.fills = Some(fills_for_order);
            order_views.push(view);
        }
    }

    // 8. Update portfolio positions
    for (&inst, pos) in backtest_res.ledger.positions() {
        let sym = symbols.get(inst as usize).cloned().unwrap_or_else(|| format!("INST_{inst}"));
        let mark_px = backtest_res.ledger.mark_price(inst).map_or(0.0, spectra_core::Fixed::to_f64);
        let _ = spectra_db::risk_center::upsert_position(
            &pool,
            ws,
            Some(req.strategy_id),
            &sym,
            pos.qty.to_f64(),
            mark_px,
        )
        .await;
    }

    // 9. Update portfolio balances and record snapshot
    let final_cash = backtest_res.ledger.cash().to_f64();
    let final_equity = backtest_res.ledger.equity().map_or(final_cash, spectra_core::Fixed::to_f64);
    let final_peak = account.balance.peak_equity.max(final_equity);

    let _ = spectra_db::risk_center::upsert_balance(
        &pool,
        ws,
        Some(req.strategy_id),
        final_cash,
        final_equity,
        final_peak,
        account.balance.daily_start_equity,
    )
    .await;

    let _ = spectra_db::risk_center::record_equity_snapshot(
        &pool,
        ws,
        Some(req.strategy_id),
        None,
        0,
        Utc::now().timestamp_nanos_opt().unwrap_or(0),
        final_equity,
        final_cash,
    )
    .await;

    // 10. Refreshed paper account
    let updated_account =
        match spectra_db::paper::get_or_init_paper_account(&pool, ws, req.strategy_id, None).await {
            Ok(acc) => acc,
            Err(e) => return db_error(&e),
        };

    let resp = StepPaperResponse {
        strategy_id: req.strategy_id,
        strategy_version_id: Some(version.id),
        orders: order_views,
        fills: fill_views,
        account: PaperAccountView::from(updated_account),
    };

    json_response(200, json!(resp).to_string())
}

/// Sync orders and fills with broker or simulated ledger (Task 10).
#[utoipa::path(post, path = "/api/v1/paper/sync", tag = "paper",
    responses((status = 200, description = "Orders synced successfully", body = SyncOrdersResponse)))]
pub(crate) async fn sync_paper_orders(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let creds = crate::providers::vault::resolve_alpaca_credentials(&pool, ws, None, &json!({})).await.ok();
    crate::broker_api::handle_sync_orders(&pool, ws, creds.as_ref()).await
}

/// Reconcile open positions between broker and local ledger (Task 11).
#[utoipa::path(get, path = "/api/v1/paper/reconcile", tag = "paper",
    responses((status = 200, description = "Position reconciliation report", body = ReconcilePositionsResponse)))]
pub(crate) async fn reconcile_paper_positions(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    uri: axum::http::Uri,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let creds = crate::providers::vault::resolve_alpaca_credentials(&pool, ws, None, &json!({})).await.ok();
    crate::broker_api::handle_reconcile_positions(&pool, ws, creds.as_ref(), uri.query().unwrap_or_default())
        .await
}
