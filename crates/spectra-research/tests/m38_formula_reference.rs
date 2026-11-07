//! M38 Formula Language & Scanner Reference Tests.

use spectra_core::fixed::{Price, Qty};
use spectra_core::Bar;
use spectra_research::formula::*;
use spectra_research::indicators::{ema, sma};

fn make_bar(ts: i64, c: f64) -> Bar {
    Bar::new(
        ts,
        0,
        Price::from_f64(c).unwrap(),
        Price::from_f64(c + 1.0).unwrap(),
        Price::from_f64(c - 1.0).unwrap(),
        Price::from_f64(c).unwrap(),
        Qty::from_f64(100.0).unwrap(),
    )
}

#[test]
fn test_m38_formula_syntax_and_operators() {
    let bars: Vec<Bar> = (1..=20).map(|i| make_bar(i * 1000, i as f64)).collect();

    // Arithmetic & logical operators
    let res = evaluate_formula("(Close > 10) AND (Close <= 15)", &bars, 10_000).unwrap();
    // Indices 0..9 (prices 1..10) -> false (0.0)
    assert_eq!(res[0], Some(0.0));
    assert_eq!(res[9], Some(0.0));
    // Indices 10..14 (prices 11..15) -> true (1.0)
    assert_eq!(res[10], Some(1.0));
    assert_eq!(res[14], Some(1.0));
    // Index 15 (price 16) -> false (0.0)
    assert_eq!(res[15], Some(0.0));
}

#[test]
fn test_m38_indicator_parity() {
    let bars: Vec<Bar> = (1..=50).map(|i| make_bar(i * 1000, (i * 2) as f64)).collect();
    let closes: Vec<f64> = bars.iter().map(|b| b.close.to_f64()).collect();

    // SMA parity
    let formula_sma = evaluate_formula("Mov(Close, 10, 0)", &bars, 10_000).unwrap();
    let builtin_sma = sma(&closes, 10);
    assert_eq!(formula_sma, builtin_sma, "Formula SMA must match built-in SMA bit for bit");

    // EMA parity
    let formula_ema = evaluate_formula("Mov(Close, 10, 1)", &bars, 10_000).unwrap();
    let builtin_ema = ema(&closes, 10);
    assert_eq!(formula_ema, builtin_ema, "Formula EMA must match built-in EMA bit for bit");
}

#[test]
fn test_m38_sandbox_and_lookahead_guards() {
    let bars = vec![make_bar(1000, 100.0), make_bar(2000, 105.0)];

    // Positive lag is forbidden
    let err_future = evaluate_formula("Ref(Close, 1)", &bars, 10_000);
    assert_eq!(err_future, Err(FormulaError::LookaheadForbidden));

    // Valid negative lag
    let ok_lag = evaluate_formula("Ref(Close, -1)", &bars, 10_000).unwrap();
    assert_eq!(ok_lag[1], Some(100.0));

    // Execution limit
    let err_limit = evaluate_formula("Close + Close", &bars, 1);
    assert_eq!(err_limit, Err(FormulaError::ExecutionLimitExceeded));
}

#[test]
fn test_m38_idea_scanner_signals() {
    let bars_winner = vec![make_bar(1000, 50.0), make_bar(2000, 120.0)];
    let bars_loser = vec![make_bar(1000, 50.0), make_bar(2000, 80.0)];

    let universe = [("AAPL", bars_winner.as_slice()), ("MSFT", bars_loser.as_slice())];

    let signals = run_idea_scanner("PriceAbove100", "Close > 100", &universe, 10_000).unwrap();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].symbol, "AAPL");
    assert_eq!(signals[0].trigger_value, 1.0);
}
