//! M11 Task 3: the attribution in a backtest summary reconciles (shared by several test files).

use spectra_backtest::BacktestOutput;
use spectra_core::{EntryType, Json, Money};

/// Checks the summary's attribution against itself and against the run's journal: the "Trading
/// Net P&L" subtotal is `total_net_pnl`, the last step is `total_account_pnl`, each change moves
/// the running total by its amount, instruments sum to `total_net_pnl`, and `total_account_pnl`
/// is the journal's realized P&L (fills and expiries, dividends, coupons, interest, less fees and
/// borrow fees).
pub fn assert_attribution_reconciles(out: &BacktestOutput, summary: &Json) {
    let a = summary.at("attribution").expect("summary has an attribution");
    let f = |j: &Json, k: &str| j.at(k).unwrap().as_double().unwrap();
    let net = f(a, "total_net_pnl");
    let account = f(a, "total_account_pnl");

    let steps = a.at("waterfall").unwrap().items().unwrap();
    let name = |s: &Json| s.at("name").unwrap().as_str().unwrap().to_string();
    let sub = |s: &Json| s.at("subtotal").unwrap().as_bool().unwrap();
    let trading = steps.iter().find(|s| name(s) == "Trading Net P&L").expect("trading subtotal");
    assert_eq!(f(trading, "running_total"), net, "trading subtotal = total_net_pnl");
    let last = steps.last().unwrap();
    assert_eq!(name(last), "Net Account P&L");
    assert_eq!(f(last, "running_total"), account, "last step = total_account_pnl");

    let mut running = f(&steps[0], "running_total");
    for s in &steps[1..] {
        if !sub(s) {
            assert!((running + f(s, "amount") - f(s, "running_total")).abs() <= 1e-6, "step {}", s.dump());
        }
        running = f(s, "running_total");
    }

    let inst: f64 = a.at("instruments").unwrap().items().unwrap().iter().map(|i| f(i, "net_pnl")).sum();
    assert!((inst - net).abs() <= 1e-6, "instruments {inst} vs net {net}");

    let mut journal = Money::ZERO;
    for e in out.result.ledger.journal() {
        let add = match e.kind {
            EntryType::Fill => e.realized.checked_sub(e.fill.fee).unwrap(),
            EntryType::Expire => e.realized,
            EntryType::Dividend | EntryType::Coupon | EntryType::Interest => e.amount,
            EntryType::BorrowFee => e.amount.checked_neg().unwrap(),
            _ => Money::ZERO,
        };
        journal = journal.checked_add(add).unwrap();
    }
    assert_eq!(account, journal.to_f64(), "total_account_pnl vs the journal");
}
