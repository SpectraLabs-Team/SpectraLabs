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
//!
//! Several strategies can share one account ([`run_multi_backtest`]): each
//! gets a share of the starting cash and keeps its own books, every order
//! records the strategy that placed it, and the account is the sum of the
//! strategies' books.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spectra_core::cfmt::fmt_g17;
use spectra_core::{
    bars_sha256, notional, validate_bars, Bar, Error, Fill, Instrument, InstrumentId, InstrumentType,
    JournalEntry, Ledger, LedgerConfig, LedgerError, Money, Price, Qty, Ratio, Reject, Result, Sha256, Side,
    ENGINE_VERSION, SCALE,
};

use crate::futures::{Chains, Continuous};
use crate::instruments::InstrumentSetup;
use crate::swaps::CurveHistory;

use crate::corporate::{
    apply_order, corporate_actions_sha256, split_price, split_qty, CorporateAction, CorporateActionKind,
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
    /// Index of the strategy that placed it (always 0 in a single-strategy run).
    pub strategy: usize,
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
    /// Optional exchange calendar: when set, orders only fill when the market is open: inside a
    /// session for intraday bars, on trading days for daily bars (see `SessionRule`).
    pub exchange: Option<spectra_data::calendar::Exchange>,
    /// Optional pre-trade risk limits (M8). When None, limits enforcement is inactive.
    pub risk_limits: Option<spectra_risk::enforcement::RiskLimits>,
    /// Emergency kill switch (M8). Blocks opening orders, allowing closing orders to flatten.
    pub kill_switch: bool,
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
            exchange: None,
            risk_limits: None,
            kill_switch: false,
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
    /// The order behind each `Fill` entry of the journal, in journal order.
    /// Not part of the result fingerprint.
    pub fill_orders: Vec<OrderId>,
    /// Each strategy's own books in a multi-strategy run, in the order given
    /// to [`run_multi_backtest`]; empty for a single-strategy run.
    pub strategies: Vec<StrategyResult>,
    /// Starting cash not allocated to any strategy (a multi-strategy run whose
    /// weights add up to less than 1), with the interest it earned.
    pub unallocated: Option<Ledger>,
    /// Futures rolls, closes and settlements the engine made (M24.2), in order.
    /// Their fills are in the journal; the events say why.
    pub rolls: Vec<RollEvent>,
    /// Each futures chain's continuous, back-adjusted series as it stood at the end.
    pub continuous: BTreeMap<String, Vec<Bar>>,
}

/// What the engine did with a futures position at its roll date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollKind {
    /// Closed and the same position opened in the next contract.
    Rolled,
    /// Closed without reopening (see the event's note).
    Closed,
    /// Still open at expiry with no price to trade at: settled at its last mark.
    Settled,
    /// A bond repaid at par at maturity.
    Redeemed,
    /// An option in the money at expiry, cash-settled at its intrinsic value.
    Exercised,
    /// An option out of the money at expiry, closed at zero.
    Expired,
}

impl RollKind {
    /// Name used in output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rolled => "rolled",
            Self::Closed => "closed",
            Self::Settled => "settled",
            Self::Redeemed => "redeemed",
            Self::Exercised => "exercised",
            Self::Expired => "expired",
        }
    }
}

/// One roll, close or settlement of a futures position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollEvent {
    /// When it happened.
    pub ts_ns: i64,
    /// The strategy whose position it was (0 in a single-strategy run).
    pub strategy: usize,
    /// The contract closed.
    pub from: InstrumentId,
    /// The contract opened, when the position was rolled.
    pub to: Option<InstrumentId>,
    /// The position closed (signed: short < 0).
    pub qty: Qty,
    /// Price the old contract was closed at (base currency).
    pub close_price: Price,
    /// Price the new contract was opened at (base currency).
    pub open_price: Option<Price>,
    /// What happened.
    pub kind: RollKind,
    /// Why a position was not reopened, when it was not.
    pub note: Option<String>,
}

/// One strategy's part of a multi-strategy run.
#[derive(Debug, Clone)]
pub struct StrategyResult {
    /// Its weight: the share of the starting cash it was given.
    pub weight: Ratio,
    /// The starting cash it was given.
    pub allocation: Money,
    /// Its equity (its cash plus its marked positions) at each timestamp.
    pub equity_curve: Vec<EquityPoint>,
    /// Its gross exposure at each timestamp.
    pub gross_exposure: Vec<Money>,
    /// Its own books: its allocation, its fills and its financing.
    pub ledger: Ledger,
    /// Metrics of its own equity curve.
    pub metrics: Metrics,
}

/// A strategy and its share of the starting cash, for [`run_multi_backtest`].
pub struct Allocation<'a> {
    /// The strategy.
    pub strategy: &'a mut dyn Strategy,
    /// Share of the starting cash, in (0, 1]. The weights of a run add up to at
    /// most 1; whatever is left stays in the account as unallocated cash.
    pub weight: f64,
}

impl fmt::Debug for Allocation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allocation").field("weight", &self.weight).finish_non_exhaustive()
    }
}

/// Most strategies in one run.
pub const MAX_STRATEGIES: usize = 32;

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
            .map(|e| {
                self.ledger
                    .notional_of(e.fill.instrument, e.fill.price, e.fill.qty)
                    .map_or(0.0, Money::to_f64)
            })
            .sum();
        let equity: Vec<f64> = self.equity_curve.iter().map(|p| p.equity.to_f64()).collect();
        annualized_turnover(traded, &equity, periods_per_year)
    }
}

/// Annualized turnover: `traded notional / mean equity × periods_per_year / n`, where `n` is the
/// number of periods (`equity.len() - 1`). 0 with fewer than two equity points or a non-positive
/// mean equity.
pub fn annualized_turnover(traded_notional: f64, equity: &[f64], periods_per_year: f64) -> f64 {
    let n = equity.len();
    if n < 2 {
        return 0.0;
    }
    let mean_eq = equity.iter().sum::<f64>() / n as f64;
    if mean_eq <= 0.0 {
        return 0.0;
    }
    traded_notional / mean_eq * periods_per_year / (n - 1) as f64
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
    /// Called for each corporate action applied at the current timestamp (the
    /// books and open orders are already adjusted), before `on_fill`.
    fn on_corporate_action(&mut self, _ctx: &mut Context<'_>, _action: &CorporateAction) {}
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

/// The strategy's window onto the simulation. In a multi-strategy run,
/// positions, cash and equity are the strategy's own books, not the account's.
#[derive(Debug)]
pub struct Context<'a> {
    book: &'a Ledger,
    account: &'a Ledger,
    history: &'a BTreeMap<InstrumentId, Vec<Bar>>,
    orders: &'a mut Vec<Order>,
    open: &'a mut OpenOrders,
    now: i64,
    step: i64,
    seed: u64,
    strategy: usize,
    setup: &'a InstrumentSetup,
    chains: &'a Chains,
    continuous: &'a Continuous,
}

impl Context<'_> {
    /// False only for a future that is not its chain's front contract now
    /// (it has passed its roll date, or an earlier contract has not).
    /// Strategies enter positions in front contracts only.
    pub fn is_front(&self, instrument: InstrumentId) -> bool {
        self.chains.is_front(instrument, self.now)
    }
    /// The front contract of futures chain `chain` now.
    pub fn front_contract(&self, chain: &str) -> Option<InstrumentId> {
        self.chains.front(chain, self.now)
    }
    /// The futures chain `instrument` belongs to, if it is a future.
    pub fn chain(&self, instrument: InstrumentId) -> Option<&str> {
        self.chains.chain(instrument)
    }
    /// Bars to compute signals on: a future's chain as one continuous,
    /// back-adjusted series (rolls up to now only); any other instrument's own
    /// history, exactly as [`Self::history`].
    pub fn signal_history(&self, instrument: InstrumentId) -> &[Bar] {
        match self.chains.chain(instrument) {
            Some(chain) => self.continuous.series(chain),
            None => self.history(instrument),
        }
    }
    /// The run's record of `instrument`, if it has one (none means a plain
    /// equity in the base currency).
    pub fn instrument(&self, instrument: InstrumentId) -> Option<&Instrument> {
        self.setup.instrument(instrument)
    }
    /// The currency the books are kept in.
    pub fn base_currency(&self) -> &str {
        &self.setup.base_currency
    }
    /// Contract multiplier of `instrument` (1 for equities).
    pub fn multiplier(&self, instrument: InstrumentId) -> Ratio {
        self.book.multiplier(instrument)
    }
    /// Base-currency value of one unit of `instrument` at quoted `price`: the
    /// price converted at the FX rate in force now, times the multiplier.
    /// For a plain equity in the base currency it is exactly `price`.
    pub fn unit_value(&self, instrument: InstrumentId, price: Price) -> Option<f64> {
        let base = self.setup.to_base(instrument, price, self.now).ok()?;
        let m = self.book.multiplier(instrument);
        if m == Ratio::ONE {
            Some(base.to_f64())
        } else {
            Some(base.to_f64() * m.to_f64())
        }
    }
    /// The smallest tradable quantity of `instrument`, when a record that
    /// changes the run sets one (a plain equity record does not).
    pub fn lot_size(&self, instrument: InstrumentId) -> Option<Qty> {
        self.setup
            .instrument(instrument)
            .filter(|i| !i.is_plain_equity(&self.setup.base_currency))
            .map(|i| i.lot_size)
    }
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
        self.book.position(instrument).qty
    }
    /// Current cash.
    pub fn cash(&self) -> Money {
        self.book.cash()
    }
    /// Current equity (cash plus marked positions); zero if it overflows.
    pub fn equity(&self) -> Money {
        self.book.equity().unwrap_or_default()
    }
    /// Equity of the whole account, all strategies together (the same as
    /// [`Self::equity`] in a single-strategy run).
    pub fn account_equity(&self) -> Money {
        self.account.equity().unwrap_or_default()
    }
    /// Index of this strategy in the run (0 in a single-strategy run).
    pub fn strategy(&self) -> usize {
        self.strategy
    }
    /// The orders so far, of every strategy (see [`Order::strategy`]).
    pub fn orders(&self) -> &[Order] {
        self.orders
    }
    /// The ledger policy of this run (shorting, negative cash, leverage limit).
    /// In a multi-strategy run each strategy is held to it on its own books.
    pub fn ledger_policy(&self) -> LedgerConfig {
        *self.book.config()
    }
    /// Marked value of a position (negative for shorts): what it adds to
    /// equity. For a future that is its unrealized P&L; see [`Self::position_exposure`].
    pub fn position_value(&self, instrument: InstrumentId) -> Money {
        self.book.position_value(instrument).unwrap_or_default()
    }
    /// Signed notional a position controls at its mark (the same as
    /// [`Self::position_value`] for anything but a future).
    pub fn position_exposure(&self, instrument: InstrumentId) -> Money {
        self.book.position_exposure(instrument).unwrap_or_default()
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
            strategy: self.strategy,
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
    /// Cancel an open order of this strategy; true if it was open and is now Cancelled.
    pub fn cancel(&mut self, id: OrderId) -> bool {
        if id == 0 || id > self.orders.len() as OrderId {
            return false;
        }
        let index = (id - 1) as usize;
        if !self.orders[index].state.is_open() || self.orders[index].strategy != self.strategy {
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

/// A market-type fill price: `base` moved against the trader by flat slippage,
/// spread, volume-based slippage, plus any impact.
fn market_price(
    base: Price,
    side: Side,
    bar: &Bar,
    qty: Qty,
    cfg: &BacktestConfig,
    impact: f64,
) -> Result<Price> {
    let bps = crate::costs::effective_slippage_bps(&cfg.costs, qty, bar.volume, impact)?;
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

/// `model_priced`: the bar was generated from a model (a swap), so it has no
/// traded volume to cap participation against.
fn evaluate(
    o: &Order,
    bar: &Bar,
    history: &[Bar],
    cfg: &BacktestConfig,
    model_priced: bool,
) -> Result<Evaluation> {
    let none = |triggered| Evaluation { fill: None, triggered };
    let mut q = o.remaining;
    if cfg.exec.max_participation.raw() > 0 && !model_priced {
        let cap =
            i128::from(bar.volume.raw()) * i128::from(cfg.exec.max_participation.raw()) / i128::from(SCALE);
        q = q.min(Qty::from_raw(spectra_core::fixed::narrow(cap)?));
    }
    let mut triggered = false;
    let price = match o.kind {
        OrderType::Market => {
            Some(market_price(bar.open, o.side, bar, q, cfg, impact_bps(&cfg.impact, history, bar, q))?)
        }
        OrderType::Limit => limit_price(o.side, o.limit, bar),
        OrderType::StopLimit if o.triggered => limit_price(o.side, o.limit, bar),
        // A stop whose stop was reached on an earlier bar (a partial fill) is a market order now.
        OrderType::Stop if o.triggered => {
            Some(market_price(bar.open, o.side, bar, q, cfg, impact_bps(&cfg.impact, history, bar, q))?)
        }
        OrderType::Stop => match stop_trigger(o.side, o.stop, bar) {
            Some(base) => {
                triggered = true;
                Some(market_price(base, o.side, bar, q, cfg, impact_bps(&cfg.impact, history, bar, q))?)
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
        Some(Reject::RiskLimitExceeded) => 6,
        Some(Reject::ContractExpired) => 7,
        Some(Reject::ContractRolled) => 8,
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
    if c.costs.spread.raw() > 0 {
        add("spread_bps", i128::from(c.costs.spread.raw()));
    }
    if c.costs.volume_slippage.raw() > 0 {
        add("volume_slippage_bps", i128::from(c.costs.volume_slippage.raw()));
    }
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
    if let Some(ex) = c.exchange {
        s.push_str(&format!("exchange={};", ex.code()));
    }
    if let Some(ref r) = c.risk_limits {
        s.push_str(&format!(
            "risk_limits={:.2},{:.2},{:.2},{:.4},{:.2},{:.4},{:.2},{:.4},{};",
            r.max_gross_exposure,
            r.max_net_exposure,
            r.max_position_size,
            r.max_concentration_pct,
            r.max_leverage,
            r.max_drawdown_pct,
            r.max_daily_loss,
            r.max_var_pct,
            r.size_reduction_enabled
        ));
    }
    if c.kill_switch {
        s.push_str("kill_switch=true;");
    }
    s
}

/// SHA-256 of the canonical configuration (recorded as `config_sha256`).
pub fn config_fingerprint(config: &BacktestConfig) -> String {
    spectra_core::sha256_hex(canonical_config(config).as_bytes())
}

/// The canonical text form of a multi-strategy run's configuration: the
/// single-run form followed by the number of strategies and their weights.
pub fn canonical_multi_config(c: &BacktestConfig, weights: &[Ratio]) -> String {
    let mut s = canonical_config(c);
    s.push_str(&format!("strategies={};", weights.len()));
    for (i, w) in weights.iter().enumerate() {
        s.push_str(&format!("weight_{i}={};", w.raw()));
    }
    s
}

fn hash_result(curve: &[EquityPoint], journal: &[JournalEntry], orders: &[Order], multi: bool) -> String {
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
            spectra_core::EntryType::Split => 5,
            spectra_core::EntryType::Dividend => 6,
            spectra_core::EntryType::Listing => 7,
            spectra_core::EntryType::Coupon => 8,
            spectra_core::EntryType::Expire => 9,
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
    if multi {
        // Which strategy placed each order; single-strategy runs keep their fingerprints.
        for o in orders {
            h.update_u64(o.strategy as u64);
        }
    }
    h.hex_digest()
}

/// Refuses data whose instruments have different median bar intervals (see
/// the module documentation). Instruments with fewer than two bars are ignored.
fn check_uniform_intervals(bars: &[Bar]) -> Result<Option<i64>> {
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
    let mut interval = None;
    if let Some((first_id, first)) = medians.next() {
        interval = Some(first);
        if let Some((id, m)) = medians.find(|&(_, m)| m != first) {
            return Err(Error::InvalidArgument(format!(
                "instruments have different bar intervals (instrument {first_id}: {first} ns, instrument {id}: \
                 {m} ns). Bars are stamped at their start, so mixing timeframes would let a strategy see a longer \
                 bar's close before trading the shorter bars inside it; resample to one interval first"
            )));
        }
    }
    Ok(interval)
}

/// When orders may fill under an exchange calendar, by bar interval:
///  * intraday bars (under 20 hours): only inside a session, at the bar's time;
///  * daily bars (20 hours to 4 days): only on trading days, judged by the
///    bar's trading date ([`spectra_data::calendar::Exchange::daily_bar_date`]),
///    since a daily bar's timestamp is rarely inside the session;
///  * weekly or longer bars: always (each spans several sessions).
#[derive(Debug, Clone, Copy)]
enum SessionRule {
    Always,
    Intraday(spectra_data::calendar::Exchange),
    Daily(spectra_data::calendar::Exchange),
}

impl SessionRule {
    const HOURS_20: i64 = 20 * 3_600_000_000_000;
    const DAYS_4: i64 = 4 * 86_400_000_000_000;

    fn new(exchange: Option<spectra_data::calendar::Exchange>, interval: Option<i64>) -> Self {
        match (exchange, interval) {
            (None, _) => Self::Always,
            (Some(ex), Some(i)) if (Self::HOURS_20..=Self::DAYS_4).contains(&i) => Self::Daily(ex),
            (Some(_), Some(i)) if i > Self::DAYS_4 => Self::Always,
            (Some(ex), _) => Self::Intraday(ex),
        }
    }

    fn is_open(self, ts: i64) -> bool {
        match self {
            Self::Always => true,
            Self::Intraday(ex) => ex.is_session_open(ts),
            Self::Daily(ex) => ex.is_trading_day_bar(ts),
        }
    }
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

/// One financing posting.
enum Charge {
    Borrow(InstrumentId, Money),
    Interest(Money),
}

/// One period of financing on the books as they stand (positions at their
/// latest marks, i.e. the previous close): a borrow fee per short position,
/// then interest on the cash left after those fees.
fn financing_charges(ledger: &Ledger, rates: [i128; 3]) -> Result<Vec<Charge>> {
    let [borrow, cash_rate, margin] = rates;
    let mut out = Vec::new();
    let mut cash = ledger.cash();
    if borrow != 0 {
        for (&id, p) in ledger.positions() {
            // No stock is borrowed to short a future.
            if p.qty.raw() >= 0 || ledger.is_marked_to_market(id) {
                continue;
            }
            let fee = apply_rate(ledger.position_value(id)?.checked_abs()?, borrow)?;
            if fee.raw() > 0 {
                cash = cash.checked_sub(fee)?;
                out.push(Charge::Borrow(id, fee));
            }
        }
    }
    let interest = match cash.raw().cmp(&0) {
        std::cmp::Ordering::Greater => apply_rate(cash, cash_rate)?,
        std::cmp::Ordering::Less => apply_rate(cash, margin)?, // negative cash, so negative interest: a charge
        std::cmp::Ordering::Equal => Money::ZERO,
    };
    if !interest.is_zero() {
        out.push(Charge::Interest(interest));
    }
    Ok(out)
}

fn post(ledger: &mut Ledger, c: &Charge, ts: i64) -> Result<()> {
    match *c {
        Charge::Borrow(id, fee) => ledger.charge_borrow_fee(id, fee, ts),
        Charge::Interest(amount) => ledger.apply_interest(amount, ts),
    }
    .map_err(ledger_err)
}

/// Sum of the absolute marked position values.
fn gross_of(ledger: &Ledger) -> Result<Money> {
    let mut gross = Money::ZERO;
    for (&id, p) in ledger.positions() {
        if !p.qty.is_zero() {
            gross = gross.checked_add(ledger.position_exposure(id)?.checked_abs()?)?;
        }
    }
    Ok(gross)
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
    run_inner(bars, &[], &mut [strategy], None, &InstrumentSetup::default(), cfg, ctl)
}

/// [`run_backtest_controlled`] on unadjusted prices with the instruments'
/// corporate actions applied as they go ex (see [`crate::corporate`]). With
/// no actions it is exactly [`run_backtest_controlled`]; with some, their
/// fingerprint is part of `config_sha256`.
///
/// # Errors
/// As [`run_backtest_controlled`], and [`RunError::Engine`] for an invalid
/// action or one on an instrument with no bars.
pub fn run_backtest_with_actions(
    bars: &[Bar],
    actions: &[CorporateAction],
    strategy: &mut dyn Strategy,
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    run_inner(bars, actions, &mut [strategy], None, &InstrumentSetup::default(), cfg, ctl)
}

/// [`run_backtest_with_actions`] with instrument records, a base currency and
/// FX rates (see [`crate::instruments`]). With a trivial setup it is exactly
/// [`run_backtest_with_actions`]; otherwise the setup's fingerprint is part of
/// `config_sha256`.
///
/// # Errors
/// As [`run_backtest_with_actions`], and [`RunError::Engine`] for an invalid
/// setup, an instrument type the engine cannot trade yet, or a missing FX rate.
pub fn run_backtest_with_setup(
    bars: &[Bar],
    actions: &[CorporateAction],
    setup: &InstrumentSetup,
    strategy: &mut dyn Strategy,
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    run_inner(bars, actions, &mut [strategy], None, setup, cfg, ctl)
}

/// Runs several strategies on one shared account. Each strategy gets
/// `weight` of the starting cash and keeps its own books: it sees its own
/// positions, cash and equity, is held to the run's ledger policy (shorting,
/// negative cash, leverage limit) on those books, and is financed on them.
/// The account is the sum of the strategies' books (plus any unallocated
/// cash), and every order records the strategy that placed it.
///
/// At each timestamp the strategies are called in the order given, after the
/// fills of every strategy's orders, which fill in submission order as in a
/// single-strategy run. A fill a strategy cannot afford on its own books is
/// rejected even if the account as a whole could afford it.
///
/// # Errors
/// As [`run_backtest_controlled`], and [`RunError::Engine`] when there are no
/// strategies or more than [`MAX_STRATEGIES`], a weight is not in (0, 1], or
/// the weights add up to more than 1.
pub fn run_multi_backtest(
    bars: &[Bar],
    allocations: &mut [Allocation<'_>],
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    run_multi_backtest_with_actions(bars, &[], allocations, cfg, ctl)
}

/// [`run_multi_backtest`] with corporate actions (see
/// [`run_backtest_with_actions`]). Each strategy's books are adjusted and
/// paid on their own positions; the account carries every change.
///
/// # Errors
/// As [`run_multi_backtest`] and [`run_backtest_with_actions`].
pub fn run_multi_backtest_with_actions(
    bars: &[Bar],
    actions: &[CorporateAction],
    allocations: &mut [Allocation<'_>],
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    run_multi_backtest_with_setup(bars, actions, &InstrumentSetup::default(), allocations, cfg, ctl)
}

/// [`run_multi_backtest_with_actions`] with an instrument setup (see
/// [`run_backtest_with_setup`]).
///
/// # Errors
/// As [`run_multi_backtest_with_actions`] and [`run_backtest_with_setup`].
pub fn run_multi_backtest_with_setup<'a>(
    bars: &[Bar],
    actions: &[CorporateAction],
    setup: &InstrumentSetup,
    allocations: &mut [Allocation<'a>],
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    let invalid = |m: String| RunError::Engine(Error::InvalidArgument(m));
    if allocations.is_empty() || allocations.len() > MAX_STRATEGIES {
        return Err(invalid(format!("a run needs 1 to {MAX_STRATEGIES} strategies")));
    }
    let mut weights = Vec::with_capacity(allocations.len());
    for (i, a) in allocations.iter().enumerate() {
        let w = if a.weight.is_finite() && a.weight > 0.0 && a.weight <= 1.0 {
            Ratio::from_f64(a.weight).ok().filter(|w| w.raw() > 0)
        } else {
            None
        };
        let Some(w) = w else { return Err(invalid(format!("strategy {i}: weight must be in (0, 1]"))) };
        weights.push(w);
    }
    if weights.iter().map(|w| i128::from(w.raw())).sum::<i128>() > i128::from(SCALE) {
        return Err(invalid("strategy weights add up to more than 1".into()));
    }
    let mut strategies: Vec<&mut (dyn Strategy + 'a)> =
        allocations.iter_mut().map(|a| &mut *a.strategy).collect();
    run_inner(bars, actions, &mut strategies, Some(&weights), setup, cfg, ctl)
}

/// Each strategy's own books in a multi-strategy run.
struct Books {
    ledgers: Vec<Ledger>,
    allocations: Vec<Money>,
    curves: Vec<Vec<EquityPoint>>,
    exposure: Vec<Vec<Money>>,
    unallocated: Option<Ledger>,
}

impl Books {
    /// Splits the starting cash by weight (rounding down); with weights adding
    /// up to exactly 1 the last strategy gets the rounding remainder,
    /// otherwise the remainder is unallocated.
    fn new(weights: &[Ratio], cfg: &BacktestConfig, ts: i64, capacity: usize) -> Result<Self> {
        let total = i128::from(cfg.initial_cash.raw());
        let mut allocations = Vec::with_capacity(weights.len());
        let mut left = total;
        for w in weights {
            let raw = total * i128::from(w.raw()) / i128::from(SCALE);
            left -= raw;
            allocations.push(raw);
        }
        let full = weights.iter().map(|w| i128::from(w.raw())).sum::<i128>() == i128::from(SCALE);
        let mut unallocated = None;
        if full {
            if let Some(last) = allocations.last_mut() {
                *last += left;
            }
        } else if left > 0 {
            let mut l = Ledger::new(cfg.ledger);
            l.deposit(Money::from_raw(spectra_core::fixed::narrow(left)?), ts).map_err(ledger_err)?;
            unallocated = Some(l);
        }
        let mut ledgers = Vec::with_capacity(weights.len());
        let mut money = Vec::with_capacity(weights.len());
        for raw in allocations {
            let a = Money::from_raw(spectra_core::fixed::narrow(raw)?);
            let mut l = Ledger::new(cfg.ledger);
            if a.raw() > 0 {
                l.deposit(a, ts).map_err(ledger_err)?;
            }
            ledgers.push(l);
            money.push(a);
        }
        Ok(Self {
            curves: vec![Vec::with_capacity(capacity); ledgers.len()],
            exposure: vec![Vec::with_capacity(capacity); ledgers.len()],
            ledgers,
            allocations: money,
            unallocated,
        })
    }
}

/// Applies one corporate action to the books and open orders.
fn apply_action(
    a: &CorporateAction,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
    orders: &mut [Order],
    open: &mut OpenOrders,
    ts: i64,
) -> std::result::Result<(), RunError> {
    let id = a.instrument;
    match a.kind {
        CorporateActionKind::Split { from, to } => {
            // Each strategy's position first; the account's is their sum, so the books still add up.
            let mut total = Qty::ZERO;
            for book in sleeves.iter_mut() {
                let q = book.position(id).qty;
                if q.is_zero() {
                    book.rescale_mark(id, from, to);
                } else {
                    let nq = split_qty(q, from, to);
                    book.apply_split(id, from, to, nq, ts).map_err(ledger_err)?;
                    total = total.checked_add(nq)?;
                }
            }
            let q = ledger.position(id).qty;
            if q.is_zero() {
                ledger.rescale_mark(id, from, to);
            } else {
                let nq = if sleeves.is_empty() { split_qty(q, from, to) } else { total };
                ledger.apply_split(id, from, to, nq, ts).map_err(ledger_err)?;
            }
            // Open orders, as a broker adjusts them: more shares at a lower price.
            for (k, o) in orders.iter_mut().enumerate() {
                if o.instrument != id || !o.state.is_open() {
                    continue;
                }
                open.remove(o, k);
                o.qty = split_qty(o.qty, from, to);
                o.remaining = split_qty(o.remaining, from, to);
                if !o.limit.is_zero() {
                    o.limit = split_price(o.limit, from, to);
                }
                if !o.stop.is_zero() {
                    o.stop = split_price(o.stop, from, to);
                }
                open.insert(o, k);
            }
        }
        CorporateActionKind::Dividend { per_share } => {
            let pay = |book: &mut Ledger| -> std::result::Result<Option<Money>, RunError> {
                let q = book.position(id).qty;
                if q.is_zero() {
                    return Ok(None);
                }
                let amount = notional(per_share, q)?;
                if amount.is_zero() {
                    return Ok(None);
                }
                book.apply_dividend(id, amount, ts).map_err(ledger_err)?;
                Ok(Some(amount))
            };
            if sleeves.is_empty() {
                pay(ledger)?;
            } else {
                for book in sleeves.iter_mut() {
                    if let Some(amount) = pay(book)? {
                        ledger.apply_dividend(id, amount, ts).map_err(ledger_err)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Checks and orders a run's corporate actions.
fn prepare_actions(actions: &[CorporateAction], bars: &[Bar]) -> Result<Vec<CorporateAction>> {
    if actions.is_empty() {
        return Ok(Vec::new());
    }
    let instruments: BTreeSet<InstrumentId> = bars.iter().map(|b| b.instrument).collect();
    for (i, a) in actions.iter().enumerate() {
        if let Some(p) = a.problem() {
            return Err(Error::InvalidArgument(format!(
                "corporate action {i} (instrument {}): {p}",
                a.instrument
            )));
        }
        if !instruments.contains(&a.instrument) {
            return Err(Error::InvalidArgument(format!(
                "corporate action {i}: instrument {} has no bars in this dataset",
                a.instrument
            )));
        }
    }
    let mut sorted = actions.to_vec();
    sorted.sort_by(apply_order);
    if let Some(w) = sorted.windows(2).find(|w| w[0] == w[1]) {
        return Err(Error::InvalidArgument(format!(
            "corporate action for instrument {} at {} is listed twice",
            w[0].instrument, w[0].ex_ts_ns
        )));
    }
    Ok(sorted)
}

/// The books a strategy trades on: its own in a multi-strategy run, else the account.
fn ledger_for<'l>(account: &'l Ledger, sleeves: &'l [Ledger], strategy: usize) -> &'l Ledger {
    sleeves.get(strategy).unwrap_or(account)
}

/// Applies an engine-made fill to the strategy's books and the account: the
/// strategy must afford it on its own books first, as with any fill. With
/// `keep_exposure` (a roll's reopening leg) the leverage limit is not applied:
/// the books already carried that exposure.
fn apply_engine_fill(
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
    owner: usize,
    f: &Fill,
    keep_exposure: bool,
) -> std::result::Result<(), LedgerError> {
    let apply =
        |l: &mut Ledger| if keep_exposure { l.apply_fill_keeping_exposure(f) } else { l.apply_fill(f) };
    match sleeves.get_mut(owner) {
        Some(book) => {
            if keep_exposure {
                book.check_fill_keeping_exposure(f)?;
            } else {
                book.check_fill(f)?;
            }
            apply(ledger)?;
            apply(book)
        }
        None => apply(ledger),
    }
}

/// What the futures roll needs for one timestamp.
struct RollCtx<'r> {
    chains: &'r Chains,
    setup: &'r InstrumentSetup,
    cfg: &'r BacktestConfig,
    ts: i64,
    group: &'r [Bar],
    session_open: bool,
    rolls: &'r mut Vec<RollEvent>,
    fill_orders: &'r mut Vec<OrderId>,
}

impl RollCtx<'_> {
    /// The fill of `qty` of `id` at this timestamp's open, through the cost model.
    fn trade_at_open(&self, ledger: &Ledger, id: InstrumentId, side: Side, qty: Qty) -> Result<Option<Fill>> {
        if !self.session_open {
            return Ok(None);
        }
        let Some(bar) = find_bar(self.group, id) else { return Ok(None) };
        let price =
            self.setup.to_base(id, market_price(bar.open, side, bar, qty, self.cfg, 0.0)?, self.ts)?;
        let fee = commission_fee(&self.cfg.costs.commission, qty, ledger.notional_of(id, price, qty)?)?;
        Ok(Some(Fill { instrument: id, side, qty, price, fee, ts_ns: self.ts }))
    }

    /// Rolls, closes or settles every futures position past its roll date.
    /// Order id 0 marks the engine's own fills in `fill_orders`.
    fn roll(
        &mut self,
        ledger: &mut Ledger,
        sleeves: &mut [Ledger],
    ) -> std::result::Result<Vec<(usize, Fill)>, RunError> {
        let mut out = Vec::new();
        let contracts: Vec<(InstrumentId, i64)> = self.chains.contracts().collect();
        let owners = sleeves.len().max(1);
        for (id, roll_at) in contracts {
            if self.ts < roll_at {
                continue;
            }
            let expired = self.chains.expiry(id).is_some_and(|e| self.ts >= e);
            for owner in 0..owners {
                let book: &Ledger = sleeves.get(owner).unwrap_or(ledger);
                let qty = book.position(id).qty;
                if qty.is_zero() {
                    continue;
                }
                let abs = qty.checked_abs()?;
                let (close_side, open_side) =
                    if qty.raw() > 0 { (Side::Sell, Side::Buy) } else { (Side::Buy, Side::Sell) };
                let (close, settled) = match self.trade_at_open(ledger, id, close_side, abs)? {
                    Some(f) => (f, false),
                    None if expired => {
                        // No price to trade at, and the contract is gone: final settlement at the last mark.
                        let Some(mark) = book.mark_price(id) else { continue };
                        (
                            Fill {
                                instrument: id,
                                side: close_side,
                                qty: abs,
                                price: mark,
                                fee: Money::ZERO,
                                ts_ns: self.ts,
                            },
                            true,
                        )
                    }
                    None => continue, // no price yet: try at the next timestamp
                };
                let mut event = RollEvent {
                    ts_ns: self.ts,
                    strategy: owner,
                    from: id,
                    to: None,
                    qty,
                    close_price: close.price,
                    open_price: None,
                    kind: if settled { RollKind::Settled } else { RollKind::Closed },
                    note: None,
                };
                match apply_engine_fill(ledger, sleeves, owner, &close, false) {
                    Ok(()) => {
                        self.fill_orders.push(0);
                        out.push((owner, close));
                    }
                    Err(LedgerError::Rejected(r)) => {
                        // A close can only be refused for the fee; keep the position and say why.
                        event.kind = RollKind::Closed;
                        event.note = Some(format!("closing was rejected: {}", r.as_str()));
                        self.rolls.push(event);
                        continue;
                    }
                    Err(e) => return Err(ledger_err(e).into()),
                }
                if settled {
                    event.note = Some("no price before expiry; settled at the last mark".into());
                    self.rolls.push(event);
                    continue;
                }
                let Some(next) = self.chains.next_after(id) else {
                    event.note = Some("no later contract in the chain; the position was closed".into());
                    self.rolls.push(event);
                    continue;
                };
                match self.trade_at_open(ledger, next, open_side, abs)? {
                    None => {
                        event.note =
                            Some("the next contract has no price now; the position was closed".into());
                    }
                    Some(open) => match apply_engine_fill(ledger, sleeves, owner, &open, true) {
                        Ok(()) => {
                            self.fill_orders.push(0);
                            event.kind = RollKind::Rolled;
                            event.to = Some(next);
                            event.open_price = Some(open.price);
                            out.push((owner, open));
                        }
                        Err(LedgerError::Rejected(r)) => {
                            event.note =
                                Some(format!("reopening in the next contract was rejected: {}", r.as_str()));
                        }
                        Err(e) => return Err(ledger_err(e).into()),
                    },
                }
                self.rolls.push(event);
            }
        }
        Ok(out)
    }
}

/// Pays every bond coupon that fell due after `prev_ts` and on or before
/// `ts`, on the positions held at the previous close: each strategy its own,
/// and the account their sum (exactly, so the books still add up).
fn pay_coupons(
    setup: &InstrumentSetup,
    prev_ts: Option<i64>,
    ts: i64,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
) -> std::result::Result<(), RunError> {
    let Some(prev) = prev_ts else { return Ok(()) };
    for &id in setup.instruments.keys() {
        let Some(terms) = setup.bond_terms(id) else { continue };
        for day in terms.coupon_days_between(prev, ts) {
            let Some((start, end)) = terms.period_around(day * 86_400_000_000_000) else { continue };
            // Per 100 face, in the bond's currency, converted at the rate known now.
            let per_unit = setup.fx_convert(id, Price::from_f64(terms.coupon_amount(start, end))?, ts)?;
            if per_unit.is_zero() {
                continue;
            }
            let owed = |book: &Ledger| -> Result<Money> {
                let q = book.position(id).qty;
                if q.is_zero() {
                    return Ok(Money::ZERO);
                }
                Ok(book.notional_of(id, per_unit, q)?)
            };
            if sleeves.is_empty() {
                let amount = owed(ledger)?;
                if !amount.is_zero() {
                    ledger.apply_coupon(id, amount, ts).map_err(ledger_err)?;
                }
                continue;
            }
            let mut total = Money::ZERO;
            for book in sleeves.iter_mut() {
                let amount = owed(book)?;
                if !amount.is_zero() {
                    book.apply_coupon(id, amount, ts).map_err(ledger_err)?;
                    total = total.checked_add(amount)?;
                }
            }
            if !total.is_zero() {
                ledger.apply_coupon(id, total, ts).map_err(ledger_err)?;
            }
        }
    }
    Ok(())
}

/// Settles every option position once its expiry has come (M24.4): in the
/// money, cash-settled at its intrinsic value against the underlying's last
/// close at or before expiry; otherwise closed at zero. European, cash-settled.
#[allow(clippy::too_many_arguments)]
fn settle_options(
    setup: &InstrumentSetup,
    ts: i64,
    history: &BTreeMap<InstrumentId, Vec<Bar>>,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
    rolls: &mut Vec<RollEvent>,
    fill_orders: &mut Vec<OrderId>,
) -> std::result::Result<Vec<(usize, Fill)>, RunError> {
    let mut out = Vec::new();
    let owners = sleeves.len().max(1);
    for (&id, inst) in &setup.instruments {
        if inst.kind != InstrumentType::Option {
            continue;
        }
        let (Some(expiry), Some(strike), Some(right)) = (inst.expiry_ns, inst.strike, inst.option_right)
        else {
            continue;
        };
        if ts < expiry {
            continue;
        }
        let held = (0..owners).any(|o| !sleeves.get(o).unwrap_or(ledger).position(id).qty.is_zero());
        if !held {
            continue;
        }
        // The underlying's last close known at expiry (never a later one).
        let spot = setup
            .underlying_of(id)
            .and_then(|u| history.get(&u))
            .and_then(|h| h.iter().rev().find(|b| b.ts_ns <= expiry))
            .map(|b| b.close);
        let intrinsic = spot.map_or(0.0, |s| right.intrinsic(s.to_f64(), strike.to_f64()));
        let settle_px = if intrinsic > 0.0 {
            Some(setup.fx_convert(id, Price::from_f64(intrinsic)?, ts)?).filter(|p| p.raw() > 0)
        } else {
            None
        };
        let note =
            spot.is_none().then(|| "no underlying price at or before expiry; closed at zero".to_string());
        for owner in 0..owners {
            let qty = sleeves.get(owner).unwrap_or(ledger).position(id).qty;
            if qty.is_zero() {
                continue;
            }
            let abs = qty.checked_abs()?;
            let settled_fill = match settle_px {
                Some(px) if !ledger.notional_of(id, px, abs)?.is_zero() => {
                    let side = if qty.raw() > 0 { Side::Sell } else { Side::Buy };
                    Some(Fill { instrument: id, side, qty: abs, price: px, fee: Money::ZERO, ts_ns: ts })
                }
                _ => None,
            };
            if let Some(f) = settled_fill {
                apply_engine_fill(ledger, sleeves, owner, &f, false).map_err(ledger_err)?;
                fill_orders.push(0);
                out.push((owner, f));
            } else {
                match sleeves.get_mut(owner) {
                    Some(book) => book.expire_position(id, ts).map_err(ledger_err)?,
                    None => ledger.expire_position(id, ts).map_err(ledger_err)?,
                }
            }
            rolls.push(RollEvent {
                ts_ns: ts,
                strategy: owner,
                from: id,
                to: None,
                qty,
                close_price: settled_fill.map_or(Price::ZERO, |f| f.price),
                open_price: None,
                kind: if settled_fill.is_some() { RollKind::Exercised } else { RollKind::Expired },
                note: note.clone(),
            });
        }
        // In a multi-strategy run the account's own position expires once, after its strategies'.
        if !sleeves.is_empty() && !ledger.position(id).qty.is_zero() {
            ledger.expire_position(id, ts).map_err(ledger_err)?;
        }
    }
    Ok(out)
}

/// Repays every bond position at par (100) once its maturity has come.
fn redeem_bonds(
    setup: &InstrumentSetup,
    ts: i64,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
    rolls: &mut Vec<RollEvent>,
    fill_orders: &mut Vec<OrderId>,
) -> std::result::Result<Vec<(usize, Fill)>, RunError> {
    let mut out = Vec::new();
    let owners = sleeves.len().max(1);
    for (&id, inst) in &setup.instruments {
        let Some(maturity) = inst.maturity_ns.filter(|_| inst.kind == InstrumentType::Bond) else { continue };
        if ts < maturity {
            continue;
        }
        // At maturity nothing has accrued: par, converted to the base currency.
        let par = setup.fx_convert(id, Price::from_units(100)?, ts)?;
        for owner in 0..owners {
            let qty = sleeves.get(owner).unwrap_or(ledger).position(id).qty;
            if qty.is_zero() {
                continue;
            }
            let side = if qty.raw() > 0 { Side::Sell } else { Side::Buy };
            let f = Fill {
                instrument: id,
                side,
                qty: qty.checked_abs()?,
                price: par,
                fee: Money::ZERO,
                ts_ns: ts,
            };
            apply_engine_fill(ledger, sleeves, owner, &f, false).map_err(ledger_err)?;
            fill_orders.push(0);
            out.push((owner, f));
            rolls.push(RollEvent {
                ts_ns: ts,
                strategy: owner,
                from: id,
                to: None,
                qty,
                close_price: par,
                open_price: None,
                kind: RollKind::Redeemed,
                note: None,
            });
        }
    }
    Ok(out)
}

/// Pays each swap's net coupon on its payment dates (M24.4), on the
/// positions held at the previous close. The receiver of fixed gets
/// `(fixed - fixing) * tau` per 1 of notional, which may be negative.
fn pay_swap_coupons(
    setup: &InstrumentSetup,
    curves: &CurveHistory,
    prev_ts: Option<i64>,
    ts: i64,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
) -> std::result::Result<(), RunError> {
    let Some(prev) = prev_ts else { return Ok(()) };
    for &id in setup.instruments.keys() {
        let Some(terms) = setup.swap_terms(id) else { continue };
        for day in terms.payment_days_between(prev, ts) {
            let held = |book: &Ledger| !book.position(id).qty.is_zero();
            if !held(ledger) && !sleeves.iter().any(held) {
                continue;
            }
            let per_unit = setup.fx_convert(id, Price::from_f64(terms.net_coupon(curves, day)?)?, ts)?;
            if per_unit.is_zero() {
                continue;
            }
            let owed = |book: &Ledger| -> Result<Money> {
                let q = book.position(id).qty;
                if q.is_zero() {
                    return Ok(Money::ZERO);
                }
                Ok(book.notional_of(id, per_unit, q)?)
            };
            if sleeves.is_empty() {
                let amount = owed(ledger)?;
                if !amount.is_zero() {
                    ledger.apply_coupon(id, amount, ts).map_err(ledger_err)?;
                }
                continue;
            }
            let mut total = Money::ZERO;
            for book in sleeves.iter_mut() {
                let amount = owed(book)?;
                if !amount.is_zero() {
                    book.apply_coupon(id, amount, ts).map_err(ledger_err)?;
                    total = total.checked_add(amount)?;
                }
            }
            if !total.is_zero() {
                ledger.apply_coupon(id, total, ts).map_err(ledger_err)?;
            }
        }
    }
    Ok(())
}

/// Closes every swap position at 100 once its maturity has come: every
/// payment has been made, so nothing is left to value.
fn mature_swaps(
    setup: &InstrumentSetup,
    ts: i64,
    ledger: &mut Ledger,
    sleeves: &mut [Ledger],
    rolls: &mut Vec<RollEvent>,
    fill_orders: &mut Vec<OrderId>,
) -> std::result::Result<Vec<(usize, Fill)>, RunError> {
    let mut out = Vec::new();
    let owners = sleeves.len().max(1);
    for (&id, inst) in &setup.instruments {
        let Some(maturity) = inst.maturity_ns.filter(|_| inst.kind == InstrumentType::Swap) else { continue };
        if ts < maturity {
            continue;
        }
        let close = setup.fx_convert(id, Price::from_units(100)?, ts)?;
        for owner in 0..owners {
            let qty = sleeves.get(owner).unwrap_or(ledger).position(id).qty;
            if qty.is_zero() {
                continue;
            }
            let side = if qty.raw() > 0 { Side::Sell } else { Side::Buy };
            let f = Fill {
                instrument: id,
                side,
                qty: qty.checked_abs()?,
                price: close,
                fee: Money::ZERO,
                ts_ns: ts,
            };
            apply_engine_fill(ledger, sleeves, owner, &f, false).map_err(ledger_err)?;
            fill_orders.push(0);
            out.push((owner, f));
            rolls.push(RollEvent {
                ts_ns: ts,
                strategy: owner,
                from: id,
                to: None,
                qty,
                close_price: close,
                open_price: None,
                kind: RollKind::Redeemed,
                note: Some("swap matured".into()),
            });
        }
    }
    Ok(out)
}

/// Refuses a setup the engine cannot run: an invalid record or rate series,
/// or an instrument type whose model is not in this engine yet.
fn check_setup(setup: &InstrumentSetup, bars: &[Bar]) -> std::result::Result<(), RunError> {
    setup.validate()?;
    let used: BTreeSet<InstrumentId> = bars.iter().map(|b| b.instrument).collect();
    for (id, inst) in &setup.instruments {
        if !used.contains(id) {
            continue;
        }
        if let Some(why) = unsupported(inst) {
            return Err(Error::InvalidArgument(format!("instrument {}: {why}", inst.symbol)).into());
        }
    }
    Ok(())
}

/// Why the engine cannot trade `inst` yet, if it cannot.
fn unsupported(inst: &Instrument) -> Option<&'static str> {
    match inst.kind {
        InstrumentType::Equity
        | InstrumentType::Future
        | InstrumentType::Bond
        | InstrumentType::Option
        | InstrumentType::Swap => None,
    }
}

fn run_inner<'s>(
    bars: &[Bar],
    actions: &[CorporateAction],
    strategies: &mut [&mut (dyn Strategy + 's)],
    weights: Option<&[Ratio]>,
    setup: &InstrumentSetup,
    cfg: &BacktestConfig,
    ctl: &mut RunControl<'_>,
) -> std::result::Result<BacktestResult, RunError> {
    let started = Instant::now();
    check_setup(setup, bars)?;
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
    let interval = check_uniform_intervals(bars)?;
    let sessions = SessionRule::new(cfg.exchange, interval);
    let chains = Chains::new(setup, &setup.roll, cfg.exchange);
    let has_bonds = setup.instruments.values().any(|i| i.kind == InstrumentType::Bond);
    let has_options = setup.instruments.values().any(|i| i.kind == InstrumentType::Option);
    let has_swaps = setup.instruments.values().any(|i| i.kind == InstrumentType::Swap);
    let swap_curves = if has_swaps { CurveHistory::build(&setup.curves)? } else { CurveHistory::default() };
    let mut prev_ts: Option<i64> = None;
    let mut continuous = Continuous::default();
    let mut rolls: Vec<RollEvent> = Vec::new();
    let actions = prepare_actions(actions, bars)?;
    let mut next_action = 0usize;
    let progress_every = ctl.progress_every.max(1);

    let mut ledger = Ledger::new(cfg.ledger);
    let mut history: BTreeMap<InstrumentId, Vec<Bar>> = BTreeMap::new();
    let mut orders: Vec<Order> = Vec::new();
    let mut open = OpenOrders::default();
    let mut curve: Vec<EquityPoint> = Vec::with_capacity(bars.len());
    let mut exposure: Vec<Money> = Vec::with_capacity(bars.len());
    let mut fill_orders: Vec<OrderId> = Vec::new();

    let first_ts = bars.first().map_or(0, |b| b.ts_ns);
    if cfg.initial_cash.raw() > 0 && ledger.deposit(cfg.initial_cash, first_ts).is_err() {
        return Err(Error::InvalidArgument("initial deposit rejected".into()).into());
    }
    let mut books = match weights {
        Some(w) => Some(Books::new(w, cfg, first_ts, bars.len())?),
        None => None,
    };
    // Each strategy's own books; empty in a single-strategy run, where the
    // strategy sees the account.
    let mut sleeves: Vec<Ledger> = books.as_mut().map(|b| std::mem::take(&mut b.ledgers)).unwrap_or_default();
    // Contract multipliers, on the account and on every strategy's own books.
    for (&id, inst) in setup.effective() {
        ledger.list(id, inst, first_ts).map_err(ledger_err)?;
        for book in &mut sleeves {
            book.list(id, inst, first_ts).map_err(ledger_err)?;
        }
    }

    // Orders placed in on_start are stamped just before the first bar so they can fill on it.
    let start_ts = if first_ts > i64::MIN { first_ts - 1 } else { first_ts };
    for (s, strategy) in strategies.iter_mut().enumerate() {
        let mut ctx = Context {
            book: sleeves.get(s).unwrap_or(&ledger),
            account: &ledger,
            history: &history,
            orders: &mut orders,
            open: &mut open,
            now: start_ts,
            step: -1,
            seed: cfg.seed,
            strategy: s,
            setup,
            chains: &chains,
            continuous: &continuous,
        };
        strategy.on_start(&mut ctx);
    }

    let min_wait = 1 + cfg.exec.delay_bars as i64;
    let mut i = 0usize;
    let mut step: i64 = 0;
    let mut current_equity_val = cfg.initial_cash.to_f64();
    let mut peak_equity_val = cfg.initial_cash.to_f64();
    let mut daily_start_equity_val = cfg.initial_cash.to_f64();
    let mut last_calendar_day: i64 = -1;
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
        if !chains.is_empty() {
            continuous.update(&chains, ts, group, &history, setup.roll.adjust)?;
        }

        if cfg.risk_limits.is_some() {
            let current_day = ts / 86_400_000_000_000;
            if current_day != last_calendar_day {
                daily_start_equity_val =
                    ledger.equity().map_or(cfg.initial_cash.to_f64(), spectra_core::Fixed::to_f64);
                last_calendar_day = current_day;
            }
            current_equity_val =
                ledger.equity().map_or(cfg.initial_cash.to_f64(), spectra_core::Fixed::to_f64);
            peak_equity_val = peak_equity_val.max(current_equity_val);
        }

        // 0. Financing for the period that just ended (none before the first bar).
        if financed && step > 0 {
            if sleeves.is_empty() {
                for c in financing_charges(&ledger, rates)? {
                    post(&mut ledger, &c, ts)?;
                }
            } else {
                // Each strategy is financed on its own books; the account carries every charge.
                let idle = books.as_mut().and_then(|b| b.unallocated.as_mut());
                for book in sleeves.iter_mut().chain(idle) {
                    for c in financing_charges(book, rates)? {
                        post(book, &c, ts)?;
                        post(&mut ledger, &c, ts)?;
                    }
                }
            }
        }

        // Corporate actions going ex now, on the positions held at the previous close.
        let mut applied: Vec<CorporateAction> = Vec::new();
        while next_action < actions.len() && actions[next_action].ex_ts_ns <= ts {
            let a = actions[next_action];
            apply_action(&a, &mut ledger, &mut sleeves, &mut orders, &mut open, ts)?;
            applied.push(a);
            next_action += 1;
        }

        // Good-till-date orders past their date expire before this timestamp's fills.
        expire(&mut orders, &mut open, |o| matches!(o.tif, TimeInForce::GoodTillDate(t) if ts > t));

        // 1. Fill orders submitted at earlier timestamps, in submission order.
        let session_open = sessions.is_open(ts);
        // Futures past their roll date roll first, at this timestamp's open (M24.2).
        let mut fills: Vec<(usize, Fill)> = Vec::new();
        // Bonds: coupons on the positions held at the previous close, then
        // redemption at maturity (M24.3).
        if has_options {
            fills.extend(settle_options(
                setup,
                ts,
                &history,
                &mut ledger,
                &mut sleeves,
                &mut rolls,
                &mut fill_orders,
            )?);
        }
        if has_bonds {
            pay_coupons(setup, prev_ts, ts, &mut ledger, &mut sleeves)?;
            fills.extend(redeem_bonds(setup, ts, &mut ledger, &mut sleeves, &mut rolls, &mut fill_orders)?);
        }
        // Swaps: net coupons, then closing at maturity (M24.4).
        if has_swaps {
            pay_swap_coupons(setup, &swap_curves, prev_ts, ts, &mut ledger, &mut sleeves)?;
            fills.extend(mature_swaps(setup, ts, &mut ledger, &mut sleeves, &mut rolls, &mut fill_orders)?);
        }
        if !chains.is_empty() {
            let mut ctx = RollCtx {
                chains: &chains,
                setup,
                cfg,
                ts,
                group,
                session_open,
                rolls: &mut rolls,
                fill_orders: &mut fill_orders,
            };
            fills.extend(ctx.roll(&mut ledger, &mut sleeves)?);
        }
        if session_open {
            for k in open.candidates(group, &orders) {
                if !orders[k].state.is_open() || step - orders[k].submit_step < min_wait {
                    continue;
                }
                let Some(bar) = find_bar(group, orders[k].instrument) else { continue };
                let past = history.get(&orders[k].instrument).map_or(&[][..], Vec::as_slice);
                let model_priced = setup.kind(orders[k].instrument) == InstrumentType::Swap;
                let ev = evaluate(&orders[k], bar, past, cfg, model_priced)?;
                if ev.triggered && !orders[k].triggered {
                    // The stop was reached: from now on a stop is a market order and a
                    // stop-limit a resting limit order.
                    open.remove(&orders[k], k);
                    orders[k].triggered = true;
                    open.insert(&orders[k], k);
                }
                let Some(mut a) = ev.fill else { continue };
                let o = &mut orders[k];

                if let Some(ref limits) = cfg.risk_limits {
                    // Risk limits see per-unit values in the base currency, multiplier included.
                    let unit = |id: InstrumentId, px: f64| {
                        let m = ledger.multiplier(id);
                        if m == Ratio::ONE {
                            px
                        } else {
                            px * m.to_f64()
                        }
                    };
                    let fill_px = setup.to_base(o.instrument, a.price, ts)?.to_f64();
                    let mut positions = std::collections::HashMap::new();
                    for (&inst, pos) in ledger.positions() {
                        if !pos.qty.is_zero() {
                            let mark_price =
                                ledger.mark_price(inst).map_or(fill_px, spectra_core::Fixed::to_f64);
                            positions.insert(
                                format!("INST_{inst}"),
                                spectra_risk::enforcement::PositionSnapshot {
                                    qty: pos.qty.to_f64(),
                                    price: unit(inst, mark_price),
                                },
                            );
                        }
                    }
                    let snap = spectra_risk::enforcement::PortfolioSnapshot {
                        cash: ledger.cash().to_f64(),
                        equity: current_equity_val,
                        peak_equity: peak_equity_val,
                        daily_start_equity: daily_start_equity_val,
                        positions,
                    };
                    let candidate = spectra_risk::enforcement::OrderCandidate {
                        symbol: format!("INST_{}", o.instrument),
                        side: match o.side {
                            Side::Buy => spectra_risk::enforcement::OrderSide::Buy,
                            Side::Sell => spectra_risk::enforcement::OrderSide::Sell,
                        },
                        qty: a.qty.to_f64(),
                        price: unit(o.instrument, fill_px),
                    };
                    let res = spectra_risk::enforcement::PreTradeChecker::evaluate(
                        limits,
                        &snap,
                        &candidate,
                        cfg.kill_switch,
                    );
                    match res.verdict {
                        spectra_risk::enforcement::PreTradeVerdict::Rejected => {
                            o.state = OrderState::Rejected;
                            o.reject_reason = Some(Reject::RiskLimitExceeded);
                            if !orders[k].state.is_open() {
                                open.remove(&orders[k], k);
                            }
                            continue;
                        }
                        spectra_risk::enforcement::PreTradeVerdict::Reduced => {
                            if let Ok(reduced_qty) = Qty::from_f64(res.approved_qty) {
                                if reduced_qty.is_zero() {
                                    o.state = OrderState::Rejected;
                                    o.reject_reason = Some(Reject::RiskLimitExceeded);
                                    if !orders[k].state.is_open() {
                                        open.remove(&orders[k], k);
                                    }
                                    continue;
                                }
                                a.qty = reduced_qty.min(a.qty);
                            }
                        }
                        spectra_risk::enforcement::PreTradeVerdict::Approved => {}
                    }
                } else if cfg.kill_switch {
                    let current_pos_qty =
                        ledger.positions().get(&o.instrument).map_or(0.0, |p| p.qty.to_f64());
                    let is_closing = match o.side {
                        Side::Buy => current_pos_qty < 0.0,
                        Side::Sell => current_pos_qty > 0.0,
                    };
                    if !is_closing || a.qty.to_f64() > current_pos_qty.abs() {
                        o.state = OrderState::Rejected;
                        o.reject_reason = Some(Reject::RiskLimitExceeded);
                        if !orders[k].state.is_open() {
                            open.remove(&orders[k], k);
                        }
                        continue;
                    }
                }

                // A contract cannot trade at or after its expiry (a bond: its maturity).
                if setup
                    .instrument(o.instrument)
                    .and_then(|i| i.expiry_ns.or(i.maturity_ns))
                    .is_some_and(|e| ts >= e)
                {
                    o.state = OrderState::Rejected;
                    o.reject_reason = Some(Reject::ContractExpired);
                    open.remove(&orders[k], k);
                    continue;
                }
                // Past its roll date, a future only closes: new exposure goes in the next contract.
                if chains.roll_ts(o.instrument).is_some_and(|r| ts >= r) {
                    let held = ledger_for(&ledger, &sleeves, o.strategy).position(o.instrument).qty;
                    let signed = if o.side == Side::Buy { a.qty } else { a.qty.checked_neg()? };
                    if held.checked_add(signed)?.checked_abs()? > held.checked_abs()? {
                        o.state = OrderState::Rejected;
                        o.reject_reason = Some(Reject::ContractRolled);
                        open.remove(&orders[k], k);
                        continue;
                    }
                }
                // Quoted price to the base currency, at the rate known now.
                let base_price = setup.to_base(o.instrument, a.price, ts)?;
                let f = Fill {
                    instrument: o.instrument,
                    side: o.side,
                    qty: a.qty,
                    price: base_price,
                    fee: commission_fee(
                        &cfg.costs.commission,
                        a.qty,
                        ledger.notional_of(o.instrument, base_price, a.qty)?,
                    )?,
                    ts_ns: ts,
                };
                // A strategy must afford a fill on its own books before the account takes it.
                let accepted = match sleeves.get(o.strategy) {
                    Some(book) => book.check_fill(&f).and_then(|()| ledger.apply_fill(&f)),
                    None => ledger.apply_fill(&f),
                };
                match accepted {
                    Ok(()) => {
                        if let Some(book) = sleeves.get_mut(o.strategy) {
                            book.apply_fill(&f).map_err(ledger_err)?;
                        }
                        o.remaining = o.remaining.checked_sub(a.qty)?;
                        o.state = if o.remaining.is_zero() {
                            OrderState::Filled
                        } else {
                            OrderState::PartiallyFilled
                        };
                        fills.push((o.strategy, f));
                        fill_orders.push(o.id);
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
        }

        // Day orders expire once they have had their one bar (one they could trade on).
        if session_open {
            expire(&mut orders, &mut open, |o| {
                o.tif == TimeInForce::Day
                    && step - o.submit_step >= min_wait
                    && find_bar(group, o.instrument).is_some()
            });
        }

        // 2. Mark to the close and record equity.
        for b in group {
            let close = setup.to_base(b.instrument, b.close, ts)?;
            let mut marked = ledger.mark(b.instrument, close);
            for book in &mut sleeves {
                marked = marked.and_then(|()| book.mark(b.instrument, close));
            }
            if marked.is_err() {
                return Err(Error::Internal("validated bar produced an invalid mark".into()).into());
            }
        }
        curve.push(EquityPoint { ts_ns: ts, equity: ledger.equity()?, cash: ledger.cash() });
        exposure.push(gross_of(&ledger)?);
        if let Some(b) = books.as_mut() {
            for (s, book) in sleeves.iter().enumerate() {
                b.curves[s].push(EquityPoint { ts_ns: ts, equity: book.equity()?, cash: book.cash() });
                b.exposure[s].push(gross_of(book)?);
            }
        }

        // 3. Let the strategies react, in order. Anything they submit fills at a later timestamp.
        for (s, strategy) in strategies.iter_mut().enumerate() {
            let mut ctx = Context {
                book: sleeves.get(s).unwrap_or(&ledger),
                account: &ledger,
                history: &history,
                orders: &mut orders,
                open: &mut open,
                now: ts,
                step,
                seed: cfg.seed,
                strategy: s,
                setup,
                chains: &chains,
                continuous: &continuous,
            };
            for a in &applied {
                strategy.on_corporate_action(&mut ctx, a);
            }
            for (_, f) in fills.iter().filter(|(owner, _)| *owner == s) {
                strategy.on_fill(&mut ctx, f);
            }
            strategy.on_bar(&mut ctx, group);
        }
        open.market.retain(|&k| orders[k].state.is_open());
        i = j;
        step += 1;
        prev_ts = Some(ts);
    }

    if let Some(cb) = ctl.progress.as_mut() {
        cb(Progress { bars_done: bars.len(), bars_total: bars.len(), timestamps_done: step as usize });
    }
    let metrics = compute_metrics(&curve, cfg.periods_per_year, cfg.risk_free_rate);
    let result_sha256 = hash_result(&curve, ledger.journal(), &orders, weights.is_some());
    let mut canonical = match weights {
        Some(w) => canonical_multi_config(cfg, w),
        None => canonical_config(cfg),
    };
    // Written only when there are actions, so runs without them keep their fingerprints.
    if !actions.is_empty() {
        canonical.push_str(&format!("corporate_actions={};", corporate_actions_sha256(&actions)));
    }
    // Written only for a setup that changes the run, so equity runs keep their fingerprints.
    if !setup.is_trivial() {
        canonical.push_str(&format!("instruments={};", setup.fingerprint()));
    }
    let config_sha256 = spectra_core::sha256_hex(canonical.as_bytes());
    let mut per_strategy = Vec::with_capacity(sleeves.len());
    let mut unallocated = None;
    if let (Some(b), Some(w)) = (books, weights) {
        unallocated = b.unallocated;
        for ((((book, curve), exposure), allocation), &weight) in
            sleeves.into_iter().zip(b.curves).zip(b.exposure).zip(b.allocations).zip(w)
        {
            per_strategy.push(StrategyResult {
                weight,
                allocation,
                metrics: compute_metrics(&curve, cfg.periods_per_year, cfg.risk_free_rate),
                equity_curve: curve,
                gross_exposure: exposure,
                ledger: book,
            });
        }
    }
    Ok(BacktestResult {
        equity_curve: curve,
        gross_exposure: exposure,
        orders,
        ledger,
        metrics,
        run: RunRecord {
            engine_version: ENGINE_VERSION.into(),
            dataset_sha256: bars_sha256(bars),
            config_sha256,
            result_sha256,
            seed: cfg.seed,
            bar_count: bars.len(),
        },
        fill_orders,
        strategies: per_strategy,
        unallocated,
        rolls,
        continuous: continuous.all().clone(),
    })
}
