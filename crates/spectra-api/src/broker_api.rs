//! Brokerage proxy routes and live/paper order execution path with pre-trade risk enforcement (M8 Task 8, M9).
//!
//! Enforces pre-trade risk checks (limits, kill switch, portfolio snapshot) before any live or
//! paper order reaches the broker adapter (Alpaca / paper simulated).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Extension;
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use spectra_db::risk_center;
use spectra_risk::enforcement::{
    OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, PreTradeChecker, PreTradeVerdict,
    RiskLimits,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::jobs_api::context;
use crate::providers::vault::resolve_alpaca_credentials;
use crate::risk_center_api::{fail_closed_response, user_of, RiskLimitsView};
use crate::{error_response, error_with_code, json_response, AppState, Identity};

const ALPACA_PAPER_BASE_URL: &str = "https://paper-api.alpaca.markets";
const FORBIDDEN_VIEWER: &str = "Viewers have read-only access to broker routes.";

#[allow(clippy::result_large_err)]
fn parse_body<T: serde::de::DeserializeOwned>(body: Result<Bytes, BytesRejection>) -> Result<T, Response> {
    let Ok(bytes) = body else {
        return Err(error_response(400, "Could not read request body."));
    };
    if bytes.is_empty() {
        return Err(error_response(400, "Request body is empty."));
    }
    serde_json::from_slice(&bytes).map_err(|e| error_response(400, format!("Malformed JSON: {e}")))
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlpacaOrderRequest {
    pub symbol: String,
    pub qty: Option<JsonValue>, // Can be numeric or string
    pub side: String,
    #[serde(rename = "type")]
    pub order_type: Option<String>,
    pub time_in_force: Option<String>,
    pub limit_price: Option<JsonValue>,
    pub client_order_id: Option<String>,
}

impl AlpacaOrderRequest {
    fn parse_qty(&self) -> Option<f64> {
        match &self.qty {
            Some(JsonValue::Number(n)) => n.as_f64(),
            Some(JsonValue::String(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    fn parse_limit_price(&self) -> Option<f64> {
        match &self.limit_price {
            Some(JsonValue::Number(n)) => n.as_f64(),
            Some(JsonValue::String(s)) => s.trim().parse::<f64>().ok(),
            _ => None,
        }
    }
}

/// Dispatches Alpaca proxy requests to live credentials or paper simulated engine.
pub(crate) async fn alpaca_proxy(
    State(st): State<AppState>,
    Extension(identity): Extension<Identity>,
    method: Method,
    uri: Uri,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let (pool, ws) = match context(&st, &identity) {
        Ok(c) => c,
        Err(r) => return r,
    };

    let user_id = user_of(&identity);

    // Viewers cannot place or modify orders
    if (method == Method::POST || method == Method::DELETE) && user_id.is_some() {
        let role: Option<(String,)> =
            sqlx::query_as("SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
                .bind(ws)
                .bind(user_id)
                .fetch_optional(&pool)
                .await
                .unwrap_or(None);

        if let Some((r,)) = role {
            if r == "viewer" {
                return error_with_code(403, "viewer_read_only", FORBIDDEN_VIEWER);
            }
        }
    }

    let path = uri.path();
    // Normalize path by stripping /api/v1/alpaca or /api/alpaca
    let subpath = if let Some(p) = path.strip_prefix("/api/v1/alpaca") {
        p.trim_start_matches('/')
    } else if let Some(p) = path.strip_prefix("/api/alpaca") {
        p.trim_start_matches('/')
    } else {
        path.trim_start_matches('/')
    };

    let creds = resolve_alpaca_credentials(&pool, ws, None, &json!({})).await.ok();

    match (method, subpath) {
        (Method::GET, "account" | "v2/account") => handle_get_account(&pool, ws, creds.as_ref()).await,
        (Method::GET, "positions" | "v2/positions") => handle_get_positions(&pool, ws, creds.as_ref()).await,
        (Method::POST, "orders" | "v2/orders") => {
            handle_create_order(&pool, ws, user_id, creds.as_ref(), body).await
        }
        (Method::DELETE, p) if p.starts_with("orders/") || p.starts_with("v2/orders/") => {
            let order_id = p.rsplit('/').next().unwrap_or_default();
            handle_cancel_order(order_id, creds.as_ref()).await
        }
        _ => {
            // Forward other GET requests to Alpaca if credentials exist, else 404
            if let Some((key_id, secret_key)) = creds {
                forward_alpaca_request(subpath, &key_id, &secret_key).await
            } else {
                error_response(404, format!("Alpaca endpoint /{subpath} not found in paper mode."))
            }
        }
    }
}

async fn handle_get_account(pool: &sqlx::PgPool, ws: Uuid, creds: Option<&(String, String)>) -> Response {
    if let Some((key_id, secret_key)) = creds {
        let client = reqwest::Client::new();
        if let Ok(resp) = client
            .get(format!("{ALPACA_PAPER_BASE_URL}/v2/account"))
            .header("APCA-API-KEY-ID", key_id)
            .header("APCA-API-SECRET-KEY", secret_key)
            .send()
            .await
        {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let body = resp.bytes().await.unwrap_or_default();
            return (status, [("content-type", "application/json")], body).into_response();
        }
    }

    // Fallback: simulated paper account from DB balances
    let balance = risk_center::get_balance(pool, ws, None).await.unwrap_or(None);
    let (cash, equity, _peak_equity, daily_start) = if let Some(b) = balance {
        (b.cash, b.equity, b.peak_equity, b.daily_start_equity)
    } else {
        (100_000.0, 100_000.0, 100_000.0, 100_000.0)
    };
    let buying_power = (cash * 2.0).max(0.0);

    json_response(
        200,
        json!({
            "id": format!("paper-{ws}"),
            "account_number": format!("PA-{}", &ws.to_string()[..8]),
            "status": "ACTIVE",
            "crypto_status": "ACTIVE",
            "currency": "USD",
            "buying_power": format!("{:.2}", buying_power),
            "regt_buying_power": format!("{:.2}", buying_power),
            "daytrading_buying_power": format!("{:.2}", buying_power),
            "cash": format!("{:.2}", cash),
            "portfolio_value": format!("{:.2}", equity),
            "equity": format!("{:.2}", equity),
            "last_equity": format!("{:.2}", daily_start),
            "multiplier": "2",
            "initial_margin": "0.00",
            "maintenance_margin": "0.00",
            "sma": "0.00",
            "daytrade_count": 0,
            "last_maintenance_margin": "0.00",
            "pattern_day_trader": false,
            "trading_blocked": false,
            "transfers_blocked": false,
            "account_blocked": false,
            "created_at": "2026-01-01T00:00:00Z",
            "trade_suspended_by_user": false,
            "shorting_enabled": true
        })
        .to_string(),
    )
}

async fn handle_get_positions(pool: &sqlx::PgPool, ws: Uuid, creds: Option<&(String, String)>) -> Response {
    if let Some((key_id, secret_key)) = creds {
        let client = reqwest::Client::new();
        if let Ok(resp) = client
            .get(format!("{ALPACA_PAPER_BASE_URL}/v2/positions"))
            .header("APCA-API-KEY-ID", key_id)
            .header("APCA-API-SECRET-KEY", secret_key)
            .send()
            .await
        {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let body = resp.bytes().await.unwrap_or_default();
            return (status, [("content-type", "application/json")], body).into_response();
        }
    }

    let positions = risk_center::get_positions(pool, ws, None).await.unwrap_or_default();
    let views: Vec<JsonValue> = positions
        .into_iter()
        .map(|p| {
            let mkt_val = (p.qty * p.price).abs();
            json!({
                "asset_id": Uuid::now_v7().to_string(),
                "symbol": p.symbol,
                "exchange": "NASDAQ",
                "asset_class": "us_equity",
                "qty": p.qty.to_string(),
                "avg_entry_price": format!("{:.2}", p.price),
                "side": if p.qty >= 0.0 { "long" } else { "short" },
                "market_value": format!("{:.2}", mkt_val),
                "cost_basis": format!("{:.2}", mkt_val),
                "unrealized_pl": "0.00",
                "unrealized_plpc": "0.00",
                "current_price": format!("{:.2}", p.price)
            })
        })
        .collect();

    json_response(200, json!(views).to_string())
}

/// Task 8: Live and paper order pre-trade verification flow.
///
/// Every broker order is evaluated through the exact same pre-trade checks as the backtest engine.
/// Orders that violate risk limits or the kill switch are rejected before reaching the broker.
async fn handle_create_order(
    pool: &sqlx::PgPool,
    ws: Uuid,
    user_id: Option<Uuid>,
    creds: Option<&(String, String)>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let order_req: AlpacaOrderRequest = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };

    if order_req.symbol.trim().is_empty() {
        return error_response(422, "Symbol is required.");
    }
    let qty = match order_req.parse_qty() {
        Some(q) if q > 0.0 => q,
        _ => return error_response(422, "Order quantity must be a positive number."),
    };

    let side = match order_req.side.to_lowercase().as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => return error_response(422, "Side must be 'buy' or 'sell'."),
    };

    // 1. Determine kill switch status (Fail closed on DB error - Task 3)
    let kill_switch_active = match risk_center::get_kill_switch(pool, ws, None).await {
        Ok(Some(ks)) => ks.active,
        Ok(None) => false,
        Err(_) => {
            return fail_closed_response(
                pool,
                ws,
                None,
                &order_req.symbol,
                side,
                qty,
                100.0,
                "Broker risk check failed: could not load kill switch state from database.",
                user_id,
            )
            .await;
        }
    };

    // 2. Load limits (Fail closed on DB error - Task 3)
    let limits = match risk_center::get_limits(pool, ws, None).await {
        Ok(Some(row)) => RiskLimits::from(&RiskLimitsView::from(row)),
        Ok(None) => RiskLimits::default(),
        Err(_) => {
            return fail_closed_response(
                pool,
                ws,
                None,
                &order_req.symbol,
                side,
                qty,
                100.0,
                "Broker risk check failed: could not load risk limits from database.",
                user_id,
            )
            .await;
        }
    };

    // 3. Load server portfolio (Fail closed on DB error - Task 1 & 3)
    let Ok(db_positions) = risk_center::get_positions(pool, ws, None).await else {
        return fail_closed_response(
            pool,
            ws,
            None,
            &order_req.symbol,
            side,
            qty,
            100.0,
            "Broker risk check failed: could not load portfolio positions from database.",
            user_id,
        )
        .await;
    };
    let Ok(db_balance) = risk_center::get_balance(pool, ws, None).await else {
        return fail_closed_response(
            pool,
            ws,
            None,
            &order_req.symbol,
            side,
            qty,
            100.0,
            "Broker risk check failed: could not load portfolio balance from database.",
            user_id,
        )
        .await;
    };

    let mut positions = HashMap::new();
    let mut symbol_price = order_req.parse_limit_price().unwrap_or(100.0);
    for p in &db_positions {
        if p.symbol == order_req.symbol && order_req.parse_limit_price().is_none() && p.price > 0.0 {
            symbol_price = p.price;
        }
        positions.insert(p.symbol.clone(), PositionSnapshot { qty: p.qty, price: p.price });
    }
    let (cash, equity, peak_equity, daily_start_equity) = if let Some(b) = db_balance {
        (b.cash, b.equity, b.peak_equity, b.daily_start_equity)
    } else {
        let gross: f64 = positions.values().map(PositionSnapshot::value).map(f64::abs).sum();
        (100_000.0, 100_000.0 + gross, 100_000.0 + gross, 100_000.0 + gross)
    };
    let portfolio = PortfolioSnapshot { cash, equity, peak_equity, daily_start_equity, positions };

    // 4. Run pre-trade check
    let candidate = OrderCandidate { symbol: order_req.symbol.clone(), side, qty, price: symbol_price };
    let eval_res = PreTradeChecker::evaluate(&limits, &portfolio, &candidate, kill_switch_active);

    // If order is rejected: STOP BEFORE REACHING BROKER!
    let breaches_json = json!(eval_res
        .breaches
        .iter()
        .map(|b| json!({
            "code": b.code.as_str(),
            "limit": b.limit,
            "projected": b.projected,
            "message": b.message.clone(),
        }))
        .collect::<Vec<_>>());
    let breach_msgs: Vec<String> = eval_res.breaches.iter().map(|b| b.message.clone()).collect();
    let metrics_json = json!({
        "gross_exposure": eval_res.projected_metrics.gross_exposure,
        "net_exposure": eval_res.projected_metrics.net_exposure,
        "leverage": eval_res.projected_metrics.leverage,
        "position_size": eval_res.projected_metrics.position_size,
        "concentration_pct": eval_res.projected_metrics.concentration_pct,
        "drawdown_pct": eval_res.projected_metrics.drawdown_pct,
        "daily_loss": eval_res.projected_metrics.daily_loss,
        "var_pct": eval_res.projected_metrics.var_pct,
    });

    if eval_res.verdict == PreTradeVerdict::Rejected {
        let _ = risk_center::record_decision(
            pool,
            ws,
            None,
            &order_req.symbol,
            match side {
                OrderSide::Buy => "buy",
                OrderSide::Sell => "sell",
            },
            qty,
            symbol_price,
            0.0,
            "rejected",
            &breaches_json,
            &breach_msgs,
            &metrics_json,
            user_id,
        )
        .await;

        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            [("content-type", "application/json")],
            json!({
                "code": 42_210_000,
                "message": format!("Pre-trade risk limit exceeded: {}", breach_msgs.join("; ")),
                "verdict": "rejected",
                "breaches": breaches_json,
                "requested_qty": qty,
                "approved_qty": 0.0,
            })
            .to_string(),
        )
            .into_response();
    }

    let approved_qty =
        if let PreTradeVerdict::Reduced = eval_res.verdict { eval_res.approved_qty } else { qty };

    // Log approved/reduced decision
    let verdict_str = match eval_res.verdict {
        PreTradeVerdict::Approved => "approved",
        PreTradeVerdict::Reduced => "reduced",
        PreTradeVerdict::Rejected => "rejected",
    };
    let _ = risk_center::record_decision(
        pool,
        ws,
        None,
        &order_req.symbol,
        match side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        },
        qty,
        symbol_price,
        approved_qty,
        verdict_str,
        &breaches_json,
        &breach_msgs,
        &metrics_json,
        user_id,
    )
    .await;

    // 5. Forward to broker or execute simulated paper fill
    if let Some((key_id, secret_key)) = creds {
        let client = reqwest::Client::new();
        let forward_body = json!({
            "symbol": &order_req.symbol,
            "qty": approved_qty.to_string(),
            "side": &order_req.side,
            "type": order_req.order_type.as_deref().unwrap_or("market"),
            "time_in_force": order_req.time_in_force.as_deref().unwrap_or("day"),
            "limit_price": order_req.parse_limit_price().map(|p| p.to_string()),
            "client_order_id": &order_req.client_order_id,
        });

        match client
            .post(format!("{ALPACA_PAPER_BASE_URL}/v2/orders"))
            .header("APCA-API-KEY-ID", key_id)
            .header("APCA-API-SECRET-KEY", secret_key)
            .json(&forward_body)
            .send()
            .await
        {
            Ok(resp) => {
                let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
                let body = resp.bytes().await.unwrap_or_default();
                return (status, [("content-type", "application/json")], body).into_response();
            }
            Err(e) => {
                warn!("Failed to forward order to Alpaca: {e}");
            }
        }
    }

    // Simulated paper order execution
    let order_id = Uuid::now_v7();
    let now = chrono::Utc::now().to_rfc3339();
    let client_order_id = order_req.client_order_id.unwrap_or_else(|| Uuid::now_v7().to_string());
    let order_type = order_req.order_type.unwrap_or_else(|| "market".to_string());
    let time_in_force = order_req.time_in_force.unwrap_or_else(|| "day".to_string());

    // Update positions in DB
    let current_pos = db_positions.into_iter().find(|p| p.symbol == order_req.symbol);
    let current_qty = current_pos.as_ref().map_or(0.0, |p| p.qty);
    let delta = match side {
        OrderSide::Buy => approved_qty,
        OrderSide::Sell => -approved_qty,
    };
    let new_qty = current_qty + delta;
    let _ = risk_center::upsert_position(pool, ws, None, &order_req.symbol, new_qty, symbol_price).await;

    // Update balance in DB
    let notional = approved_qty * symbol_price;
    let new_cash = match side {
        OrderSide::Buy => cash - notional,
        OrderSide::Sell => cash + notional,
    };
    let _ = risk_center::upsert_balance(
        pool,
        ws,
        None,
        new_cash,
        equity,
        peak_equity.max(equity),
        daily_start_equity,
    )
    .await;

    info!(
        workspace_id = %ws,
        symbol = %order_req.symbol,
        approved_qty = %approved_qty,
        "Paper order pre-trade verified and executed"
    );

    json_response(
        201,
        json!({
            "id": order_id.to_string(),
            "client_order_id": client_order_id,
            "created_at": now,
            "updated_at": now,
            "submitted_at": now,
            "filled_at": now,
            "symbol": order_req.symbol,
            "qty": approved_qty.to_string(),
            "filled_qty": approved_qty.to_string(),
            "type": order_type,
            "side": match side { OrderSide::Buy => "buy", OrderSide::Sell => "sell" },
            "time_in_force": time_in_force,
            "status": "filled",
            "filled_avg_price": format!("{:.2}", symbol_price),
            "pre_trade_verdict": verdict_str,
        })
        .to_string(),
    )
}

async fn handle_cancel_order(order_id: &str, creds: Option<&(String, String)>) -> Response {
    if let Some((key_id, secret_key)) = creds {
        let client = reqwest::Client::new();
        if let Ok(resp) = client
            .delete(format!("{ALPACA_PAPER_BASE_URL}/v2/orders/{order_id}"))
            .header("APCA-API-KEY-ID", key_id)
            .header("APCA-API-SECRET-KEY", secret_key)
            .send()
            .await
        {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::NO_CONTENT);
            return (status, [("content-type", "application/json")], "").into_response();
        }
    }

    // Simulated cancellation
    StatusCode::NO_CONTENT.into_response()
}

async fn forward_alpaca_request(subpath: &str, key_id: &str, secret_key: &str) -> Response {
    let client = reqwest::Client::new();
    match client
        .get(format!("{ALPACA_PAPER_BASE_URL}/{subpath}"))
        .header("APCA-API-KEY-ID", key_id)
        .header("APCA-API-SECRET-KEY", secret_key)
        .send()
        .await
    {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let body = resp.bytes().await.unwrap_or_default();
            (status, [("content-type", "application/json")], body).into_response()
        }
        Err(e) => error_response(502, format!("Alpaca proxy gateway error: {e}")),
    }
}
