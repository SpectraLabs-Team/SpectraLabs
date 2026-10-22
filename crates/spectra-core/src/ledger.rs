//! The accounting ledger.
//!
//! A single-currency, multi-instrument ledger on exact fixed-point arithmetic.
//! Every accepted cash movement or fill is appended to an append-only journal,
//! and the live state can always be re-derived from that journal and checked
//! against the books ([`Ledger::check_invariants`]).
//!
//! Cost accounting uses the average-cost method. Positions may be long or short
//! (when permitted) and may flip through zero in a single fill.
//!
//! Error handling: business-rule rejections and arithmetic overflow both return
//! a [`LedgerError`] and leave the ledger untouched. Nothing panics.
//!
//! Ported from the C++ reference `quantforge/ledger.{hpp,cpp}` (v0.6.0), with
//! one deliberate change: a **gross leverage limit** ([`LedgerConfig::max_gross_leverage`]).
//! In the C++ engine, short-sale proceeds could fund unlimited further buying
//! even with negative cash disabled. The limit is admission policy, not
//! accounting: it is checked when a fill would increase an instrument's gross
//! exposure, and it is not re-applied by [`Ledger::replay`].

use std::collections::BTreeMap;
use std::fmt;

use crate::fixed::{notional, prorate, scale_money, ArithmeticError, Money, Price, Qty, Ratio};

/// Identifies an instrument within one ledger.
pub type InstrumentId = u32;

/// Direction of a fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Side {
    /// Increases the position.
    #[default]
    Buy,
    /// Decreases the position.
    Sell,
}

/// Why the ledger refused an operation. The ledger is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reject {
    /// A deposit or withdrawal amount that is not positive.
    InvalidAmount,
    /// A malformed fill (non-positive quantity or price, negative fee, notional
    /// rounding to zero) or a non-positive mark price.
    InvalidFill,
    /// Cash would go negative while negative cash is disabled.
    InsufficientCash,
    /// The position would go short while shorting is disabled.
    ShortNotAllowed,
    /// Gross exposure would exceed `max_gross_leverage` times equity.
    LeverageExceeded,
    /// Pre-trade risk limit exceeded or emergency kill switch active.
    RiskLimitExceeded,
}

impl Reject {
    /// The name used by the C++ reference and the parity golden files.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidAmount => "InvalidAmount",
            Self::InvalidFill => "InvalidFill",
            Self::InsufficientCash => "InsufficientCash",
            Self::ShortNotAllowed => "ShortNotAllowed",
            Self::LeverageExceeded => "LeverageExceeded",
            Self::RiskLimitExceeded => "RiskLimitExceeded",
        }
    }
}

/// An operation the ledger did not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerError {
    /// A business rule refused it.
    Rejected(Reject),
    /// An exact result did not fit the fixed-point range.
    Arithmetic(ArithmeticError),
}

impl From<ArithmeticError> for LedgerError {
    fn from(e: ArithmeticError) -> Self {
        Self::Arithmetic(e)
    }
}

impl From<Reject> for LedgerError {
    fn from(r: Reject) -> Self {
        Self::Rejected(r)
    }
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(r) => f.write_str(r.as_str()),
            Self::Arithmetic(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LedgerError {}

/// An executed trade. `qty` and `price` are positive; direction is `side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fill {
    /// Instrument traded.
    pub instrument: InstrumentId,
    /// Buy or sell.
    pub side: Side,
    /// Quantity, > 0.
    pub qty: Qty,
    /// Price per unit, > 0.
    pub price: Price,
    /// Fee charged in addition to the notional, >= 0.
    pub fee: Money,
    /// Event time, nanoseconds since the Unix epoch.
    pub ts_ns: i64,
}

/// Open position for one instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// Signed quantity (short < 0).
    pub qty: Qty,
    /// Signed cost basis of the open quantity (same sign as `qty`).
    pub cost: Money,
    /// Lifetime realized P&L, before fees.
    pub realized: Money,
    /// Lifetime fees paid.
    pub fees: Money,
}

/// What a journal entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryType {
    /// Cash in.
    Deposit,
    /// Cash out.
    Withdrawal,
    /// An executed trade.
    Fill,
    /// Interest on cash (`amount` > 0, earned) or on a margin debit
    /// (`amount` < 0, paid). Only written when financing is configured.
    Interest,
    /// Borrow fee on a short position (`amount` > 0, paid; `fill.instrument`
    /// names the instrument). Only written when financing is configured.
    BorrowFee,
    /// A stock split of `fill.instrument`: `fill.qty` is the position's new
    /// quantity. The cost basis and realized P&L are unchanged; no cash moves.
    Split,
    /// A cash dividend on `fill.instrument`: `amount` > 0 received on a long
    /// position, < 0 paid on a short one; `fill.qty` is the position it was
    /// paid on.
    Dividend,
}

/// One accepted operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalEntry {
    /// Position in the journal, starting at 0.
    pub seq: u64,
    /// Kind of entry.
    pub kind: EntryType,
    /// Event time, nanoseconds.
    pub ts_ns: i64,
    /// Deposits, withdrawals and financing entries: the amount (see [`EntryType`] for signs).
    pub amount: Money,
    /// Fills: the fill.
    pub fill: Fill,
    /// Realized P&L produced by this entry.
    pub realized: Money,
    /// Cash balance after this entry.
    pub cash_after: Money,
}

/// Ledger policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerConfig {
    /// Permit negative positions.
    pub allow_short: bool,
    /// Permit negative cash (margin borrowing).
    pub allow_negative_cash: bool,
    /// Maximum gross exposure (sum of absolute position values) as a multiple
    /// of equity, checked whenever a fill increases an instrument's gross
    /// exposure. `None` disables the check, which reproduces the C++ reference.
    /// Default 1.0: no leverage. Raise it together with `allow_negative_cash`
    /// or `allow_short` to permit margin.
    pub max_gross_leverage: Option<Ratio>,
}

impl Default for LedgerConfig {
    fn default() -> Self {
        Self { allow_short: false, allow_negative_cash: false, max_gross_leverage: Some(Ratio::ONE) }
    }
}

impl LedgerConfig {
    /// The exact policy of the C++ reference ledger (no leverage limit).
    pub const fn reference(allow_short: bool, allow_negative_cash: bool) -> Self {
        Self { allow_short, allow_negative_cash, max_gross_leverage: None }
    }
}

/// A fill that passed every check, ready to commit.
struct PreparedFill {
    position: Position,
    cash: Money,
    realized: Money,
    realized_total: Money,
    fees_total: Money,
}

/// The accounting ledger. See the module documentation.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    cfg: LedgerConfig,
    cash: Money,
    net_deposits: Money,
    realized: Money,
    fees: Money,
    /// Net financing: interest earned minus interest and borrow fees paid.
    financing: Money,
    /// Net dividends: received on longs minus paid on shorts.
    dividends: Money,
    positions: BTreeMap<InstrumentId, Position>,
    marks: BTreeMap<InstrumentId, Price>,
    journal: Vec<JournalEntry>,
}

type Result<T> = std::result::Result<T, LedgerError>;

impl Ledger {
    /// An empty ledger with the given policy.
    pub fn new(cfg: LedgerConfig) -> Self {
        Self { cfg, ..Self::default() }
    }

    /// Add cash.
    ///
    /// # Errors
    /// `InvalidAmount` if `amount <= 0`; overflow.
    pub fn deposit(&mut self, amount: Money, ts_ns: i64) -> Result<()> {
        if amount.raw() <= 0 {
            return Err(Reject::InvalidAmount.into());
        }
        let new_cash = self.cash.checked_add(amount)?;
        let new_net = self.net_deposits.checked_add(amount)?;
        self.push_cash_entry(EntryType::Deposit, amount, ts_ns, new_cash);
        self.cash = new_cash;
        self.net_deposits = new_net;
        Ok(())
    }

    /// Remove cash.
    ///
    /// # Errors
    /// `InvalidAmount` if `amount <= 0`; `InsufficientCash`; overflow.
    pub fn withdraw(&mut self, amount: Money, ts_ns: i64) -> Result<()> {
        if amount.raw() <= 0 {
            return Err(Reject::InvalidAmount.into());
        }
        let new_cash = self.cash.checked_sub(amount)?;
        if !self.cfg.allow_negative_cash && new_cash.raw() < 0 {
            return Err(Reject::InsufficientCash.into());
        }
        let new_net = self.net_deposits.checked_sub(amount)?;
        self.push_cash_entry(EntryType::Withdrawal, amount, ts_ns, new_cash);
        self.cash = new_cash;
        self.net_deposits = new_net;
        Ok(())
    }

    fn push_cash_entry(&mut self, kind: EntryType, amount: Money, ts_ns: i64, cash_after: Money) {
        self.journal.push(JournalEntry {
            seq: self.journal.len() as u64,
            kind,
            ts_ns,
            amount,
            fill: Fill::default(),
            realized: Money::ZERO,
            cash_after,
        });
    }

    /// Post interest: `amount` > 0 credits cash (interest earned), `amount` < 0
    /// debits it (interest on a margin debit). A financing charge is a debt, not
    /// a choice, so it posts even when it takes cash below zero.
    ///
    /// # Errors
    /// `InvalidAmount` if `amount` is zero; overflow.
    pub fn apply_interest(&mut self, amount: Money, ts_ns: i64) -> Result<()> {
        if amount.is_zero() {
            return Err(Reject::InvalidAmount.into());
        }
        let new_cash = self.cash.checked_add(amount)?;
        let new_fin = self.financing.checked_add(amount)?;
        self.push_cash_entry(EntryType::Interest, amount, ts_ns, new_cash);
        self.cash = new_cash;
        self.financing = new_fin;
        Ok(())
    }

    /// Charge a borrow fee of `amount` (> 0) for a short position in
    /// `instrument`. Posts even when it takes cash below zero (see
    /// [`Self::apply_interest`]).
    ///
    /// # Errors
    /// `InvalidAmount` if `amount <= 0`; overflow.
    pub fn charge_borrow_fee(&mut self, instrument: InstrumentId, amount: Money, ts_ns: i64) -> Result<()> {
        if amount.raw() <= 0 {
            return Err(Reject::InvalidAmount.into());
        }
        let new_cash = self.cash.checked_sub(amount)?;
        let new_fin = self.financing.checked_sub(amount)?;
        self.journal.push(JournalEntry {
            seq: self.journal.len() as u64,
            kind: EntryType::BorrowFee,
            ts_ns,
            amount,
            fill: Fill { instrument, ts_ns, ..Fill::default() },
            realized: Money::ZERO,
            cash_after: new_cash,
        });
        self.cash = new_cash;
        self.financing = new_fin;
        Ok(())
    }

    /// Apply a stock split to the position in `instrument`: its quantity
    /// becomes `new_qty` (same sign, not zero), and its mark, if any, is
    /// scaled by `from / to`. The cost basis is unchanged, so no P&L appears.
    /// The caller computes `new_qty` (normally `qty * to / from`, rounded).
    ///
    /// # Errors
    /// `InvalidFill` if there is no position, `new_qty` changes sign or is
    /// zero, or `from`/`to` is zero; overflow.
    pub fn apply_split(
        &mut self,
        instrument: InstrumentId,
        from: u32,
        to: u32,
        new_qty: Qty,
        ts_ns: i64,
    ) -> Result<()> {
        if from == 0 || to == 0 {
            return Err(Reject::InvalidFill.into());
        }
        self.split_quantity(instrument, new_qty, ts_ns)?;
        self.rescale_mark(instrument, from, to);
        Ok(())
    }

    /// Scales the mark of `instrument` by `from / to` (a split); positions are
    /// untouched. Rounds half away from zero and never below one tick.
    pub fn rescale_mark(&mut self, instrument: InstrumentId, from: u32, to: u32) {
        if from == 0 || to == 0 {
            return;
        }
        if let Some(px) = self.marks.get_mut(&instrument) {
            let raw =
                crate::fixed::div_round_half_away(i128::from(px.raw()) * i128::from(from), i128::from(to));
            *px = Price::from_raw(i64::try_from(raw).unwrap_or(i64::MAX).max(1));
        }
    }

    /// The journaled part of a split (what replay re-applies; marks are not journaled).
    fn split_quantity(&mut self, instrument: InstrumentId, new_qty: Qty, ts_ns: i64) -> Result<()> {
        let cur = self.position(instrument);
        if cur.qty.is_zero() || new_qty.is_zero() || cur.qty.signum() != new_qty.signum() {
            return Err(Reject::InvalidFill.into());
        }
        self.journal.push(JournalEntry {
            seq: self.journal.len() as u64,
            kind: EntryType::Split,
            ts_ns,
            amount: Money::ZERO,
            fill: Fill { instrument, qty: new_qty, ts_ns, ..Fill::default() },
            realized: Money::ZERO,
            cash_after: self.cash,
        });
        self.positions.insert(instrument, Position { qty: new_qty, ..cur });
        Ok(())
    }

    /// Post a cash dividend on the position in `instrument`: `amount` > 0
    /// credits a long, < 0 debits a short. Like financing, a dividend owed on
    /// a short is a debt, so it posts even when it takes cash below zero.
    ///
    /// # Errors
    /// `InvalidAmount` if `amount` is zero; `InvalidFill` if there is no
    /// position or the sign does not match it; overflow.
    pub fn apply_dividend(&mut self, instrument: InstrumentId, amount: Money, ts_ns: i64) -> Result<()> {
        if amount.is_zero() {
            return Err(Reject::InvalidAmount.into());
        }
        let qty = self.position(instrument).qty;
        if qty.is_zero() || qty.signum() != amount.signum() {
            return Err(Reject::InvalidFill.into());
        }
        let new_cash = self.cash.checked_add(amount)?;
        let new_div = self.dividends.checked_add(amount)?;
        self.journal.push(JournalEntry {
            seq: self.journal.len() as u64,
            kind: EntryType::Dividend,
            ts_ns,
            amount,
            fill: Fill { instrument, qty, ts_ns, ..Fill::default() },
            realized: Money::ZERO,
            cash_after: new_cash,
        });
        self.cash = new_cash;
        self.dividends = new_div;
        Ok(())
    }

    /// Apply an executed fill. Nothing changes unless it returns `Ok`.
    ///
    /// # Errors
    /// `InvalidFill`, `InsufficientCash`, `ShortNotAllowed`,
    /// `LeverageExceeded`, or overflow.
    pub fn apply_fill(&mut self, f: &Fill) -> Result<()> {
        let p = self.prepare_fill(f)?;
        self.journal.push(JournalEntry {
            seq: self.journal.len() as u64,
            kind: EntryType::Fill,
            ts_ns: f.ts_ns,
            amount: Money::ZERO,
            fill: *f,
            realized: p.realized,
            cash_after: p.cash,
        });
        self.positions.insert(f.instrument, p.position);
        self.cash = p.cash;
        self.realized = p.realized_total;
        self.fees = p.fees_total;
        Ok(())
    }

    /// Whether [`Self::apply_fill`] would accept `f`, without changing anything.
    ///
    /// # Errors
    /// The error `apply_fill` would return.
    pub fn check_fill(&self, f: &Fill) -> Result<()> {
        self.prepare_fill(f).map(|_| ())
    }

    /// Every check and computation of a fill; the caller commits the result.
    fn prepare_fill(&self, f: &Fill) -> Result<PreparedFill> {
        if f.qty.raw() <= 0 || f.price.raw() <= 0 || f.fee.raw() < 0 {
            return Err(Reject::InvalidFill.into());
        }
        // Notional is rounded exactly once per fill; every other number for
        // this fill derives from it, so cash and cost basis never drift apart.
        let n = notional(f.price, f.qty)?;
        if n.raw() <= 0 {
            return Err(Reject::InvalidFill.into()); // rounds to zero
        }

        let buy = f.side == Side::Buy;
        let spend = if buy { n } else { n.checked_neg()? }; // cash out (+) or in (-), before fees
        let new_cash = self.cash.checked_sub(spend)?.checked_sub(f.fee)?;
        if !self.cfg.allow_negative_cash && new_cash.raw() < 0 {
            return Err(Reject::InsufficientCash.into());
        }

        let cur = self.position(f.instrument);
        let fq = if buy { f.qty } else { f.qty.checked_neg()? }; // signed fill quantity
        let new_qty = cur.qty.checked_add(fq)?;
        if !self.cfg.allow_short && new_qty.raw() < 0 {
            return Err(Reject::ShortNotAllowed.into());
        }

        let mut realized = Money::ZERO;
        let opening = cur.qty.is_zero() || cur.qty.signum() == fq.signum();
        let new_cost = if opening {
            cur.cost.checked_add(spend)?
        } else {
            // The fill closes part or all of the position and may open the
            // opposite side with whatever is left over.
            let abs_q = cur.qty.checked_abs()?;
            let abs_f = fq.checked_abs()?;
            let closed = abs_q.min(abs_f);

            let cost_abs = cur.cost.checked_abs()?;
            let released_abs = if closed == abs_q { cost_abs } else { prorate(cost_abs, closed, abs_q)? };
            let released = if cur.qty.signum() > 0 { released_abs } else { released_abs.checked_neg()? };

            // Split the single rounded notional so the parts add back to `n`.
            let n_close = if closed == abs_f { n } else { notional(f.price, closed)? };
            let n_open = n.checked_sub(n_close)?;

            let close_value = if cur.qty.signum() > 0 { n_close } else { n_close.checked_neg()? };
            realized = close_value.checked_sub(released)?;

            let open_spend = if fq.signum() > 0 { n_open } else { n_open.checked_neg()? };
            cur.cost.checked_sub(released)?.checked_add(open_spend)?
        };

        if let Some(limit) = self.cfg.max_gross_leverage {
            if new_qty.checked_abs()? > cur.qty.checked_abs()? {
                self.check_leverage(f, new_qty, new_cash, limit)?;
            }
        }

        let next = Position {
            qty: new_qty,
            cost: new_cost,
            realized: cur.realized.checked_add(realized)?,
            fees: cur.fees.checked_add(f.fee)?,
        };
        let new_realized_total = self.realized.checked_add(realized)?;
        let new_fees_total = self.fees.checked_add(f.fee)?;

        Ok(PreparedFill {
            position: next,
            cash: new_cash,
            realized,
            realized_total: new_realized_total,
            fees_total: new_fees_total,
        })
    }

    /// Post-trade gross exposure must not exceed `limit` times post-trade
    /// equity. The traded instrument is valued at the fill price; others at
    /// their latest mark, or at cost if never marked.
    fn check_leverage(&self, f: &Fill, new_qty: Qty, new_cash: Money, limit: Ratio) -> Result<()> {
        let mut equity = new_cash;
        let mut gross = Money::ZERO;
        let traded = notional(f.price, new_qty)?;
        equity = equity.checked_add(traded)?;
        gross = gross.checked_add(traded.checked_abs()?)?;
        for (&id, p) in &self.positions {
            if id == f.instrument || p.qty.is_zero() {
                continue;
            }
            let v = self.value_of(id, p)?;
            equity = equity.checked_add(v)?;
            gross = gross.checked_add(v.checked_abs()?)?;
        }
        if gross > scale_money(equity, limit)? {
            return Err(Reject::LeverageExceeded.into());
        }
        Ok(())
    }

    /// Set the valuation price for an instrument. Marks are state, not
    /// journaled. Unmarked positions are valued at cost.
    ///
    /// # Errors
    /// `InvalidFill` if `price <= 0`.
    pub fn mark(&mut self, instrument: InstrumentId, price: Price) -> Result<()> {
        if price.raw() <= 0 {
            return Err(Reject::InvalidFill.into());
        }
        self.marks.insert(instrument, price);
        Ok(())
    }

    fn value_of(&self, id: InstrumentId, p: &Position) -> std::result::Result<Money, ArithmeticError> {
        if p.qty.is_zero() {
            return Ok(Money::ZERO);
        }
        match self.marks.get(&id) {
            Some(&px) => notional(px, p.qty),
            None => Ok(p.cost),
        }
    }

    /// Cash balance.
    pub fn cash(&self) -> Money {
        self.cash
    }
    /// Deposits minus withdrawals.
    pub fn net_deposits(&self) -> Money {
        self.net_deposits
    }
    /// Total realized P&L, before fees.
    pub fn realized_pnl(&self) -> Money {
        self.realized
    }
    /// Total fees paid.
    pub fn total_fees(&self) -> Money {
        self.fees
    }
    /// Net financing: interest earned minus interest and borrow fees paid.
    pub fn financing(&self) -> Money {
        self.financing
    }
    /// Net dividends: received on long positions minus paid on short ones.
    pub fn dividends(&self) -> Money {
        self.dividends
    }
    /// The policy this ledger enforces.
    pub fn config(&self) -> &LedgerConfig {
        &self.cfg
    }
    /// All positions that have ever traded, including flat ones.
    pub fn positions(&self) -> &BTreeMap<InstrumentId, Position> {
        &self.positions
    }
    /// The append-only journal.
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// The position, or a zero position if the instrument never traded.
    pub fn position(&self, instrument: InstrumentId) -> Position {
        self.positions.get(&instrument).copied().unwrap_or_default()
    }

    /// Marked valuation price for an instrument, if any.
    pub fn mark_price(&self, instrument: InstrumentId) -> Option<Price> {
        self.marks.get(&instrument).copied()
    }

    /// Marked value of one position (cost if unmarked).
    ///
    /// # Errors
    /// Overflow.
    pub fn position_value(&self, instrument: InstrumentId) -> std::result::Result<Money, ArithmeticError> {
        self.positions.get(&instrument).map_or(Ok(Money::ZERO), |p| self.value_of(instrument, p))
    }

    /// Sum over positions of marked value minus cost.
    ///
    /// # Errors
    /// Overflow.
    pub fn unrealized_pnl(&self) -> std::result::Result<Money, ArithmeticError> {
        let mut total = Money::ZERO;
        for (&id, p) in &self.positions {
            total = total.checked_add(self.value_of(id, p)?.checked_sub(p.cost)?)?;
        }
        Ok(total)
    }

    /// Cash plus marked position values.
    ///
    /// # Errors
    /// Overflow.
    pub fn equity(&self) -> std::result::Result<Money, ArithmeticError> {
        let mut total = self.cash;
        for (&id, p) in &self.positions {
            total = total.checked_add(self.value_of(id, p)?)?;
        }
        Ok(total)
    }

    /// Rebuild a ledger from a journal. Leverage limits are admission policy
    /// and are not re-applied (marks are not journaled); every other rule is.
    ///
    /// # Errors
    /// The first entry that is rejected, with its sequence number, which means
    /// the journal is inconsistent with `cfg`.
    pub fn replay(cfg: LedgerConfig, journal: &[JournalEntry]) -> std::result::Result<Self, String> {
        let mut out = Self::new(LedgerConfig { max_gross_leverage: None, ..cfg });
        for e in journal {
            let r = match e.kind {
                EntryType::Deposit => out.deposit(e.amount, e.ts_ns),
                EntryType::Withdrawal => out.withdraw(e.amount, e.ts_ns),
                EntryType::Fill => out.apply_fill(&e.fill),
                EntryType::Interest => out.apply_interest(e.amount, e.ts_ns),
                EntryType::BorrowFee => out.charge_borrow_fee(e.fill.instrument, e.amount, e.ts_ns),
                EntryType::Split => out.split_quantity(e.fill.instrument, e.fill.qty, e.ts_ns),
                EntryType::Dividend => out.apply_dividend(e.fill.instrument, e.amount, e.ts_ns),
            };
            if let Err(err) = r {
                return Err(format!("replay: journal entry {} rejected: {err}", e.seq));
            }
        }
        out.cfg = cfg;
        Ok(out)
    }

    /// Re-derive cash, quantities, fees and realized P&L from the journal
    /// alone; cross-check them against cost-basis bookkeeping and the equity
    /// identity; replay the journal into a fresh ledger and require an
    /// identical result.
    ///
    /// # Errors
    /// A description of the first violated invariant.
    pub fn check_invariants(&self) -> std::result::Result<(), String> {
        self.check_invariants_inner().map_err(|e| match e {
            InvariantFailure::Violated(s) => s,
            InvariantFailure::Arithmetic(a) => format!("exception during invariant check: {a}"),
        })
    }

    /// Step 1 of the invariant check: totals recomputed from the journal alone,
    /// verifying sequence numbers and every `cash_after` on the way.
    fn journal_totals(&self) -> std::result::Result<JournalTotals, InvariantFailure> {
        use InvariantFailure::Violated as fail;
        let mut t = JournalTotals::default();
        for (i, e) in self.journal.iter().enumerate() {
            if e.seq != i as u64 {
                return Err(fail(format!("journal sequence gap at index {i}")));
            }
            match e.kind {
                EntryType::Deposit => {
                    t.cash = t.cash.checked_add(e.amount)?;
                    t.net = t.net.checked_add(e.amount)?;
                }
                EntryType::Withdrawal => {
                    t.cash = t.cash.checked_sub(e.amount)?;
                    t.net = t.net.checked_sub(e.amount)?;
                }
                EntryType::Fill => {
                    let f = &e.fill;
                    let n = notional(f.price, f.qty)?;
                    let buy = f.side == Side::Buy;
                    t.cash =
                        t.cash.checked_sub(if buy { n } else { n.checked_neg()? })?.checked_sub(f.fee)?;
                    t.fees = t.fees.checked_add(f.fee)?;
                    t.realized = t.realized.checked_add(e.realized)?;
                    let q = t.qty.entry(f.instrument).or_default();
                    *q = q.checked_add(if buy { f.qty } else { f.qty.checked_neg()? })?;
                }
                EntryType::Interest => {
                    t.cash = t.cash.checked_add(e.amount)?;
                    t.financing = t.financing.checked_add(e.amount)?;
                }
                EntryType::BorrowFee => {
                    t.cash = t.cash.checked_sub(e.amount)?;
                    t.financing = t.financing.checked_sub(e.amount)?;
                }
                EntryType::Split => {
                    t.qty.insert(e.fill.instrument, e.fill.qty);
                }
                EntryType::Dividend => {
                    t.cash = t.cash.checked_add(e.amount)?;
                    t.dividends = t.dividends.checked_add(e.amount)?;
                }
            }
            if e.cash_after != t.cash {
                return Err(fail(format!("cash_after mismatch at seq {}", e.seq)));
            }
        }
        Ok(t)
    }

    fn check_invariants_inner(&self) -> std::result::Result<(), InvariantFailure> {
        use InvariantFailure::Violated as fail;

        // 1. Independent recomputation from the journal alone.
        let JournalTotals { cash, net, fees, realized, financing, dividends, qty } = self.journal_totals()?;
        if cash != self.cash {
            return Err(fail(format!("cash {} != journal {cash}", self.cash)));
        }
        if net != self.net_deposits {
            return Err(fail("net deposits differ from journal".into()));
        }
        if fees != self.fees {
            return Err(fail("total fees differ from journal".into()));
        }
        if realized != self.realized {
            return Err(fail("realized P&L differs from journal".into()));
        }
        if financing != self.financing {
            return Err(fail("financing differs from journal".into()));
        }
        if dividends != self.dividends {
            return Err(fail("dividends differ from journal".into()));
        }

        // 2. Quantities from the journal match the positions.
        for (&id, p) in &self.positions {
            if p.qty != qty.get(&id).copied().unwrap_or_default() {
                return Err(fail(format!("quantity mismatch for instrument {id}")));
            }
        }
        for (&id, &q) in &qty {
            if q != self.position(id).qty {
                return Err(fail(format!("journal quantity missing from positions for instrument {id}")));
            }
        }

        // 3. cash = net deposits - fees - open cost basis + realized P&L.
        let mut total_cost = Money::ZERO;
        let mut pos_realized = Money::ZERO;
        let mut pos_fees = Money::ZERO;
        for (&id, p) in &self.positions {
            total_cost = total_cost.checked_add(p.cost)?;
            pos_realized = pos_realized.checked_add(p.realized)?;
            pos_fees = pos_fees.checked_add(p.fees)?;
            if p.qty.is_zero() && !p.cost.is_zero() {
                return Err(fail(format!("flat position with non-zero cost, instrument {id}")));
            }
            if p.qty.signum() * p.cost.signum() < 0 {
                return Err(fail(format!("cost basis sign disagrees with position, instrument {id}")));
            }
            if !self.cfg.allow_short && p.qty.raw() < 0 {
                return Err(fail(format!("short position while shorting is disabled, instrument {id}")));
            }
        }
        let expected_cash = self
            .net_deposits
            .checked_sub(self.fees)?
            .checked_sub(total_cost)?
            .checked_add(self.realized)?
            .checked_add(self.financing)?
            .checked_add(self.dividends)?;
        if self.cash != expected_cash {
            return Err(fail("cash does not reconcile with cost basis and realized P&L".into()));
        }
        if pos_realized != self.realized {
            return Err(fail("per-position realized P&L != total".into()));
        }
        if pos_fees != self.fees {
            return Err(fail("per-position fees != total".into()));
        }
        // Financing charges and dividends owed on shorts post even past zero cash; nothing else may.
        if !self.cfg.allow_negative_cash
            && self.cash.raw() < 0
            && self.financing.raw() >= 0
            && self.dividends.raw() >= 0
        {
            return Err(fail("negative cash while disabled".into()));
        }

        // 4. equity = net deposits - fees + realized + unrealized + financing.
        let identity = self
            .net_deposits
            .checked_sub(self.fees)?
            .checked_add(self.realized)?
            .checked_add(self.unrealized_pnl()?)?
            .checked_add(self.financing)?
            .checked_add(self.dividends)?;
        if self.equity()? != identity {
            return Err(fail("equity identity violated".into()));
        }

        // 5. Replaying the journal reproduces the books exactly.
        let r = Self::replay(self.cfg, &self.journal).map_err(fail)?;
        if r.journal != self.journal {
            return Err(fail("replayed journal differs".into()));
        }
        if r.cash != self.cash
            || r.net_deposits != self.net_deposits
            || r.realized != self.realized
            || r.fees != self.fees
            || r.financing != self.financing
            || r.dividends != self.dividends
        {
            return Err(fail("replayed totals differ".into()));
        }
        if r.positions != self.positions {
            return Err(fail("replayed positions differ".into()));
        }
        Ok(())
    }
}

#[derive(Default)]
struct JournalTotals {
    cash: Money,
    net: Money,
    fees: Money,
    realized: Money,
    financing: Money,
    dividends: Money,
    qty: BTreeMap<InstrumentId, Qty>,
}

enum InvariantFailure {
    Violated(String),
    Arithmetic(ArithmeticError),
}

impl From<ArithmeticError> for InvariantFailure {
    fn from(e: ArithmeticError) -> Self {
        Self::Arithmetic(e)
    }
}
