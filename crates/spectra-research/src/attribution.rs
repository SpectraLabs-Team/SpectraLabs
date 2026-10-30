//! P&L Attribution analysis (Module 11).
//!
//! Decomposes realized and unrealized strategy returns by:
//! - Instrument: per-symbol gross, fees, net, trade count, win rate, and return share.
//! - Direction: long vs. short gross and net P&L, trade counts and win ratios.
//! - Execution Costs: commissions, slippage, financing interest, and borrow fees.
//! - Attribution Waterfall: step-by-step Bridge from gross trading performance to bottom-line net return.

use std::collections::BTreeMap;

use spectra_core::{EntryType, InstrumentId, Json, Ledger, Money, Result};

use crate::trades::extract_trades;

/// Instrument-level P&L attribution record.
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

/// Long vs. short directional attribution.
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

/// Detailed breakdown of trading, market impact, and financing costs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CostDecomposition {
    pub commission_fees: f64,
    pub slippage_costs: f64,
    pub borrow_fees: f64,
    pub financing_interest: f64,
    pub dividends: f64,
    pub total_costs: f64,
}

/// A single step in the waterfall bridge from gross trading profit to net P&L.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaterfallStep {
    pub name: String,
    pub amount: f64,
    pub running_total: f64,
}

/// Full strategy P&L attribution result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PnlAttribution {
    pub total_gross_pnl: f64,
    pub total_fees: f64,
    pub total_net_pnl: f64,
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
            .with("borrow_fees", num(self.costs.borrow_fees))
            .with("financing_interest", num(self.costs.financing_interest))
            .with("dividends", num(self.costs.dividends))
            .with("total_costs", num(self.costs.total_costs));

        let mut waterfall_arr = Vec::new();
        for step in &self.waterfall {
            waterfall_arr.push(
                Json::object()
                    .with("name", Json::string(&step.name))
                    .with("amount", num(step.amount))
                    .with("running_total", num(step.running_total)),
            );
        }

        Json::object()
            .with("total_gross_pnl", num(self.total_gross_pnl))
            .with("total_fees", num(self.total_fees))
            .with("total_net_pnl", num(self.total_net_pnl))
            .with("total_trades", Json::integer(self.total_trades as i64))
            .with("overall_win_rate", num(self.overall_win_rate))
            .with("direction", direction_obj)
            .with("costs", costs_obj)
            .with("instruments", Json::Array(inst_arr))
            .with("waterfall", Json::Array(waterfall_arr))
    }
}

/// Computes comprehensive P&L attribution from a ledger's journal and closed trade lifecycle.
///
/// # Arguments
/// - `ledger`: executed ledger containing journal and positions.
/// - `symbols`: optional symbol lookup slice matching instrument IDs by index.
/// - `slippage_bps`: optional slippage rate in basis points for execution cost estimation (e.g. 5.0).
///
/// # Errors
/// Returns an error if monetary calculations or trade extraction encounter numeric overflow.
pub fn compute_pnl_attribution(
    ledger: &Ledger,
    symbols: Option<&[String]>,
    slippage_bps: Option<f64>,
) -> Result<PnlAttribution> {
    let extraction = extract_trades(ledger)?;
    let trades = &extraction.trades;

    // 1. Costs and Journal Accounting
    let mut commission_fees = Money::ZERO;
    let mut borrow_fees = Money::ZERO;
    let mut financing_interest = Money::ZERO;
    let mut dividends = Money::ZERO;
    let mut total_notional = 0.0f64;

    for e in ledger.journal() {
        match e.kind {
            EntryType::Fill => {
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
                }
            }
            EntryType::Dividend => {
                dividends = dividends.checked_add(e.amount)?;
            }
            _ => {}
        }
    }

    let bps = slippage_bps.unwrap_or(0.0).max(0.0);
    let slippage_cost_f64 = total_notional * (bps / 10_000.0);

    // 2. Instrument & Direction Aggregation
    #[derive(Default)]
    struct InstAgg {
        gross: f64,
        fees: f64,
        net: f64,
        trades: usize,
        wins: usize,
        losses: usize,
        long_pnl: f64,
        short_pnl: f64,
    }

    let mut inst_map: BTreeMap<InstrumentId, InstAgg> = BTreeMap::new();
    let mut direction = DirectionalAttribution::default();
    let mut total_gross_pnl = 0.0f64;
    let mut total_net_pnl = 0.0f64;
    let mut total_wins = 0usize;

    for t in trades {
        let gross = t.gross.to_f64();
        let fees = t.fees.to_f64();
        let net = t.net.to_f64();

        total_gross_pnl += gross;
        total_net_pnl += net;
        if net > 0.0 {
            total_wins += 1;
        }

        let is_long = t.side > 0;
        if is_long {
            direction.long_gross_pnl += gross;
            direction.long_fees += fees;
            direction.long_net_pnl += net;
            direction.long_trades += 1;
            if net > 0.0 {
                direction.long_wins += 1;
            }
        } else {
            direction.short_gross_pnl += gross;
            direction.short_fees += fees;
            direction.short_net_pnl += net;
            direction.short_trades += 1;
            if net > 0.0 {
                direction.short_wins += 1;
            }
        }

        let entry = inst_map.entry(t.instrument).or_default();
        entry.gross += gross;
        entry.fees += fees;
        entry.net += net;
        entry.trades += 1;
        if net > 0.0 {
            entry.wins += 1;
        } else if net < 0.0 {
            entry.losses += 1;
        }
        if is_long {
            entry.long_pnl += net;
        } else {
            entry.short_pnl += net;
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

    let total_abs_net = inst_map.values().map(|v| v.net.abs()).sum::<f64>();

    let mut instruments = Vec::new();
    for (inst_id, agg) in inst_map {
        let sym = symbols.and_then(|s| s.get(inst_id as usize)).cloned();
        let wr = if agg.trades > 0 { agg.wins as f64 / agg.trades as f64 } else { 0.0 };
        let pnl_share = if total_abs_net > 0.0 { agg.net / total_abs_net } else { 0.0 };

        instruments.push(InstrumentAttribution {
            instrument: inst_id,
            symbol: sym,
            gross_pnl: agg.gross,
            fees: agg.fees,
            net_pnl: agg.net,
            trades_count: agg.trades,
            win_trades: agg.wins,
            loss_trades: agg.losses,
            win_rate: wr,
            long_pnl: agg.long_pnl,
            short_pnl: agg.short_pnl,
            pnl_share,
        });
    }

    // 3. Cost Decomposition
    let comm_f64 = commission_fees.to_f64();
    let borrow_f64 = borrow_fees.to_f64();
    let int_f64 = financing_interest.to_f64().abs();
    let div_f64 = dividends.to_f64();
    let total_costs = comm_f64 + slippage_cost_f64 + borrow_f64 + int_f64;

    let costs = CostDecomposition {
        commission_fees: comm_f64,
        slippage_costs: slippage_cost_f64,
        borrow_fees: borrow_f64,
        financing_interest: int_f64,
        dividends: div_f64,
        total_costs,
    };

    // 4. Waterfall Bridge
    let mut waterfall = Vec::new();
    let mut running = total_gross_pnl;
    waterfall.push(WaterfallStep {
        name: "Gross Trading P&L".into(),
        amount: total_gross_pnl,
        running_total: running,
    });

    if comm_f64 > 0.0 {
        running -= comm_f64;
        waterfall.push(WaterfallStep {
            name: "Commissions & Fees".into(),
            amount: -comm_f64,
            running_total: running,
        });
    }

    if slippage_cost_f64 > 0.0 {
        running -= slippage_cost_f64;
        waterfall.push(WaterfallStep {
            name: "Slippage & Market Impact".into(),
            amount: -slippage_cost_f64,
            running_total: running,
        });
    }

    let borrow_and_financing = borrow_f64 + int_f64;
    if borrow_and_financing > 0.0 {
        running -= borrow_and_financing;
        waterfall.push(WaterfallStep {
            name: "Financing & Borrow Costs".into(),
            amount: -borrow_and_financing,
            running_total: running,
        });
    }

    if div_f64 != 0.0 {
        running += div_f64;
        waterfall.push(WaterfallStep {
            name: "Dividends Received".into(),
            amount: div_f64,
            running_total: running,
        });
    }

    waterfall.push(WaterfallStep {
        name: "Net Strategy Realized P&L".into(),
        amount: running,
        running_total: running,
    });

    Ok(PnlAttribution {
        total_gross_pnl,
        total_fees: comm_f64,
        total_net_pnl,
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
