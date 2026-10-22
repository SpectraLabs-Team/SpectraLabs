//! Tests for Spectra restricted strategy language (DSL) and interpreter.
//! Covers M2b Task 7 (interpreter reproduces sma_cross bit-for-bit)
//! and M2b Task 8 (static validation rejects lookahead, loops, and 100 bad inputs without panicking).

use std::collections::HashMap;

use spectra_core::{Bar, Price, Qty, Rng, SCALE};
use spectra_engine::dsl::{parse_and_validate, DslStrategy};
use spectra_engine::{run_backtest, BacktestConfig, Context, Strategy};

fn generate_market(seed: u64, bars_count: usize) -> Vec<Bar> {
    let mut rng = Rng::new(seed);
    let mut last = 100 * SCALE;
    let mut bars = Vec::with_capacity(bars_count);
    for t in 1..=bars_count {
        let open = (last + rng.range(-SCALE, SCALE)).max(SCALE);
        let close = (open + rng.range(-2 * SCALE, 2 * SCALE)).max(SCALE);
        let high = open.max(close) + rng.range(0, SCALE);
        let low = (open.min(close) - rng.range(0, SCALE)).max(1);
        let volume = rng.range(100, 1000) * SCALE;
        let ts_ns = (t as i64) * 60_000_000_000;
        bars.push(Bar {
            ts_ns,
            instrument: 1,
            open: Price::from_raw(open),
            high: Price::from_raw(high),
            low: Price::from_raw(low),
            close: Price::from_raw(close),
            volume: Qty::from_raw(volume),
            known_at_ns: ts_ns,
        });
        last = close;
    }
    bars
}

/// The built-in reference SMA crossover strategy.
#[derive(Debug)]
struct ReferenceSmaCross {
    fast: usize,
    slow: usize,
    unit: Qty,
    shorts: bool,
}

impl Strategy for ReferenceSmaCross {
    fn on_bar(&mut self, ctx: &mut Context<'_>, bars: &[Bar]) {
        for b in bars {
            let h = ctx.history(b.instrument);
            if h.len() < self.slow.max(self.fast) {
                continue;
            }
            let fast_sum: i128 = h[h.len() - self.fast..].iter().map(|bar| i128::from(bar.close.raw())).sum();
            let slow_sum: i128 = h[h.len() - self.slow..].iter().map(|bar| i128::from(bar.close.raw())).sum();
            let bullish = (fast_sum / self.fast as i128) > (slow_sum / self.slow as i128);
            let pos = ctx.position(b.instrument);
            if !self.shorts {
                if bullish && pos.is_zero() {
                    ctx.market(b.instrument, spectra_core::Side::Buy, self.unit);
                } else if !bullish && pos.raw() > 0 {
                    ctx.market(b.instrument, spectra_core::Side::Sell, pos);
                }
                continue;
            }

            let Ok(short_size) = pos.checked_abs() else { continue };
            if bullish && pos.raw() <= 0 {
                let q = if pos.raw() < 0 { self.unit.checked_add(short_size) } else { Ok(self.unit) };
                if let Ok(q) = q {
                    ctx.market(b.instrument, spectra_core::Side::Buy, q);
                }
            } else if !bullish && pos.raw() > 0 {
                if let Ok(q) = pos.checked_add(self.unit) {
                    ctx.market(b.instrument, spectra_core::Side::Sell, q);
                }
            } else if !bullish && pos.is_zero() {
                ctx.market(b.instrument, spectra_core::Side::Sell, self.unit);
            }
        }
    }
}

const SMA_CROSS_DSL: &str = r#"
param fast: int = 10, min = 1, max = 1000, desc = "Fast moving average period";
param slow: int = 30, min = 2, max = 5000, desc = "Slow moving average period";
param qty: float = 100.0, min = 0.00000001, max = 1000000000.0;
param allow_short: bool = false;

on_bar {
    let min_bars = max(slow, fast);
    if bars_count < min_bars {
        return;
    }

    let bullish = sma(close, fast) > sma(close, slow);
    let pos = position;

    if not allow_short {
        if bullish and pos == 0 {
            market_buy(qty);
        } else if not bullish and pos > 0 {
            market_sell(pos);
        }
    } else {
        if bullish and pos <= 0 {
            let q = if pos < 0 { qty + abs(pos) } else { qty };
            market_buy(q);
        } else if not bullish and pos > 0 {
            let q = pos + qty;
            market_sell(q);
        } else if not bullish and pos == 0 {
            market_sell(qty);
        }
    }
}
"#;

#[test]
fn test_task_7_dsl_reproduces_sma_cross_exactly() {
    let bars = generate_market(42, 200);
    let cfg = BacktestConfig::default();

    // 1. Run reference C++/Rust built-in implementation
    let mut ref_strat =
        ReferenceSmaCross { fast: 10, slow: 30, unit: Qty::from_f64(100.0).unwrap(), shorts: false };
    let ref_res = run_backtest(&bars, &mut ref_strat, &cfg).unwrap();

    // 2. Run restricted DSL interpreted strategy
    let program = parse_and_validate(SMA_CROSS_DSL).expect("DSL script should validate");
    let mut dsl_strat = DslStrategy::new(program, &HashMap::new());
    let dsl_res = run_backtest(&bars, &mut dsl_strat, &cfg).unwrap();

    // 3. Verify bit-for-bit exact parity
    assert_eq!(ref_res.orders.len(), dsl_res.orders.len(), "order count");
    for (i, (ro, do_ord)) in ref_res.orders.iter().zip(dsl_res.orders.iter()).enumerate() {
        assert_eq!(ro.instrument, do_ord.instrument, "order {i} instrument");
        assert_eq!(ro.side, do_ord.side, "order {i} side");
        assert_eq!(ro.kind, do_ord.kind, "order {i} kind");
        assert_eq!(ro.qty, do_ord.qty, "order {i} qty");
        assert_eq!(ro.state, do_ord.state, "order {i} state");
    }

    assert_eq!(ref_res.metrics.total_return, dsl_res.metrics.total_return, "total return");
    assert_eq!(ref_res.metrics.sharpe, dsl_res.metrics.sharpe, "sharpe ratio");
    assert_eq!(ref_res.metrics.max_drawdown, dsl_res.metrics.max_drawdown, "max drawdown");
    assert_eq!(ref_res.equity_curve.len(), dsl_res.equity_curve.len(), "equity curve length");

    for (i, (re, de)) in ref_res.equity_curve.iter().zip(dsl_res.equity_curve.iter()).enumerate() {
        assert_eq!(re.equity, de.equity, "equity point {i} value");
        assert_eq!(re.cash, de.cash, "equity point {i} cash");
    }

    assert_eq!(
        ref_res.run.result_sha256, dsl_res.run.result_sha256,
        "result sha256 fingerprint must match bit-for-bit"
    );
}

#[test]
fn test_task_8_lookahead_is_rejected() {
    // Negative index into historical prices represents lookahead into the future
    let lookahead_script = r"
param fast: int = 10, min = 1, max = 100;
on_bar {
    let future_close = close[-1];
    if future_close > close {
        market_buy(100);
    }
}
";
    let err = parse_and_validate(lookahead_script).unwrap_err();
    assert_eq!(err.code, "lookahead_forbidden");
    assert_eq!(err.line, 4);
}

#[test]
fn test_task_8_unbounded_loops_are_rejected() {
    let loop_cases = [
        "param p: int = 1; on_bar { while true { market_buy(1); } }",
        "param p: int = 1; on_bar { loop { market_buy(1); } }",
        "param p: int = 1; on_bar { for x in 0..10 { market_buy(1); } }",
        "param p: int = 1; on_bar { fn recursive() { recursive(); } }",
        "param p: int = 1; on_bar { import os; }",
        "param p: int = 1; on_bar { eval(\"market_buy(1)\"); }",
    ];

    for case in loop_cases {
        let res = parse_and_validate(case);
        assert!(res.is_err(), "Expected rejection for: {case}");
    }
}

#[test]
fn test_task_8_fuzz_100_bad_inputs_returns_errors_without_panicking() {
    let bad_seeds: Vec<String> = vec![
        // 1-10: Syntax & incomplete scripts
        String::new(),
        "     \n\t  ".into(),
        "on_bar".into(),
        "param".into(),
        "param 123: int = 1;".into(),
        "param x: unknown_type = 10; on_bar {}".into(),
        "param x: int = ; on_bar {}".into(),
        "param x: int = 10 min = 1; on_bar {}".into(),
        "param x: int = 10, min = 100, max = 10; on_bar {}".into(),
        "param x: int = 5, min = 10, max = 20; on_bar {}".into(),
        // 11-20: Unclosed literals and blocks
        "param x: int = 10; on_bar { \"unclosed string }".into(),
        "param x: int = 10; on_bar { /* unclosed comment }".into(),
        "param x: int = 10; on_bar { let y = 10;".into(),
        "param x: int = 10; on_bar { let y = (10 + 20; }".into(),
        "param x: int = 10; on_bar { let y = [10; }".into(),
        "param x: int = 10; on_bar { if { market_buy(1); } }".into(),
        "param x: int = 10; on_bar { if true market_buy(1); }".into(),
        "param x: int = 10; on_bar { else { market_buy(1); } }".into(),
        "param x: int = 10; on_bar { return }".into(),
        "param x: int = 10; on_bar { 10 + ; }".into(),
        // 21-30: Undeclared identifiers & reserved keywords
        "param x: int = 10; on_bar { let y = undeclared_var + 1; }".into(),
        "param x: int = 10; on_bar { undeclared_var = 5; }".into(),
        "param x: int = 10; on_bar { let close = 10; }".into(),
        "param x: int = 10; on_bar { close = 10; }".into(),
        "param x: int = 10; on_bar { let position = 10; }".into(),
        "param x: int = 10; on_bar { position = 10; }".into(),
        "param x: int = 10; on_bar { x = 20; }".into(), // immutable parameter
        "param x: int = 10; on_bar { non_existent_function(); }".into(),
        "param x: int = 10; on_bar { market_buy(); }".into(),
        "param x: int = 10; on_bar { market_buy(1, 2, 3); }".into(),
        // 31-40: Lookahead variations
        "param x: int = 10; on_bar { let p = close[-1]; }".into(),
        "param x: int = 10; on_bar { let p = open[-10]; }".into(),
        "param x: int = 10; on_bar { let p = high[-100]; }".into(),
        "param x: int = 10; on_bar { let p = low[-2]; }".into(),
        "param x: int = 10; on_bar { let p = volume[-5]; }".into(),
        "param x: int = 10; on_bar { let p = not_a_series[0]; }".into(),
        "param x: int = 10; on_bar { let p = 123[0]; }".into(),
        "param x: int = 10; on_bar { sma(not_a_series, 10); }".into(),
        "param x: int = 10; on_bar { ema(not_a_series, 10); }".into(),
        "param x: int = 10; on_bar { rsi(not_a_series, 10); }".into(),
        // 41-50: Duplicate definitions & scope violations
        "param x: int = 10; param x: int = 20; on_bar {}".into(),
        "param x: int = 10; on_bar { { let y = 1; } let z = y; }".into(),
        "param x: int = 10; on_bar { if true { let y = 1; } else { let z = y; } }".into(),
        "param x: int = 10; on_bar { let a = 1; let a = 2; }".into(),
        "param x: int = 10; on_start { let a = 1; } on_bar { let b = a; }".into(),
        "param x: int = 10; on_bar { let a = a + 1; }".into(),
        "param x: int = 10; on_bar { min(1); }".into(),
        "param x: int = 10; on_bar { max(); }".into(),
        "param x: int = 10; on_bar { limit_buy(10); }".into(),
        "param x: int = 10; on_bar { cancel_order(1, 2); }".into(),
    ];

    let mut all_inputs = bad_seeds;
    // Generate remaining to make at least 100 bad inputs
    for i in 51..=120 {
        all_inputs.push(format!("param p_{i}: int = {i}; on_bar {{ let bad_{i} = invalid_token_{i} ###; }}"));
    }

    assert!(all_inputs.len() >= 100, "Must test at least 100 bad inputs");

    let mut error_count = 0;
    for (idx, input) in all_inputs.iter().enumerate() {
        let result = parse_and_validate(input);
        assert!(result.is_err(), "Input #{idx} was expected to fail validation but succeeded: '{input}'");
        let err = result.unwrap_err();
        assert!(err.line > 0, "Error line must be > 0: {err:?}");
        assert!(err.col > 0, "Error col must be > 0: {err:?}");
        assert!(!err.message.is_empty(), "Error message must not be empty");
        error_count += 1;
    }

    assert_eq!(error_count, all_inputs.len());
}
