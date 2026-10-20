//! What a finished run stores: CSV artifacts, a summary document and
//! chart-sized series.
//!
//!  * **Artifacts** ([`artifacts`]): `equity.csv`, `orders.csv`, `fills.csv`
//!    and `returns.csv`, every row of the run. Money, prices and quantities are
//!    written exactly (eight decimals, the ledger's own text form). The
//!    artifact store compresses them; this crate stays standard-library only.
//!  * **Summary** ([`summary_json`]): headline metrics, drawdown and tail
//!    statistics, activity, trade statistics and the run's fingerprints, small
//!    enough for the `runs` row. Exact amounts are decimal strings; ratios are
//!    numbers, and a ratio that is not finite (a profit factor with no losing
//!    trades) is `null`.
//!  * **Series** ([`series_json`]): equity or drawdown, downsampled so a chart
//!    never loads every bar. Each bucket keeps its lowest and highest point, so
//!    the deepest drawdown and the equity peak always survive downsampling.

use std::fmt::Write as _;

use spectra_core::{EntryType, Error, Json, Money, Result, Side};
use spectra_engine::{
    compute_drawdown_stats, compute_tail_stats, returns_from_curve, BacktestResult, EquityPoint, OrderState,
    OrderType,
};
use spectra_research::{compute_trade_stats, extract_trades};

use crate::config::config_to_user_json;
use crate::run::BacktestOutput;

/// Version tag written into every summary, bumped when its shape changes.
pub const SUMMARY_SCHEMA: &str = "spectra.backtest.summary/v1";

/// Default number of points for chart series.
pub const DEFAULT_SERIES_POINTS: usize = 1000;

/// One file to store with a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// File name, e.g. `equity.csv`.
    pub name: &'static str,
    /// Media type.
    pub media_type: &'static str,
    /// Contents.
    pub bytes: Vec<u8>,
}

const fn side_str(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// `ts_ns,equity,cash,drawdown`, one row per timestamp.
pub fn equity_csv(r: &BacktestResult) -> String {
    let dd = drawdowns(&r.equity_curve);
    let mut s = String::from("ts_ns,equity,cash,drawdown\n");
    for (p, d) in r.equity_curve.iter().zip(dd) {
        let _ = writeln!(s, "{},{},{},{}", p.ts_ns, p.equity, p.cash, d);
    }
    s
}

/// Every order with its final state.
pub fn orders_csv(r: &BacktestResult) -> String {
    let mut s = String::from(
        "id,instrument,side,type,qty,remaining,limit,submit_ts_ns,submit_step,state,reject_reason\n",
    );
    for o in &r.orders {
        let limit = if o.kind == OrderType::Limit { o.limit.to_string() } else { String::new() };
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{},{},{}",
            o.id,
            o.instrument,
            side_str(o.side),
            if o.kind == OrderType::Limit { "limit" } else { "market" },
            o.qty,
            o.remaining,
            limit,
            o.submit_ts,
            o.submit_step,
            o.state.as_str(),
            o.reject_reason.map_or("", spectra_core::Reject::as_str),
        );
    }
    s
}

/// Every fill from the ledger journal, with its effect on cash.
pub fn fills_csv(r: &BacktestResult) -> String {
    let mut s = String::from("seq,ts_ns,instrument,side,qty,price,fee,realized,cash_after\n");
    for e in r.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill) {
        let f = &e.fill;
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{}",
            e.seq,
            f.ts_ns,
            f.instrument,
            side_str(f.side),
            f.qty,
            f.price,
            f.fee,
            e.realized,
            e.cash_after
        );
    }
    s
}

/// `ts_ns,return`: the simple return ending at each timestamp. Periods whose
/// prior equity is not positive are skipped, exactly as the metrics skip them.
pub fn returns_csv(r: &BacktestResult) -> String {
    let mut s = String::from("ts_ns,return\n");
    for w in r.equity_curve.windows(2) {
        let (prev, cur) = (w[0].equity.to_f64(), w[1].equity.to_f64());
        if prev > 0.0 {
            let _ = writeln!(s, "{},{}", w[1].ts_ns, spectra_core::cfmt::fmt_g17(cur / prev - 1.0));
        }
    }
    s
}

/// The four CSV artifacts of a run.
pub fn artifacts(r: &BacktestResult) -> Vec<Artifact> {
    let csv = |name, text: String| Artifact { name, media_type: "text/csv", bytes: text.into_bytes() };
    vec![
        csv("equity.csv", equity_csv(r)),
        csv("orders.csv", orders_csv(r)),
        csv("fills.csv", fills_csv(r)),
        csv("returns.csv", returns_csv(r)),
    ]
}

/// A finite number, or `null`.
fn num(v: f64) -> Json {
    Json::number(v).unwrap_or(Json::Null)
}

fn money(m: Money) -> Json {
    Json::string(m.to_string())
}

/// The run's summary document (see the module documentation).
///
/// # Errors
/// Overflow while valuing the final books or extracting trades (impossible for
/// a ledger that passed its invariants).
pub fn summary_json(out: &BacktestOutput) -> Result<Json> {
    let r = &out.result;
    let req = &out.request;
    let eq: Vec<f64> = r.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
    let m = &r.metrics;
    let dd = compute_drawdown_stats(&eq);
    let tail = compute_tail_stats(&returns_from_curve(&r.equity_curve), 0.95);
    let extraction = extract_trades(&r.ledger)?;
    let t = compute_trade_stats(&extraction.trades);

    let mut by_state = Json::object();
    for state in [
        OrderState::Pending,
        OrderState::PartiallyFilled,
        OrderState::Filled,
        OrderState::Cancelled,
        OrderState::Rejected,
    ] {
        let n = r.orders.iter().filter(|o| o.state == state).count();
        by_state.set(state.as_str(), Json::integer(n as i64))?;
    }

    let run = Json::object()
        .with("engine_version", Json::string(&r.run.engine_version))
        .with("dataset_sha256", Json::string(&r.run.dataset_sha256))
        .with("config_sha256", Json::string(&r.run.config_sha256))
        .with("result_sha256", Json::string(&r.run.result_sha256))
        .with("seed", Json::string(r.run.seed.to_string()))
        .with("bar_count", Json::integer(r.run.bar_count as i64))
        .with("strategy", Json::string(req.strategy.name))
        .with("strategy_sha256", Json::string(req.strategy.source_sha256()))
        .with("params", spectra_registry::params_to_json(&req.params)?)
        .with("config", config_to_user_json(&req.config)?);

    let metrics = Json::object()
        .with("valid", Json::boolean(m.valid))
        .with("periods", Json::integer(i64::from(m.periods)))
        .with("total_return", num(m.total_return))
        .with("annualized_return", num(m.annualized_return))
        .with("annualized_volatility", num(m.annualized_volatility))
        .with("sharpe", num(m.sharpe))
        .with("max_drawdown", num(m.max_drawdown));

    let drawdown = Json::object()
        .with("valid", Json::boolean(dd.valid))
        .with("max_drawdown", num(dd.max_drawdown))
        .with("current_drawdown", num(dd.current_drawdown))
        .with("longest_underwater_periods", Json::integer(dd.longest_underwater as i64))
        .with("ulcer_index", num(dd.ulcer_index));

    let tail = Json::object()
        .with("valid", Json::boolean(tail.valid))
        .with("level", num(tail.level))
        .with("var", num(tail.var))
        .with("expected_shortfall", num(tail.es))
        .with("worst_return", num(tail.worst));

    let activity = Json::object()
        .with("order_count", Json::integer(r.orders.len() as i64))
        .with("orders_by_state", by_state)
        .with("fill_count", Json::integer(r.fill_count() as i64))
        .with("turnover_annualized", num(r.turnover(req.config.periods_per_year)))
        .with("total_fees", money(r.ledger.total_fees()))
        .with("realized_pnl", money(r.ledger.realized_pnl()))
        .with("final_cash", money(r.ledger.cash()))
        .with("final_equity", money(r.ledger.equity()?))
        .with("open_positions", Json::integer(extraction.open_positions as i64));

    let trades = Json::object()
        .with("count", Json::integer(t.n as i64))
        .with("wins", Json::integer(t.wins as i64))
        .with("losses", Json::integer(t.losses as i64))
        .with("win_rate", num(t.win_rate))
        .with("avg_win", num(t.avg_win))
        .with("avg_loss", num(t.avg_loss))
        .with("payoff_ratio", num(t.payoff_ratio))
        .with("expectancy", num(t.expectancy))
        .with("profit_factor", num(t.profit_factor))
        .with("total_net", num(t.total_net))
        .with("best", num(t.best))
        .with("worst", num(t.worst))
        .with("top_trade_share", num(t.top_trade_share))
        .with("avg_holding_ns", num(t.avg_holding_ns));

    Ok(Json::object()
        .with("schema", Json::string(SUMMARY_SCHEMA))
        .with("run", run)
        .with("metrics", metrics)
        .with("drawdown", drawdown)
        .with("tail", tail)
        .with("activity", activity)
        .with("trades", trades))
}

/// Which series to chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesKind {
    /// Equity at each timestamp.
    Equity,
    /// Fall from the running peak, as a positive fraction.
    Drawdown,
}

impl SeriesKind {
    /// Name used in the API (`?kind=equity`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equity => "equity",
            Self::Drawdown => "drawdown",
        }
    }

    /// The kind called `name`.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for other names.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "equity" => Ok(Self::Equity),
            "drawdown" => Ok(Self::Drawdown),
            _ => {
                Err(Error::InvalidArgument(format!("unknown series \"{name}\"; expected equity or drawdown")))
            }
        }
    }
}

/// One point of a chart series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeriesPoint {
    /// Timestamp.
    pub ts_ns: i64,
    /// Value.
    pub value: f64,
}

/// Drawdown at each point, computed exactly as
/// [`spectra_engine::compute_drawdown_stats`] does, so the series' maximum is
/// bit-identical to the reported maximum drawdown.
fn drawdowns(curve: &[EquityPoint]) -> Vec<f64> {
    let mut peak = curve.first().map_or(0.0, |p| p.equity.to_f64());
    curve
        .iter()
        .map(|p| {
            let e = p.equity.to_f64();
            if e > peak {
                peak = e;
            }
            if peak > 0.0 {
                (peak - e) / peak
            } else {
                0.0
            }
        })
        .collect()
}

/// The full series of `kind`, one point per timestamp.
pub fn series(curve: &[EquityPoint], kind: SeriesKind) -> Vec<SeriesPoint> {
    match kind {
        SeriesKind::Equity => {
            curve.iter().map(|p| SeriesPoint { ts_ns: p.ts_ns, value: p.equity.to_f64() }).collect()
        }
        SeriesKind::Drawdown => curve
            .iter()
            .zip(drawdowns(curve))
            .map(|(p, value)| SeriesPoint { ts_ns: p.ts_ns, value })
            .collect(),
    }
}

/// At most `max_points` points (minimum 4) that keep the series' shape: the
/// first and last points, and the lowest and highest point of each of
/// `(max_points - 2) / 2` equal buckets in between, in time order. The global
/// minimum and maximum are therefore always kept.
pub fn downsample(points: &[SeriesPoint], max_points: usize) -> Vec<SeriesPoint> {
    let max_points = max_points.max(4);
    if points.len() <= max_points {
        return points.to_vec();
    }
    let inner = &points[1..points.len() - 1];
    let buckets = (max_points - 2) / 2;
    let mut out = Vec::with_capacity(max_points);
    out.push(points[0]);
    for b in 0..buckets {
        let lo = b * inner.len() / buckets;
        let hi = (b + 1) * inner.len() / buckets;
        let chunk = &inner[lo..hi];
        if chunk.is_empty() {
            continue;
        }
        let (mut imin, mut imax) = (0, 0);
        for (i, p) in chunk.iter().enumerate() {
            if p.value < chunk[imin].value {
                imin = i;
            }
            if p.value > chunk[imax].value {
                imax = i;
            }
        }
        let (first, second) = if imin <= imax { (imin, imax) } else { (imax, imin) };
        out.push(chunk[first]);
        if second != first {
            out.push(chunk[second]);
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// `{kind, total_points, downsampled, points: [[ts_ns, value], ...]}` for a chart.
///
/// # Errors
/// Never in practice (values are finite).
pub fn series_json(curve: &[EquityPoint], kind: SeriesKind, max_points: usize) -> Result<Json> {
    let full = series(curve, kind);
    let pts = downsample(&full, max_points);
    let mut arr = Json::array();
    for p in &pts {
        let mut pair = Json::array();
        pair.push(Json::integer(p.ts_ns))?;
        pair.push(num(p.value))?;
        arr.push(pair)?;
    }
    Ok(Json::object()
        .with("kind", Json::string(kind.as_str()))
        .with("total_points", Json::integer(full.len() as i64))
        .with("downsampled", Json::boolean(pts.len() < full.len()))
        .with("points", arr))
}
