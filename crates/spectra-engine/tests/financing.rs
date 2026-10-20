//! Financing (M3 task 9): borrow fees on shorts, interest on cash and on a
//! margin debit. Exact amounts on flat prices, journal entries that re-derive
//! the books, and unchanged fingerprints when financing is off.

use spectra_core::{Bar, EntryType, LedgerConfig, Money, Price, Qty, Ratio, Side, SCALE};
use spectra_engine::{config_fingerprint, run_backtest, BacktestConfig, Context, FinancingConfig, Strategy};

/// `n` daily bars of one instrument at a constant price.
fn flat(n: usize, price: i64) -> Vec<Bar> {
    (0..n)
        .map(|i| Bar {
            ts_ns: (i as i64 + 1) * 86_400_000_000_000,
            instrument: 1,
            open: Price::from_raw(price * SCALE),
            high: Price::from_raw(price * SCALE),
            low: Price::from_raw(price * SCALE),
            close: Price::from_raw(price * SCALE),
            volume: Qty::from_raw(1_000_000 * SCALE),
        })
        .collect()
}

/// Sends one market order on the first bar.
struct Once(Side, i64, bool);
impl Strategy for Once {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        if !self.2 {
            self.2 = true;
            ctx.market(bars[0].instrument, self.0, Qty::from_raw(self.1 * SCALE));
        }
    }
}

fn money(v: f64) -> Money {
    Money::from_f64(v).unwrap()
}

fn entries(r: &spectra_engine::BacktestResult, kind: EntryType) -> Vec<Money> {
    r.ledger.journal().iter().filter(|e| e.kind == kind).map(|e| e.amount).collect()
}

fn base() -> BacktestConfig {
    BacktestConfig {
        initial_cash: Money::from_raw(100_000 * SCALE),
        periods_per_year: 252.0,
        ..BacktestConfig::default()
    }
}

#[test]
fn borrow_fee_is_charged_each_period_on_short_value() {
    // Short 10 at 100 = 1,000 of short value; 25.2% a year over 252 periods = 0.1% a period = 1.00.
    let cfg = BacktestConfig {
        ledger: LedgerConfig { allow_short: true, ..LedgerConfig::default() },
        financing: FinancingConfig { borrow_rate: 0.252, ..FinancingConfig::default() },
        ..base()
    };
    let r = run_backtest(&flat(11, 100), &mut Once(Side::Sell, 10, false), &cfg).unwrap();
    let fees = entries(&r, EntryType::BorrowFee);
    // Filled at bar 2's open; charged at bars 3..11 for the periods it was held.
    assert_eq!(fees, vec![money(1.0); 9]);
    assert!(r
        .ledger
        .journal()
        .iter()
        .filter(|e| e.kind == EntryType::BorrowFee)
        .all(|e| e.fill.instrument == 1));
    assert_eq!(r.ledger.financing(), money(-9.0));
    assert_eq!(r.ledger.equity().unwrap(), money(100_000.0 - 9.0));
    assert_eq!(r.equity_curve.last().unwrap().equity, money(100_000.0 - 9.0));
    r.ledger.check_invariants().unwrap();
}

#[test]
fn cash_earns_interest_that_compounds() {
    // 2.52% a year over 252 periods = 0.01% a period.
    let cfg = BacktestConfig {
        financing: FinancingConfig { cash_rate: 0.0252, ..FinancingConfig::default() },
        ..base()
    };
    let r = run_backtest(&flat(4, 100), &mut Once(Side::Buy, 0, true), &cfg).unwrap();
    let interest = entries(&r, EntryType::Interest);
    assert_eq!(interest, vec![money(10.0), money(10.001), money(10.002_000_1)]);
    assert_eq!(r.ledger.cash(), money(100_030.003_000_1));
    r.ledger.check_invariants().unwrap();
}

#[test]
fn a_margin_debit_pays_interest() {
    // Buy 1,500 at 100 with 100,000: cash -50,000. 2.52% a year = 0.01% a period = 5.00, then 5.0005.
    let cfg = BacktestConfig {
        ledger: LedgerConfig {
            allow_negative_cash: true,
            max_gross_leverage: Some(Ratio::from_raw(2 * SCALE)),
            ..LedgerConfig::default()
        },
        financing: FinancingConfig { margin_rate: 0.0252, ..FinancingConfig::default() },
        ..base()
    };
    let r = run_backtest(&flat(4, 100), &mut Once(Side::Buy, 1500, false), &cfg).unwrap();
    assert_eq!(entries(&r, EntryType::Interest), vec![money(-5.0), money(-5.0005)]);
    assert_eq!(r.ledger.cash(), money(-50_010.000_5));
    assert_eq!(r.ledger.financing(), money(-10.0005));
    r.ledger.check_invariants().unwrap();
}

#[test]
fn financing_off_changes_nothing() {
    let bars = flat(20, 100);
    let plain = base();
    let explicit_zero = BacktestConfig { financing: FinancingConfig::default(), ..base() };
    assert_eq!(config_fingerprint(&plain), config_fingerprint(&explicit_zero));
    let a = run_backtest(&bars, &mut Once(Side::Buy, 10, false), &plain).unwrap();
    assert!(!a.ledger.journal().iter().any(|e| matches!(e.kind, EntryType::Interest | EntryType::BorrowFee)));

    // Any rate changes the configuration fingerprint.
    let with = BacktestConfig {
        financing: FinancingConfig { cash_rate: 0.01, ..FinancingConfig::default() },
        ..base()
    };
    assert_ne!(config_fingerprint(&with), config_fingerprint(&plain));
}

#[test]
fn non_finite_rates_are_refused() {
    let cfg = BacktestConfig {
        financing: FinancingConfig { borrow_rate: f64::NAN, ..FinancingConfig::default() },
        ..base()
    };
    let err = run_backtest(&flat(3, 100), &mut Once(Side::Buy, 1, false), &cfg).unwrap_err();
    assert!(err.to_string().contains("borrow_rate"), "{err}");
}
