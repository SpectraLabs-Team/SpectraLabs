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
    TimeInForce,
};
use spectra_research::{compute_pnl_attribution, compute_trade_stats, extract_trades};

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

/// Every order with its final state; a multi-strategy run adds the index of
/// the strategy that placed it as a last `strategy` column.
pub fn orders_csv(r: &BacktestResult) -> String {
    let multi = !r.strategies.is_empty();
    let mut s = String::from(
        "id,instrument,side,type,qty,remaining,limit,stop,time_in_force,submit_ts_ns,submit_step,state,reject_reason",
    );
    s.push_str(if multi { ",strategy\n" } else { "\n" });
    for o in &r.orders {
        let limit = if o.limit.is_zero() { String::new() } else { o.limit.to_string() };
        let stop = if o.stop.is_zero() { String::new() } else { o.stop.to_string() };
        let tif = match o.tif {
            TimeInForce::GoodTillDate(t) => format!("gtd:{t}"),
            other => other.as_str().to_string(),
        };
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            o.id,
            o.instrument,
            side_str(o.side),
            o.kind.as_str(),
            o.qty,
            o.remaining,
            limit,
            stop,
            tif,
            o.submit_ts,
            o.submit_step,
            o.state.as_str(),
            o.reject_reason.map_or("", spectra_core::Reject::as_str),
        );
        if multi {
            s.pop();
            let _ = writeln!(s, ",{}", o.strategy);
        }
    }
    s
}

/// Every fill from the ledger journal, with its effect on cash; a
/// multi-strategy run adds `order_id,strategy` columns.
pub fn fills_csv(r: &BacktestResult) -> String {
    let multi = !r.strategies.is_empty();
    let mut s = String::from("seq,ts_ns,instrument,side,qty,price,fee,realized,cash_after");
    s.push_str(if multi { ",order_id,strategy\n" } else { "\n" });
    for (n, e) in r.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill).enumerate() {
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
        if multi {
            let id = r.fill_orders.get(n).copied().unwrap_or(0);
            let strategy = id.checked_sub(1).and_then(|k| r.orders.get(k as usize)).map(|o| o.strategy);
            s.pop();
            let _ = writeln!(s, ",{id},{}", strategy.map_or(String::new(), |x| x.to_string()));
        }
    }
    s
}

/// `ts_ns,strategy,equity,cash,gross_exposure`: each strategy's own books at
/// every timestamp (multi-strategy runs only; empty header otherwise).
pub fn strategies_csv(r: &BacktestResult) -> String {
    let mut s = String::from("ts_ns,strategy,equity,cash,gross_exposure\n");
    for (i, st) in r.strategies.iter().enumerate() {
        for (p, g) in st.equity_curve.iter().zip(&st.gross_exposure) {
            let _ = writeln!(s, "{},{i},{},{},{g}", p.ts_ns, p.equity, p.cash);
        }
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

/// The CSV artifacts of a run: four, plus `strategies.csv` for a multi-strategy run.
pub fn artifacts(r: &BacktestResult) -> Vec<Artifact> {
    let csv = |name, text: String| Artifact { name, media_type: "text/csv", bytes: text.into_bytes() };
    let mut out = vec![
        csv("equity.csv", equity_csv(r)),
        csv("orders.csv", orders_csv(r)),
        csv("fills.csv", fills_csv(r)),
        csv("returns.csv", returns_csv(r)),
    ];
    if !r.strategies.is_empty() {
        out.push(csv("strategies.csv", strategies_csv(r)));
    }
    if !r.continuous.is_empty() {
        out.push(csv("rolls.csv", rolls_csv(r)));
        out.push(csv("continuous.csv", continuous_csv(r)));
    }
    out
}

/// Every futures roll, close and settlement the engine made (instrument ids;
/// the summary names them). Prices are in the base currency.
pub fn rolls_csv(r: &BacktestResult) -> String {
    let mut s = String::from("ts_ns,strategy,kind,from,to,qty,close_price,open_price,note\n");
    for e in &r.rolls {
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{},{},{}",
            e.ts_ns,
            e.strategy,
            e.kind.as_str(),
            e.from,
            e.to.map_or(String::new(), |t| t.to_string()),
            e.qty,
            e.close_price,
            e.open_price.map_or(String::new(), |p| p.to_string()),
            csv_text(e.note.as_deref().unwrap_or("")),
        );
    }
    s
}

/// Each futures chain's continuous, back-adjusted series as it stood at the
/// end of the run (`contract` is the front contract each bar came from).
pub fn continuous_csv(r: &BacktestResult) -> String {
    let mut s = String::from("chain,ts_ns,contract,open,high,low,close,volume\n");
    for (chain, bars) in &r.continuous {
        for b in bars {
            let _ = writeln!(
                s,
                "{},{},{},{},{},{},{},{}",
                csv_text(chain),
                b.ts_ns,
                b.instrument,
                b.open,
                b.high,
                b.low,
                b.close,
                b.volume
            );
        }
    }
    s
}

/// A CSV field: quoted when it holds a comma, quote or line break.
fn csv_text(v: &str) -> String {
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
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
        OrderState::Expired,
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
        .with("bar_count", Json::integer(r.run.bar_count as i64));
    let run = if req.is_multi() {
        run.with("strategy", Json::string("multi"))
            .with("strategy_sha256", Json::string(req.strategy_sha256()))
            .with("config", config_to_user_json(&req.config)?)
    } else {
        run.with("strategy", Json::string(req.strategy.name))
            .with("strategy_sha256", Json::string(req.strategy.source_sha256()))
            .with("params", spectra_registry::params_to_json(&req.params)?)
            .with("config", config_to_user_json(&req.config)?)
    };

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
        .with("financing", money(r.ledger.financing()))
        .with("dividends", money(r.ledger.dividends()))
        .with("coupons", money(r.ledger.coupons()))
        .with("corporate_actions_applied", Json::integer(applied_actions(r) as i64))
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
        .with("avg_holding_ns", num(t.avg_holding_ns))
        .with("avg_holding_days", num(t.avg_holding_ns / 86_400e9));

    let run = if req.corporate_actions.is_empty() {
        run
    } else {
        run.with("corporate_actions", crate::corporate::actions_to_json(&req.corporate_actions)?)
    };
    let run = run.with("base_currency", Json::string(out.setup.base_currency.clone()));
    let run = if out.setup.is_trivial() {
        run
    } else {
        run.with("instruments_sha256", Json::string(out.setup.fingerprint())).with("fx_currencies", {
            let mut a = Json::array();
            for c in out.setup.foreign_currencies() {
                a.push(Json::string(c))?;
            }
            a
        })
    };
    let summary = Json::object()
        .with("schema", Json::string(SUMMARY_SCHEMA))
        .with("run", run)
        .with("metrics", metrics)
        .with("performance", performance_json(out))
        .with("drawdown", drawdown)
        .with("tail", tail)
        .with("exposure", exposure_json(r))
        .with("activity", activity)
        .with("trades", trades)
        .with("attribution", compute_pnl_attribution(&r.ledger, Some(&out.symbols), None)?.to_json())
        .with("instruments", instruments_json(out)?)
        .with("benchmark", benchmark_json(out)?);
    let summary = match futures_json(out)? {
        Some(f) => summary.with("futures", f),
        None => summary,
    };
    let summary = match bonds_json(out)? {
        Some(b) => summary.with("bonds", b),
        None => summary,
    };
    if req.is_multi() {
        return Ok(summary.with("strategies", strategies_json(out)?));
    }
    Ok(summary)
}

/// Each strategy of a multi-strategy run on its own books, and the
/// unallocated cash. `contribution` is the strategy's profit as a share of the
/// account's starting cash, so the contributions add up to the account's
/// total return less the unallocated cash's interest.
fn strategies_json(out: &BacktestOutput) -> Result<Json> {
    let r = &out.result;
    let start = r.equity_curve.first().map_or(0.0, |_| out.request.config.initial_cash.to_f64());
    let mut list = Json::array();
    for (i, (a, s)) in out.request.strategies.iter().zip(&r.strategies).enumerate() {
        let m = &s.metrics;
        let equity = s.ledger.equity()?;
        let profit = equity.checked_sub(s.allocation)?;
        let orders = r.orders.iter().filter(|o| o.strategy == i).count();
        let fills = s.ledger.journal().iter().filter(|e| e.kind == EntryType::Fill).count();
        let average_gross = {
            let (mut sum, mut n) = (0.0, 0usize);
            for (p, g) in s.equity_curve.iter().zip(&s.gross_exposure) {
                if p.equity.raw() > 0 {
                    sum += g.to_f64() / p.equity.to_f64();
                    n += 1;
                }
            }
            if n == 0 {
                Json::Null
            } else {
                num(sum / n as f64)
            }
        };
        list.push(
            Json::object()
                .with("index", Json::integer(i as i64))
                .with("strategy", Json::string(a.strategy.name))
                .with("strategy_sha256", Json::string(a.strategy.source_sha256()))
                .with("params", spectra_registry::params_to_json(&a.params)?)
                .with("weight", num(a.weight))
                .with("allocation", money(s.allocation))
                .with("final_equity", money(equity))
                .with("profit", money(profit))
                .with("contribution", if start > 0.0 { num(profit.to_f64() / start) } else { Json::Null })
                .with("total_return", num(m.total_return))
                .with("annualized_return", num(m.annualized_return))
                .with("annualized_volatility", num(m.annualized_volatility))
                .with("sharpe", num(m.sharpe))
                .with("max_drawdown", num(m.max_drawdown))
                .with("average_gross_exposure", average_gross)
                .with("order_count", Json::integer(orders as i64))
                .with("fill_count", Json::integer(fills as i64))
                .with("total_fees", money(s.ledger.total_fees()))
                .with("financing", money(s.ledger.financing()))
                .with("dividends", money(s.ledger.dividends()))
                .with("realized_pnl", money(s.ledger.realized_pnl())),
        )?;
    }
    let unallocated = match &r.unallocated {
        Some(l) => Json::object()
            .with("allocation", money(l.net_deposits()))
            .with("final_cash", money(l.cash()))
            .with("financing", money(l.financing())),
        None => Json::Null,
    };
    Ok(Json::object().with("list", list).with("unallocated", unallocated))
}

fn opt(v: Option<f64>) -> Json {
    v.map_or(Json::Null, num)
}

/// Ratios beyond the headline metrics, on the equity curve's period returns.
fn performance_json(out: &BacktestOutput) -> Json {
    let cfg = &out.request.config;
    let r = returns_from_curve(&out.result.equity_curve);
    let p = spectra_engine::performance::summarize(&r, cfg.periods_per_year, cfg.risk_free_rate);
    Json::object()
        .with("cagr", opt(p.cagr))
        .with("sortino", opt(p.sortino))
        .with("calmar", opt(p.calmar))
        .with("omega", opt(p.omega))
        .with("downside_deviation", opt(p.downside_deviation))
        .with("skewness", opt(p.skewness))
        .with("excess_kurtosis", opt(p.excess_kurtosis))
        .with("drawdown_episodes", Json::integer(p.drawdown_episodes as i64))
        .with("average_drawdown", opt(p.average_drawdown))
}

/// Exposure as a share of equity: gross (sum of absolute position values) and
/// net (long minus short), averaged over the curve, plus time in the market.
fn exposure_json(r: &BacktestResult) -> Json {
    let (mut n, mut invested) = (0usize, 0usize);
    let (mut gross_sum, mut gross_max, mut net_sum) = (0.0f64, 0.0f64, 0.0f64);
    let (mut net_min, mut net_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for (p, g) in r.equity_curve.iter().zip(&r.gross_exposure) {
        let eq = p.equity.to_f64();
        if eq <= 0.0 {
            continue;
        }
        let gross = g.to_f64() / eq;
        let net = (p.equity.to_f64() - p.cash.to_f64()) / eq;
        n += 1;
        invested += usize::from(!g.is_zero());
        gross_sum += gross;
        gross_max = gross_max.max(gross);
        net_sum += net;
        net_min = net_min.min(net);
        net_max = net_max.max(net);
    }
    if n == 0 {
        return Json::object().with("valid", Json::boolean(false));
    }
    let nf = n as f64;
    Json::object()
        .with("valid", Json::boolean(true))
        .with("average_gross", num(gross_sum / nf))
        .with("max_gross", num(gross_max))
        .with("average_net", num(net_sum / nf))
        .with("min_net", num(net_min))
        .with("max_net", num(net_max))
        .with("time_in_market", num(invested as f64 / nf))
}

/// Splits and dividends applied to the account's books.
fn applied_actions(r: &BacktestResult) -> usize {
    r.ledger.journal().iter().filter(|e| matches!(e.kind, EntryType::Split | EntryType::Dividend)).count()
}

/// Net P&L of a position: `realized + unrealized − fees − borrow + dividends`.
///
/// # Errors
/// Overflow.
pub fn net_pnl(
    realized: Money,
    unrealized: Money,
    fees: Money,
    borrow: Money,
    dividends: Money,
) -> Result<Money> {
    Ok(realized.checked_add(unrealized)?.checked_sub(fees)?.checked_sub(borrow)?.checked_add(dividends)?)
}

/// P&L and costs per instrument, from the ledger.
fn instruments_json(out: &BacktestOutput) -> Result<Json> {
    let l = &out.result.ledger;
    // (fills, traded notional, borrow fees, dividends, coupons) per instrument.
    let mut fills: std::collections::BTreeMap<spectra_core::InstrumentId, (i64, f64, Money, Money, Money)> =
        std::collections::BTreeMap::new();
    for e in l.journal() {
        let slot = fills.entry(e.fill.instrument).or_insert((0, 0.0, Money::ZERO, Money::ZERO, Money::ZERO));
        match e.kind {
            EntryType::Fill => {
                slot.0 += 1;
                slot.1 += l.notional_of(e.fill.instrument, e.fill.price, e.fill.qty)?.to_f64();
            }
            EntryType::BorrowFee => slot.2 = slot.2.checked_add(e.amount)?,
            EntryType::Dividend => slot.3 = slot.3.checked_add(e.amount)?,
            EntryType::Coupon => slot.4 = slot.4.checked_add(e.amount)?,
            _ => {}
        }
    }
    let mut arr = Json::array();
    for (&id, p) in l.positions() {
        // Market value is the notional a position controls; for a future that
        // notional never changed hands, so its P&L is reported separately.
        let value = l.position_exposure(id)?;
        let unrealized = l.position_unrealized(id)?;
        let (count, traded, borrow, dividends, coupons) =
            fills.get(&id).copied().unwrap_or((0, 0.0, Money::ZERO, Money::ZERO, Money::ZERO));
        // Income is income: a coupon adds to net P&L as a dividend does.
        let net = net_pnl(p.realized, unrealized, p.fees, borrow, dividends.checked_add(coupons)?)?;
        let symbol = out.symbols.get(id as usize).map_or(Json::Null, |s| Json::string(s.clone()));
        let record = out.setup.instrument(id);
        let underlying = record.and_then(|r| r.underlying.clone()).map_or(Json::Null, Json::string);
        let currency = record.map_or(out.setup.base_currency.as_str(), |r| r.currency.as_str());
        let expiry = record.and_then(|r| r.expiry_ns).map_or(Json::Null, Json::integer);
        arr.push(
            Json::object()
                .with("instrument", Json::integer(i64::from(id)))
                .with("symbol", symbol)
                .with("type", Json::string(out.setup.kind(id).as_str()))
                .with("currency", Json::string(currency))
                .with("multiplier", Json::string(l.multiplier(id).to_string()))
                .with("expiry", expiry)
                .with("underlying", underlying)
                .with("fills", Json::integer(count))
                .with("traded_notional", num(traded))
                .with("final_qty", Json::string(p.qty.to_string()))
                .with("market_value", money(value))
                .with("realized_pnl", money(p.realized))
                .with("unrealized_pnl", money(unrealized))
                .with("fees", money(p.fees))
                .with("borrow_fees", money(borrow))
                .with("dividends", money(dividends))
                .with("coupons", money(coupons))
                .with("net_pnl", money(net)),
        )?;
    }
    Ok(arr)
}

/// What the run says about bonds, when it has any: the quote convention,
/// redemptions, and each bond's yield and interest-rate risk at its last price.
fn bonds_json(out: &BacktestOutput) -> Result<Option<Json>> {
    let bonds: Vec<(spectra_core::InstrumentId, spectra_engine::BondTerms)> =
        out.setup.instruments.keys().filter_map(|&id| out.setup.bond_terms(id).map(|t| (id, t))).collect();
    if bonds.is_empty() {
        return Ok(None);
    }
    let name = |id: spectra_core::InstrumentId| {
        out.symbols
            .get(id as usize)
            .cloned()
            .or_else(|| out.setup.instrument(id).map(|i| i.symbol.clone()))
            .unwrap_or_else(|| format!("#{id}"))
    };
    let mut list = Json::array();
    for (id, terms) in &bonds {
        let mut j = Json::object()
            .with("symbol", Json::string(name(*id)))
            .with("coupon_rate", num(terms.coupon_rate))
            .with("coupon_frequency", Json::integer(i64::from(terms.frequency)))
            .with("day_count", Json::string(terms.day_count.as_str()))
            .with("maturity", Json::integer(terms.maturity_ns));
        if let Some(&(ts, close)) = out.last_quotes.get(id) {
            let quoted = close.to_f64();
            let accrued = terms.accrued(ts);
            let (clean, dirty) = match out.setup.bond_quote {
                spectra_engine::BondQuote::Clean => (quoted, quoted + accrued),
                spectra_engine::BondQuote::Dirty => (quoted - accrued, quoted),
            };
            j = j
                .with("as_of", Json::integer(ts))
                .with("clean_price", num(clean))
                .with("dirty_price", num(dirty))
                .with("accrued_interest", num(accrued));
            if let Ok(y) = terms.yield_from_price(dirty, ts) {
                let r = terms.risk(y, ts);
                j = j
                    .with("yield_to_maturity", num(y))
                    .with("macaulay_duration", num(r.macaulay))
                    .with("modified_duration", num(r.modified))
                    .with("convexity", num(r.convexity))
                    .with("dv01_per_100", num(r.dv01));
            }
        }
        list.push(j)?;
    }
    let mut redemptions = Json::array();
    for e in out.result.rolls.iter().filter(|e| e.kind == spectra_engine::RollKind::Redeemed) {
        redemptions.push(
            Json::object()
                .with("ts_ns", Json::integer(e.ts_ns))
                .with("strategy", Json::integer(e.strategy as i64))
                .with("symbol", Json::string(name(e.from)))
                .with("qty", Json::string(e.qty.to_string()))
                .with("price", Json::string(e.close_price.to_string())),
        )?;
    }
    Ok(Some(
        Json::object()
            .with("quote", Json::string(out.setup.bond_quote.as_str()))
            .with("coupons", money(out.result.ledger.coupons()))
            .with("bonds", list)
            .with("redemptions", redemptions)
            .with("gaps", {
                let mut g = Json::array();
                g.push(Json::string(BOND_GAP))?;
                g
            }),
    ))
}

/// What a bond run does not model, stated in its summary.
pub const BOND_GAP: &str = "Ex-coupon periods, settlement lags, default and call features are not modelled: \
    the holder at the close before a coupon date receives the coupon, trades settle at the bar, and every bond \
    is repaid at par at maturity.";

/// What a futures run does not model (M24.2 task 10), stated in its summary.
pub const MARGIN_GAP: &str = "Margin is not modelled. Futures are marked to market (no notional changes \
    hands; P&L settles to cash) and the leverage limit applies to their notional, but no initial margin is \
    held, margin calls are not simulated, and no interest is paid on posted margin.";

/// What the run says about futures, when it has any: the roll rule, every
/// roll, close and settlement, and what the model leaves out.
fn futures_json(out: &BacktestOutput) -> Result<Option<Json>> {
    let has_futures = out.setup.instruments.values().any(|i| i.kind == spectra_core::InstrumentType::Future);
    if !has_futures {
        return Ok(None);
    }
    let name = |id: spectra_core::InstrumentId| {
        out.symbols
            .get(id as usize)
            .cloned()
            .or_else(|| out.setup.instrument(id).map(|i| i.symbol.clone()))
            .unwrap_or_else(|| format!("#{id}"))
    };
    let rule = &out.setup.roll;
    let mut rolls = Json::array();
    for e in out.result.rolls.iter().filter(|e| e.kind != spectra_engine::RollKind::Redeemed) {
        rolls.push(
            Json::object()
                .with("ts_ns", Json::integer(e.ts_ns))
                .with("strategy", Json::integer(e.strategy as i64))
                .with("kind", Json::string(e.kind.as_str()))
                .with("from", Json::string(name(e.from)))
                .with("to", e.to.map_or(Json::Null, |t| Json::string(name(t))))
                .with("qty", Json::string(e.qty.to_string()))
                .with("close_price", Json::string(e.close_price.to_string()))
                .with("open_price", e.open_price.map_or(Json::Null, |p| Json::string(p.to_string())))
                .with("note", e.note.as_ref().map_or(Json::Null, |n| Json::string(n.clone()))),
        )?;
    }
    let mut chains = Json::array();
    for chain in out.result.continuous.keys() {
        chains.push(Json::string(chain.clone()))?;
    }
    Ok(Some(
        Json::object()
            .with(
                "roll_rule",
                Json::object()
                    .with(
                        "business_days_before_expiry",
                        Json::integer(i64::from(rule.business_days_before_expiry)),
                    )
                    .with("adjust", Json::string(rule.adjust.as_str())),
            )
            .with("chains", chains)
            .with("rolls", rolls)
            .with("gaps", {
                let mut gaps = Json::array();
                gaps.push(Json::string(MARGIN_GAP))?;
                gaps
            }),
    ))
}

/// The strategy against an equal-weight buy-and-hold of the same bars (no costs).
fn benchmark_json(out: &BacktestOutput) -> Result<Json> {
    let s = spectra_research::aligned_returns(&out.result.equity_curve);
    let b = &out.benchmark;
    if b.len() != s.len() || s.len() < 2 {
        return Ok(Json::object().with("valid", Json::boolean(false)));
    }
    let m = spectra_research::compute_benchmark_metrics(&s, b, out.request.config.periods_per_year)?;
    let total = b.iter().fold(1.0, |acc, x| acc * (1.0 + x)) - 1.0;
    Ok(Json::object()
        .with("valid", Json::boolean(m.valid))
        .with("name", Json::string("equal-weight buy and hold of every instrument, no costs"))
        .with("total_return", num(total))
        .with("excess_total_return", num(out.result.metrics.total_return - total))
        .with("beta", num(m.beta))
        .with("alpha_annualized", num(m.alpha_annualized))
        .with("correlation", num(m.correlation))
        .with("tracking_error", num(m.tracking_error))
        .with("information_ratio", num(m.information_ratio))
        .with("up_capture", num(m.up_capture))
        .with("down_capture", num(m.down_capture)))
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
