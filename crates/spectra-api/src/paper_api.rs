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
use spectra_engine::costs::{commission_fee, slipped_price, CommissionModel, CostModel};
use spectra_engine::{OrderState, Strategy};
use spectra_risk::enforcement::{
    OrderCandidate, OrderSide, PortfolioSnapshot, PositionSnapshot, PreTradeChecker, PreTradeVerdict,
};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error};
use crate::risk_center_api::{PortfolioPositionView, RiskBreachView};
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

/// `true` if `strat_id` is a strategy of workspace `ws`.
async fn strategy_in_workspace(
    pool: &spectra_db::PgPool,
    ws: Uuid,
    strat_id: Uuid,
) -> Result<bool, spectra_db::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM strategies WHERE id = $1 AND workspace_id = $2)")
        .bind(strat_id)
        .bind(ws)
        .fetch_one(pool)
        .await
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
    /// 1-day 99% VaR as a fraction of equity; `null` when it cannot be computed (see
    /// `var_unavailable_reason`).
    pub var_pct: Option<f64>,
    /// VaR in currency; `null` with `var_pct`.
    pub var_dollar: Option<f64>,
    /// 1-day 99% CVaR (mean loss at or beyond VaR) in currency, from the same simulation as
    /// VaR; `null` with `var_pct`.
    pub cvar_dollar: Option<f64>,
    /// Why VaR and CVaR are `null`, for example a position with no volatility.
    pub var_unavailable_reason: Option<String>,
}

/// Slippage applied to every paper fill, in basis points.
pub(crate) const PAPER_SLIPPAGE_BPS: f64 = 5.0;
/// Minimum commission charged on each paper fill.
pub(crate) const PAPER_MIN_FEE: f64 = 1.0;

/// A named, versioned paper cost configuration. Each fill it prices stores it in
/// `paper_fills.cost_config`, so a fill says which costs it was charged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaperCostConfig {
    pub name: &'static str,
    /// Bumped whenever a value below changes.
    pub version: u32,
    pub slippage_bps: f64,
    pub min_fee: f64,
}

/// The costs of manual paper fills (submitted orders): slippage and a minimum fee, no other
/// commission, spread or volume slippage.
pub(crate) const PAPER_COST_CONFIG: PaperCostConfig = PaperCostConfig {
    name: "paper-default",
    version: 1,
    slippage_bps: PAPER_SLIPPAGE_BPS,
    min_fee: PAPER_MIN_FEE,
};

impl PaperCostConfig {
    /// The cost model applied to paper fills.
    ///
    /// # Errors
    /// The named cost values do not convert to fixed point.
    fn cost_model(self) -> Result<CostModel, spectra_core::ArithmeticError> {
        Ok(CostModel {
            slippage: spectra_core::Bps::from_f64(self.slippage_bps)?,
            commission: CommissionModel { min_fee: Money::from_f64(self.min_fee)?, ..Default::default() },
            ..Default::default()
        })
    }

    /// The form stored with each fill and returned in fill views.
    pub(crate) fn to_json(self) -> serde_json::Value {
        json!({
            "name": self.name,
            "version": self.version,
            "slippage_bps": self.slippage_bps,
            "min_fee": self.min_fee,
        })
    }
}

/// The cost config of a fill from a paper strategy run, which is priced by the run's config
/// preset (not [`PAPER_COST_CONFIG`]): the preset's name and every cost it charges.
fn preset_cost_config_json(preset: Preset, costs: &CostModel) -> serde_json::Value {
    json!({
        "name": format!("preset:{}", preset.name()),
        "version": 1,
        "slippage_bps": costs.slippage.to_f64(),
        "min_fee": costs.commission.min_fee.to_f64(),
        "commission_bps": costs.commission.bps_of_notional.to_f64(),
        "commission_per_unit": costs.commission.per_unit.to_f64(),
        "spread_bps": costs.spread.to_f64(),
        "volume_slippage_bps": costs.volume_slippage.to_f64(),
    })
}

impl PaperAccountView {
    /// The view, with each position valued at its price times its contract
    /// multiplier (`records`: the workspace's instrument records).
    #[allow(clippy::unnecessary_wraps)]
    fn new(
        acc: PaperAccount,
        records: &HashMap<String, spectra_core::Instrument>,
        volatilities: &HashMap<String, f64>,
    ) -> spectra_core::Result<Self> {
        let mut positions_map = HashMap::new();
        for pos in &acc.positions {
            let price = pos.price * crate::instruments_api::multiplier_of(records, &pos.symbol);
            let vol = volatilities.get(&pos.symbol).copied();
            positions_map
                .insert(pos.symbol.clone(), PositionSnapshot { qty: pos.qty, price, volatility: vol });
        }
        let snap = PortfolioSnapshot {
            cash: acc.balance.cash,
            equity: acc.balance.equity,
            peak_equity: acc.balance.peak_equity,
            daily_start_equity: acc.balance.daily_start_equity,
            positions: positions_map,
        };
        // VaR and CVaR come from one simulation. This is a read-only view, so when they cannot
        // be computed it reports why instead of failing the whole account.
        let mut missing: Vec<&str> = snap
            .positions
            .iter()
            .filter(|(_, p)| p.qty.abs() > 1e-8 && p.volatility.is_none())
            .map(|(sym, _)| sym.as_str())
            .collect();
        missing.sort_unstable();
        let (var_pct, cvar_pct, var_unavailable_reason) = if missing.is_empty() {
            match snap.var_and_cvar_pct() {
                Ok((var, cvar)) => (Some(var), Some(cvar), None),
                Err(e) => (None, None, Some(e.to_string())),
            }
        } else {
            let reason =
                format!("no volatility for {} (no dataset with at least 60 bars)", missing.join(", "));
            (None, None, Some(reason))
        };
        let var_dollar = var_pct.map(|v| v * snap.equity);
        let cvar_dollar = cvar_pct.map(|c| c * snap.equity);

        Ok(Self {
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
            var_unavailable_reason,
        })
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
    #[serde(default)]
    pub dataset_version_id: Option<Uuid>,
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
    /// The cost configuration that priced the fill (`name`, `version`, `slippage_bps`,
    /// `min_fee`, and for a strategy run the preset's other costs); `null` for fills recorded
    /// before it was stored.
    #[schema(value_type = Option<Object>)]
    pub cost_config: Option<serde_json::Value>,
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
            cost_config: r.cost_config,
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

    let records = match crate::instruments_api::order_instruments(&pool, ws).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    match spectra_db::paper::get_or_init_paper_account(&pool, ws, q.strategy_id, q.initial_cash).await {
        Ok(acc) => {
            let default_dir = std::path::PathBuf::from("data");
            let data_dir = st.cfg.data_dir.as_deref().unwrap_or(&default_dir);
            let pos_syms: Vec<&str> = acc.positions.iter().map(|p| p.symbol.as_str()).collect();
            let vols = if pos_syms.is_empty() {
                HashMap::new()
            } else {
                match crate::risk_center_api::lookup_portfolio_volatilities(
                    &pool, ws, data_dir, None, &pos_syms,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        return error_response(422, format!("Failed to compute paper account metrics: {e}"))
                    }
                }
            };
            match PaperAccountView::new(acc, &records, &vols) {
                Ok(v) => json_response(200, json!(v).to_string()),
                Err(e) => error_response(422, format!("Failed to compute paper account metrics: {e}")),
            }
        }
        Err(e) => db_error(&e),
    }
}

/// Get P&L attribution breakdown for a paper trading strategy account (Module 11).
#[utoipa::path(
    get,
    path = "/api/v1/paper/attribution",
    tag = "paper",
    params(PaperAccountQuery),
    responses(
        (status = 200, description = "Paper trading P&L attribution", body = serde_json::Value),
        (status = 400, description = "The account has no workspace (code `no_workspace`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "No such strategy in this workspace", body = crate::openapi::ErrorDoc),
    )
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

    // A read: attribution never creates a strategy or an account. A strategy that is not in the
    // signed workspace is a 404, like any other workspace's data (M11 Decision 4).
    match strategy_in_workspace(&pool, ws, q.strategy_id).await {
        Ok(true) => {}
        Ok(false) => return error_response(404, "No such paper strategy in this workspace."),
        Err(e) => return db_error(&e),
    }

    let records = match crate::instruments_api::order_instruments(&pool, ws).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    // Only fills since the start of the current account count: a reset keeps old fills. A
    // strategy with no account yet has no fills, so its breakdown is empty.
    let fills = match spectra_db::paper::paper_account_started_at(&pool, ws, q.strategy_id).await {
        Ok(Some(started_at)) => {
            match spectra_db::paper::list_paper_fills_since(&pool, ws, q.strategy_id, started_at).await {
                Ok(f) => f,
                Err(e) => return db_error(&e),
            }
        }
        Ok(None) => Vec::new(),
        Err(e) => return db_error(&e),
    };
    match paper_attribution(&fills, &records) {
        Ok(attr) => json_response(200, attr.to_json().dump()),
        Err(e) => error_response(422, format!("Failed to compute paper attribution: {e}")),
    }
}

/// The P&L attribution of one paper account's fills (oldest first): the fills are replayed into
/// a ledger and the backtest attribution runs over it, with the paper slippage rate. As for
/// backtests, it covers closed trades; positions still open are not in it.
///
/// # Errors
/// A fill with an unknown side, a value that does not convert to fixed point, a time out of
/// range, or a ledger or attribution error.
fn paper_attribution(
    fills: &[PaperFillRow],
    records: &HashMap<String, spectra_core::Instrument>,
) -> Result<spectra_research::PnlAttribution, String> {
    let mut symbols: Vec<String> = fills.iter().map(|f| f.symbol.clone()).collect();
    symbols.sort_unstable();
    symbols.dedup();
    let id_of = |sym: &str| -> Result<spectra_core::InstrumentId, String> {
        let i =
            symbols.binary_search_by(|s| s.as_str().cmp(sym)).map_err(|_| format!("unknown symbol {sym}"))?;
        u32::try_from(i).map_err(|_| format!("too many instruments ({i})"))
    };
    let ts_ns_of = |f: &PaperFillRow| -> Result<i64, String> {
        f.created_at.timestamp_nanos_opt().ok_or_else(|| format!("fill {} time is out of range", f.id))
    };

    // Shorts and margin are allowed: attribution measures P&L, not admission.
    let mut ledger = spectra_core::Ledger::new(spectra_core::LedgerConfig::reference(true, true));
    let first_ts = match fills.first() {
        Some(f) => ts_ns_of(f)?,
        None => 0,
    };
    for sym in &symbols {
        let record = crate::instruments_api::record_of(records, sym);
        let multiplier = record.map_or(spectra_core::Ratio::ONE, |r| r.multiplier);
        let marked_to_market = record.is_some_and(|r| r.kind == spectra_core::InstrumentType::Future);
        ledger
            .register_contract(id_of(sym)?, multiplier, marked_to_market, first_ts)
            .map_err(|e| format!("instrument {sym}: {e}"))?;
    }
    for f in fills {
        let side = match f.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => return Err(format!("fill {} has unknown side {other}", f.id)),
        };
        let fill = spectra_core::Fill {
            instrument: id_of(&f.symbol)?,
            side,
            qty: Qty::from_f64(f.qty)
                .map_err(|e| format!("fill {}: invalid quantity {}: {e}", f.id, f.qty))?,
            price: Price::from_f64(f.price)
                .map_err(|e| format!("fill {}: invalid price {}: {e}", f.id, f.price))?,
            fee: Money::from_f64(f.fee).map_err(|e| format!("fill {}: invalid fee {}: {e}", f.id, f.fee))?,
            ts_ns: ts_ns_of(f)?,
        };
        ledger.apply_fill(&fill).map_err(|e| format!("fill {}: {e}", f.id))?;
    }
    spectra_research::compute_pnl_attribution(&ledger, Some(&symbols), Some(PAPER_SLIPPAGE_BPS))
        .map_err(|e| e.to_string())
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

    let records = match crate::instruments_api::order_instruments(&pool, ws).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    match spectra_db::paper::reset_paper_account(&pool, ws, req.strategy_id, req.initial_cash).await {
        Ok(acc) => match PaperAccountView::new(acc, &records, &HashMap::new()) {
            Ok(v) => json_response(200, json!(v).to_string()),
            Err(e) => error_response(422, format!("Failed to compute paper account metrics: {e}")),
        },
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
    // M24: every type trades on paper with its contract multiplier; a contract past
    // its expiry or maturity does not trade. Fails closed when records cannot be read.
    let records = match crate::instruments_api::order_instruments(&pool, ws).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let now_ns = Utc::now().timestamp_nanos_opt().unwrap_or(i64::MAX);
    if let Some(refusal) =
        crate::instruments_api::refuse_expired(crate::instruments_api::record_of(&records, &symbol), now_ns)
    {
        return refusal;
    }
    let mult = |sym: &str| crate::instruments_api::multiplier_of(&records, sym);
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

    let mut current_sym_price = None;
    for p in &account.positions {
        if p.symbol == symbol {
            current_sym_price = Some(p.price);
        }
    }

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

    if kill_switch_active {
        let reason = "Kill switch is active; all trading halted.";
        // `None` when no price is known (a market order with no position): the decision is
        // still recorded, with no price, instead of a 0.0 the audit table refuses.
        let known_price = req.fill_price.or(req.limit_price).or(current_sym_price);
        if let Err(e) = spectra_db::risk_center::record_decision(
            &pool,
            ws,
            Some(strat_id),
            &symbol,
            &side_str,
            req.qty,
            known_price,
            0.0,
            "rejected",
            &json!([{
                "code": "KILL_SWITCH",
                "limit": 0.0,
                "projected": 0.0,
                "message": reason
            }]),
            &["KILL_SWITCH".to_string()],
            &json!({}),
            user_id,
        )
        .await
        {
            return crate::risk_center_api::decision_not_recorded(&e);
        }

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
            Some(reason),
            user_id,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => return db_error(&e),
        };

        return json_response(422, json!(PaperOrderView::from(order)).to_string());
    }

    let default_dir = std::path::PathBuf::from("data");
    let data_dir = st.cfg.data_dir.as_deref().unwrap_or(&default_dir);

    let eval_price = if order_type_str == "market" {
        match crate::dataset_prices::latest_prices(&pool, ws, data_dir, req.dataset_version_id, &[&symbol])
            .await
        {
            Ok(prices) => match prices.get(&symbol) {
                Some(&p) => p,
                None => return error_response(422, format!("no bars for {symbol}")),
            },
            Err(e) => return error_response(422, e),
        }
    } else {
        let Some(p) = req.fill_price.or(req.limit_price).or(current_sym_price) else {
            return error_response(422, "Order price could not be determined.");
        };
        p
    };
    if eval_price <= 0.0 || !eval_price.is_finite() {
        return error_response(422, format!("Non-positive or non-finite order price: {eval_price}"));
    }

    let mut syms_to_lookup = vec![symbol.as_str()];
    for p in &account.positions {
        syms_to_lookup.push(&p.symbol);
    }

    let vols = match crate::risk_center_api::lookup_portfolio_volatilities(
        &pool,
        ws,
        data_dir,
        req.dataset_version_id,
        &syms_to_lookup,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return error_response(422, format!("Pre-trade risk check failed: {e}")),
    };

    let mut positions_map = HashMap::new();
    for p in &account.positions {
        let vol = vols.get(&p.symbol).copied();
        // Risk sees value per unit: price times the contract multiplier.
        positions_map.insert(
            p.symbol.clone(),
            PositionSnapshot { qty: p.qty, price: p.price * mult(&p.symbol), volatility: vol },
        );
    }
    let cand_vol = vols.get(&symbol).copied();
    positions_map.entry(symbol.clone()).or_insert_with(|| PositionSnapshot {
        qty: 0.0,
        price: eval_price * mult(&symbol),
        volatility: cand_vol,
    });

    let candidate =
        OrderCandidate { symbol: symbol.clone(), side, qty: req.qty, price: eval_price * mult(&symbol) };

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

    // Load limits. A DB error stops the order; it never falls back to default limits.
    let limits = match crate::risk_center_api::load_effective_limits(&pool, ws, Some(strat_id)).await {
        Ok(l) => l,
        Err(e) => return db_error(&e),
    };

    // Run M8 Pre-Trade Risk Check (Task 3)
    let pre_trade = match PreTradeChecker::evaluate(&limits, &snapshot, &candidate, kill_switch_active) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Pre-trade risk check failed: {e}")),
    };

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
        "portfolio_snapshot": crate::risk_center_api::portfolio_snapshot_json(&snapshot),
        "risk_limits": crate::risk_center_api::risk_limits_json(&limits),
    });

    if let Err(e) = spectra_db::risk_center::record_decision(
        &pool,
        ws,
        Some(strat_id),
        &symbol,
        &side_str,
        req.qty,
        Some(eval_price),
        pre_trade.approved_qty,
        verdict_str,
        &breaches_json,
        &breach_codes,
        &metrics_json,
        user_id,
    )
    .await
    {
        return crate::risk_center_api::decision_not_recorded(&e);
    }

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

    // Order is accepted (with pre-trade size reduction if applicable)
    let order_qty =
        if pre_trade.verdict == PreTradeVerdict::Reduced { pre_trade.approved_qty } else { req.qty };
    let order = match spectra_db::paper::insert_paper_order(
        &pool,
        ws,
        strat_id,
        &symbol,
        &side_str,
        &order_type_str,
        order_qty,
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
    // Cost model: routed through spectra_engine::costs (CostModel, slipped_price, commission_fee)
    let cost_model = match PAPER_COST_CONFIG.cost_model() {
        Ok(m) => m,
        Err(e) => return error_response(500, format!("Paper cost model is invalid: {e}")),
    };
    let base_price_f64 = eval_price;
    let Ok(base_price_val) = Price::from_f64(base_price_f64) else {
        return error_response(422, format!("Invalid base price: {base_price_f64}"));
    };
    let engine_side = match side {
        OrderSide::Buy => Side::Buy,
        OrderSide::Sell => Side::Sell,
    };
    let fill_price = match slipped_price(base_price_val, engine_side, cost_model.slippage) {
        Ok(p) => spectra_core::Fixed::to_f64(p),
        Err(e) => return error_response(422, format!("Failed to compute slipped fill price: {e}")),
    };
    let fill_qty =
        if pre_trade.verdict == PreTradeVerdict::Reduced { pre_trade.approved_qty } else { req.qty };
    let mult_val = mult(&symbol);
    let notional_f64 = fill_qty * fill_price * mult_val;
    let Ok(notional_val) = Money::from_f64(notional_f64) else {
        return error_response(422, format!("Invalid fill notional: {notional_f64}"));
    };
    let Ok(fill_qty_fixed) = Qty::from_f64(fill_qty) else {
        return error_response(422, format!("Invalid fill quantity: {fill_qty}"));
    };
    let fee = match commission_fee(&cost_model.commission, fill_qty_fixed, notional_val) {
        Ok(f) => spectra_core::Fixed::to_f64(f),
        Err(e) => return error_response(422, format!("Failed to compute commission: {e}")),
    };

    // Record fill
    let fill_row = match spectra_db::paper::insert_paper_fill(
        &pool,
        order.id,
        ws,
        strat_id,
        &symbol,
        &side_str,
        fill_qty,
        fill_price,
        fee,
        &PAPER_COST_CONFIG.to_json(),
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
    let notional = fill_qty * fill_price * mult(&symbol);
    let cash_delta = match side {
        OrderSide::Buy => -notional - fee,
        OrderSide::Sell => notional - fee,
    };
    let new_cash = account.balance.cash + cash_delta;

    // Calculate new total equity = cash + sum(pos.qty * pos.price)
    let updated_positions =
        spectra_db::risk_center::get_positions(&pool, ws, Some(strat_id)).await.unwrap_or_default();
    let pos_value: f64 = updated_positions.iter().map(|p| p.qty * p.price * mult(&p.symbol)).sum();
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

    // A DB error stops the step; it never falls back to default limits.
    let limits = match crate::risk_center_api::load_effective_limits(&pool, ws, Some(req.strategy_id)).await {
        Ok(l) => l,
        Err(e) => return db_error(&e),
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

    // M24: the workspace's instrument records (multipliers, expiries, currencies), as backtests use them.
    let setup = match crate::instruments_api::setup_for(&pool, ws, &symbols).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let step_records = match crate::instruments_api::order_instruments(&pool, ws).await {
        Ok(r) => r,
        Err(r) => return r,
    };

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

        let mut ctl = spectra_engine::RunControl::default();
        match spectra_engine::run_backtest_with_setup(
            &engine_bars,
            &[],
            &setup,
            strategy.as_mut(),
            &cfg,
            &mut ctl,
        ) {
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

        // Record the risk decision first: if the audit insert fails, the order is not stored (fail closed).
        let verdict_str = if o.state == OrderState::Rejected { "rejected" } else { "approved" };
        let eval_price = match limit_px
            .or(stop_px)
            .or_else(|| engine_bars.iter().rfind(|b| b.instrument == o.instrument).map(|b| b.close.to_f64()))
        {
            Some(px) if px > 0.0 && px.is_finite() => px,
            _ => {
                return error_response(
                    422,
                    format!("Could not determine evaluation price for order on symbol {sym}"),
                );
            }
        };
        let breach_msgs = if o.state == OrderState::Rejected {
            vec![reject_reason.clone().unwrap_or_else(|| "Rejected by risk check".to_string())]
        } else {
            vec![]
        };
        if let Err(e) = spectra_db::risk_center::record_decision(
            &pool,
            ws,
            Some(req.strategy_id),
            &sym,
            side_str,
            qty,
            Some(eval_price),
            if o.state == OrderState::Rejected { 0.0 } else { qty },
            verdict_str,
            &json!([]),
            &breach_msgs,
            &json!({}),
            user_id,
        )
        .await
        {
            return crate::risk_center_api::decision_not_recorded(&e);
        }

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

        order_id_to_db.insert(o.id, inserted);
    }

    // Persist fills
    let cost_config = preset_cost_config_json(preset, &cfg.costs);
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
                    &cost_config,
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

    let pos_syms: Vec<&str> = updated_account.positions.iter().map(|p| p.symbol.as_str()).collect();
    let mut vols = HashMap::new();
    for sym in &pos_syms {
        if let Some(&inst) = sym_map.get(*sym) {
            let sbars: Vec<Bar> = engine_bars.iter().filter(|b| b.instrument == inst).copied().collect();
            // Too few step bars: the stored dataset is used below. Any other error is reported.
            if sbars.len() >= spectra_risk::MIN_VOLATILITY_BARS {
                match spectra_risk::volatility_from_bars(sym, &sbars, 252.0) {
                    Ok(vol) => {
                        vols.insert(sym.to_string(), vol);
                    }
                    Err(e) => {
                        return error_response(422, format!("Failed to compute volatility for {sym}: {e}"))
                    }
                }
            }
        }
    }
    let default_dir = std::path::PathBuf::from("data");
    let data_dir = st.cfg.data_dir.as_deref().unwrap_or(&default_dir);
    let missing_syms: Vec<&str> = pos_syms.into_iter().filter(|s| !vols.contains_key(*s)).collect();
    if !missing_syms.is_empty() {
        match crate::risk_center_api::lookup_portfolio_volatilities(&pool, ws, data_dir, None, &missing_syms)
            .await
        {
            Ok(db_vols) => vols.extend(db_vols),
            Err(e) => return error_response(422, format!("Failed to compute paper account metrics: {e}")),
        }
    }

    let account = match PaperAccountView::new(updated_account, &step_records, &vols) {
        Ok(v) => v,
        Err(e) => return error_response(422, format!("Failed to compute paper account metrics: {e}")),
    };

    let resp = StepPaperResponse {
        strategy_id: req.strategy_id,
        strategy_version_id: Some(version.id),
        orders: order_views,
        fills: fill_views,
        account,
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
