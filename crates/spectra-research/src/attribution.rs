//! P&L Attribution analysis (Module 11).
//!
//! Decomposes a ledger's realized P&L by:
//! - Instrument: per-symbol gross, fees, net, trade count, win rate, and return share.
//! - Direction: long vs. short gross and net P&L, trade counts and win ratios.
//! - Costs: commissions, borrow fees and financing interest charged, interest income earned,
//!   and dividends and coupons (memo lines; they are part of trade gross). Slippage is an
//!   estimate of what is already in the fill prices, never charged a second time.
//! - Waterfall: a two-stage bridge (M11 Task 3, 6 Nov 2026). Gross P&L of closed trades, less
//!   their fees, gives Trading Net P&L, which equals `total_net_pnl`. Adding what positions still
//!   open have realized so far (net of their fees), less borrow fees and financing interest, plus
//!   interest income, gives Net Account P&L, which equals `total_account_pnl`. Both identities are
//!   exact: every amount is summed in fixed point and converted once.
//!
//! Unrealized P&L (mark-to-market of open positions) is not in either total.

use std::collections::BTreeMap;

use spectra_core::{EntryType, Error, InstrumentId, Json, Ledger, Money, Result};

use crate::trades::extract_trades;

/// Instrument-level P&L attribution record (closed trades).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InstrumentAttribution {
    pub instrument: InstrumentId,
    pub symbol: Option<String>,
    pub gross_pnl: f64,
    pub fees: f64,
    pub net_pnl: f64,
    pub trades_count: usize,
    pub win_trades: usize,
    pub loss_trades: usize,
    pub win_rate: f64,
    pub long_pnl: f64,
    pub short_pnl: f64,
    pub pnl_share: f64,
}

/// Long vs. short directional attribution (closed trades).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DirectionalAttribution {
    pub long_gross_pnl: f64,
    pub long_fees: f64,
    pub long_net_pnl: f64,
    pub long_trades: usize,
    pub long_wins: usize,
    pub long_win_rate: f64,
    pub short_gross_pnl: f64,
    pub short_fees: f64,
    pub short_net_pnl: f64,
    pub short_trades: usize,
    pub short_wins: usize,
    pub short_win_rate: f64,
}

/// Costs and income from the ledger journal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CostDecomposition {
    /// Fees on every fill, closed trades and open positions alike.
    pub commission_fees: f64,
    /// Estimated slippage: total fill notional times the slippage rate. Fill prices already
    /// include it, so it is a memo line: it is not in `total_costs` or the waterfall.
    pub slippage_costs: f64,
    /// Borrow fees charged on short positions (positive).
    pub borrow_fees: f64,
    /// Financing interest paid on margin debit (positive).
    pub financing_interest: f64,
    /// Interest earned on cash (positive).
    pub interest_income: f64,
    /// Dividends received (paid when negative, on shorts). A memo line: part of trade gross.
    pub dividends: f64,
    /// Bond coupons received (paid when negative, on shorts). A memo line: part of trade gross.
    pub coupons: f64,
    /// Costs charged: commissions, borrow fees and financing interest.
    pub total_costs: f64,
}

/// A single step in the waterfall bridge.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaterfallStep {
    pub name: String,
    /// The step's change; for a subtotal, the subtotal itself.
    pub amount: f64,
    pub running_total: f64,
    /// `true` for a level drawn from zero (the starting gross and the two subtotals), `false`
    /// for a change from the previous running total.
    pub subtotal: bool,
}

/// Full strategy P&L attribution result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PnlAttribution {
    /// Gross P&L of closed trades (price P&L plus dividends and coupons while held).
    pub total_gross_pnl: f64,
    /// Fees on every fill (the same as `costs.commission_fees`).
    pub total_fees: f64,
    /// Net P&L of closed trades: their gross less their fees.
    pub total_net_pnl: f64,
    /// Realized P&L so far on positions still open, net of their fees.
    pub open_position_pnl: f64,
    /// Net account P&L: `total_net_pnl` + `open_position_pnl` - borrow fees - financing
    /// interest + interest income. Excludes unrealized P&L.
    pub total_account_pnl: f64,
    pub total_trades: usize,
    pub overall_win_rate: f64,
    pub costs: CostDecomposition,
    pub direction: DirectionalAttribution,
    pub instruments: Vec<InstrumentAttribution>,
    pub waterfall: Vec<WaterfallStep>,
}

fn num(v: f64) -> Json {
    Json::number(v).unwrap_or(Json::Null)
}

impl PnlAttribution {
    /// Serializes the attribution report into standard JSON.
    pub fn to_json(&self) -> Json {
        let mut inst_arr = Vec::new();
        for inst in &self.instruments {
            let symbol_val = inst.symbol.as_ref().map_or(Json::Null, |s| Json::string(s.clone()));
            inst_arr.push(
                Json::object()
                    .with("instrument", Json::integer(i64::from(inst.instrument)))
                    .with("symbol", symbol_val)
                    .with("gross_pnl", num(inst.gross_pnl))
                    .with("fees", num(inst.fees))
                    .with("net_pnl", num(inst.net_pnl))
                    .with("trades_count", Json::integer(inst.trades_count as i64))
                    .with("win_trades", Json::integer(inst.win_trades as i64))
                    .with("loss_trades", Json::integer(inst.loss_trades as i64))
                    .with("win_rate", num(inst.win_rate))
                    .with("long_pnl", num(inst.long_pnl))
                    .with("short_pnl", num(inst.short_pnl))
                    .with("pnl_share", num(inst.pnl_share)),
            );
        }

        let direction_obj = Json::object()
            .with("long_gross_pnl", num(self.direction.long_gross_pnl))
            .with("long_fees", num(self.direction.long_fees))
            .with("long_net_pnl", num(self.direction.long_net_pnl))
            .with("long_trades", Json::integer(self.direction.long_trades as i64))
            .with("long_wins", Json::integer(self.direction.long_wins as i64))
            .with("long_win_rate", num(self.direction.long_win_rate))
            .with("short_gross_pnl", num(self.direction.short_gross_pnl))
            .with("short_fees", num(self.direction.short_fees))
            .with("short_net_pnl", num(self.direction.short_net_pnl))
            .with("short_trades", Json::integer(self.direction.short_trades as i64))
            .with("short_wins", Json::integer(self.direction.short_wins as i64))
            .with("short_win_rate", num(self.direction.short_win_rate));

        let costs_obj = Json::object()
            .with("commission_fees", num(self.costs.commission_fees))
            .with("slippage_costs", num(self.costs.slippage_costs))
            .with("slippage_included_in_fill_prices", Json::Bool(true))
            .with("borrow_fees", num(self.costs.borrow_fees))
            .with("financing_interest", num(self.costs.financing_interest))
            .with("interest_income", num(self.costs.interest_income))
            .with("dividends", num(self.costs.dividends))
            .with("coupons", num(self.costs.coupons))
            .with("total_costs", num(self.costs.total_costs));

        let mut waterfall_arr = Vec::new();
        for step in &self.waterfall {
            waterfall_arr.push(
                Json::object()
                    .with("name", Json::string(&step.name))
                    .with("amount", num(step.amount))
                    .with("running_total", num(step.running_total))
                    .with("subtotal", Json::Bool(step.subtotal)),
            );
        }

        Json::object()
            .with("total_gross_pnl", num(self.total_gross_pnl))
            .with("total_fees", num(self.total_fees))
            .with("total_net_pnl", num(self.total_net_pnl))
            .with("open_position_pnl", num(self.open_position_pnl))
            .with("total_account_pnl", num(self.total_account_pnl))
            .with("total_trades", Json::integer(self.total_trades as i64))
            .with("overall_win_rate", num(self.overall_win_rate))
            .with("direction", direction_obj)
            .with("costs", costs_obj)
            .with("instruments", Json::Array(inst_arr))
            .with("waterfall", Json::Array(waterfall_arr))
    }
}

/// Pushes a change step onto the waterfall, unless it is zero.
fn push_change(steps: &mut Vec<WaterfallStep>, running: &mut Money, name: &str, amount: Money) -> Result<()> {
    if !amount.is_zero() {
        *running = running.checked_add(amount)?;
        steps.push(WaterfallStep {
            name: name.into(),
            amount: amount.to_f64(),
            running_total: running.to_f64(),
            subtotal: false,
        });
    }
    Ok(())
}

/// A level step: the start or a subtotal, drawn from zero.
fn level(name: &str, total: Money) -> WaterfallStep {
    WaterfallStep { name: name.into(), amount: total.to_f64(), running_total: total.to_f64(), subtotal: true }
}

/// Builds the waterfall: closed-trade gross, less their fees, to Trading Net P&L; then open
/// positions' realized P&L, borrow fees, financing interest and interest income, to Net Account
/// P&L. Returns the steps and the two subtotals. Zero changes are left out; the start and both
/// subtotals are always there. `borrow` and `financing` are amounts paid (positive).
fn waterfall(
    gross: Money,
    closed_fees: Money,
    open_pnl: Money,
    borrow: Money,
    financing: Money,
    income: Money,
) -> Result<(Vec<WaterfallStep>, Money, Money)> {
    let mut steps = vec![level("Gross Trading P&L", gross)];
    let mut running = gross;
    push_change(&mut steps, &mut running, "Fees on Closed Trades", closed_fees.checked_neg()?)?;
    let trading_net = running;
    steps.push(level("Trading Net P&L", trading_net));
    push_change(&mut steps, &mut running, "Open Positions (Realized So Far)", open_pnl)?;
    push_change(&mut steps, &mut running, "Borrow Fees", borrow.checked_neg()?)?;
    push_change(&mut steps, &mut running, "Financing Interest", financing.checked_neg()?)?;
    push_change(&mut steps, &mut running, "Interest Income", income)?;
    let account = running;
    steps.push(level("Net Account P&L", account));
    Ok((steps, trading_net, account))
}

/// Computes comprehensive P&L attribution from a ledger's journal and closed trade lifecycle.
///
/// # Arguments
/// - `ledger`: executed ledger containing journal and positions.
/// - `symbols`: optional symbol lookup slice matching instrument IDs by index.
/// - `slippage_bps`: slippage rate in basis points for the slippage estimate (e.g. 5.0). Required
///   when the ledger has fills.
///
/// # Errors
/// Missing slippage with fills, or numeric overflow.
pub fn compute_pnl_attribution(
    ledger: &Ledger,
    symbols: Option<&[String]>,
    slippage_bps: Option<f64>,
) -> Result<PnlAttribution> {
    let extraction = extract_trades(ledger)?;
    let trades = &extraction.trades;

    // 1. Costs and income from the journal.
    let mut commission_fees = Money::ZERO;
    let mut borrow_fees = Money::ZERO;
    let mut financing_interest = Money::ZERO; // negative entries, summed as signed
    let mut interest_income = Money::ZERO;
    let mut dividends = Money::ZERO;
    let mut coupons = Money::ZERO;
    let mut total_notional = 0.0f64;

    let mut first_fill = None;
    for e in ledger.journal() {
        match e.kind {
            EntryType::Fill => {
                if first_fill.is_none() {
                    first_fill = Some(e);
                }
                commission_fees = commission_fees.checked_add(e.fill.fee)?;
                let n = ledger.notional_of(e.fill.instrument, e.fill.price, e.fill.qty)?.to_f64();
                total_notional += n;
            }
            EntryType::BorrowFee => {
                borrow_fees = borrow_fees.checked_add(e.amount)?;
            }
            EntryType::Interest => {
                if e.amount.raw() < 0 {
                    financing_interest = financing_interest.checked_add(e.amount)?;
                } else if e.amount.raw() > 0 {
                    interest_income = interest_income.checked_add(e.amount)?;
                }
            }
            EntryType::Dividend => {
                dividends = dividends.checked_add(e.amount)?;
            }
            EntryType::Coupon => {
                coupons = coupons.checked_add(e.amount)?;
            }
            _ => {}
        }
    }

    let bps = if let Some(bps) = slippage_bps {
        bps.max(0.0)
    } else {
        if let Some(entry) = first_fill {
            let sym = symbols.and_then(|s| s.get(entry.fill.instrument as usize)).map_or("", String::as_str);
            let sym_str = if sym.is_empty() { entry.fill.instrument.to_string() } else { sym.to_string() };
            return Err(Error::InvalidArgument(format!(
                "attribution requires slippage for each fill; missing for {sym_str} at {}",
                entry.ts_ns
            )));
        }
        0.0
    };
    let slippage_cost_f64 = total_notional * (bps / 10_000.0);

    // 2. Instrument and direction aggregation over closed trades. Money sums stay exact, so the
    //    instruments' net adds up to `total_net_pnl` exactly in fixed point.
    #[derive(Default)]
    struct InstAgg {
        gross: Money,
        fees: Money,
        net: Money,
        trades: usize,
        wins: usize,
        losses: usize,
        long_pnl: Money,
        short_pnl: Money,
    }

    let mut inst_map: BTreeMap<InstrumentId, InstAgg> = BTreeMap::new();
    let mut direction = DirectionalAttribution::default();
    let mut gross_m = Money::ZERO;
    let mut closed_fees_m = Money::ZERO;
    let mut total_wins = 0usize;

    for t in trades {
        gross_m = gross_m.checked_add(t.gross)?;
        closed_fees_m = closed_fees_m.checked_add(t.fees)?;
        let (gross, fees, net) = (t.gross.to_f64(), t.fees.to_f64(), t.net.to_f64());
        let won = t.net.raw() > 0;
        if won {
            total_wins += 1;
        }

        let is_long = t.side > 0;
        if is_long {
            direction.long_gross_pnl += gross;
            direction.long_fees += fees;
            direction.long_net_pnl += net;
            direction.long_trades += 1;
            if won {
                direction.long_wins += 1;
            }
        } else {
            direction.short_gross_pnl += gross;
            direction.short_fees += fees;
            direction.short_net_pnl += net;
            direction.short_trades += 1;
            if won {
                direction.short_wins += 1;
            }
        }

        let entry = inst_map.entry(t.instrument).or_default();
        entry.gross = entry.gross.checked_add(t.gross)?;
        entry.fees = entry.fees.checked_add(t.fees)?;
        entry.net = entry.net.checked_add(t.net)?;
        entry.trades += 1;
        if won {
            entry.wins += 1;
        } else if t.net.raw() < 0 {
            entry.losses += 1;
        }
        if is_long {
            entry.long_pnl = entry.long_pnl.checked_add(t.net)?;
        } else {
            entry.short_pnl = entry.short_pnl.checked_add(t.net)?;
        }
    }

    if direction.long_trades > 0 {
        direction.long_win_rate = direction.long_wins as f64 / direction.long_trades as f64;
    }
    if direction.short_trades > 0 {
        direction.short_win_rate = direction.short_wins as f64 / direction.short_trades as f64;
    }

    let total_trades = trades.len();
    let overall_win_rate = if total_trades > 0 { total_wins as f64 / total_trades as f64 } else { 0.0 };

    let total_abs_net = inst_map.values().map(|v| v.net.to_f64().abs()).sum::<f64>();

    let mut instruments = Vec::new();
    for (inst_id, agg) in inst_map {
        let sym = symbols.and_then(|s| s.get(inst_id as usize)).cloned();
        let wr = if agg.trades > 0 { agg.wins as f64 / agg.trades as f64 } else { 0.0 };
        let net = agg.net.to_f64();
        let pnl_share = if total_abs_net > 0.0 { net / total_abs_net } else { 0.0 };

        instruments.push(InstrumentAttribution {
            instrument: inst_id,
            symbol: sym,
            gross_pnl: agg.gross.to_f64(),
            fees: agg.fees.to_f64(),
            net_pnl: net,
            trades_count: agg.trades,
            win_trades: agg.wins,
            loss_trades: agg.losses,
            win_rate: wr,
            long_pnl: agg.long_pnl.to_f64(),
            short_pnl: agg.short_pnl.to_f64(),
            pnl_share,
        });
    }

    // 3. Costs. Slippage is a memo line (already in fill prices); dividends and coupons are memo
    //    lines (already in trade gross).
    let financing_paid = financing_interest.checked_neg()?;
    let charged = commission_fees.checked_add(borrow_fees)?.checked_add(financing_paid)?;
    let costs = CostDecomposition {
        commission_fees: commission_fees.to_f64(),
        slippage_costs: slippage_cost_f64,
        borrow_fees: borrow_fees.to_f64(),
        financing_interest: financing_paid.to_f64(),
        interest_income: interest_income.to_f64(),
        dividends: dividends.to_f64(),
        coupons: coupons.to_f64(),
        total_costs: charged.to_f64(),
    };

    // 4. Waterfall bridge.
    let open_pnl = extraction.open_gross.checked_sub(extraction.open_fees)?;
    let (waterfall, trading_net, account) =
        waterfall(gross_m, closed_fees_m, open_pnl, borrow_fees, financing_paid, interest_income)?;

    Ok(PnlAttribution {
        total_gross_pnl: gross_m.to_f64(),
        total_fees: commission_fees.to_f64(),
        total_net_pnl: trading_net.to_f64(),
        open_position_pnl: open_pnl.to_f64(),
        total_account_pnl: account.to_f64(),
        total_trades,
        overall_win_rate,
        costs,
        direction,
        instruments,
        waterfall,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::{Fill, LedgerConfig, Price, Qty, Side};

    fn make_test_ledger() -> Ledger {
        let mut l = Ledger::new(LedgerConfig {
            allow_short: true,
            allow_negative_cash: true,
            ..LedgerConfig::default()
        });
        l.deposit(Money::from_f64(100_000.0).unwrap(), 0).unwrap();

        // Trade 1: Long AAPL (inst 0), buy 100 @ 150, sell 100 @ 160 -> +$1,000 gross
        l.apply_fill(&Fill {
            instrument: 0,
            side: Side::Buy,
            price: Price::from_f64(150.0).unwrap(),
            qty: Qty::from_f64(100.0).unwrap(),
            fee: Money::from_f64(5.0).unwrap(),
            ts_ns: 1_000,
        })
        .unwrap();
        l.apply_fill(&Fill {
            instrument: 0,
            side: Side::Sell,
            price: Price::from_f64(160.0).unwrap(),
            qty: Qty::from_f64(100.0).unwrap(),
            fee: Money::from_f64(5.0).unwrap(),
            ts_ns: 2_000,
        })
        .unwrap();

        // Trade 2: Short MSFT (inst 1), sell 50 @ 300, buy 50 @ 310 -> -$500 gross
        l.apply_fill(&Fill {
            instrument: 1,
            side: Side::Sell,
            price: Price::from_f64(300.0).unwrap(),
            qty: Qty::from_f64(50.0).unwrap(),
            fee: Money::from_f64(3.0).unwrap(),
            ts_ns: 3_000,
        })
        .unwrap();
        l.apply_fill(&Fill {
            instrument: 1,
            side: Side::Buy,
            price: Price::from_f64(310.0).unwrap(),
            qty: Qty::from_f64(50.0).unwrap(),
            fee: Money::from_f64(3.0).unwrap(),
            ts_ns: 4_000,
        })
        .unwrap();

        l
    }

    #[test]
    fn pnl_attribution_decomposes_instruments_and_direction() {
        let ledger = make_test_ledger();
        let symbols = vec!["AAPL".to_string(), "MSFT".to_string()];
        let attr = compute_pnl_attribution(&ledger, Some(&symbols), Some(5.0)).unwrap();

        assert_eq!(attr.total_trades, 2);
        assert!((attr.total_gross_pnl - 500.0).abs() < 1e-4);
        assert!((attr.total_fees - 16.0).abs() < 1e-4);
        assert!((attr.overall_win_rate - 0.5).abs() < 1e-4);

        // Directional attribution
        assert_eq!(attr.direction.long_trades, 1);
        assert_eq!(attr.direction.long_wins, 1);
        assert!((attr.direction.long_gross_pnl - 1000.0).abs() < 1e-4);
        assert!((attr.direction.long_net_pnl - 990.0).abs() < 1e-4);

        assert_eq!(attr.direction.short_trades, 1);
        assert_eq!(attr.direction.short_wins, 0);
        assert!((attr.direction.short_gross_pnl - (-500.0)).abs() < 1e-4);
        assert!((attr.direction.short_net_pnl - (-506.0)).abs() < 1e-4);

        // Instruments attribution
        assert_eq!(attr.instruments.len(), 2);
        let aapl = &attr.instruments[0];
        assert_eq!(aapl.symbol.as_deref(), Some("AAPL"));
        assert!((aapl.net_pnl - 990.0).abs() < 1e-4);

        let msft = &attr.instruments[1];
        assert_eq!(msft.symbol.as_deref(), Some("MSFT"));
        assert!((msft.net_pnl - (-506.0)).abs() < 1e-4);

        // Waterfall
        assert!(!attr.waterfall.is_empty());
        assert_eq!(attr.waterfall[0].name, "Gross Trading P&L");
        assert!((attr.waterfall[0].amount - 500.0).abs() < 1e-4);

        let json = attr.to_json();
        assert!(json.dump().contains("waterfall"));
        assert!(json.dump().contains("direction"));
    }
}
