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
use spectra_db::paper::{PaperAccount, PaperFillRow, PaperOrderRow};
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
}

impl From<PaperAccount> for PaperAccountView {
    fn from(acc: PaperAccount) -> Self {
        Self {
            workspace_id: acc.workspace_id,
            strategy_id: acc.strategy_id,
            cash: acc.balance.cash,
            equity: acc.balance.equity,
            peak_equity: acc.balance.peak_equity,
            daily_start_equity: acc.balance.daily_start_equity,
            positions: acc.positions.into_iter().map(PortfolioPositionView::from).collect(),
            updated_at: acc.balance.updated_at,
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

    match spectra_db::paper::get_or_init_paper_account(&pool, ws, q.strategy_id, q.initial_cash).await {
        Ok(acc) => json_response(200, json!(PaperAccountView::from(acc)).to_string()),
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
