//! Trade-level analysis: round trips from the ledger, trade statistics, and
//! resampling of the trade sequence.
//!
//! A TRADE is one position lifecycle in one instrument: it opens when the
//! position leaves zero, may be scaled in and partially closed, and ends when
//! the position returns to zero or flips sign. Realized P&L comes from the
//! journal (average cost); ALL fees on the trade's fills are charged to it,
//! and a flipping fill's fee is charged to the trade it closes. Positions
//! still open at the end are counted separately.

use std::collections::BTreeMap;

use spectra_core::{EntryType, InstrumentId, Ledger, Money, Qty, Result, Rng, Side};

use crate::stats::nearest_rank_index;

/// One closed round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Trade {
    /// Instrument.
    pub instrument: InstrumentId,
    /// +1 long, -1 short.
    pub side: i32,
    /// Opening fill time.
    pub open_ts: i64,
    /// Closing fill time.
    pub close_ts: i64,
    /// Realized P&L before fees.
    pub gross: Money,
    /// Fees.
    pub fees: Money,
    /// gross - fees.
    pub net: Money,
}

impl Trade {
    /// Holding time in nanoseconds.
    pub fn holding_ns(&self) -> i64 {
        self.close_ts - self.open_ts
    }
}

/// Closed trades and the count of positions still open.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TradeExtraction {
    /// Closed trades in closing order.
    pub trades: Vec<Trade>,
    /// Instruments still holding a position at the end.
    pub open_positions: usize,
}

#[derive(Default, Clone, Copy)]
struct Open {
    qty: Qty,
    side: i32,
    open_ts: i64,
    gross: Money,
    fees: Money,
}

/// Extracts round trips from the journal.
///
/// # Errors
/// Overflow (impossible for a ledger that passed its invariants).
pub fn extract_trades(ledger: &Ledger) -> Result<TradeExtraction> {
    let mut out = TradeExtraction::default();
    let mut state: BTreeMap<InstrumentId, Open> = BTreeMap::new();
    for e in ledger.journal() {
        match e.kind {
            EntryType::Fill => {}
            // A split changes the open quantity, not the trade.
            EntryType::Split => {
                if let Some(s) = state.get_mut(&e.fill.instrument) {
                    if !s.qty.is_zero() {
                        s.qty = e.fill.qty;
                    }
                }
                continue;
            }
            // A dividend is part of the open trade's P&L.
            EntryType::Dividend => {
                if let Some(s) = state.get_mut(&e.fill.instrument) {
                    if !s.qty.is_zero() {
                        s.gross = s.gross.checked_add(e.amount)?;
                    }
                }
                continue;
            }
            _ => continue,
        }
        let f = &e.fill;
        let fq = if f.side == Side::Buy { f.qty } else { f.qty.checked_neg()? };
        let s = state.entry(f.instrument).or_default();
        if s.qty.is_zero() {
            *s = Open { side: fq.signum() as i32, open_ts: f.ts_ns, ..Open::default() };
        }
        s.gross = s.gross.checked_add(e.realized)?;
        s.fees = s.fees.checked_add(f.fee)?;
        let post = s.qty.checked_add(fq)?;
        let closed = post.is_zero();
        let flipped = !post.is_zero() && post.signum() as i32 != s.side;
        if closed || flipped {
            out.trades.push(Trade {
                instrument: f.instrument,
                side: s.side,
                open_ts: s.open_ts,
                close_ts: f.ts_ns,
                gross: s.gross,
                fees: s.fees,
                net: s.gross.checked_sub(s.fees)?,
            });
            *s = Open::default();
            if flipped {
                // The new trade starts on the flipping fill; its fee went to the old one.
                *s = Open { qty: post, side: post.signum() as i32, open_ts: f.ts_ns, ..Open::default() };
            }
        } else {
            s.qty = post;
        }
    }
    out.open_positions = state.values().filter(|s| !s.qty.is_zero()).count();
    Ok(out)
}

/// Statistics of closed trades.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TradeStats {
    /// Number of trades.
    pub n: usize,
    /// Trades with net > 0.
    pub wins: usize,
    /// Trades with net < 0.
    pub losses: usize,
    /// wins / n.
    pub win_rate: f64,
    /// Mean net P&L of winners.
    pub avg_win: f64,
    /// Mean net P&L of losers, as a positive number.
    pub avg_loss: f64,
    /// avg_win / avg_loss (0 if no losers or winners).
    pub payoff_ratio: f64,
    /// Mean net P&L per trade.
    pub expectancy: f64,
    /// Gross profit / gross loss (+inf if no losers, 0 if no winners).
    pub profit_factor: f64,
    /// Sum of net P&L.
    pub total_net: f64,
    /// Best trade.
    pub best: f64,
    /// Worst trade.
    pub worst: f64,
    /// Best trade / total profit of winners (0 if no winners).
    pub top_trade_share: f64,
    /// Mean holding time, nanoseconds.
    pub avg_holding_ns: f64,
}

/// Computes [`TradeStats`].
pub fn compute_trade_stats(trades: &[Trade]) -> TradeStats {
    let mut st = TradeStats { n: trades.len(), ..TradeStats::default() };
    if trades.is_empty() {
        return st;
    }
    let (mut win_sum, mut loss_sum, mut hold) = (0.0, 0.0, 0.0);
    st.best = f64::NEG_INFINITY;
    st.worst = f64::INFINITY;
    for t in trades {
        let v = t.net.to_f64();
        st.total_net += v;
        hold += t.holding_ns() as f64;
        st.best = st.best.max(v);
        st.worst = st.worst.min(v);
        if t.net.raw() > 0 {
            st.wins += 1;
            win_sum += v;
        } else if t.net.raw() < 0 {
            st.losses += 1;
            loss_sum += -v;
        }
    }
    let n = st.n as f64;
    st.win_rate = st.wins as f64 / n;
    st.expectancy = st.total_net / n;
    st.avg_holding_ns = hold / n;
    if st.wins > 0 {
        st.avg_win = win_sum / st.wins as f64;
    }
    if st.losses > 0 {
        st.avg_loss = loss_sum / st.losses as f64;
    }
    if st.wins > 0 && st.losses > 0 {
        st.payoff_ratio = st.avg_win / st.avg_loss;
    }
    if loss_sum > 0.0 {
        st.profit_factor = win_sum / loss_sum;
    } else if win_sum > 0.0 {
        st.profit_factor = f64::INFINITY;
    }
    if win_sum > 0.0 {
        st.top_trade_share = st.best.max(0.0) / win_sum;
    }
    st
}

/// 5th, 50th and 95th percentiles.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PercentileBand {
    /// 5th percentile.
    pub p05: f64,
    /// Median.
    pub p50: f64,
    /// 95th percentile.
    pub p95: f64,
}

/// Monte Carlo over the order and identity of trades (net P&L as fixed
/// dollar amounts). Bootstrap draws n trades with replacement; reshuffle
/// permutes the same trades (only the path, so the drawdown, changes).
/// Trades are treated as exchangeable and drawdown is measured on closed
/// trades only, so it understates true drawdown.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TradeResampling {
    /// False with fewer than 2 trades.
    pub valid: bool,
    /// Resamples per method.
    pub resamples: usize,
    /// Drawdown in the order trades happened.
    pub observed_max_drawdown: f64,
    /// Bootstrap terminal P&L.
    pub bootstrap_terminal_pnl: PercentileBand,
    /// Bootstrap max drawdown.
    pub bootstrap_max_drawdown: PercentileBand,
    /// Fraction of bootstrap resamples ending below the starting equity.
    pub bootstrap_prob_loss: f64,
    /// Reshuffled max drawdown.
    pub reshuffle_max_drawdown: PercentileBand,
    /// Fraction of orderings with a deeper drawdown than observed.
    pub reshuffle_prob_worse_drawdown: f64,
}

fn band(mut v: Vec<f64>) -> PercentileBand {
    if v.is_empty() {
        return PercentileBand::default();
    }
    v.sort_by(f64::total_cmp);
    let at = |p: f64| v[nearest_rank_index(v.len(), p)];
    PercentileBand { p05: at(0.05), p50: at(0.50), p95: at(0.95) }
}

fn max_drawdown_of(pnl: &[f64], e0: f64) -> f64 {
    let (mut eq, mut peak, mut dd) = (e0, e0, 0.0f64);
    for p in pnl {
        eq += p;
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 {
            dd = dd.max((peak - eq) / peak);
        }
    }
    dd
}

/// Bootstrap and reshuffle the trade sequence.
pub fn resample_trades(pnl: &[f64], e0: f64, resamples: usize, seed: u64) -> TradeResampling {
    let mut r = TradeResampling::default();
    if pnl.len() < 2 || resamples == 0 {
        return r;
    }
    r.valid = true;
    r.resamples = resamples;
    r.observed_max_drawdown = max_drawdown_of(pnl, e0);

    let mut rng = Rng::new(seed);
    let n = pnl.len();
    let mut draw = vec![0.0; n];
    let mut terminal = Vec::with_capacity(resamples);
    let mut boot_dd = Vec::with_capacity(resamples);
    let mut losses = 0usize;
    for _ in 0..resamples {
        let mut total = 0.0;
        for d in &mut draw {
            *d = pnl[rng.range(0, n as i64 - 1) as usize];
            total += *d;
        }
        terminal.push(total);
        boot_dd.push(max_drawdown_of(&draw, e0));
        if total < 0.0 {
            losses += 1;
        }
    }
    r.bootstrap_terminal_pnl = band(terminal);
    r.bootstrap_max_drawdown = band(boot_dd);
    r.bootstrap_prob_loss = losses as f64 / resamples as f64;

    let mut perm = pnl.to_vec();
    let mut shuffle_dd = Vec::with_capacity(resamples);
    let mut worse = 0usize;
    for _ in 0..resamples {
        for i in (2..=n).rev() {
            let j = rng.range(0, i as i64 - 1) as usize;
            perm.swap(i - 1, j);
        }
        let dd = max_drawdown_of(&perm, e0);
        shuffle_dd.push(dd);
        if dd > r.observed_max_drawdown + 1e-12 {
            worse += 1;
        }
    }
    r.reshuffle_max_drawdown = band(shuffle_dd);
    r.reshuffle_prob_worse_drawdown = worse as f64 / resamples as f64;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_core::{Fill, LedgerConfig, Price};

    fn fill(ts: i64, side: Side, qty: f64, px: f64, fee: f64) -> Fill {
        Fill {
            instrument: 1,
            side,
            qty: Qty::from_f64(qty).unwrap(),
            price: Price::from_f64(px).unwrap(),
            fee: Money::from_f64(fee).unwrap(),
            ts_ns: ts,
        }
    }

    #[test]
    fn round_trips_flips_and_open_positions() {
        let mut l = Ledger::new(LedgerConfig::reference(true, false));
        l.deposit(Money::from_f64(10_000.0).unwrap(), 0).unwrap();
        l.apply_fill(&fill(1, Side::Buy, 10.0, 100.0, 1.0)).unwrap();
        l.apply_fill(&fill(2, Side::Sell, 15.0, 110.0, 1.0)).unwrap(); // closes long (+100), opens short 5
        l.apply_fill(&fill(3, Side::Buy, 5.0, 100.0, 1.0)).unwrap(); // closes short (+50)
        l.apply_fill(&fill(4, Side::Buy, 1.0, 100.0, 0.0)).unwrap(); // stays open
        let x = extract_trades(&l).unwrap();
        assert_eq!(x.trades.len(), 2);
        assert_eq!(x.trades[0].net, Money::from_f64(98.0).unwrap());
        assert_eq!(x.trades[0].side, 1);
        assert_eq!(x.trades[1].side, -1);
        assert_eq!(x.trades[1].net, Money::from_f64(49.0).unwrap());
        assert_eq!(x.open_positions, 1);
        let st = compute_trade_stats(&x.trades);
        assert_eq!(st.wins, 2);
        assert_eq!(st.profit_factor, f64::INFINITY);
        assert!((st.top_trade_share - 98.0 / 147.0).abs() < 1e-12);
    }

    #[test]
    fn resampling_is_deterministic() {
        let pnl = [10.0, -5.0, 3.0, -8.0, 12.0, -1.0];
        let a = resample_trades(&pnl, 100.0, 500, 7);
        let b = resample_trades(&pnl, 100.0, 500, 7);
        assert_eq!(a, b);
        assert!(a.valid);
        assert!(!resample_trades(&pnl[..1], 100.0, 500, 7).valid);
    }
}
