//! Portfolio backtests (M15 Task 6): run a saved portfolio through the rebalancing simulator on a
//! dataset version, store the run, list a portfolio's runs, read one, and store its report.
//!
//! Prices come from the dataset version the caller names, read the way the chart route reads it
//! (the API's local dataset store or the artifact store, verified), so a portfolio run sees the
//! same bars as a backtest of that version. Each holding's close is its last bar of each UTC day
//! in the date range. A holding with no bars in the range is refused (422); a day on which some
//! holding has no bar is skipped, never filled with another day's price, and the count of skipped
//! days is recorded.
//!
//! A run is stored as `running` once its inputs are checked (with a snapshot of the portfolio,
//! so it stays readable after the portfolio is edited or deleted), then ended as `succeeded` with
//! its result or `failed` with the reason. Runs are listed per portfolio, newest first, with a
//! cursor, like backtests. Starting a run and storing its report are writes (editor role).
//!
//! The report is the M15 portfolio report (`portfolio_report::render`), stored through M16 as
//! `report_type = portfolio`, `run_kind = portfolio_backtest`. It is rendered from the stored
//! result, so the same run gives the same bytes, and an export identical to the newest version
//! reuses that version.

use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Extension;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use spectra_backtest::portfolio::Holding;
use spectra_backtest::portfolio_backtest::{run_portfolio_backtest, summarize, PortfolioBacktestSummary};
use spectra_backtest::rebalance::{
    Date, Frequency, HoldingWeight, Reason, RebalanceEvent, RebalanceRule, RebalanceRun, RebalanceTrade,
    TradeCostConfig,
};
use spectra_db::portfolio_runs::{self, PortfolioRunRow};
use spectra_db::portfolios;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::jobs_api::{context, db_error, to_json};
use crate::portfolios_api::ensure_can_edit;
use crate::{error_response, AppState, Identity};

const RUN_NOT_FOUND: &str = "Portfolio run not found.";
const DAY_NS: i64 = 86_400_000_000_000;
/// Largest starting cash a run accepts, to keep every amount well inside `f64` precision.
const MAX_INITIAL_CASH: f64 = 1e12;
/// The name recorded for a cost config given as values rather than by name.
const CUSTOM_COST_CONFIG: &str = "custom";

/// When a run rebalances. At least one of the two must be set.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRuleBody {
    /// `monthly` or `quarterly`: rebalance on the first trading day of each new month or quarter.
    #[serde(default)]
    pub calendar: Option<String>,
    /// Rebalance when a holding's weight is more than this far from its target (a fraction in
    /// (0, 1], e.g. 0.05 for 5 percentage points).
    #[serde(default)]
    pub drift_threshold: Option<f64>,
}

/// The cost config every rebalance trade is priced with. Omit it, or give `{"name":
/// "paper-default"}`, for the M26 paper config; or give `slippage_bps` and `min_fee` together,
/// recorded as the `custom` config.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CostConfigBody {
    #[serde(default)]
    pub name: Option<String>,
    /// Adverse price move on every trade, in basis points.
    #[serde(default)]
    pub slippage_bps: Option<f64>,
    /// Commission on each trade, in currency.
    #[serde(default)]
    pub min_fee: Option<f64>,
}

/// `POST /api/v1/portfolios/{id}/runs` body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RunPortfolioBody {
    /// A dataset version of the caller's workspace with bars for every holding.
    pub dataset_version_id: Uuid,
    /// First day of the run, `YYYY-MM-DD` (UTC).
    pub start: String,
    /// Last day of the run, `YYYY-MM-DD` (UTC), on or after `start`.
    pub end: String,
    pub rule: RunRuleBody,
    /// Starting cash, positive.
    pub initial_cash: f64,
    #[serde(default)]
    pub cost_config: Option<CostConfigBody>,
}

/// Headline numbers of a finished run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PortfolioRunSummary {
    /// Final equity over starting cash, minus one.
    pub total_return: f64,
    /// Largest peak-to-trough fall, a positive fraction.
    pub max_drawdown: f64,
    /// Sum of every rebalance's turnover.
    pub total_turnover: f64,
    /// Costs of every rebalance, in currency.
    pub total_cost: f64,
    /// Rebalances, including the initial one.
    pub rebalance_count: usize,
    pub initial_cash: f64,
    pub final_equity: f64,
}

/// Equity at one day's close.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EquityPointView {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub equity: f64,
}

/// A holding's weight before and after one rebalance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WeightView {
    pub symbol: String,
    pub before: f64,
    pub after: f64,
}

/// One rebalance trade with its recorded cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TradeView {
    pub symbol: String,
    /// Positive buys, negative sells.
    pub qty: f64,
    /// The close the trade is priced from.
    pub price: f64,
    /// The price after slippage.
    pub exec_price: f64,
    pub slippage_cost: f64,
    pub fee: f64,
}

/// One rebalance: why, how much it traded and cost, and the weights around it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RebalanceView {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `initial`, `calendar` and/or `drift`.
    pub reasons: Vec<String>,
    /// Traded notional over equity before the rebalance.
    pub turnover: f64,
    /// Cost of its trades, in currency.
    pub cost: f64,
    pub weights: Vec<WeightView>,
    pub trades: Vec<TradeView>,
}

/// The cost config the run's trades were priced with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CostConfigView {
    pub name: String,
    pub version: u32,
    pub slippage_bps: f64,
    pub min_fee: f64,
}

/// The days the run priced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PriceCoverageView {
    /// First and last priced day, `YYYY-MM-DD`.
    pub first: String,
    pub last: String,
    /// Days on which every holding had a close.
    pub days: usize,
    /// Days in the range on which some holding had a bar but another did not; not priced.
    pub skipped_days: usize,
}

/// The result of a finished run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PortfolioRunResult {
    pub summary: PortfolioRunSummary,
    /// Equity at every priced day's close.
    pub equity: Vec<EquityPointView>,
    /// Every rebalance in order: weights over time, turnover and costs.
    pub rebalances: Vec<RebalanceView>,
    pub cost_config: CostConfigView,
    pub prices: PriceCoverageView,
}

/// A portfolio run as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PortfolioRunView {
    pub id: Uuid,
    /// The portfolio run; null once it has been deleted.
    pub portfolio_id: Option<Uuid>,
    /// `running`, `succeeded` or `failed`.
    pub status: String,
    /// Every input: the portfolio snapshot, dataset version, dates, rule, cash and cost config.
    #[schema(value_type = Object)]
    pub request: serde_json::Value,
    /// Once succeeded.
    pub result: Option<PortfolioRunResult>,
    /// Why it failed.
    pub error: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// One run in a list: its inputs, state and headline numbers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PortfolioRunListItem {
    pub id: Uuid,
    pub portfolio_id: Option<Uuid>,
    /// `running`, `succeeded` or `failed`.
    pub status: String,
    #[schema(value_type = Object)]
    pub request: serde_json::Value,
    /// Once succeeded.
    pub headline: Option<PortfolioRunSummary>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A page of a portfolio's runs, newest first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PortfolioRunPage {
    pub items: Vec<PortfolioRunListItem>,
    /// Pass as `cursor` for the next page; null on the last.
    pub next_cursor: Option<Uuid>,
}

/// `GET /api/v1/portfolios/{id}/runs` query.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListRunsQuery {
    /// `next_cursor` from the previous page.
    pub cursor: Option<Uuid>,
    /// 1..200, default 50.
    pub limit: Option<u32>,
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn parsed_result(row: &PortfolioRunRow) -> Result<Option<PortfolioRunResult>, Response> {
    match &row.result {
        None => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|_| error_response(500, "Corrupted portfolio run result.")),
    }
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn run_view(row: PortfolioRunRow) -> Result<PortfolioRunView, Response> {
    let result = parsed_result(&row)?;
    Ok(PortfolioRunView {
        id: row.id,
        portfolio_id: row.portfolio_id,
        status: row.status,
        request: row.request,
        result,
        error: row.error,
        created_by: row.created_by,
        created_at: row.created_at,
        finished_at: row.finished_at,
    })
}

#[allow(clippy::result_large_err)] // the error is the finished response
fn list_item(row: PortfolioRunRow) -> Result<PortfolioRunListItem, Response> {
    let headline = parsed_result(&row)?.map(|r| r.summary);
    Ok(PortfolioRunListItem {
        id: row.id,
        portfolio_id: row.portfolio_id,
        status: row.status,
        request: row.request,
        headline,
        error: row.error,
        created_at: row.created_at,
        finished_at: row.finished_at,
    })
}

fn parse_date(text: &str) -> Option<Date> {
    let d = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()?;
    Some(Date::new(d.year(), d.month(), d.day()))
}

/// A run's checked inputs.
struct Checked {
    start: Date,
    end: Date,
    rule: RebalanceRule,
    initial_cash: f64,
    cost: TradeCostConfig,
}

/// Checks a run body; every problem is reported at once (`details.errors`).
#[allow(clippy::result_large_err)] // the error is the finished response
fn checked(req: &RunPortfolioBody) -> Result<Checked, Response> {
    let mut errors: Vec<(String, String)> = Vec::new();
    let start = parse_date(&req.start);
    let end = parse_date(&req.end);
    if start.is_none() {
        errors.push(("start".into(), "use YYYY-MM-DD".into()));
    }
    if end.is_none() {
        errors.push(("end".into(), "use YYYY-MM-DD".into()));
    }
    if let (Some(s), Some(e)) = (start, end) {
        if e < s {
            errors.push(("end".into(), "must not be before start".into()));
        }
    }
    let calendar = match req.rule.calendar.as_deref() {
        None => None,
        Some("monthly") => Some(Frequency::Monthly),
        Some("quarterly") => Some(Frequency::Quarterly),
        Some(_) => {
            errors.push(("rule.calendar".into(), "must be monthly or quarterly".into()));
            None
        }
    };
    if let Some(t) = req.rule.drift_threshold {
        if !(t.is_finite() && t > 0.0 && t <= 1.0) {
            errors.push(("rule.drift_threshold".into(), "must be a fraction in (0, 1]".into()));
        }
    }
    if req.rule.calendar.is_none() && req.rule.drift_threshold.is_none() {
        errors.push(("rule".into(), "give a calendar, a drift threshold, or both".into()));
    }
    if !(req.initial_cash.is_finite() && req.initial_cash > 0.0 && req.initial_cash <= MAX_INITIAL_CASH) {
        errors.push(("initial_cash".into(), "must be positive and at most 1e12".into()));
    }
    let cost = match &req.cost_config {
        None => Some(paper_cost_config()),
        Some(c) => match (c.name.as_deref(), c.slippage_bps, c.min_fee) {
            (None | Some("paper-default"), None, None) => Some(paper_cost_config()),
            (None | Some(CUSTOM_COST_CONFIG), Some(slippage_bps), Some(min_fee)) => {
                if slippage_bps.is_finite() && slippage_bps >= 0.0 && min_fee.is_finite() && min_fee >= 0.0 {
                    Some(TradeCostConfig {
                        name: CUSTOM_COST_CONFIG.into(),
                        version: 1,
                        slippage_bps,
                        min_fee,
                    })
                } else {
                    errors
                        .push(("cost_config".into(), "slippage_bps and min_fee must be non-negative".into()));
                    None
                }
            }
            _ => {
                errors.push((
                    "cost_config".into(),
                    "give {\"name\": \"paper-default\"}, or slippage_bps and min_fee together".into(),
                ));
                None
            }
        },
    };
    match (start, end, cost, errors.is_empty()) {
        (Some(start), Some(end), Some(cost), true) => Ok(Checked {
            start,
            end,
            rule: RebalanceRule { calendar, drift_threshold: req.rule.drift_threshold },
            initial_cash: req.initial_cash,
            cost,
        }),
        _ => Err(crate::invalid_fields(&errors)),
    }
}

/// The M26 named paper cost config, as the simulator takes it.
fn paper_cost_config() -> TradeCostConfig {
    let c = crate::paper_api::PAPER_COST_CONFIG;
    TradeCostConfig {
        name: c.name.into(),
        version: c.version,
        slippage_bps: c.slippage_bps,
        min_fee: c.min_fee,
    }
}

/// The days a run prices, one row of closes per day, and the count of skipped days.
pub(crate) type PriceTable = (Vec<Date>, Vec<Vec<f64>>, usize);

/// The closes the run uses: per UTC day in `[start, end]`, each holding's last bar of the day,
/// on the days every holding has one. Returns the days, one row of closes per day (in `symbols`
/// order) and the number of days skipped because some holding had no bar.
///
/// # Errors
/// A message naming the holding, if a holding is not in the dataset or has no bar in the range,
/// or if the holdings share no day.
pub(crate) fn price_table(
    dataset: &spectra_data::Dataset,
    symbols: &[String],
    start: Date,
    end: Date,
) -> Result<PriceTable, String> {
    let (first_day, last_day) = (start.to_days(), end.to_days());
    let mut ids = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        let Some(id) = dataset.symbols.id(symbol) else {
            return Err(format!("No bars for {symbol}: it is not in this dataset version."));
        };
        ids.push(id);
    }
    // Per holding: day -> (timestamp, close) of its last bar that day.
    let mut by_day: Vec<BTreeMap<i64, (i64, f64)>> = vec![BTreeMap::new(); symbols.len()];
    for bar in &dataset.bars {
        let day = bar.ts_ns.div_euclid(DAY_NS);
        if day < first_day || day > last_day {
            continue;
        }
        for (k, id) in ids.iter().enumerate() {
            if *id == bar.instrument {
                let entry = by_day[k].entry(day).or_insert((bar.ts_ns, bar.close.to_f64()));
                if bar.ts_ns >= entry.0 {
                    *entry = (bar.ts_ns, bar.close.to_f64());
                }
            }
        }
    }
    for (symbol, days) in symbols.iter().zip(&by_day) {
        if days.is_empty() {
            return Err(format!("No bars for {symbol} between {start} and {end} in this dataset version."));
        }
    }
    let all: BTreeSet<i64> = by_day.iter().flat_map(|m| m.keys().copied()).collect();
    let common: Vec<i64> = all.iter().copied().filter(|d| by_day.iter().all(|m| m.contains_key(d))).collect();
    if common.is_empty() {
        return Err(format!(
            "The holdings have no day with a bar for every one of them between {start} and {end}."
        ));
    }
    let mut dates = Vec::with_capacity(common.len());
    let mut closes = Vec::with_capacity(common.len());
    for day in &common {
        let Some(date) = Date::from_days(*day) else {
            return Err(format!("Day {day} is outside the supported calendar."));
        };
        dates.push(date);
        closes.push(by_day.iter().map(|m| m[day].1).collect());
    }
    Ok((dates, closes, all.len() - common.len()))
}

fn reason_name(r: Reason) -> &'static str {
    match r {
        Reason::Initial => "initial",
        Reason::Calendar => "calendar",
        Reason::Drift => "drift",
    }
}

/// The stored result of a simulated run.
pub(crate) fn result_of(
    run: &RebalanceRun,
    summary: &PortfolioBacktestSummary,
    initial_cash: f64,
    cost: &TradeCostConfig,
    skipped_days: usize,
) -> PortfolioRunResult {
    let final_equity = run.equity.last().map_or(initial_cash, |(_, e)| *e);
    PortfolioRunResult {
        summary: PortfolioRunSummary {
            total_return: summary.total_return,
            max_drawdown: summary.max_drawdown,
            total_turnover: summary.total_turnover,
            total_cost: summary.total_cost,
            rebalance_count: summary.rebalance_count,
            initial_cash,
            final_equity,
        },
        equity: run.equity.iter().map(|(d, e)| EquityPointView { date: d.to_string(), equity: *e }).collect(),
        rebalances: run
            .events
            .iter()
            .map(|e| RebalanceView {
                date: e.date.to_string(),
                reasons: e.reasons.iter().map(|r| reason_name(*r).to_string()).collect(),
                turnover: e.turnover,
                cost: e.cost,
                weights: e
                    .weights
                    .iter()
                    .map(|w| WeightView { symbol: w.symbol.clone(), before: w.before, after: w.after })
                    .collect(),
                trades: e
                    .trades
                    .iter()
                    .map(|t| TradeView {
                        symbol: t.symbol.clone(),
                        qty: t.qty,
                        price: t.price,
                        exec_price: t.exec_price,
                        slippage_cost: t.slippage_cost,
                        fee: t.fee,
                    })
                    .collect(),
            })
            .collect(),
        cost_config: CostConfigView {
            name: cost.name.clone(),
            version: cost.version,
            slippage_bps: cost.slippage_bps,
            min_fee: cost.min_fee,
        },
        prices: PriceCoverageView {
            first: run.equity.first().map(|(d, _)| d.to_string()).unwrap_or_default(),
            last: run.equity.last().map(|(d, _)| d.to_string()).unwrap_or_default(),
            days: run.equity.len(),
            skipped_days,
        },
    }
}

/// The simulator's run and summary rebuilt from a stored result, for the report.
///
/// # Errors
/// A message if a stored date or reason does not parse (a corrupted result).
pub(crate) fn run_of(
    result: &PortfolioRunResult,
) -> Result<(RebalanceRun, PortfolioBacktestSummary), String> {
    let date = |s: &str| parse_date(s).ok_or_else(|| format!("bad stored date {s:?}"));
    let cost_config = TradeCostConfig {
        name: result.cost_config.name.clone(),
        version: result.cost_config.version,
        slippage_bps: result.cost_config.slippage_bps,
        min_fee: result.cost_config.min_fee,
    };
    let mut events = Vec::with_capacity(result.rebalances.len());
    for r in &result.rebalances {
        let mut reasons = Vec::with_capacity(r.reasons.len());
        for name in &r.reasons {
            reasons.push(match name.as_str() {
                "initial" => Reason::Initial,
                "calendar" => Reason::Calendar,
                "drift" => Reason::Drift,
                other => return Err(format!("bad stored reason {other:?}")),
            });
        }
        events.push(RebalanceEvent {
            date: date(&r.date)?,
            reasons,
            weights: r
                .weights
                .iter()
                .map(|w| HoldingWeight { symbol: w.symbol.clone(), before: w.before, after: w.after })
                .collect(),
            trades: r
                .trades
                .iter()
                .map(|t| RebalanceTrade {
                    symbol: t.symbol.clone(),
                    qty: t.qty,
                    price: t.price,
                    exec_price: t.exec_price,
                    slippage_cost: t.slippage_cost,
                    fee: t.fee,
                })
                .collect(),
            cost: r.cost,
            turnover: r.turnover,
            cost_config: cost_config.clone(),
        });
    }
    let mut equity = Vec::with_capacity(result.equity.len());
    for p in &result.equity {
        equity.push((date(&p.date)?, p.equity));
    }
    let run = RebalanceRun {
        events,
        equity,
        total_cost: result.summary.total_cost,
        total_turnover: result.summary.total_turnover,
    };
    let summary = summarize(&run, result.summary.initial_cash);
    Ok((run, summary))
}

/// Run a saved portfolio through the rebalancing simulator on a dataset version.
#[utoipa::path(post, path = "/api/v1/portfolios/{id}/runs", tag = "portfolios", request_body = RunPortfolioBody,
    params(("id" = Uuid, Path, description = "The portfolio id")),
    responses(
        (status = 201, description = "The run, finished: `succeeded` with its result", body = PortfolioRunView),
        (status = 403, description = "Viewers cannot run portfolios (code `viewer_read_only`)", body = crate::openapi::ErrorDoc),
        (status = 404, description = "No such portfolio or dataset version in this workspace", body = crate::openapi::ErrorDoc),
        (status = 409, description = "The dataset version is sealed as a holdout, or its bars are not reachable", body = crate::openapi::ErrorDoc),
        (status = 422, description = "Invalid dates, rule, cash or cost config (`details.errors`), or a holding with no bars; a refused run is stored as `failed`", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn run_portfolio(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let Ok(Path(portfolio_id)) = path else { return error_response(404, crate::portfolios_api::NOT_FOUND) };
    let Ok(bytes) = body else { return error_response(400, "Could not read the request body.") };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let req: RunPortfolioBody = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => return error_response(422, format!("Invalid portfolio run: {e}")),
    };
    let checked = match checked(&req) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let portfolio = match portfolios::get_portfolio(&pool, ws, portfolio_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return error_response(404, crate::portfolios_api::NOT_FOUND),
        Err(e) => return db_error(&e),
    };
    // The bars come from the checksum the database hands out, which it refuses (and logs) for a
    // sealed holdout.
    let access =
        spectra_db::datasets::Access { purpose: "portfolio_backtest", user: Uuid::parse_str(&id.user).ok() };
    let version = match spectra_db::datasets::resolve_version(&pool, ws, req.dataset_version_id, access).await
    {
        Ok(spectra_db::datasets::Resolved::Found(v)) => v,
        Ok(spectra_db::datasets::Resolved::Sealed { holdout }) => {
            return crate::optimization_api::sealed_response(&holdout, "running a portfolio backtest on it")
        }
        Ok(spectra_db::datasets::Resolved::Missing) => {
            return error_response(404, "No such dataset version.")
        }
        Err(e) => return db_error(&e),
    };
    let dataset =
        match crate::datasets_api::load_for_chart(&st, &version.checksum, version.blob_sha256.as_deref())
            .await
        {
            Ok(d) => d,
            Err(r) => return r,
        };

    let symbols: Vec<String> = portfolio.holdings.iter().map(|h| h.symbol.clone()).collect();
    let targets: Vec<Holding> =
        portfolio.holdings.iter().map(|h| Holding { symbol: h.symbol.clone(), weight: h.weight }).collect();
    let request = serde_json::json!({
        "portfolio": {
            "id": portfolio.portfolio.id,
            "name": portfolio.portfolio.name,
            "method": portfolio.portfolio.method,
            "holdings": portfolio.holdings.iter().map(|h| serde_json::json!({"symbol": h.symbol, "weight": h.weight})).collect::<Vec<_>>(),
        },
        "dataset_version_id": version.id,
        "dataset_checksum": version.checksum,
        "start": checked.start.to_string(),
        "end": checked.end.to_string(),
        "rule": {
            "calendar": req.rule.calendar,
            "drift_threshold": req.rule.drift_threshold,
        },
        "initial_cash": checked.initial_cash,
        "cost_config": {
            "name": checked.cost.name,
            "version": checked.cost.version,
            "slippage_bps": checked.cost.slippage_bps,
            "min_fee": checked.cost.min_fee,
        },
    });
    let row =
        match portfolio_runs::start_run(&pool, ws, portfolio_id, &request, Uuid::parse_str(&id.user).ok())
            .await
        {
            Ok(r) => r,
            Err(e) => return db_error(&e),
        };

    let Checked { start, end, rule, initial_cash, cost } = checked;
    let outcome = tokio::task::spawn_blocking(move || {
        let (dates, closes, skipped) = price_table(&dataset, &symbols, start, end)?;
        let (run, summary) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, initial_cash, rule, &cost)
                .map_err(|e| e.to_string())?;
        Ok::<_, String>(result_of(&run, &summary, initial_cash, &cost, skipped))
    })
    .await;

    match outcome {
        Ok(Ok(result)) => {
            let Ok(value) = serde_json::to_value(&result) else {
                return error_response(500, "Could not encode the run result.");
            };
            match portfolio_runs::succeed_run(&pool, ws, row.id, &value).await {
                Ok(Some(done)) => match run_view(done) {
                    Ok(v) => to_json(201, &v),
                    Err(r) => r,
                },
                Ok(None) => error_response(409, "The run was ended elsewhere."),
                Err(e) => db_error(&e),
            }
        }
        Ok(Err(message)) => {
            if let Err(e) = portfolio_runs::fail_run(&pool, ws, row.id, &message).await {
                return db_error(&e);
            }
            error_response(422, message)
        }
        Err(_) => {
            let message = "The portfolio run crashed.";
            let _ = portfolio_runs::fail_run(&pool, ws, row.id, message).await;
            error_response(500, message)
        }
    }
}

/// A portfolio's runs, newest first, with headline numbers once finished.
#[utoipa::path(get, path = "/api/v1/portfolios/{id}/runs", tag = "portfolios",
    params(("id" = Uuid, Path, description = "The portfolio id"), ListRunsQuery),
    responses(
        (status = 200, body = PortfolioRunPage),
        (status = 404, description = "No such portfolio in this workspace", body = crate::openapi::ErrorDoc),
        (status = 422, body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn list_portfolio_runs(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<ListRunsQuery>, QueryRejection>,
) -> Response {
    let Ok(Path(portfolio_id)) = path else { return error_response(404, crate::portfolios_api::NOT_FOUND) };
    let Ok(Query(q)) = query else { return error_response(422, "Invalid query: use cursor and limit.") };
    let (pool, ws) = match context(&st, &id) {
        Ok(c) => c,
        Err(r) => return r,
    };
    match portfolios::get_portfolio(&pool, ws, portfolio_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(404, crate::portfolios_api::NOT_FOUND),
        Err(e) => return db_error(&e),
    }
    match portfolio_runs::list_runs(&pool, ws, portfolio_id, q.cursor, q.limit.unwrap_or(50)).await {
        Ok(page) => {
            let mut items = Vec::with_capacity(page.items.len());
            for row in page.items {
                match list_item(row) {
                    Ok(item) => items.push(item),
                    Err(r) => return r,
                }
            }
            to_json(200, &PortfolioRunPage { items, next_cursor: page.next_cursor })
        }
        Err(e) => db_error(&e),
    }
}

#[allow(clippy::result_large_err)] // the error is the finished response
async fn run_row(
    st: &AppState,
    id: &Identity,
    run_id: Uuid,
) -> Result<(spectra_db::PgPool, Uuid, PortfolioRunRow), Response> {
    let (pool, ws) = context(st, id)?;
    match portfolio_runs::get_run(&pool, ws, run_id).await {
        Ok(Some(row)) => Ok((pool, ws, row)),
        Ok(None) => Err(error_response(404, RUN_NOT_FOUND)),
        Err(e) => Err(db_error(&e)),
    }
}

/// One portfolio run: its inputs, status, and result once succeeded.
#[utoipa::path(get, path = "/api/v1/portfolio-runs/{id}", tag = "portfolios",
    params(("id" = Uuid, Path, description = "The portfolio run id")),
    responses((status = 200, body = PortfolioRunView), (status = 404, body = crate::openapi::ErrorDoc)))]
pub(crate) async fn get_portfolio_run(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(run_id)) = path else { return error_response(404, RUN_NOT_FOUND) };
    match run_row(&st, &id, run_id).await {
        Ok((_, _, row)) => match run_view(row) {
            Ok(v) => to_json(200, &v),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

/// The run's portfolio report PDF, stored as a report version (M16). An export identical to the
/// newest version reuses it.
#[utoipa::path(get, path = "/api/v1/portfolio-runs/{id}/report", tag = "portfolios",
    params(("id" = Uuid, Path, description = "The portfolio run id")),
    responses(
        (status = 200, description = "The portfolio report PDF, stored as a report version; `x-report-id`, `x-report-version` and `x-report-sha256` describe it", content_type = "application/pdf"),
        (status = 403, description = "Viewers cannot store reports (code `viewer_read_only`)", body = crate::openapi::ErrorDoc),
        (status = 404, body = crate::openapi::ErrorDoc),
        (status = 409, description = "The run has not succeeded", body = crate::openapi::ErrorDoc),
        (status = 413, description = "The report is over 5 MiB and was not stored", body = crate::openapi::ErrorDoc),
    ))]
pub(crate) async fn portfolio_run_report(
    State(st): State<AppState>,
    Extension(id): Extension<Identity>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    let Ok(Path(run_id)) = path else { return error_response(404, RUN_NOT_FOUND) };
    let (pool, ws, row) = match run_row(&st, &id, run_id).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = ensure_can_edit(&pool, ws, &id).await {
        return r;
    }
    let result = match parsed_result(&row) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return error_response(
                409,
                format!("The run is {}; its report exists once it has succeeded.", row.status),
            )
        }
        Err(r) => return r,
    };
    let Ok((run, summary)) = run_of(&result) else {
        return error_response(500, "Corrupted portfolio run result.");
    };
    let title =
        row.request.pointer("/portfolio/name").and_then(serde_json::Value::as_str).unwrap_or("Portfolio");
    let body = crate::portfolio_report::render(&run, &summary, title);
    let report = spectra_db::reports::NewReport {
        report_type: spectra_db::reports::report_type::PORTFOLIO,
        run_kind: spectra_db::reports::run_kind::PORTFOLIO_BACKTEST,
        run_id,
        format: "pdf",
        content_type: "application/pdf",
        content: &body,
        created_by: Uuid::parse_str(&id.user).ok(),
    };
    match spectra_db::reports::create_version(&pool, ws, &report).await {
        Ok(meta) => {
            crate::reports_api::report_response(&meta, &format!("portfolio-run-{run_id}-report.pdf"), body)
        }
        Err(e) => crate::reports_api::store_error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::fixed::Price;
    use spectra_core::{Bar, Qty};
    use spectra_data::dataset::SymbolTable;
    use spectra_data::Dataset;

    fn day_ns(d: Date) -> i64 {
        d.to_days() * DAY_NS
    }

    /// Bars as (symbol index, date, hour, close).
    fn dataset(symbols: &[&str], bars: &[(u32, Date, i64, f64)]) -> Dataset {
        let table = SymbolTable::from_symbols(symbols.iter().copied());
        let bars = bars
            .iter()
            .map(|&(k, d, hour, close)| {
                let id = table.id(symbols[k as usize]).unwrap();
                let ts = day_ns(d) + hour * 3_600_000_000_000;
                let p = Price::from_f64(close).unwrap();
                Bar {
                    ts_ns: ts,
                    known_at_ns: ts,
                    instrument: id,
                    open: p,
                    high: p,
                    low: p,
                    close: p,
                    volume: Qty::from_units(1).unwrap(),
                }
            })
            .collect();
        Dataset { symbols: table, bars, ..Dataset::default() }
    }

    fn syms(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn closes_are_each_days_last_bar_on_common_days_only() {
        let (d1, d2, d3) = (Date::new(2026, 1, 2), Date::new(2026, 1, 5), Date::new(2026, 1, 6));
        let ds = dataset(
            &["AAA", "BBB"],
            &[
                (0, d1, 10, 100.0),
                (0, d1, 15, 101.0), // the day's last bar wins
                (1, d1, 15, 50.0),
                (0, d2, 15, 102.0), // BBB has no bar on d2: the day is skipped, not filled
                (0, d3, 15, 103.0),
                (1, d3, 15, 52.0),
            ],
        );
        let (dates, closes, skipped) = price_table(&ds, &syms(&["AAA", "BBB"]), d1, d3).unwrap();
        assert_eq!(dates, vec![d1, d3]);
        assert_eq!(closes, vec![vec![101.0, 50.0], vec![103.0, 52.0]]);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn a_holding_without_bars_is_refused_by_name() {
        let d1 = Date::new(2026, 1, 2);
        let ds = dataset(&["AAA", "BBB"], &[(0, d1, 15, 100.0)]);
        let err = price_table(&ds, &syms(&["AAA", "BBB"]), d1, d1).unwrap_err();
        assert!(err.contains("No bars for BBB"), "{err}");
        let err = price_table(&ds, &syms(&["AAA", "ZZZ"]), d1, d1).unwrap_err();
        assert!(err.contains("ZZZ") && err.contains("not in this dataset"), "{err}");
        // Bars only outside the range count as none.
        let err =
            price_table(&ds, &syms(&["AAA"]), Date::new(2026, 2, 1), Date::new(2026, 2, 28)).unwrap_err();
        assert!(err.contains("No bars for AAA between"), "{err}");
    }

    #[test]
    fn a_stored_result_rebuilds_the_same_run_and_report() {
        let symbols = syms(&["A", "B"]);
        let targets =
            vec![Holding { symbol: "A".into(), weight: 0.5 }, Holding { symbol: "B".into(), weight: 0.5 }];
        let dates = [Date::new(2026, 1, 2), Date::new(2026, 2, 2), Date::new(2026, 2, 3)];
        let closes = vec![vec![100.0, 100.0], vec![140.0, 100.0], vec![150.0, 95.0]];
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: Some(0.1) };
        let cost = paper_cost_config();
        let (run, summary) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, 10_000.0, rule, &cost).unwrap();
        let stored = serde_json::to_value(result_of(&run, &summary, 10_000.0, &cost, 0)).unwrap();
        // Through JSON text, as the database stores it. serde_json's default float parser may move
        // a value by one unit in the last place, so the rebuilt run is compared with a tolerance;
        // what must hold exactly is that the stored result always gives the same report.
        let reload = || -> PortfolioRunResult { serde_json::from_str(&stored.to_string()).unwrap() };
        let (run2, summary2) = run_of(&reload()).unwrap();
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(1.0);
        assert_eq!(run2.events.len(), run.events.len());
        for (a, b) in run2.events.iter().zip(&run.events) {
            assert_eq!((a.date, &a.reasons, a.trades.len()), (b.date, &b.reasons, b.trades.len()));
            assert!(close(a.cost, b.cost) && close(a.turnover, b.turnover));
            for (x, y) in a.trades.iter().zip(&b.trades) {
                assert!(x.symbol == y.symbol && close(x.qty, y.qty) && close(x.fee, y.fee));
            }
            for (x, y) in a.weights.iter().zip(&b.weights) {
                assert!(x.symbol == y.symbol && close(x.before, y.before) && close(x.after, y.after));
            }
        }
        for ((d1, e1), (d2, e2)) in run2.equity.iter().zip(&run.equity) {
            assert!(d1 == d2 && close(*e1, *e2));
        }
        assert!(close(summary2.total_return, summary.total_return));
        // The same stored result gives byte-identical reports, every time.
        let (run3, summary3) = run_of(&reload()).unwrap();
        let first = crate::portfolio_report::render(&run2, &summary2, "P");
        assert_eq!(first, crate::portfolio_report::render(&run3, &summary3, "P"));
        // And the report shows the same numbers as one rendered from the simulation itself.
        assert_eq!(first, crate::portfolio_report::render(&run, &summary, "P"));
    }

    fn body(
        start: &str,
        end: &str,
        calendar: Option<&str>,
        drift: Option<f64>,
        cash: f64,
    ) -> RunPortfolioBody {
        RunPortfolioBody {
            dataset_version_id: Uuid::nil(),
            start: start.into(),
            end: end.into(),
            rule: RunRuleBody { calendar: calendar.map(Into::into), drift_threshold: drift },
            initial_cash: cash,
            cost_config: None,
        }
    }

    #[test]
    fn checks_dates_rule_cash_and_cost_config() {
        assert!(checked(&body("2026-01-01", "2026-06-30", Some("monthly"), None, 1000.0)).is_ok());
        assert!(checked(&body("2026-01-01", "2026-06-30", None, Some(0.05), 1000.0)).is_ok());
        for bad in [
            body("2026-13-01", "2026-06-30", Some("monthly"), None, 1000.0),
            body("2026-06-30", "2026-01-01", Some("monthly"), None, 1000.0),
            body("2026-01-01", "2026-06-30", Some("weekly"), None, 1000.0),
            body("2026-01-01", "2026-06-30", None, None, 1000.0),
            body("2026-01-01", "2026-06-30", None, Some(1.5), 1000.0),
            body("2026-01-01", "2026-06-30", Some("monthly"), None, 0.0),
        ] {
            assert!(checked(&bad).is_err());
        }
        let mut custom = body("2026-01-01", "2026-06-30", Some("monthly"), None, 1000.0);
        custom.cost_config = Some(CostConfigBody { name: None, slippage_bps: Some(2.0), min_fee: Some(0.0) });
        let c = checked(&custom).ok().unwrap();
        assert_eq!((c.cost.name.as_str(), c.cost.slippage_bps), ("custom", 2.0));
        custom.cost_config = Some(CostConfigBody {
            name: Some("paper-default".into()),
            slippage_bps: Some(2.0),
            min_fee: None,
        });
        assert!(checked(&custom).is_err(), "a name with partial values is refused");
        let paper = checked(&body("2026-01-01", "2026-06-30", Some("monthly"), None, 1000.0)).ok().unwrap();
        assert_eq!(paper.cost.name, "paper-default");
    }
}
