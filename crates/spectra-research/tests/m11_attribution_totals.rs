//! M11 Task 3: the attribution totals add up.
//!
//! The waterfall is a two-stage bridge (decided 6 Nov 2026):
//!
//! ```text
//! Gross Trading P&L - Fees on Closed Trades                         = Trading Net P&L  (= total_net_pnl)
//! + Open Positions (Realized So Far) - Borrow Fees - Financing Interest
//! + Interest Income                                                 = Net Account P&L  (= total_account_pnl)
//! ```
//!
//! Slippage is a memo line: fill prices already include it, so it is in neither total.
//! Dividends and coupons are part of trade gross, so they are not separate steps.
//!
//! Every case checks the same identities:
//! 1. the "Trading Net P&L" subtotal equals `total_net_pnl`, exactly;
//! 2. the last step ("Net Account P&L") equals `total_account_pnl`, exactly;
//! 3. each change step moves the running total by its amount;
//! 4. per-instrument net, and long plus short net, sum to `total_net_pnl`;
//! 5. `total_account_pnl` equals an independent reconciliation from the journal:
//!    realized P&L of every fill and expiry, plus dividends and coupons, less every fee, plus
//!    interest (signed), less borrow fees.

use spectra_core::{EntryType, Fill, Ledger, LedgerConfig, Money, Price, Qty, Rng, Side};
use spectra_research::attribution::{compute_pnl_attribution, PnlAttribution};

/// Tolerance for sums of a few `f64` values converted from fixed point.
const TOL: f64 = 1e-6;

fn m(v: f64) -> Money {
    Money::from_f64(v).unwrap()
}

fn ledger() -> Ledger {
    let mut l =
        Ledger::new(LedgerConfig { allow_short: true, allow_negative_cash: true, ..LedgerConfig::default() });
    l.deposit(m(100_000.0), 0).unwrap();
    l
}

fn fill(l: &mut Ledger, inst: u32, side: Side, px: f64, qty: f64, fee: f64, ts: i64) {
    l.apply_fill(&Fill {
        instrument: inst,
        side,
        price: Price::from_f64(px).unwrap(),
        qty: Qty::from_f64(qty).unwrap(),
        fee: m(fee),
        ts_ns: ts,
    })
    .unwrap();
}

/// The account's realized P&L straight from the journal, in fixed point.
fn journal_account_pnl(l: &Ledger) -> Money {
    let mut total = Money::ZERO;
    for e in l.journal() {
        let add = match e.kind {
            EntryType::Fill => e.realized.checked_sub(e.fill.fee).unwrap(),
            EntryType::Expire => e.realized,
            EntryType::Dividend | EntryType::Coupon | EntryType::Interest => e.amount,
            EntryType::BorrowFee => e.amount.checked_neg().unwrap(),
            _ => Money::ZERO,
        };
        total = total.checked_add(add).unwrap();
    }
    total
}

fn step<'a>(a: &'a PnlAttribution, name: &str) -> &'a spectra_research::attribution::WaterfallStep {
    a.waterfall
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no step {name:?}: {:#?}", a.waterfall))
}

fn has_step(a: &PnlAttribution, name: &str) -> bool {
    a.waterfall.iter().any(|s| s.name == name)
}

/// Checks identities 1–5 and returns the attribution.
fn check(l: &Ledger, case: &str) -> PnlAttribution {
    let a = compute_pnl_attribution(l, None, Some(10.0)).unwrap();
    let w = &a.waterfall;

    // Shape: starts with gross, ends with the account total, trading net in between.
    assert!(w.len() >= 3, "{case}: {w:#?}");
    assert_eq!(w[0].name, "Gross Trading P&L", "{case}");
    assert!(w[0].subtotal, "{case}");
    assert_eq!(w[0].amount, a.total_gross_pnl, "{case}: gross step");
    let last = w.last().unwrap();
    assert_eq!(last.name, "Net Account P&L", "{case}");
    assert!(last.subtotal, "{case}");

    // 1 and 2: exact, not within a tolerance: both come from the same fixed-point sums.
    let trading = step(&a, "Trading Net P&L");
    assert!(trading.subtotal, "{case}");
    assert_eq!(trading.running_total, a.total_net_pnl, "{case}: trading net subtotal = total_net_pnl");
    assert_eq!(last.running_total, a.total_account_pnl, "{case}: last step = total_account_pnl");

    // 3: each change moves the running total by its amount; a subtotal restates it.
    let mut running = w[0].running_total;
    for s in &w[1..] {
        if s.subtotal {
            assert_eq!(s.amount, s.running_total, "{case}: subtotal {s:?}");
            assert!((s.running_total - running).abs() <= TOL, "{case}: subtotal {s:?} vs running {running}");
        } else {
            assert!(s.amount != 0.0, "{case}: zero step {s:?} should be left out");
            assert!(
                (running + s.amount - s.running_total).abs() <= TOL,
                "{case}: step {s:?} from running {running}"
            );
        }
        running = s.running_total;
    }

    // 4: the breakdowns add up to the trading total.
    let inst_net: f64 = a.instruments.iter().map(|i| i.net_pnl).sum();
    assert!(
        (inst_net - a.total_net_pnl).abs() <= TOL,
        "{case}: instruments {inst_net} vs {}",
        a.total_net_pnl
    );
    let dir_net = a.direction.long_net_pnl + a.direction.short_net_pnl;
    assert!(
        (dir_net - a.total_net_pnl).abs() <= TOL,
        "{case}: long + short {dir_net} vs {}",
        a.total_net_pnl
    );

    // 5: the account total matches the journal, exactly.
    assert_eq!(a.total_account_pnl, journal_account_pnl(l).to_f64(), "{case}: account vs journal");

    // Slippage is a memo line: never a waterfall step, never in total_costs.
    assert!(!w.iter().any(|s| s.name.contains("Slippage")), "{case}: slippage in the bridge");
    let charged = a.costs.commission_fees + a.costs.borrow_fees + a.costs.financing_interest;
    assert!((a.costs.total_costs - charged).abs() <= TOL, "{case}: total_costs excludes slippage");
    a
}

// ------------------------------------------------------------------ fixed cases

#[test]
fn m26_reference_round_trip_with_financing() {
    // Buy 100 at 150, sell 100 at 160, fees 5 + 5, financing charge 20.
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 150.0, 100.0, 5.0, 1_000);
    fill(&mut l, 0, Side::Sell, 160.0, 100.0, 5.0, 2_000);
    l.apply_interest(m(-20.0), 3_000).unwrap();
    let a = check(&l, "m26 reference");
    assert_eq!(a.total_net_pnl, 990.0);
    assert_eq!(a.total_account_pnl, 970.0);
    assert_eq!(step(&a, "Financing Interest").amount, -20.0);
    // The old bridge ended at 939: slippage (31) was charged a second time.
    assert_eq!(a.costs.slippage_costs, 31.0, "still reported, as a memo");
}

#[test]
fn probe_case_from_the_task_3_finding() {
    // The case that showed the old bridge ending at 924 against a net of 990.
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 150.0, 100.0, 5.0, 1_000);
    fill(&mut l, 0, Side::Sell, 160.0, 100.0, 5.0, 2_000);
    fill(&mut l, 0, Side::Buy, 100.0, 100.0, 5.0, 2_500); // still open
    l.apply_interest(m(-20.0), 3_000).unwrap();
    l.apply_interest(m(45.0), 3_500).unwrap();
    let a = check(&l, "probe");
    assert_eq!(a.total_net_pnl, 990.0);
    assert_eq!(a.open_position_pnl, -5.0, "the open buy's fee");
    assert_eq!(step(&a, "Interest Income").amount, 45.0);
    assert_eq!(a.total_account_pnl, 990.0 - 5.0 - 20.0 + 45.0);
}

#[test]
fn no_trades_only_interest() {
    let mut l = ledger();
    l.apply_interest(m(12.5), 1_000).unwrap();
    l.apply_interest(m(-2.5), 2_000).unwrap();
    // No fills, so no slippage rate is needed.
    let a = compute_pnl_attribution(&l, None, None).unwrap();
    assert_eq!(a.total_trades, 0);
    let a2 = check(&l, "interest only");
    assert_eq!(a, a2, "the slippage rate changes nothing without fills");
    assert_eq!(a.total_net_pnl, 0.0);
    assert_eq!(a.total_account_pnl, 10.0);
    assert!(!has_step(&a, "Fees on Closed Trades"));
}

#[test]
fn empty_ledger_is_three_zero_levels() {
    let l = ledger();
    let a = check(&l, "empty");
    let names: Vec<&str> = a.waterfall.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Gross Trading P&L", "Trading Net P&L", "Net Account P&L"]);
    assert_eq!(a.total_account_pnl, 0.0);
}

#[test]
fn short_only_with_borrow_fee() {
    // Short 50 at 200, cover at 190: gross +500, fees 2 + 2, borrow 7.
    let mut l = ledger();
    fill(&mut l, 0, Side::Sell, 200.0, 50.0, 2.0, 1_000);
    l.charge_borrow_fee(0, m(7.0), 1_500).unwrap();
    fill(&mut l, 0, Side::Buy, 190.0, 50.0, 2.0, 2_000);
    let a = check(&l, "short only");
    assert_eq!(a.direction.short_trades, 1);
    assert_eq!(a.direction.long_trades, 0);
    assert_eq!(a.total_net_pnl, 496.0);
    assert_eq!(step(&a, "Borrow Fees").amount, -7.0);
    assert_eq!(a.total_account_pnl, 489.0);
}

#[test]
fn partial_close_leaves_realized_pnl_on_the_open_position() {
    // Buy 100 at 50; sell 40 at 60 (realizes 400); 60 still open; a dividend of 30 while held.
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 50.0, 100.0, 1.0, 1_000);
    fill(&mut l, 0, Side::Sell, 60.0, 40.0, 1.0, 2_000);
    l.apply_dividend(0, m(30.0), 2_500).unwrap();
    let a = check(&l, "partial close");
    assert_eq!(a.total_trades, 0, "no round trip has closed");
    assert_eq!(a.total_net_pnl, 0.0);
    // 400 realized + 30 dividend - 2 fees, all on the open position.
    assert_eq!(a.open_position_pnl, 428.0);
    assert_eq!(a.total_account_pnl, 428.0);
}

#[test]
fn dividend_on_a_closed_trade_is_counted_once() {
    // The old bridge added dividends on top of a gross that already held them.
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 100.0, 10.0, 1.0, 1_000);
    l.apply_dividend(0, m(25.0), 1_500).unwrap();
    fill(&mut l, 0, Side::Sell, 100.0, 10.0, 1.0, 2_000);
    let a = check(&l, "dividend on closed trade");
    assert_eq!(a.total_gross_pnl, 25.0, "the dividend is in trade gross");
    assert_eq!(a.costs.dividends, 25.0, "and reported as a memo");
    assert!(!a.waterfall.iter().any(|s| s.name.contains("Dividend")));
    assert_eq!(a.total_account_pnl, 23.0);
}

#[test]
fn flip_from_long_to_short_in_one_fill() {
    // Buy 10 at 100; sell 30 at 110 closes the long (+100) and opens a short of 20.
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 100.0, 10.0, 1.0, 1_000);
    fill(&mut l, 0, Side::Sell, 110.0, 30.0, 3.0, 2_000);
    let a = check(&l, "flip");
    assert_eq!(a.total_trades, 1);
    // The flipping fill's whole fee goes to the closed trade.
    assert_eq!(a.total_net_pnl, 100.0 - 1.0 - 3.0);
    assert_eq!(a.open_position_pnl, 0.0);
    assert!(!has_step(&a, "Open Positions (Realized So Far)"), "zero steps are left out");
}

#[test]
fn fill_on_the_last_timestamp_closes_the_trade() {
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 10.0, 1.0, 0.0, 1_000);
    fill(&mut l, 1, Side::Buy, 20.0, 1.0, 0.0, 1_000);
    fill(&mut l, 0, Side::Sell, 12.0, 1.0, 0.0, i64::MAX);
    let a = check(&l, "last timestamp");
    assert_eq!(a.total_trades, 1);
    assert_eq!(a.total_net_pnl, 2.0);
    assert!(!has_step(&a, "Fees on Closed Trades"), "no fees, no step");
}

#[test]
fn zero_quantity_fill_is_refused_by_the_ledger() {
    // A zero-quantity fill never reaches attribution: the ledger refuses it.
    let mut l = ledger();
    let bad = Fill {
        instrument: 0,
        side: Side::Buy,
        price: Price::from_f64(10.0).unwrap(),
        qty: Qty::ZERO,
        fee: Money::ZERO,
        ts_ns: 1_000,
    };
    assert!(l.apply_fill(&bad).is_err());
    check(&l, "after a refused fill");
}

#[test]
fn several_instruments_split_the_trading_total() {
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 10.0, 100.0, 1.0, 1_000);
    fill(&mut l, 1, Side::Sell, 50.0, 10.0, 1.0, 1_100);
    fill(&mut l, 0, Side::Sell, 9.5, 100.0, 1.0, 2_000);
    fill(&mut l, 1, Side::Buy, 45.0, 10.0, 1.0, 2_100);
    let a = check(&l, "two instruments");
    assert_eq!(a.instruments.len(), 2);
    assert_eq!(a.instruments[0].net_pnl, -52.0);
    assert_eq!(a.instruments[1].net_pnl, 48.0);
    assert_eq!(a.total_net_pnl, -4.0);
}

#[test]
fn json_carries_the_new_totals_and_subtotal_flags() {
    let mut l = ledger();
    fill(&mut l, 0, Side::Buy, 150.0, 100.0, 5.0, 1_000);
    fill(&mut l, 0, Side::Sell, 160.0, 100.0, 5.0, 2_000);
    let j = check(&l, "json").to_json();
    let f = |k: &str| j.at(k).unwrap().as_double().unwrap();
    assert_eq!(f("total_account_pnl"), 990.0);
    assert_eq!(f("open_position_pnl"), 0.0);
    let costs = j.at("costs").unwrap();
    assert!(costs.at("slippage_included_in_fill_prices").unwrap().as_bool().unwrap());
    let w = j.at("waterfall").unwrap().items().unwrap();
    assert_eq!(w.last().unwrap().at("running_total").unwrap().as_double().unwrap(), f("total_account_pnl"));
    assert!(w[0].at("subtotal").unwrap().as_bool().unwrap());
    assert!(!w[1].at("subtotal").unwrap().as_bool().unwrap());
}

// ------------------------------------------------------------------ randomized

/// Random ledgers: fills on three instruments (opens, adds, partial closes, closes, flips),
/// interest both ways, borrow fees on shorts and dividends on held positions.
#[test]
fn identities_hold_on_random_ledgers() {
    for seed in 1..=300u64 {
        let mut rng = Rng::new(seed);
        let mut l = ledger();
        let mut ts = 1_000i64;
        let steps = 5 + (rng.next_u64() % 40) as usize;
        for _ in 0..steps {
            ts += 1 + (rng.next_u64() % 1_000) as i64;
            let inst = (rng.next_u64() % 3) as u32;
            let r = rng.uniform();
            let held = l.position(inst).qty;
            if r < 0.70 {
                let side = if rng.uniform() < 0.5 { Side::Buy } else { Side::Sell };
                let px = 20.0 + (rng.uniform() * 180.0 * 100.0).round() / 100.0;
                let qty = 1.0 + (rng.next_u64() % 50) as f64;
                let fee = (rng.uniform() * 300.0).round() / 100.0;
                fill(&mut l, inst, side, px, qty, fee, ts);
            } else if r < 0.80 {
                let amt = ((rng.uniform() - 0.5) * 4_000.0).round() / 100.0;
                if amt != 0.0 {
                    l.apply_interest(m(amt), ts).unwrap();
                }
            } else if r < 0.90 {
                if held.raw() < 0 {
                    let amt = 0.01 + (rng.uniform() * 500.0).round() / 100.0;
                    l.charge_borrow_fee(inst, m(amt), ts).unwrap();
                }
            } else if !held.is_zero() {
                let amt = 0.01 + (rng.uniform() * 500.0).round() / 100.0;
                let amt = if held.raw() < 0 { -amt } else { amt };
                l.apply_dividend(inst, m(amt), ts).unwrap();
            }
        }
        check(&l, &format!("seed {seed}"));
    }
}
