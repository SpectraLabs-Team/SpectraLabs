//! C++ <-> Rust parity: replay every script in `parity/ops/` on the Rust
//! ledger with the reference policy and require output byte-identical to the
//! golden file the frozen C++ engine produced (`parity/golden/`).
//!
//! Regenerate goldens with `parity/regen.sh <path-to-cpp-reference>`.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use spectra_core::*;

fn parity_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity")
}

fn status(r: std::result::Result<(), LedgerError>) -> &'static str {
    match r {
        Ok(()) => "Ok",
        Err(LedgerError::Rejected(rej)) => rej.as_str(),
        Err(LedgerError::Arithmetic(ArithmeticError::Overflow)) => "Overflow",
        Err(LedgerError::Arithmetic(e)) => panic!("unexpected arithmetic error {e:?}"),
    }
}

fn or_overflow(v: std::result::Result<Money, ArithmeticError>) -> String {
    v.map_or_else(|_| "Overflow".to_string(), |m| m.raw().to_string())
}

fn num<T: std::str::FromStr>(it: &mut std::str::SplitWhitespace<'_>) -> T
where
    T::Err: std::fmt::Debug,
{
    it.next().expect("missing field").parse().expect("bad number")
}

fn run_script(script: &str) -> String {
    let mut lines = script.lines();
    let mut hdr = lines.next().expect("empty script").split_whitespace();
    assert_eq!(hdr.next(), Some("config"));
    let allow_short = num::<u8>(&mut hdr) != 0;
    let allow_neg = num::<u8>(&mut hdr) != 0;
    let mut l = Ledger::new(LedgerConfig::reference(allow_short, allow_neg));

    let mut out = String::new();
    for (op, line) in lines.filter(|s| !s.is_empty()).enumerate() {
        let mut it = line.split_whitespace();
        let kind = it.next().unwrap();
        let r = match kind {
            "D" | "W" => {
                let amt = Money::from_raw(num(&mut it));
                let ts: i64 = num(&mut it);
                if kind == "D" {
                    l.deposit(amt, ts)
                } else {
                    l.withdraw(amt, ts)
                }
            }
            "F" => {
                let instrument: InstrumentId = num(&mut it);
                let side = if it.next() == Some("B") { Side::Buy } else { Side::Sell };
                let f = Fill {
                    instrument,
                    side,
                    qty: Qty::from_raw(num(&mut it)),
                    price: Price::from_raw(num(&mut it)),
                    fee: Money::from_raw(num(&mut it)),
                    ts_ns: num(&mut it),
                };
                l.apply_fill(&f)
            }
            "M" => {
                let inst: InstrumentId = num(&mut it);
                l.mark(inst, Price::from_raw(num(&mut it)))
            }
            other => panic!("unknown op {other}"),
        };
        writeln!(
            out,
            "{op} {} cash={} net={} real={} fees={} eq={} unr={}",
            status(r),
            l.cash().raw(),
            l.net_deposits().raw(),
            l.realized_pnl().raw(),
            l.total_fees().raw(),
            or_overflow(l.equity()),
            or_overflow(l.unrealized_pnl()),
        )
        .unwrap();
    }
    for (id, p) in l.positions() {
        writeln!(out, "POS {id} {} {} {} {}", p.qty.raw(), p.cost.raw(), p.realized.raw(), p.fees.raw())
            .unwrap();
    }
    for e in l.journal() {
        let t = match e.kind {
            EntryType::Deposit => "D",
            EntryType::Withdrawal => "W",
            EntryType::Fill => "F",
            // Not produced by the reference engine; never in the goldens.
            EntryType::Interest => "I",
            EntryType::BorrowFee => "B",
        };
        let side = if e.fill.side == Side::Buy { 'B' } else { 'S' };
        writeln!(
            out,
            "J {} {t} {} {} {} {side} {} {} {} {} {}",
            e.seq,
            e.ts_ns,
            e.amount.raw(),
            e.fill.instrument,
            e.fill.qty.raw(),
            e.fill.price.raw(),
            e.fill.fee.raw(),
            e.realized.raw(),
            e.cash_after.raw()
        )
        .unwrap();
    }
    match l.check_invariants() {
        Ok(()) => out.push_str("INV ok\n"),
        Err(e) => writeln!(out, "INV {e}").unwrap(),
    }
    out
}

#[test]
fn ledger_matches_cpp_reference_byte_for_byte() {
    let ops_dir = parity_dir().join("ops");
    let mut scripts: Vec<_> = fs::read_dir(&ops_dir)
        .expect("parity/ops missing")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ops"))
        .collect();
    scripts.sort();
    assert!(scripts.len() >= 12, "expected the 12 committed parity scripts");

    let mut ops_checked = 0usize;
    for script in &scripts {
        let golden_path =
            parity_dir().join("golden").join(script.with_extension("golden").file_name().unwrap());
        let golden = fs::read_to_string(&golden_path).expect("golden file missing");
        let actual = run_script(&fs::read_to_string(script).unwrap());
        if actual != golden {
            let (n, (a, g)) = actual
                .lines()
                .zip(golden.lines())
                .enumerate()
                .find(|(_, (a, g))| a != g)
                .unwrap_or((0, ("<length differs>", "")));
            panic!("{}: first difference at line {}\n  rust: {a}\n  c++ : {g}", script.display(), n + 1);
        }
        ops_checked += golden.lines().take_while(|l| !l.starts_with("POS") && !l.starts_with('J')).count();
    }
    assert!(ops_checked > 4000);
}
