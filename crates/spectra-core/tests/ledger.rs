//! Ledger tests: a line-by-line port of the C++ reference tests
//! (`tests/test_ledger.cpp`, `tests/test_ledger_property.cpp`), plus tests for
//! the gross leverage limit that the C++ engine lacked.

use spectra_core::fixed::{MAX_RAW, SCALE};
use spectra_core::Ratio;
use spectra_core::*;

fn m(v: f64) -> Money {
    Money::from_f64(v).unwrap()
}
fn p(v: f64) -> Price {
    Price::from_f64(v).unwrap()
}
fn q(v: f64) -> Qty {
    Qty::from_f64(v).unwrap()
}
fn fill(id: InstrumentId, side: Side, qty: f64, price: f64, fee: f64) -> Fill {
    Fill { instrument: id, side, qty: q(qty), price: p(price), fee: m(fee), ts_ns: 0 }
}
fn rej(r: Reject) -> std::result::Result<(), LedgerError> {
    Err(LedgerError::Rejected(r))
}
fn expect_invariants(l: &Ledger) {
    if let Err(e) = l.check_invariants() {
        panic!("invariant violated: {e}");
    }
}

/// The C++ reference policy (no leverage limit), so ported tests keep their
/// original meaning.
fn reference(allow_short: bool, allow_negative_cash: bool) -> Ledger {
    Ledger::new(LedgerConfig::reference(allow_short, allow_negative_cash))
}

const AAPL: InstrumentId = 1;

#[test]
fn deposit_and_withdraw() {
    let mut l = reference(false, false);
    assert_eq!(l.deposit(m(1000.0), 0), Ok(()));
    assert_eq!(l.cash(), m(1000.0));
    assert_eq!(l.deposit(m(0.0), 0), rej(Reject::InvalidAmount));
    assert_eq!(l.deposit(m(-5.0), 0), rej(Reject::InvalidAmount));
    assert_eq!(l.withdraw(m(400.0), 0), Ok(()));
    assert_eq!(l.withdraw(m(700.0), 0), rej(Reject::InsufficientCash));
    assert_eq!(l.cash(), m(600.0));
    assert_eq!(l.net_deposits(), m(600.0));
    assert_eq!(l.journal().len(), 2);
    expect_invariants(&l);
}

#[test]
fn long_round_trip_realizes_pnl_and_fees() {
    let mut l = reference(false, false);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 100.0, 1.0)).unwrap();
    assert_eq!(l.cash(), m(8_999.0));
    assert_eq!(l.position(AAPL).qty, q(10.0));
    assert_eq!(l.position(AAPL).cost, m(1_000.0));

    l.mark(AAPL, p(110.0)).unwrap();
    assert_eq!(l.unrealized_pnl().unwrap(), m(100.0));
    assert_eq!(l.equity().unwrap(), m(10_099.0));

    l.apply_fill(&fill(AAPL, Side::Sell, 10.0, 110.0, 1.0)).unwrap();
    assert_eq!(l.cash(), m(10_098.0));
    assert_eq!(l.realized_pnl(), m(100.0));
    assert_eq!(l.total_fees(), m(2.0));
    assert!(l.position(AAPL).qty.is_zero());
    assert!(l.position(AAPL).cost.is_zero());
    assert_eq!(l.equity().unwrap(), l.net_deposits() + l.realized_pnl() - l.total_fees());
    expect_invariants(&l);
}

#[test]
fn rejected_fill_changes_nothing() {
    let mut l = reference(false, false);
    l.deposit(m(100.0), 0).unwrap();
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 100.0, 0.0)), rej(Reject::InsufficientCash));
    // Notional fits but the fee pushes the total over the available cash.
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 1.0, 100.0, 1.0)), rej(Reject::InsufficientCash));
    assert_eq!(l.cash(), m(100.0));
    assert_eq!(l.journal().len(), 1);
    assert!(l.position(AAPL).qty.is_zero());
    expect_invariants(&l);
}

#[test]
fn rejects_malformed_fills() {
    let mut l = reference(false, false);
    l.deposit(m(1000.0), 0).unwrap();
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 0.0, 100.0, 0.0)), rej(Reject::InvalidFill));
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 1.0, 0.0, 0.0)), rej(Reject::InvalidFill));
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 1.0, 100.0, -1.0)), rej(Reject::InvalidFill));
    let dust = Fill { instrument: AAPL, qty: Qty::from_raw(1), price: Price::from_raw(1), ..Fill::default() };
    assert_eq!(l.apply_fill(&dust), rej(Reject::InvalidFill)); // notional rounds to zero
    assert_eq!(l.mark(AAPL, p(0.0)), rej(Reject::InvalidFill));
    assert_eq!(l.journal().len(), 1);
}

#[test]
fn short_selling_respects_config() {
    let mut no_short = reference(false, false);
    no_short.deposit(m(1000.0), 0).unwrap();
    assert_eq!(no_short.apply_fill(&fill(AAPL, Side::Sell, 1.0, 100.0, 0.0)), rej(Reject::ShortNotAllowed));
    assert_eq!(no_short.cash(), m(1000.0));

    let mut l = reference(true, false);
    l.deposit(m(1000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 1.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.position(AAPL).qty, q(-1.0));
    expect_invariants(&l);
}

#[test]
fn uses_average_cost_on_partial_close() {
    let mut l = reference(false, false);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 100.0, 0.0)).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 120.0, 0.0)).unwrap();
    assert_eq!(l.position(AAPL).cost, m(2_200.0)); // average cost 110

    l.apply_fill(&fill(AAPL, Side::Sell, 5.0, 130.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(100.0)); // 650 proceeds - 550 released cost
    assert_eq!(l.position(AAPL).qty, q(15.0));
    assert_eq!(l.position(AAPL).cost, m(1_650.0));
    expect_invariants(&l);
}

#[test]
fn short_position_pnl() {
    let mut l = reference(true, false);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 10.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.cash(), m(11_000.0));
    assert_eq!(l.position(AAPL).cost, m(-1_000.0));

    l.apply_fill(&fill(AAPL, Side::Buy, 4.0, 90.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(40.0)); // sold at 100, bought back at 90
    assert_eq!(l.position(AAPL).qty, q(-6.0));
    assert_eq!(l.position(AAPL).cost, m(-600.0));

    l.mark(AAPL, p(95.0)).unwrap();
    assert_eq!(l.unrealized_pnl().unwrap(), m(30.0));
    expect_invariants(&l);
}

#[test]
fn position_can_flip_through_zero() {
    let mut l = reference(true, false);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 100.0, 0.0)).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 15.0, 110.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(100.0));
    assert_eq!(l.position(AAPL).qty, q(-5.0));
    assert_eq!(l.position(AAPL).cost, m(-550.0));
    assert_eq!(l.cash(), m(10_650.0));
    expect_invariants(&l);

    l.apply_fill(&fill(AAPL, Side::Buy, 5.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(150.0));
    assert!(l.position(AAPL).qty.is_zero());
    assert!(l.position(AAPL).cost.is_zero());
    expect_invariants(&l);
}

#[test]
fn reconciles_with_awkward_rounding() {
    let mut l = reference(true, true);
    l.deposit(m(1_000_000.0), 0).unwrap();
    let mut f = Fill {
        instrument: AAPL,
        side: Side::Buy,
        qty: Qty::from_raw(33_333_333),
        price: Price::from_raw(12_345_678_901),
        ..Fill::default()
    };
    l.apply_fill(&f).unwrap();
    f.qty = Qty::from_raw(77_777_777);
    f.price = Price::from_raw(9_876_543_211);
    l.apply_fill(&f).unwrap();
    f.side = Side::Sell;
    f.qty = Qty::from_raw(50_000_001);
    f.price = Price::from_raw(11_111_111_111);
    l.apply_fill(&f).unwrap();
    f.qty = Qty::from_raw(99_999_999); // flips through zero
    l.apply_fill(&f).unwrap();
    expect_invariants(&l);
}

#[test]
fn values_unmarked_positions_at_cost() {
    let mut l = reference(false, false);
    l.deposit(m(1_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 5.0, 100.0, 2.0)).unwrap();
    assert!(l.unrealized_pnl().unwrap().is_zero());
    assert_eq!(l.equity().unwrap(), m(998.0));
    expect_invariants(&l);
}

#[test]
fn replay_reproduces_state() {
    let mut l = reference(true, false);
    l.deposit(m(5_000.0), 1).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 3.0, 50.0, 0.5)).unwrap();
    l.apply_fill(&fill(2, Side::Buy, 7.0, 20.25, 0.1)).unwrap();
    l.withdraw(m(100.0), 2).unwrap();

    let r = Ledger::replay(*l.config(), l.journal()).unwrap();
    assert_eq!(r.cash(), l.cash());
    assert_eq!(r.realized_pnl(), l.realized_pnl());
    assert_eq!(r.journal(), l.journal());
    assert_eq!(r.positions(), l.positions());
}

#[test]
fn replay_rejects_inconsistent_journal() {
    let e = JournalEntry {
        seq: 0,
        kind: EntryType::Fill,
        ts_ns: 0,
        amount: Money::ZERO,
        fill: fill(AAPL, Side::Buy, 1.0, 100.0, 0.0), // no cash was ever deposited
        realized: Money::ZERO,
        cash_after: Money::ZERO,
    };
    assert!(Ledger::replay(LedgerConfig::default(), &[e]).is_err());
}

#[test]
fn overflow_leaves_state_untouched() {
    let mut l = reference(false, false);
    l.deposit(Money::from_raw(MAX_RAW - 10), 0).unwrap();
    assert_eq!(l.deposit(Money::from_raw(100), 0), Err(LedgerError::Arithmetic(ArithmeticError::Overflow)));
    assert_eq!(l.cash(), Money::from_raw(MAX_RAW - 10));
    assert_eq!(l.journal().len(), 1);
    expect_invariants(&l);
}

// --- Gross leverage limit (new in the Rust port) ------------------------------

/// The exact scenario that exposed the C++ defect: short $100k of A, then use
/// the proceeds to buy $190k of B. The reference engine accepted both
/// (2.9x gross exposure with negative cash disabled).
#[test]
fn leverage_limit_blocks_short_proceeds_from_funding_more_buying() {
    let cfg = LedgerConfig { allow_short: true, ..LedgerConfig::default() };
    let mut l = Ledger::new(cfg);
    l.deposit(m(100_000.0), 0).unwrap();
    l.mark(1, p(100.0)).unwrap();
    l.mark(2, p(100.0)).unwrap();
    assert_eq!(l.apply_fill(&fill(1, Side::Sell, 1000.0, 100.0, 0.0)), Ok(())); // exactly 1.0x
    assert_eq!(l.apply_fill(&fill(2, Side::Buy, 1900.0, 100.0, 0.0)), rej(Reject::LeverageExceeded));
    assert_eq!(l.journal().len(), 2);
    expect_invariants(&l);

    // The reference policy reproduces the old behaviour.
    let mut old = reference(true, false);
    old.deposit(m(100_000.0), 0).unwrap();
    old.apply_fill(&fill(1, Side::Sell, 1000.0, 100.0, 0.0)).unwrap();
    assert_eq!(old.apply_fill(&fill(2, Side::Buy, 1900.0, 100.0, 0.0)), Ok(()));
}

#[test]
fn leverage_limit_allows_configured_leverage() {
    let two_x = Ratio::from_units(2).unwrap();
    let cfg = LedgerConfig { allow_short: true, allow_negative_cash: false, max_gross_leverage: Some(two_x) };
    let mut l = Ledger::new(cfg);
    l.deposit(m(100_000.0), 0).unwrap();
    l.apply_fill(&fill(1, Side::Sell, 1000.0, 100.0, 0.0)).unwrap(); // 100k gross
    l.apply_fill(&fill(2, Side::Buy, 1000.0, 100.0, 0.0)).unwrap(); // 200k gross = 2.0x
    assert_eq!(l.apply_fill(&fill(2, Side::Buy, 1.0, 100.0, 0.0)), rej(Reject::LeverageExceeded));
    expect_invariants(&l);
}

#[test]
fn reducing_exposure_is_always_allowed() {
    let cfg = LedgerConfig { allow_short: true, ..LedgerConfig::default() };
    let mut l = Ledger::new(cfg);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(1, Side::Sell, 100.0, 100.0, 0.0)).unwrap(); // 1.0x short
                                                                    // The price triples: equity collapses and the position is now far above
                                                                    // the limit. Buying back part of it must still be accepted.
    l.mark(1, p(300.0)).unwrap();
    assert_eq!(l.apply_fill(&fill(1, Side::Buy, 50.0, 300.0, 0.0)), Ok(()));
    // But adding to the losing short is refused.
    assert_eq!(l.apply_fill(&fill(1, Side::Sell, 1.0, 300.0, 0.0)), rej(Reject::LeverageExceeded));
    expect_invariants(&l);
}

#[test]
fn long_only_cash_account_never_hits_the_default_limit() {
    // With cash >= 0 and no shorts, gross exposure can never exceed equity, so
    // the default 1.0x limit changes nothing for long-only strategies.
    let mut l = Ledger::new(LedgerConfig::default());
    l.deposit(m(1_000.0), 0).unwrap();
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 9.0, 110.0, 10.0)), Ok(())); // spends all cash
    assert_eq!(l.cash(), m(0.0));
    expect_invariants(&l);
}

#[test]
fn replay_does_not_reapply_leverage_policy() {
    let cfg = LedgerConfig { allow_short: true, ..LedgerConfig::default() };
    let mut l = Ledger::new(cfg);
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(1, Side::Sell, 100.0, 100.0, 0.0)).unwrap();
    l.mark(1, p(50.0)).unwrap(); // a mark the replay never sees
    l.apply_fill(&fill(2, Side::Buy, 100.0, 100.0, 0.0)).unwrap();
    let r = Ledger::replay(cfg, l.journal()).unwrap();
    assert_eq!(r.positions(), l.positions());
    assert_eq!(r.config(), l.config());
    expect_invariants(&l);
}

// --- Randomised property tests (port of test_ledger_property.cpp) -------------

/// splitmix64, bit-identical to the C++ test generator, so a failing seed
/// reproduces in both languages.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo) as u64 + 1;
        lo + (self.next() % span) as i64
    }
}

#[derive(Default)]
struct RunStats {
    rejected: u32,
    flips: u32,
    fills: u32,
}

const INSTRUMENTS: usize = 3;
const STEPS: usize = 400;

fn random_fill(rng: &mut Rng, prices: &[i64; INSTRUMENTS]) -> Fill {
    let inst = rng.range(0, INSTRUMENTS as i64 - 1) as usize;
    let side = if rng.range(0, 1) == 0 { Side::Buy } else { Side::Sell };
    let mode = rng.range(0, 9);
    let qty = if mode < 3 {
        rng.range(1, 100_000)
    } else if mode < 8 {
        rng.range(1, 10 * SCALE)
    } else {
        rng.range(1, 100 * SCALE)
    };
    let fee = if rng.range(0, 3) == 0 { 0 } else { rng.range(0, 5_000_000) };
    Fill {
        instrument: inst as InstrumentId,
        side,
        qty: Qty::from_raw(qty),
        price: Price::from_raw(prices[inst]),
        fee: Money::from_raw(fee),
        ts_ns: 0,
    }
}

fn run_random_session(cfg: LedgerConfig, seed: u64, initial: i64, stats: &mut RunStats) {
    let mut rng = Rng(seed);
    let mut l = Ledger::new(cfg);
    l.deposit(Money::from_raw(initial * SCALE), 0).unwrap();

    let mut prices = [0i64; INSTRUMENTS];
    for px in &mut prices {
        *px = rng.range(20 * SCALE, 500 * SCALE);
    }

    for step in 0..STEPS {
        for px in &mut prices {
            *px += rng.range(-SCALE, SCALE);
            if *px < SCALE {
                *px = SCALE;
            }
        }
        let journal_before = l.journal().len();
        let cash_before = l.cash();
        let op = rng.range(0, 99);
        let mut fill_info = None;
        let r = if op < 88 {
            let f = random_fill(&mut rng, &prices);
            fill_info = Some((f.instrument, l.position(f.instrument).qty.signum()));
            l.apply_fill(&f)
        } else if op < 92 {
            l.deposit(Money::from_raw(rng.range(1, 50_000 * SCALE)), 0)
        } else if op < 96 {
            l.withdraw(Money::from_raw(rng.range(1, 50_000 * SCALE)), 0)
        } else {
            let inst = rng.range(0, INSTRUMENTS as i64 - 1) as usize;
            l.mark(inst as InstrumentId, Price::from_raw(prices[inst]))
        };

        if r.is_err() {
            stats.rejected += 1;
            assert_eq!(l.journal().len(), journal_before);
            assert_eq!(l.cash(), cash_before);
        } else if let Some((inst, before)) = fill_info {
            stats.fills += 1;
            let after = l.position(inst).qty.signum();
            if before != 0 && after != 0 && before != after {
                stats.flips += 1;
            }
        }

        let identity = l.net_deposits() - l.total_fees() + l.realized_pnl() + l.unrealized_pnl().unwrap();
        assert_eq!(l.equity().unwrap(), identity, "seed {seed} step {step}");
        if !cfg.allow_negative_cash {
            assert!(l.cash().raw() >= 0);
        }
        if !cfg.allow_short {
            assert!(l.positions().values().all(|p| p.qty.raw() >= 0));
        }
        if step % 50 == 0 {
            if let Err(e) = l.check_invariants() {
                panic!("seed {seed} step {step}: {e}");
            }
        }
    }
    if let Err(e) = l.check_invariants() {
        panic!("seed {seed} final: {e}");
    }
}

#[test]
fn property_ledger_always_reconciles() {
    let mut stats = RunStats::default();
    for (c, (s, n)) in [(false, false), (true, false), (false, true), (true, true)].into_iter().enumerate() {
        for seed in 1..=40u64 {
            run_random_session(LedgerConfig::reference(s, n), seed * 1000 + c as u64, 1_000_000, &mut stats);
        }
    }
    assert!(stats.fills > 1000);
    assert!(stats.rejected > 0);
    assert!(stats.flips > 0);
}

#[test]
fn property_ledger_reconciles_with_leverage_limits() {
    let mut stats = RunStats::default();
    for (c, lev) in [1i64, 2, 4].into_iter().enumerate() {
        let cfg = LedgerConfig {
            allow_short: true,
            allow_negative_cash: true,
            max_gross_leverage: Some(Ratio::from_units(lev).unwrap()),
        };
        for seed in 1..=40u64 {
            // A small account so the limit is actually reached.
            run_random_session(cfg, seed * 7919 + c as u64, 20_000, &mut stats);
        }
    }
    eprintln!("leverage sessions: fills={} rejected={}", stats.fills, stats.rejected);
    assert!(stats.rejected > 200, "the limit must actually be exercised");
    assert!(stats.fills > 1000);
    assert!(stats.rejected > 0);
}

#[test]
fn financing_entries_reconcile_and_replay() {
    use spectra_core::{EntryType, Ledger, LedgerConfig, Money, Reject};
    let m = |v: f64| Money::from_f64(v).unwrap();
    let mut l = Ledger::new(LedgerConfig { allow_short: true, ..LedgerConfig::default() });
    l.deposit(m(100.0), 1).unwrap();
    l.apply_interest(m(2.5), 2).unwrap();
    l.apply_interest(m(-0.5), 3).unwrap();
    l.charge_borrow_fee(7, m(1.25), 4).unwrap();
    assert_eq!(l.cash(), m(100.75));
    assert_eq!(l.financing(), m(0.75));
    assert_eq!(l.equity().unwrap(), m(100.75));
    let kinds: Vec<EntryType> = l.journal().iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EntryType::Deposit, EntryType::Interest, EntryType::Interest, EntryType::BorrowFee]);
    assert_eq!(l.journal()[3].fill.instrument, 7);
    l.check_invariants().unwrap();

    // Zero and negative amounts are refused where they make no sense.
    assert_eq!(l.apply_interest(Money::ZERO, 5), Err(Reject::InvalidAmount.into()));
    assert_eq!(l.charge_borrow_fee(7, m(-1.0), 5), Err(Reject::InvalidAmount.into()));
    assert_eq!(l.journal().len(), 4);
}

#[test]
fn financing_charges_may_take_cash_below_zero() {
    use spectra_core::{Ledger, LedgerConfig, Money};
    let m = |v: f64| Money::from_f64(v).unwrap();
    // Negative cash is otherwise disabled; a borrow fee is a debt and still posts.
    let mut l = Ledger::new(LedgerConfig { allow_short: true, ..LedgerConfig::default() });
    l.deposit(m(1.0), 1).unwrap();
    l.charge_borrow_fee(3, m(1.5), 2).unwrap();
    assert_eq!(l.cash(), m(-0.5));
    l.check_invariants().unwrap();
    // A withdrawal still may not.
    assert!(l.withdraw(m(0.1), 3).is_err());
}

#[test]
fn check_fill_answers_like_apply_fill_and_changes_nothing() {
    let mut l = reference(false, false);
    l.deposit(m(1_000.0), 1).unwrap();
    let ok = fill(1, Side::Buy, 5.0, 100.0, 1.0);
    let too_big = fill(1, Side::Buy, 20.0, 100.0, 0.0);
    let short = fill(1, Side::Sell, 10.0, 100.0, 0.0);
    assert_eq!(l.check_fill(&too_big), rej(Reject::InsufficientCash));
    assert_eq!(l.check_fill(&short), rej(Reject::ShortNotAllowed));
    assert_eq!(l.check_fill(&ok), Ok(()));
    assert_eq!((l.cash(), l.journal().len()), (m(1_000.0), 1), "checking changes nothing");
    l.apply_fill(&ok).unwrap();
    assert_eq!(l.cash(), m(499.0));
    expect_invariants(&l);
}

#[test]
fn splits_and_dividends_are_journaled_and_replay() {
    let mut l = Ledger::new(LedgerConfig { allow_short: true, ..LedgerConfig::default() });
    l.deposit(m(10_000.0), 1).unwrap();
    l.apply_fill(&fill(1, Side::Buy, 10.0, 100.0, 1.0)).unwrap();
    l.apply_fill(&fill(2, Side::Sell, 4.0, 50.0, 0.0)).unwrap();
    l.mark(1, p(100.0)).unwrap();
    l.mark(2, p(50.0)).unwrap();
    let before = l.equity().unwrap();

    // 2-for-1 on instrument 1: twice the shares at half the mark, same value and cost.
    l.apply_split(1, 1, 2, q(20.0), 2).unwrap();
    assert_eq!(l.position(1).qty, q(20.0));
    assert_eq!(l.position(1).cost, m(1_000.0));
    assert_eq!(l.position_value(1).unwrap(), m(1_000.0));
    assert_eq!(l.equity().unwrap(), before);

    // Dividends: a long receives, a short pays (and may go past zero cash like financing).
    l.apply_dividend(1, m(10.0), 3).unwrap();
    l.apply_dividend(2, m(-2.0), 3).unwrap();
    assert_eq!(l.dividends(), m(8.0));
    assert_eq!(l.equity().unwrap(), before.checked_add(m(8.0)).unwrap());
    expect_invariants(&l);
    let r = Ledger::replay(*l.config(), l.journal()).unwrap();
    assert_eq!((r.cash(), r.dividends(), r.position(1)), (l.cash(), l.dividends(), l.position(1)));

    // Refused: no position, a sign change, zero, a dividend of the wrong sign.
    assert_eq!(l.apply_split(3, 1, 2, q(1.0), 4), rej(Reject::InvalidFill));
    assert_eq!(l.apply_split(1, 1, 2, q(-20.0), 4), rej(Reject::InvalidFill));
    assert_eq!(l.apply_split(1, 0, 2, q(40.0), 4), rej(Reject::InvalidFill));
    assert_eq!(l.apply_dividend(1, m(-1.0), 4), rej(Reject::InvalidFill));
    assert_eq!(l.apply_dividend(1, Money::ZERO, 4), rej(Reject::InvalidAmount));
    assert_eq!(l.apply_dividend(3, m(1.0), 4), rej(Reject::InvalidFill));
    expect_invariants(&l);
}

// ---------------------------------------------------------------- M24.1 multipliers

fn r(v: i64) -> Ratio {
    Ratio::from_units(v).unwrap()
}

#[test]
fn futures_pnl_is_multiplier_times_price_move() {
    // M24.1 task 6: multiplier 50, buy 2 at 100, sell at 103: P&L = 3 * 2 * 50 = 300.
    let mut l = reference(false, false);
    l.register_instrument(AAPL, r(50), 0).unwrap();
    l.deposit(m(100_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.position(AAPL).cost, m(10_000.0)); // 2 * 100 * 50
    l.apply_fill(&fill(AAPL, Side::Sell, 2.0, 103.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(300.0));
    assert_eq!(l.cash(), m(100_300.0));
    expect_invariants(&l);
}

#[test]
fn multiplier_flows_through_marks_and_unrealized() {
    let mut l = reference(false, false);
    l.register_instrument(AAPL, r(50), 0).unwrap();
    l.deposit(m(100_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 1.0, 100.0, 0.0)).unwrap();
    l.mark(AAPL, p(101.0)).unwrap();
    assert_eq!(l.unrealized_pnl(), Ok(m(50.0)));
    assert_eq!(l.equity(), Ok(m(100_050.0)));
    expect_invariants(&l);
}

#[test]
fn multiplier_survives_replay() {
    let mut l = reference(true, false);
    l.register_instrument(AAPL, r(50), 0).unwrap();
    l.deposit(m(100_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 100.0, 1.0)).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 3.0, 101.0, 1.0)).unwrap(); // flips short through zero
    expect_invariants(&l);
    let r2 = Ledger::replay(*l.config(), l.journal()).unwrap();
    assert_eq!(r2.multiplier(AAPL), r(50));
    assert_eq!(r2.journal(), l.journal());
    assert_eq!(r2.realized_pnl(), l.realized_pnl());
}

#[test]
fn unit_multiplier_writes_nothing() {
    let mut l = reference(false, false);
    l.register_instrument(AAPL, Ratio::ONE, 0).unwrap();
    assert!(l.journal().is_empty());
    assert_eq!(l.multiplier(AAPL), Ratio::ONE);
}

#[test]
fn multiplier_registration_rules() {
    let mut l = reference(false, false);
    assert_eq!(l.register_instrument(AAPL, Ratio::ZERO, 0), rej(Reject::InvalidFill));
    assert_eq!(l.register_instrument(AAPL, r(-1), 0), rej(Reject::InvalidFill));
    l.register_instrument(AAPL, r(50), 0).unwrap();
    assert_eq!(l.register_instrument(AAPL, r(10), 0), rej(Reject::InvalidFill));
    l.deposit(m(100_000.0), 0).unwrap();
    l.register_instrument(2, r(10), 0).unwrap();
    l.apply_fill(&fill(2, Side::Buy, 1.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.register_instrument(2, r(10), 0), rej(Reject::InvalidFill));
    expect_invariants(&l);
}

#[test]
fn listing_validates_the_instrument_record() {
    use spectra_core::{Instrument, InstrumentType};
    let mut l = reference(false, false);
    let mut fut = Instrument::equity("ESZ6", "USD", p(0.25), Qty::from_raw(SCALE));
    fut.kind = InstrumentType::Future;
    fut.multiplier = r(50);
    fut.expiry_ns = Some(1);
    l.list(AAPL, &fut, 0).unwrap();
    assert_eq!(l.multiplier(AAPL), r(50));

    let mut bad = Instrument::equity("SAP", "EUR", p(0.01), Qty::from_raw(SCALE));
    bad.expiry_ns = Some(1); // an equity cannot expire
    assert_eq!(l.list(2, &bad, 0), rej(Reject::InvalidFill));
    assert_eq!(l.multiplier(2), Ratio::ONE);
}

// ---------------------------------------------------------------- M24.2 marked to market

fn futures_ledger(cash: f64) -> Ledger {
    let mut l = reference(true, false);
    l.register_contract(AAPL, r(50), true, 0).unwrap();
    l.deposit(m(cash), 0).unwrap();
    l
}

#[test]
fn opening_a_future_moves_only_its_fee() {
    let mut l = futures_ledger(10_000.0);
    // 10 contracts at 100 x 50 = 50,000 notional on 10,000 of cash: no notional changes hands.
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 100.0, 5.0)).unwrap();
    assert_eq!(l.cash(), m(9_995.0));
    assert_eq!(l.position(AAPL).cost, m(50_000.0));
    assert_eq!(l.position_exposure(AAPL), Ok(m(50_000.0)));
    l.mark(AAPL, p(101.0)).unwrap();
    assert_eq!(l.unrealized_pnl(), Ok(m(500.0)));
    assert_eq!(l.position_value(AAPL), Ok(m(500.0)), "a future's value is its P&L");
    assert_eq!(l.equity(), Ok(m(10_495.0)));
    expect_invariants(&l);
}

#[test]
fn closing_a_future_settles_its_pnl_to_cash() {
    let mut l = futures_ledger(10_000.0);
    l.apply_fill(&fill(AAPL, Side::Buy, 10.0, 100.0, 0.0)).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 4.0, 103.0, 0.0)).unwrap();
    assert_eq!(l.realized_pnl(), m(600.0)); // 4 * 3 * 50
    assert_eq!(l.cash(), m(10_600.0));
    // A loss bigger than the cash still closes: it is a debt, not a choice.
    l.apply_fill(&fill(AAPL, Side::Sell, 6.0, 60.0, 0.0)).unwrap();
    assert_eq!(l.cash(), m(10_600.0 - 6.0 * 40.0 * 50.0));
    assert!(l.cash().raw() < 0);
    assert_eq!(l.position(AAPL).qty, Qty::ZERO);
    expect_invariants(&l);
    let replayed = Ledger::replay(*l.config(), l.journal()).unwrap();
    assert!(replayed.is_marked_to_market(AAPL));
    assert_eq!(replayed.cash(), l.cash());
}

#[test]
fn a_future_flips_and_shorts_like_any_position() {
    let mut l = futures_ledger(10_000.0);
    l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 100.0, 0.0)).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 5.0, 98.0, 1.0)).unwrap(); // close 2 at a loss, open 3 short
    assert_eq!(l.realized_pnl(), m(-200.0));
    assert_eq!(l.cash(), m(10_000.0 - 200.0 - 1.0));
    assert_eq!(l.position(AAPL).qty, q(-3.0));
    l.mark(AAPL, p(97.0)).unwrap();
    assert_eq!(l.unrealized_pnl(), Ok(m(150.0)));
    expect_invariants(&l);
}

#[test]
fn leverage_counts_a_futures_notional() {
    let mut l = Ledger::new(LedgerConfig { allow_short: true, ..LedgerConfig::default() }); // 1.0x
    l.register_contract(AAPL, r(50), true, 0).unwrap();
    l.deposit(m(10_000.0), 0).unwrap();
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 100.0, 0.0)), Ok(())); // 10,000 notional: 1.0x
    assert_eq!(l.apply_fill(&fill(AAPL, Side::Buy, 1.0, 100.0, 0.0)), rej(Reject::LeverageExceeded));
    // Keeping an exposure (a roll into the next contract) is not a new decision.
    l.register_contract(2, r(50), true, 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Sell, 2.0, 100.0, 0.0)).unwrap();
    assert_eq!(l.apply_fill_keeping_exposure(&fill(2, Side::Buy, 2.0, 110.0, 0.0)), Ok(()));
    expect_invariants(&l);
}

#[test]
fn listing_a_future_marks_it_to_market() {
    use spectra_core::{Instrument, InstrumentType};
    let mut l = reference(false, false);
    let mut fut = Instrument::equity("ESZ6", "USD", p(0.25), Qty::from_raw(SCALE));
    fut.kind = InstrumentType::Future;
    fut.expiry_ns = Some(1); // multiplier 1: still marked to market, so still listed
    l.list(AAPL, &fut, 0).unwrap();
    assert!(l.is_marked_to_market(AAPL));
    assert_eq!(l.journal().len(), 1);
    assert_eq!(l.journal()[0].kind, EntryType::Listing);
}

// ---------------------------------------------------------------- M24.4 expiry at zero

#[test]
fn an_expired_option_closes_at_zero() {
    let mut l = reference(true, false);
    l.register_instrument(AAPL, r(100), 0).unwrap();
    l.deposit(m(10_000.0), 0).unwrap();
    l.apply_fill(&fill(AAPL, Side::Buy, 2.0, 3.5, 1.0)).unwrap(); // premium 700 + 1 fee
    l.expire_position(AAPL, 5).unwrap();
    assert_eq!(l.position(AAPL).qty, Qty::ZERO);
    assert_eq!(l.realized_pnl(), m(-700.0));
    assert_eq!(l.cash(), m(10_000.0 - 701.0), "no cash moves at expiry");
    expect_invariants(&l);
    // A short keeps the premium.
    l.register_instrument(2, r(100), 0).unwrap();
    l.apply_fill(&fill(2, Side::Sell, 1.0, 2.0, 0.0)).unwrap();
    l.expire_position(2, 6).unwrap();
    assert_eq!(l.position(2).realized, m(200.0));
    expect_invariants(&l);
    assert_eq!(l.expire_position(2, 7), rej(Reject::InvalidFill), "nothing left to expire");
    assert_eq!(Ledger::replay(*l.config(), l.journal()).unwrap().realized_pnl(), l.realized_pnl());
}
