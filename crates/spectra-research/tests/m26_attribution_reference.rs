//! M26 task 3e (part 4): reference-value tests for P&L attribution
//! (`src/attribution.rs`, `compute_pnl_attribution`), built from a small ledger.
//!
//! Scenario: deposit 100,000; buy 100 at 150 (fee 5); sell 100 at 160 (fee 5);
//! a financing charge of 20 (negative interest). Expected values are the sums
//! the doc describes (`docs/models/research-modules.md`), worked by hand:
//! gross = (160 - 150) * 100 = 1,000; fees = 10; net = 990; notional = 15,000 + 16,000
//! = 31,000; slippage cost = 31,000 * 10 / 10,000 = 31; financing = 20.
//!
//! Not covered here: positive interest income is dropped (a documented defect,
//! task 8), and an unknown slippage defaults to zero (task 8). Neither is encoded.

use spectra_core::{Fill, Ledger, LedgerConfig, Money, Price, Qty, Side};
use spectra_research::attribution::compute_pnl_attribution;

const TOL: f64 = 1e-9;

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: got {got}, want {want} (tol {TOL})");
}

fn scenario_with_interest(interest: Option<f64>) -> Ledger {
    let mut l =
        Ledger::new(LedgerConfig { allow_short: true, allow_negative_cash: true, ..LedgerConfig::default() });
    l.deposit(Money::from_f64(100_000.0).unwrap(), 0).unwrap();
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
    if let Some(amt) = interest {
        l.apply_interest(Money::from_f64(amt).unwrap(), 3_000).unwrap();
    }
    l
}

fn scenario() -> Ledger {
    scenario_with_interest(Some(-20.0))
}

#[test]
fn gross_fees_and_net_of_one_round_trip() {
    let a = compute_pnl_attribution(&scenario(), None, Some(10.0)).unwrap();
    close(a.total_gross_pnl, 1_000.0, "gross P&L");
    close(a.total_fees, 10.0, "fees");
    close(a.total_net_pnl, 990.0, "net P&L");
    assert_eq!(a.total_trades, 1);
    close(a.overall_win_rate, 1.0, "win rate");
}

#[test]
fn costs_are_commission_slippage_and_financing() {
    let a = compute_pnl_attribution(&scenario(), None, Some(10.0)).unwrap();
    close(a.costs.commission_fees, 10.0, "commission");
    // Notional 15,000 + 16,000 = 31,000 at 10 bps.
    close(a.costs.slippage_costs, 31.0, "slippage");
    close(a.costs.financing_interest, 20.0, "financing (absolute)");
    close(a.costs.total_costs, 61.0, "total costs");
}

#[test]
fn a_single_instrument_carries_the_whole_result_and_direction() {
    let a = compute_pnl_attribution(&scenario(), None, Some(10.0)).unwrap();
    assert_eq!(a.instruments.len(), 1);
    let i = &a.instruments[0];
    close(i.gross_pnl, 1_000.0, "instrument gross");
    close(i.net_pnl, 990.0, "instrument net");
    close(i.fees, 10.0, "instrument fees");
    close(i.pnl_share, 1.0, "instrument share of |net|");
    assert_eq!(i.win_trades, 1);
    assert_eq!(i.loss_trades, 0);
    close(i.long_pnl, 990.0, "the round trip was long");
    close(i.short_pnl, 0.0, "no short P&L");
    close(a.direction.long_net_pnl, 990.0, "long net");
    assert_eq!(a.direction.long_trades, 1);
    assert_eq!(a.direction.short_trades, 0);
}

#[test]
fn positive_interest_is_reported_as_interest_income() {
    let baseline = compute_pnl_attribution(&scenario_with_interest(None), None, Some(10.0)).unwrap();
    let with_interest =
        compute_pnl_attribution(&scenario_with_interest(Some(45.0)), None, Some(10.0)).unwrap();

    close(with_interest.costs.interest_income, 45.0, "interest income (absolute)");
    close(with_interest.costs.financing_interest, 0.0, "financing interest");
    close(with_interest.total_net_pnl, baseline.total_net_pnl, "total net P&L unchanged");
}

#[test]
fn attribution_requires_slippage_when_fills_present() {
    let err = compute_pnl_attribution(&scenario(), None, None).unwrap_err();
    match err {
        spectra_core::Error::InvalidArgument(msg) => {
            assert!(
                msg.contains("attribution requires slippage for each fill"),
                "unexpected error message: {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}
