//! Event-driven backtester.
//!
//! Timing contract (no look-ahead by construction):
//!  * Bars are fed one timestamp at a time. At timestamp T the engine
//!    1. fills orders submitted at earlier timestamps against the bars of T,
//!    2. marks positions at T's close and records an equity point,
//!    3. calls [`Strategy::on_fill`] for those fills, then [`Strategy::on_bar`].
//!  * Orders a strategy submits while handling T fill no earlier than the next
//!    timestamp that has a bar for the instrument.
//!  * A strategy only ever sees history up to and including T.
//!
//! Fill rules:
//!  * Market: the bar's open, moved against the trader by the slippage.
//!  * Limit buy: the open if open <= limit, else the limit if the bar's low
//!    reaches it. Limit sell is symmetric. No slippage on limits.
//!  * Participation cap (optional): an order takes at most a fraction of the
//!    bar's volume per bar; the rest stays open (partial fills).
//!  * A fill the ledger rejects marks the order Rejected with the reason.
//!
//! Ported from the C++ reference (`quantforge` v0.6.0). With
//! [`LedgerConfig::reference`] policy, run fingerprints are identical to the
//! C++ engine's. Three deliberate changes, each closing a defect found in the
//! audit of the C++ engine:
//!  * the ledger enforces a gross leverage limit (default 1.0x);
//!  * open orders are kept in a price-indexed book, so a step only visits
//!    orders that can fill on it (the C++ engine re-scanned every order ever
//!    submitted at every step and slowed down quadratically);
//!  * datasets whose instruments have different median bar intervals are
//!    refused, because a bar stamped at its start time exposes its close to
//!    strategies trading other instruments inside that bar (look-ahead).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spectra_core::cfmt::fmt_g17;
use spectra_core::{
    bars_sha256, notional, validate_bars, Bar, Error, Fill, InstrumentId, JournalEntry, Ledger, LedgerConfig,
    LedgerError, Money, Price, Qty, Reject, Result, Sha256, Side, ENGINE_VERSION, SCALE,
};

use crate::costs::{commission_fee, slipped_price, CostModel};
use crate::metrics::{compute_metrics, EquityPoint, Metrics};

/// Order identifier; ids start at 1 in submission order.
pub type OrderId = u64;

/// Order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderType {
    /// Fill at the next open plus slippage.
    Market,
    /// Fill at the limit or better.
    Limit,
    /// Becomes a market order when the bar trades through the stop: a buy stop
    /// when the high reaches it, a sell stop when the low does. Fills at the
    /// stop, or at the open if the bar gaps through it, plus slippage.
    Stop,
    /// Becomes a limit order when the bar trades through the stop. On the
    /// trigger bar it fills at the trigger price (stop, or a gapped open) if
    /// that is within the limit; otherwise it rests as a limit order.
    StopLimit,
}

impl OrderType {
    /// Name used in outputs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Limit => "limit",
            Self::Stop => "stop",
            Self::StopLimit => "stop_limit",
        }
    }
    /// Stable numeric code hashed into result fingerprints (0 and 1 as before).
    const fn code(self) -> u64 {
        match self {
            Self::Market => 0,
            Self::Limit => 1,
            Self::Stop => 2,
            Self::StopLimit => 3,
        }
    }
}

/// How long an order stays open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TimeInForce {
    /// Until filled or cancelled (the default, and the only behaviour before).
    #[default]
    GoodTillCancelled,
    /// For one bar: expires if not completely filled on the first bar it is
    /// allowed to fill on.
    Day,
    /// Until this timestamp (inclusive), nanoseconds.
    GoodTillDate(i64),
}

impl TimeInForce {
    /// Name used in outputs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GoodTillCancelled => "gtc",
            Self::Day => "day",
            Self::GoodTillDate(_) => "gtd",
        }
    }
}

/// Order lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderState {
    /// Waiting to fill.
    Pending,
    /// Some quantity filled, the rest still open.
    PartiallyFilled,
    /// Completely filled.
    Filled,
    /// Cancelled by the strategy.
    Cancelled,
    /// Refused (malformed, or the ledger refused the fill).
    Rejected,
    /// Its time in force ran out before it was completely filled.
    Expired,
}

impl OrderState {
    /// Display name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::PartiallyFilled => "PartiallyFilled",
            Self::Filled => "Filled",
            Self::Cancelled => "Cancelled",
            Self::Rejected => "Rejected",
            Self::Expired => "Expired",
        }
    }
    /// True while the order can still fill.
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::PartiallyFilled)
    }
}

/// An order and its current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    /// Identifier (1-based).
    pub id: OrderId,
    /// Instrument.
    pub instrument: InstrumentId,
    /// Buy or sell.
    pub side: Side,
    /// Market, limit, stop or stop-limit.
    pub kind: OrderType,
    /// Original quantity.
    pub qty: Qty,
    /// Still unfilled.
    pub remaining: Qty,
    /// Limit price (limit and stop-limit orders only).
    pub limit: Price,
    /// Stop (trigger) price (stop and stop-limit orders only).
    pub stop: Price,
    /// How long the order stays open.
    pub tif: TimeInForce,
    /// The stop has been reached: a stop now behaves as a market order, a
    /// stop-limit as a limit order.
    pub triggered: bool,
    /// Timestamp at submission.
    pub submit_ts: i64,
    /// Index of the timestamp being processed when submitted (-1 in `on_start`).
    pub submit_step: i64,
    /// Current state.
    pub state: OrderState,
    /// Why it was rejected, if it was.
    pub reject_reason: Option<Reject>,
}

/// Execution realism settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecutionConfig {
    /// Extra timestamps an order waits beyond the earliest possible fill. 0
    /// fills at the next timestamp's open; 2 at the open two timestamps later.
    /// Models latency and stale signals. Cancels take effect immediately.
    pub delay_bars: usize,
    /// Maximum fraction of a bar's volume one order may take per bar, scaled
    /// like all fixed-point values (0.1 = 10%). Zero disables the cap.
    pub max_participation: Qty,
}

/// Financing charged or paid once per bar period, on the books as they stood
/// at the end of the previous period (positions valued at the previous close).
/// Rates are annual fractions (0.05 = 5% a year), divided by
/// `periods_per_year` for each period. All zero (the default) charges nothing,
/// and the run is identical to one without financing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FinancingConfig {
    /// Borrow fee on the market value of each short position.
    pub borrow_rate: f64,
    /// Interest earned on positive cash (including short-sale proceeds).
    pub cash_rate: f64,
    /// Interest paid on negative cash (a margin debit).
    pub margin_rate: f64,
}

impl FinancingConfig {
    /// True when nothing would ever be charged or paid.
    pub fn is_zero(&self) -> bool {
        self.borrow_rate == 0.0 && self.cash_rate == 0.0 && self.margin_rate == 0.0
    }
}

/// Square-root market impact on market and stop fills:
/// `impact = coefficient * sigma * sqrt(fill qty / bar volume)`, as a fraction
/// of price, added to the flat slippage and capped at 10%. `sigma` is the
/// standard deviation of the instrument's close-to-close returns over the last
/// `window` bars *before* the fill bar (no look-ahead); with fewer than two
/// such returns there is no impact. A coefficient of 0 (the default) turns it off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpactConfig {
    /// Impact coefficient; around 1 is a common starting point.
    pub coefficient: f64,
    /// Bars of history for the volatility estimate.
    pub window: usize,
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self { coefficient: 0.0, window: 20 }
    }
}

/// Largest impact applied to one fill, as a fraction of price.
const MAX_IMPACT: f64 = 0.10;

/// Everything that defines a backtest besides the bars and the strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BacktestConfig {
    /// Starting cash.
    pub initial_cash: Money,
    /// Ledger policy (shorting, negative cash, leverage limit).
    pub ledger: LedgerConfig,
    /// Commission and slippage.
    pub costs: CostModel,
    /// Delay and participation cap.
    pub exec: ExecutionConfig,
    /// Exposed to strategies; recorded in the run record.
    pub seed: u64,
    /// Periods per year for annualization.
    pub periods_per_year: f64,
    /// Annual risk-free rate for the Sharpe ratio.
    pub risk_free_rate: f64,
    /// Borrow fees and interest (off by default).
    pub financing: FinancingConfig,
    /// Volume-based market impact (off by default).
    pub impact: ImpactConfig,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            initial_cash: Money::from_raw(100_000 * SCALE),
            ledger: LedgerConfig::default(),
            costs: CostModel::default(),
            exec: ExecutionConfig::default(),
            seed: 0,
            periods_per_year: 252.0,
            risk_free_rate: 0.0,
            financing: FinancingConfig::default(),
            impact: ImpactConfig::default(),
        }
    }
}

/// Everything needed to identify and later verify a run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunRecord {
    /// Engine that produced the run.
    pub engine_version: String,
    /// Fingerprint of every bar field, in order.
    pub dataset_sha256: String,
    /// Fingerprint of the canonical configuration.
    pub config_sha256: String,
    /// Fingerprint of the equity curve, journal and orders.
    pub result_sha256: String,
    /// Seed.
    pub seed: u64,
    /// Number of bars.
    pub bar_count: usize,
}

/// The outcome of a backtest.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    /// One point per timestamp.
    pub equity_curve: Vec<EquityPoint>,
    /// Gross exposure (sum of the absolute marked position values) at each
    /// point of the equity curve. Not part of the result fingerprint.
    pub gross_exposure: Vec<Money>,
    /// Every order in submission order.
    pub orders: Vec<Order>,
    /// The final books.
    pub ledger: Ledger,
    /// Headline metrics.
    pub metrics: Metrics,
    /// Identity of the run.
    pub run: RunRecord,
}

impl BacktestResult {
    /// Number of fills in the journal.
    pub fn fill_count(&self) -> usize {
        self.ledger.journal().iter().filter(|e| e.kind == spectra_core::EntryType::Fill).count()
    }

    /// Annualized turnover: traded notional / mean equity, scaled to a year.
    pub fn turnover(&self, periods_per_year: f64) -> f64 {
        let traded: f64 = self
            .ledger
            .journal()
            .iter()
            .filter(|e| e.kind == spectra_core::EntryType::Fill)
            .map(|e| notional(e.fill.price, e.fill.qty).map_or(0.0, Money::to_f64))
            .sum();
        let n = self.equity_curve.len();
        if n < 2 {
            return 0.0;
        }
        let mean_eq = self.equity_curve.iter().map(|p| p.equity.to_f64()).sum::<f64>() / n as f64;
        if mean_eq <= 0.0 {
            return 0.0;
        }
        traded / mean_eq * periods_per_year / (n - 1) as f64
    }
}

/// A trading strategy. Callbacks run on the engine's thread; a strategy object
/// is used for exactly one run.
pub trait Strategy {
    /// Called once before the first bar. Orders placed here may fill on the first bar.
    fn on_start(&mut self, _ctx: &mut Context<'_>) {}
    /// Called after each timestamp's fills and marks, with that timestamp's bars.
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]);
    /// Called for each fill at the current timestamp, before `on_bar`.
    fn on_fill(&mut self, _ctx: &mut Context<'_>, _fill: &Fill) {}
}

/// Open orders, indexed so a step only visits orders that can fill on it.
///
/// Market orders are kept in submission order. Limit orders are kept per
/// instrument in price order, so a bar only visits buy limits at or above its
/// low and sell limits at or below its high: resting orders far from the
/// market cost O(log n) per step instead of a scan. Candidates are then
/// processed in submission order, exactly as the C++ engine processed its
/// full scan, so fills (and fingerprints) are identical.
#[derive(Debug, Default)]
struct OpenOrders {
    market: Vec<usize>,
    /// Keyed by (-limit, order index): ascending iteration = highest bid first.
    buys: BTreeMap<InstrumentId, BTreeSet<(i64, usize)>>,
    /// Keyed by (limit, order index).
    sells: BTreeMap<InstrumentId, BTreeSet<(i64, usize)>>,
    /// Untriggered buy stops keyed by (stop, index): a bar triggers those at or below its high.
    stop_buys: BTreeMap<InstrumentId, BTreeSet<(i64, usize)>>,
    /// Untriggered sell stops keyed by (-stop, index): a bar triggers those at or above its low.
    stop_sells: BTreeMap<InstrumentId, BTreeSet<(i64, usize)>>,
    /// Open orders with a time in force other than good-till-cancelled.
    timed: Vec<usize>,
}

/// Where an open order rests in [`OpenOrders`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Book {
    Market,
    Limit,
    Stop,
}

fn book(o: &Order) -> Book {
    match o.kind {
        OrderType::Market => Book::Market,
        OrderType::Limit => Book::Limit,
        OrderType::StopLimit if o.triggered => Book::Limit,
        OrderType::Stop if o.triggered => Book::Market,
        OrderType::Stop | OrderType::StopLimit => Book::Stop,
    }
}

impl OpenOrders {
    fn key(o: &Order, index: usize) -> (i64, usize) {
        if o.side == Side::Buy {
            (-o.limit.raw(), index)
        } else {
            (o.limit.raw(), index)
        }
    }

    fn stop_key(o: &Order, index: usize) -> (i64, usize) {
        if o.side == Side::Buy {
            (o.stop.raw(), index)
        } else {
            (-o.stop.raw(), index)
        }
    }

    fn insert(&mut self, o: &Order, index: usize) {
        if o.tif != TimeInForce::GoodTillCancelled && !self.timed.contains(&index) {
            self.timed.push(index);
        }
        match (book(o), o.side) {
            (Book::Market, _) => self.market.push(index),
            (Book::Limit, Side::Buy) => {
                self.buys.entry(o.instrument).or_default().insert(Self::key(o, index));
            }
            (Book::Limit, Side::Sell) => {
                self.sells.entry(o.instrument).or_default().insert(Self::key(o, index));
            }
            (Book::Stop, Side::Buy) => {
                self.stop_buys.entry(o.instrument).or_default().insert(Self::stop_key(o, index));
            }
            (Book::Stop, Side::Sell) => {
                self.stop_sells.entry(o.instrument).or_default().insert(Self::stop_key(o, index));
            }
        }
    }

    fn remove(&mut self, o: &Order, index: usize) {
        let (books, key) = match (book(o), o.side) {
            (Book::Market, _) => return, // pruned lazily: market orders are short-lived
            (Book::Limit, Side::Buy) => (&mut self.buys, Self::key(o, index)),
            (Book::Limit, Side::Sell) => (&mut self.sells, Self::key(o, index)),
            (Book::Stop, Side::Buy) => (&mut self.stop_buys, Self::stop_key(o, index)),
            (Book::Stop, Side::Sell) => (&mut self.stop_sells, Self::stop_key(o, index)),
        };
        if let Some(set) = books.get_mut(&o.instrument) {
            set.remove(&key);
        }
    }

    /// Orders that might fill against `group`, in submission order.
    fn candidates(&self, group: &[Bar], orders: &[Order]) -> Vec<usize> {
        let mut out: Vec<usize> = self
            .market
            .iter()
            .copied()
            .filter(|&k| orders[k].state.is_open() && find_bar(group, orders[k].instrument).is_some())
            .collect();
        for b in group {
            if let Some(set) = self.buys.get(&b.instrument) {
                out.extend(set.range(..=(-b.low.raw(), usize::MAX)).map(|&(_, k)| k));
            }
            if let Some(set) = self.sells.get(&b.instrument) {
                out.extend(set.range(..=(b.high.raw(), usize::MAX)).map(|&(_, k)| k));
            }
            if let Some(set) = self.stop_buys.get(&b.instrument) {
                out.extend(set.range(..=(b.high.raw(), usize::MAX)).map(|&(_, k)| k));
            }
            if let Some(set) = self.stop_sells.get(&b.instrument) {
                out.extend(set.range(..=(-b.low.raw(), usize::MAX)).map(|&(_, k)| k));
            }
        }
        out.sort_unstable();
        out
    }
}

/// Everything needed to place an order (see [`Context::order`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSpec {
    /// Instrument.
    pub instrument: InstrumentId,
    /// Buy or sell.
    pub side: Side,
    /// Order type.
    pub kind: OrderType,
    /// Quantity, > 0.
    pub qty: Qty,
    /// Limit price (limit and stop-limit).
    pub limit: Price,
    /// Stop price (stop and stop-limit).
    pub stop: Price,
    /// Time in force.
    pub tif: TimeInForce,
}

impl OrderSpec {
    /// A good-till-cancelled order without prices; set `limit` / `stop` as the kind needs.
    pub fn new(instrument: InstrumentId, side: Side, kind: OrderType, qty: Qty) -> Self {
        Self {
            instrument,
            side,
            kind,
            qty,
            limit: Price::ZERO,
            stop: Price::ZERO,
            tif: TimeInForce::default(),
        }
    }
}

/// The strategy's window onto the simulation.
#[derive(Debug)]
pub struct Context<'a> {
    ledger: &'a Ledger,
    history: &'a BTreeMap<InstrumentId, Vec<Bar>>,
    orders: &'a mut Vec<Order>,
    open: &'a mut OpenOrders,
    now: i64,
    step: i64,
    seed: u64,
}

impl Context<'_> {
    /// Current timestamp.
    pub fn now(&self) -> i64 {
        self.now
    }
    /// The run's seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// Bars seen so far for the instrument, oldest first, including the current one.
    pub fn history(&self, instrument: InstrumentId) -> &[Bar] {
        self.history.get(&instrument).map_or(&[], Vec::as_slice)
    }
    /// Current position quantity.
    pub fn position(&self, instrument: InstrumentId) -> Qty {
        self.ledger.position(instrument).qty
    }
    /// Current cash.
    pub fn cash(&self) -> Money {
        self.ledger.cash()
    }
    /// Current equity (cash plus marked positions); zero if it overflows.
    pub fn equity(&self) -> Money {
        self.ledger.equity().unwrap_or_default()
    }
    /// The orders so far.
    pub fn orders(&self) -> &[Order] {
        self.orders
    }
    /// The ledger policy of this run (shorting, negative cash, leverage limit).
    pub fn ledger_policy(&self) -> LedgerConfig {
        *self.ledger.config()
    }
    /// Marked value of a position (negative for shorts).
    pub fn position_value(&self, instrument: InstrumentId) -> Money {
        self.ledger.position_value(instrument).unwrap_or_default()
    }

    fn submit(&mut self, spec: OrderSpec) -> OrderId {
        let OrderSpec { instrument, side, kind, qty, limit, stop, tif } = spec;
        let id = self.orders.len() as OrderId + 1;
        let needs_limit = matches!(kind, OrderType::Limit | OrderType::StopLimit);
        let needs_stop = matches!(kind, OrderType::Stop | OrderType::StopLimit);
        let expired = matches!(tif, TimeInForce::GoodTillDate(t) if t < self.now);
        let bad =
            qty.raw() <= 0 || (needs_limit && limit.raw() <= 0) || (needs_stop && stop.raw() <= 0) || expired;
        self.orders.push(Order {
            id,
            instrument,
            side,
            kind,
            qty,
            remaining: qty,
            limit: if needs_limit { limit } else { Price::ZERO },
            stop: if needs_stop { stop } else { Price::ZERO },
            tif,
            triggered: false,
            submit_ts: self.now,
            submit_step: self.step,
            state: if bad { OrderState::Rejected } else { OrderState::Pending },
            reject_reason: bad.then_some(Reject::InvalidFill),
        });
        if !bad {
            let index = self.orders.len() - 1;
            self.open.insert(&self.orders[index], index);
        }
        id
    }

    /// Submit a market order. Malformed orders are recorded as Rejected.
    pub fn market(&mut self, instrument: InstrumentId, side: Side, qty: Qty) -> OrderId {
        self.submit(OrderSpec::new(instrument, side, OrderType::Market, qty))
    }
    /// Submit a limit order. Malformed orders are recorded as Rejected.
    pub fn limit(&mut self, instrument: InstrumentId, side: Side, qty: Qty, limit_price: Price) -> OrderId {
        self.submit(OrderSpec {
            limit: limit_price,
            ..OrderSpec::new(instrument, side, OrderType::Limit, qty)
        })
    }
    /// Submit a stop (stop-market) order. Malformed orders are recorded as Rejected.
    pub fn stop(&mut self, instrument: InstrumentId, side: Side, qty: Qty, stop_price: Price) -> OrderId {
        self.submit(OrderSpec { stop: stop_price, ..OrderSpec::new(instrument, side, OrderType::Stop, qty) })
    }
    /// Submit a stop-limit order. Malformed orders are recorded as Rejected.
    pub fn stop_limit(
        &mut self,
        instrument: InstrumentId,
        side: Side,
        qty: Qty,
        stop_price: Price,
        limit_price: Price,
    ) -> OrderId {
        self.submit(OrderSpec {
            stop: stop_price,
            limit: limit_price,
            ..OrderSpec::new(instrument, side, OrderType::StopLimit, qty)
        })
    }
    /// Submit any order, including a time in force. A good-till-date order
    /// whose date has already passed is recorded as Rejected.
    pub fn order(&mut self, spec: OrderSpec) -> OrderId {
        self.submit(spec)
    }
    /// Cancel an open order; true if it was open and is now Cancelled.
    pub fn cancel(&mut self, id: OrderId) -> bool {
        if id == 0 || id > self.orders.len() as OrderId {
            return false;
        }
        let index = (id - 1) as usize;
        if !self.orders[index].state.is_open() {
            return false;
        }
        self.orders[index].state = OrderState::Cancelled;
        self.open.remove(&self.orders[index], index);
        true
    }
}

struct FillAttempt {
    price: Price,
    qty: Qty,
}

/// What a bar does to an order: possibly a fill, and whether a stop-limit's
/// stop was reached (it then rests as a limit order).
struct Evaluation {
    fill: Option<FillAttempt>,
    triggered: bool,
}

/// Market impact for a fill of `qty`, in basis points (see [`ImpactConfig`]).
fn impact_bps(cfg: &ImpactConfig, history: &[Bar], bar: &Bar, qty: Qty) -> f64 {
    if cfg.coefficient <= 0.0 || bar.volume.raw() <= 0 {
        return 0.0;
    }
    // History includes the fill bar itself (pushed before fills): leave it out.
    let past = &history[..history.len().saturating_sub(1)];
    let past = &past[past.len().saturating_sub(cfg.window + 1)..];
    let r: Vec<f64> = past
        .windows(2)
        .filter(|w| w[0].close.raw() > 0)
        .map(|w| w[1].close.to_f64() / w[0].close.to_f64() - 1.0)
        .collect();
    let Some(sigma) = crate::performance::stdev(&r) else { return 0.0 };
    let frac = cfg.coefficient * sigma * (qty.to_f64() / bar.volume.to_f64()).sqrt();
    frac.min(MAX_IMPACT) * 10_000.0
}

/// A market-type fill price: `base` moved against the trader by the flat
/// slippage plus any impact.
fn market_price(base: Price, side: Side, cfg: &BacktestConfig, impact: f64) -> Result<Price> {
    let mut bps = cfg.costs.slippage;
    if impact > 0.0 {
        bps = bps.checked_add(spectra_core::Bps::from_f64(impact)?)?;
    }
    Ok(slipped_price(base, side, bps)?)
}

/// A limit order's fill price against `bar`, if it trades at the limit or better.
fn limit_price(side: Side, limit: Price, bar: &Bar) -> Option<Price> {
    match side {
        Side::Buy if bar.open <= limit => Some(bar.open), // gapped through: price improvement
        Side::Buy if bar.low <= limit => Some(limit),
        Side::Sell if bar.open >= limit => Some(bar.open),
        Side::Sell if bar.high >= limit => Some(limit),
        _ => None,
    }
}

/// The price at which `bar` trades through a stop, if it does: the stop, or the
/// open when the bar gaps past it.
fn stop_trigger(side: Side, stop: Price, bar: &Bar) -> Option<Price> {
    match side {
        Side::Buy if bar.high >= stop => Some(bar.open.max(stop)),
        Side::Sell if bar.low <= stop => Some(bar.open.min(stop)),
        _ => None,
    }
}

fn evaluate(o: &Order, bar: &Bar, history: &[Bar], cfg: &BacktestConfig) -> Result<Evaluation> {
    let none = |triggered| Evaluation { fill: None, triggered };
    let mut q = o.remaining;
    if cfg.exec.max_participation.raw() > 0 {
        let cap =
            i128::from(bar.volume.raw()) * i128::from(cfg.exec.max_participation.raw()) / i128::from(SCALE);
        q = q.min(Qty::from_raw(spectra_core::fixed::narrow(cap)?));
    }
    let mut triggered = false;
    let price = match o.kind {
        OrderType::Market => {
            Some(market_price(bar.open, o.side, cfg, impact_bps(&cfg.impact, history, bar, q))?)
        }
        OrderType::Limit => limit_price(o.side, o.limit, bar),
        OrderType::StopLimit if o.triggered => limit_price(o.side, o.limit, bar),
        // A stop whose stop was reached on an earlier bar (a partial fill) is a market order now.
        OrderType::Stop if o.triggered => {
            Some(market_price(bar.open, o.side, cfg, impact_bps(&cfg.impact, history, bar, q))?)
        }
        OrderType::Stop => match stop_trigger(o.side, o.stop, bar) {
            Some(base) => {
                triggered = true;
                Some(market_price(base, o.side, cfg, impact_bps(&cfg.impact, history, bar, q))?)
            }
            None => None,
        },
        OrderType::StopLimit => match stop_trigger(o.side, o.stop, bar) {
            Some(t) => {
                triggered = true;
                let within = if o.side == Side::Buy { t <= o.limit } else { t >= o.limit };
                within.then_some(t)
            }
            None => None,
        },
    };
    let Some(price) = price else { return Ok(none(triggered)) };
    if q.raw() <= 0 {
        return Ok(none(triggered));
    }
    Ok(Evaluation { fill: Some(FillAttempt { price, qty: q }), triggered })
}

/// Expires every open order with a time in force for which `due` holds.
fn expire(orders: &mut [Order], open: &mut OpenOrders, mut due: impl FnMut(&Order) -> bool) {
    if open.timed.is_empty() {
        return;
    }
    let timed = std::mem::take(&mut open.timed);
    let mut keep = Vec::with_capacity(timed.len());
    for k in timed {
        if !orders[k].state.is_open() {
            continue;
        }
        if due(&orders[k]) {
            orders[k].state = OrderState::Expired;
            open.remove(&orders[k], k);
        } else {
            keep.push(k);
        }
    }
    open.timed = keep;
}

fn find_bar(group: &[Bar], id: InstrumentId) -> Option<&Bar> {
    group.iter().find(|b| b.instrument == id)
}

/// Stable numeric code of a rejection, as hashed into result fingerprints
/// (0 = none; codes 1-4 match the C++ `Status` enum).
const fn reject_code(r: Option<Reject>) -> u64 {
    match r {
        None => 0,
        Some(Reject::InvalidAmount) => 1,
        Some(Reject::InvalidFill) => 2,
        Some(Reject::InsufficientCash) => 3,
        Some(Reject::ShortNotAllowed) => 4,
        Some(Reject::LeverageExceeded) => 5,
    }
}

fn hash_fill(h: &mut Sha256, f: &Fill) {
    h.update_u64(u64::from(f.instrument));
    h.update_u64(u64::from(f.side == Side::Sell));
    h.update_i64(f.qty.raw());
    h.update_i64(f.price.raw());
    h.update_i64(f.fee.raw());
    h.update_i64(f.ts_ns);
}

/// The canonical text form of a configuration. Without a leverage limit it
/// is byte-identical to the C++ engine's.
pub fn canonical_config(c: &BacktestConfig) -> String {
    let mut s = String::from("quantforge-config-v1;");
    let mut add = |key: &str, v: i128| {
        s.push_str(&format!("{key}={v};"));
    };
    add("initial_cash", i128::from(c.initial_cash.raw()));
    add("allow_short", i128::from(c.ledger.allow_short));
    add("allow_negative_cash", i128::from(c.ledger.allow_negative_cash));
    if let Some(l) = c.ledger.max_gross_leverage {
        add("max_gross_leverage", i128::from(l.raw()));
    }
    add("commission_per_unit", i128::from(c.costs.commission.per_unit.raw()));
    add("commission_bps", i128::from(c.costs.commission.bps_of_notional.raw()));
    add("commission_min", i128::from(c.costs.commission.min_fee.raw()));
    add("slippage_bps", i128::from(c.costs.slippage.raw()));
    add("max_participation", i128::from(c.exec.max_participation.raw()));
    add("delay_bars", c.exec.delay_bars as i128);
    add("seed", i128::from(c.seed as i64)); // the C++ engine printed the seed as a signed long long
    s.push_str(&format!("periods_per_year={};", fmt_g17(c.periods_per_year)));
    s.push_str(&format!("risk_free_rate={};", fmt_g17(c.risk_free_rate)));
    // Written only when set, so configurations without financing keep their fingerprints.
    let f = &c.financing;
    for (key, v) in
        [("borrow_rate", f.borrow_rate), ("cash_rate", f.cash_rate), ("margin_rate", f.margin_rate)]
    {
        if v != 0.0 {
            s.push_str(&format!("{key}={};", fmt_g17(v)));
        }
    }
    if c.impact.coefficient != 0.0 {
        s.push_str(&format!("impact_coefficient={};", fmt_g17(c.impact.coefficient)));
        s.push_str(&format!("impact_window={};", c.impact.window));
    }
    s
}

/// SHA-256 of the canonical configuration (recorded as `config_sha256`).
pub fn config_fingerprint(config: &BacktestConfig) -> String {
    spectra_core::sha256_hex(canonical_config(config).as_bytes())
}

fn hash_result(curve: &[EquityPoint], journal: &[JournalEntry], orders: &[Order]) -> String {
    let mut h = Sha256::new();
    h.update_u64(curve.len() as u64);
    for p in curve {
        h.update_i64(p.ts_ns);
        h.update_i64(p.equity.raw());
        h.update_i64(p.cash.raw());
    }
    h.update_u64(journal.len() as u64);
    for e in journal {
        h.update_u64(e.seq);
        h.update_u64(match e.kind {
            spectra_core::EntryType::Deposit => 0,
            spectra_core::EntryType::Withdrawal => 1,
            spectra_core::EntryType::Fill => 2,
            spectra_core::EntryType::Interest => 3,
            spectra_core::EntryType::BorrowFee => 4,
        });
        h.update_i64(e.ts_ns);
        h.update_i64(e.amount.raw());
        hash_fill(&mut h, &e.fill);
        h.update_i64(e.realized.raw());
        h.update_i64(e.cash_after.raw());
    }
    h.update_u64(orders.len() as u64);
    for o in orders {
        h.update_u64(o.id);
        h.update_u64(u64::from(o.instrument));
        h.update_u64(u64::from(o.side == Side::Sell));
        h.update_u64(o.kind.code());
        h.update_i64(o.qty.raw());
        h.update_i64(o.remaining.raw());
        h.update_i64(o.limit.raw());
        h.update_i64(o.submit_ts);
        h.update_i64(o.submit_step);
        h.update_u64(match o.state {
            OrderState::Pending => 0,
            OrderState::PartiallyFilled => 1,
            OrderState::Filled => 2,
            OrderState::Cancelled => 3,
            OrderState::Rejected => 4,
            OrderState::Expired => 5,
        });
        h.update_u64(reject_code(o.reject_reason));
        // Hashed only for orders that use them, so earlier runs keep their fingerprints.
        if matches!(o.kind, OrderType::Stop | OrderType::StopLimit) || o.tif != TimeInForce::GoodTillCancelled
        {
            h.update_i64(o.stop.raw());
            h.update_u64(u64::from(o.triggered));
            match o.tif {
                TimeInForce::GoodTillCancelled => h.update_u64(0),
                TimeInForce::Day => h.update_u64(1),
                TimeInForce::GoodTillDate(t) => {
                    h.update_u64(2);
                    h.update_i64(t);
                }
            }
        }
    }
    h.hex_digest()
}

/// Refuses data whose instruments have different median bar intervals (see
/// the module documentation). Instruments with fewer than two bars are ignored.
fn check_uniform_intervals(bars: &[Bar]) -> Result<()> {
    let mut last: BTreeMap<InstrumentId, i64> = BTreeMap::new();
    let mut deltas: BTreeMap<InstrumentId, Vec<i64>> = BTreeMap::new();
    for b in bars {
        if let Some(prev) = last.insert(b.instrument, b.ts_ns) {
            deltas.entry(b.instrument).or_default().push(b.ts_ns - prev);
        }
    }
    let mut medians = deltas.into_iter().map(|(id, mut d)| {
        let mid = d.len() / 2;
        (id, *d.select_nth_unstable(mid).1)
    });
    if let Some((first_id, first)) = medians.next() {
        if let Some((id, m)) = medians.find(|&(_, m)| m != first) {
            return Err(Error::InvalidArgument(format!(
                "instruments have different bar intervals (instrument {first_id}: {first} ns, instrument {id}: \
                 {m} ns). Bars are stamped at their start, so mixing timeframes would let a strategy see a longer \
                 bar's close before trading the shorter bars inside it; resample to one interval first"
            )));
        }
    }
    Ok(())
}

fn ledger_err(e: LedgerError) -> Error {
    e.into()
}

/// Size and time limits for one run. `None` means unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunLimits {
    /// Maximum number of bars (all instruments together).
    pub max_bars: Option<usize>,
    /// Maximum number of distinct instruments.
    pub max_instruments: Option<usize>,
    /// Maximum wall-clock time for the run, checked every few timestamps.
    pub max_wall_time: Option<Duration>,
}

/// How far a run has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bars processed so far (all instruments).
    pub bars_done: usize,
    /// Bars in the dataset.
    pub bars_total: usize,
    /// Timestamps processed so far.
    pub timestamps_done: usize,
}

impl Progress {
    /// Share of bars processed, in `[0, 1]`.
    pub fn fraction(&self) -> f64 {
        if self.bars_total == 0 {
            1.0
        } else {
            self.bars_done as f64 / self.bars_total as f64
        }
    }
}

/// Optional controls for [`run_backtest_controlled`]: limits, a cancel flag
/// and a progress callback. None of them changes the simulation, so a run that
/// completes has the same fingerprints as [`run_backtest`].
#[derive(Default)]
pub struct RunControl<'a> {
    /// Size and time limits.
    pub limits: RunLimits,
    /// Checked before every timestamp; when set the run stops with
    /// [`RunError::Cancelled`].
    pub cancel: Option<&'a AtomicBool>,
    /// Called every `progress_every` timestamps and once at the end.
    pub progress: Option<&'a mut dyn FnMut(Progress)>,
    /// Timestamps between progress calls (0 is treated as 1).
    pub progress_every: usize,
}

impl fmt::Debug for RunControl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunControl")
            .field("limits", &self.limits)
            .field("cancel", &self.cancel.map(|c| c.load(Ordering::Relaxed)))
            .field("progress", &self.progress.is_some())
            .field("progress_every", &self.progress_every)
            .finish()
    }
}

/// Why a controlled run stopped without a result.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// The cancel flag was set.
    Cancelled(Progress),
    /// The run took longer than [`RunLimits::max_wall_time`].
    TimedOut(Progress),
    /// The dataset is larger than [`RunLimits`] allow; nothing was simulated.
    TooLarge(String),
    /// The engine refused the input or failed (same errors as [`run_backtest`]).
    Engine(Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(p) => write!(f, "run cancelled after {} of {} bars", p.bars_done, p.bars_total),
            Self::TimedOut(p) => {
                write!(f, "run exceeded its time limit after {} of {} bars", p.bars_done, p.bars_total)
            }
            Self::TooLarge(s) => f.write_str(s),
            Self::Engine(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<Error> for RunError {
    fn from(e: Error) -> Self {
        Self::Engine(e)
    }
}

impl From<spectra_core::ArithmeticError> for RunError {
    fn from(e: spectra_core::ArithmeticError) -> Self {
        Self::Engine(e.into())
    }
}

/// How often (in timestamps) the wall clock is read when a time limit is set.
const CLOCK_CHECK_EVERY: usize = 256;

/// Fixed-point scale of per-period financing rates (1e-12 resolution).
const RATE_SCALE: i128 = 1_000_000_000_000;

/// An annual rate as a per-period rate scaled by [`RATE_SCALE`].
fn per_period_rate(annual: f64, periods_per_year: f64) -> i128 {
    (annual / periods_per_year * RATE_SCALE as f64).round() as i128
}

/// `amount * rate / RATE_SCALE`, rounded half away from zero.
fn apply_rate(amount: Money, rate: i128) -> Result<Money> {
    let raw = spectra_core::fixed::div_round_half_away(i128::from(amount.raw()) * rate, RATE_SCALE);
    Ok(Money::from_raw(spectra_core::fixed::narrow(raw)?))
}

/// Charges one period of financing on the books as they stand (positions at
/// their latest marks, i.e. the previous close).
fn charge_financing(ledger: &mut Ledger, rates: [i128; 3], ts: i64) -> Result<()> {
    let [borrow, cash_rate, margin] = rates;
    if borrow != 0 {
        let shorts: Vec<InstrumentId> =
            ledger.positions().iter().filter(|(_, p)| p.qty.raw() < 0).map(|(&id, _)| id).collect();
        for id in shorts {
            let value = ledger.position_value(id)?.checked_abs()?;
            let fee = apply_rate(value, borrow)?;
            if fee.raw() > 0 {
                ledger.charge_borrow_fee(id, fee, ts).map_err(ledger_err)?;
            }
        }
    }
    let cash = ledger.cash();
    let interest = match cash.raw().cmp(&0) {
        std::cmp::Ordering::Greater => apply_rate(cash, cash_rate)?,
        std::cmp::Ordering::Less => apply_rate(cash, margin)?, // negative cash, so negative interest: a charge
        std::cmp::Ordering::Equal => Money::ZERO,
    };
    if !interest.is_zero() {
        ledger.apply_interest(interest, ts).map_err(ledger_err)?;
    }
    Ok(())
}

fn check_size(bars: &[Bar], limits: &RunLimits) -> std::result::Result<(), RunError> {
    if let Some(max) = limits.max_bars {
        if bars.len() > max {
            return Err(RunError::TooLarge(format!(
                "dataset has {} bars; this workspace allows at most {max} per run",
                bars.len()
            )));
        }
    }
    if let Some(max) = limits.max_instruments {
        let n = bars.iter().map(|b| b.instrument).collect::<BTreeSet<_>>().len();
        if n > max {
            return Err(RunError::TooLarge(format!(
                "dataset has {n} instruments; this workspace allows at most {max} per run"
            )));
        }
    }
    Ok(())
}

/// Runs a backtest.
///
/// # Errors
/// [`Error::InvalidArgument`] for invalid bars or configuration and
/// [`Error::Overflow`] on arithmetic overflow.
pub fn run_backtest(
    bars: &[Bar],
    strategy: &mut dyn Strategy,
    cfg: &BacktestConfig,
) -> Result<BacktestResult> {
    run_backtest_controlled(bars, strategy, cfg, &mut RunControl::default()).map_err(|e| match e {
        RunError::Engine(e) => e,
        // Unreachable without limits or a cancel flag; kept total rather than panicking.
        other => Error::Internal(other.to_string()),
    })
}

/// [`run_backtest`] with limits, cancellation and progress reporting. A run
/// that completes is identical to [`run_backtest`]'s, fingerprints included.
///
/// # Errors
/// [`RunError::TooLarge`] before simulating anything, [`RunError::Cancelled`]
/// or [`RunError::TimedOut`] part-way, and [`RunError::Engine`] for the errors
/// [`run_backtest`] returns.
pub fn run_backtest_controlled(
    bars: &[Bar],
    strategy: &mut dyn Strategy,
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    let started = Instant::now();
    check_size(bars, &ctl.limits)?;
    if let Some(err) = validate_bars(bars) {
        return Err(Error::InvalidArgument(format!("invalid bars: {err}")).into());
    }
    if cfg.initial_cash.raw() < 0 {
        return Err(Error::InvalidArgument("initial_cash must be >= 0".into()).into());
    }
    if cfg.periods_per_year <= 0.0 || cfg.periods_per_year.is_nan() {
        return Err(Error::InvalidArgument("periods_per_year must be > 0".into()).into());
    }
    let fin = &cfg.financing;
    for (name, v) in
        [("borrow_rate", fin.borrow_rate), ("cash_rate", fin.cash_rate), ("margin_rate", fin.margin_rate)]
    {
        if !v.is_finite() || v.abs() > 10.0 {
            return Err(
                Error::InvalidArgument(format!("financing {name} must be a finite annual rate")).into()
            );
        }
    }
    let imp = &cfg.impact;
    if !imp.coefficient.is_finite() || imp.coefficient < 0.0 || imp.coefficient > 100.0 {
        return Err(Error::InvalidArgument("impact coefficient must be between 0 and 100".into()).into());
    }
    if imp.coefficient > 0.0 && imp.window < 2 {
        return Err(Error::InvalidArgument("impact window must be at least 2 bars".into()).into());
    }
    let rates = [
        per_period_rate(fin.borrow_rate, cfg.periods_per_year),
        per_period_rate(fin.cash_rate, cfg.periods_per_year),
        per_period_rate(fin.margin_rate, cfg.periods_per_year),
    ];
    let financed = !fin.is_zero();
    check_uniform_intervals(bars)?;
    let progress_every = ctl.progress_every.max(1);

    let mut ledger = Ledger::new(cfg.ledger);
    let mut history: BTreeMap<InstrumentId, Vec<Bar>> = BTreeMap::new();
    let mut orders: Vec<Order> = Vec::new();
    let mut open = OpenOrders::default();
    let mut curve: Vec<EquityPoint> = Vec::with_capacity(bars.len());
    let mut exposure: Vec<Money> = Vec::with_capacity(bars.len());

    let first_ts = bars.first().map_or(0, |b| b.ts_ns);
    if cfg.initial_cash.raw() > 0 && ledger.deposit(cfg.initial_cash, first_ts).is_err() {
        return Err(Error::InvalidArgument("initial deposit rejected".into()).into());
    }

    // Orders placed in on_start are stamped just before the first bar so they can fill on it.
    {
        let mut ctx = Context {
            ledger: &ledger,
            history: &history,
            orders: &mut orders,
            open: &mut open,
            now: if first_ts > i64::MIN { first_ts - 1 } else { first_ts },
            step: -1,
            seed: cfg.seed,
        };
        strategy.on_start(&mut ctx);
    }

    let min_wait = 1 + cfg.exec.delay_bars as i64;
    let mut i = 0usize;
    let mut step: i64 = 0;
    while i < bars.len() {
        // Progress first, so a cancel requested from the callback takes effect at once.
        let done = Progress { bars_done: i, bars_total: bars.len(), timestamps_done: step as usize };
        if done.timestamps_done > 0 && done.timestamps_done.is_multiple_of(progress_every) {
            if let Some(cb) = ctl.progress.as_mut() {
                cb(done);
            }
        }
        if ctl.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(RunError::Cancelled(done));
        }
        if let Some(max) = ctl.limits.max_wall_time {
            if done.timestamps_done.is_multiple_of(CLOCK_CHECK_EVERY) && started.elapsed() > max {
                return Err(RunError::TimedOut(done));
            }
        }
        let mut j = i;
        while j < bars.len() && bars[j].ts_ns == bars[i].ts_ns {
            j += 1;
        }
        let group = &bars[i..j];
        let ts = bars[i].ts_ns;
        for b in group {
            history.entry(b.instrument).or_default().push(*b);
        }

        // 0. Financing for the period that just ended (none before the first bar).
        if financed && step > 0 {
            charge_financing(&mut ledger, rates, ts)?;
        }

        // Good-till-date orders past their date expire before this timestamp's fills.
        expire(&mut orders, &mut open, |o| matches!(o.tif, TimeInForce::GoodTillDate(t) if ts > t));

        // 1. Fill orders submitted at earlier timestamps, in submission order.
        let mut fills: Vec<Fill> = Vec::new();
        for k in open.candidates(group, &orders) {
            if !orders[k].state.is_open() || step - orders[k].submit_step < min_wait {
                continue;
            }
            let Some(bar) = find_bar(group, orders[k].instrument) else { continue };
            let past = history.get(&orders[k].instrument).map_or(&[][..], Vec::as_slice);
            let ev = evaluate(&orders[k], bar, past, cfg)?;
            if ev.triggered && !orders[k].triggered {
                // The stop was reached: from now on a stop is a market order and a
                // stop-limit a resting limit order.
                open.remove(&orders[k], k);
                orders[k].triggered = true;
                open.insert(&orders[k], k);
            }
            let Some(a) = ev.fill else { continue };
            let o = &mut orders[k];
            let f = Fill {
                instrument: o.instrument,
                side: o.side,
                qty: a.qty,
                price: a.price,
                fee: commission_fee(&cfg.costs.commission, a.qty, notional(a.price, a.qty)?)?,
                ts_ns: ts,
            };
            match ledger.apply_fill(&f) {
                Ok(()) => {
                    o.remaining = o.remaining.checked_sub(a.qty)?;
                    o.state =
                        if o.remaining.is_zero() { OrderState::Filled } else { OrderState::PartiallyFilled };
                    fills.push(f);
                }
                Err(LedgerError::Rejected(r)) => {
                    o.state = OrderState::Rejected;
                    o.reject_reason = Some(r);
                }
                Err(e) => return Err(ledger_err(e).into()),
            }
            if !orders[k].state.is_open() {
                open.remove(&orders[k], k);
            }
        }

        // Day orders expire once they have had their one bar.
        expire(&mut orders, &mut open, |o| {
            o.tif == TimeInForce::Day
                && step - o.submit_step >= min_wait
                && find_bar(group, o.instrument).is_some()
        });

        // 2. Mark to the close and record equity.
        for b in group {
            if ledger.mark(b.instrument, b.close).is_err() {
                return Err(Error::Internal("validated bar produced an invalid mark".into()).into());
            }
        }
        curve.push(EquityPoint { ts_ns: ts, equity: ledger.equity()?, cash: ledger.cash() });
        let mut gross = Money::ZERO;
        for (&id, p) in ledger.positions() {
            if !p.qty.is_zero() {
                gross = gross.checked_add(ledger.position_value(id)?.checked_abs()?)?;
            }
        }
        exposure.push(gross);

        // 3. Let the strategy react. Anything it submits fills at a later timestamp.
        let mut ctx = Context {
            ledger: &ledger,
            history: &history,
            orders: &mut orders,
            open: &mut open,
            now: ts,
            step,
            seed: cfg.seed,
        };
        for f in &fills {
            strategy.on_fill(&mut ctx, f);
        }
        strategy.on_bar(&mut ctx, group);
        open.market.retain(|&k| orders[k].state.is_open());
        i = j;
        step += 1;
    }

    if let Some(cb) = ctl.progress.as_mut() {
        cb(Progress { bars_done: bars.len(), bars_total: bars.len(), timestamps_done: step as usize });
    }
    let metrics = compute_metrics(&curve, cfg.periods_per_year, cfg.risk_free_rate);
    let result_sha256 = hash_result(&curve, ledger.journal(), &orders);
    Ok(BacktestResult {
        equity_curve: curve,
        gross_exposure: exposure,
        orders,
        ledger,
        metrics,
        run: RunRecord {
            engine_version: ENGINE_VERSION.into(),
            dataset_sha256: bars_sha256(bars),
            config_sha256: config_fingerprint(cfg),
            result_sha256,
            seed: cfg.seed,
            bar_count: bars.len(),
        },
    })
}
