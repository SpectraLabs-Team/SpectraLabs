//! Portfolio report (M15 Task 5): a rebalancing run rendered to PDF through the shared M16 builder.
//!
//! The report shows the run's headline numbers, each holding's weight before and after every
//! rebalance (weights over time), each rebalance's turnover and cost, and every trade with its
//! recorded cost. It draws with the tear sheet's writer, so sections and tables look the same as
//! the tear sheet's allocation table.
//!
//! Deterministic: there is no generation time in the file and every number is formatted with a
//! fixed number of decimals, so the same run gives byte-identical output. Storing the report as
//! a versioned M16 report needs the portfolio run endpoint (M15 Task 6) and is not done here.

use spectra_backtest::portfolio_backtest::PortfolioBacktestSummary;
use spectra_backtest::rebalance::{Reason, RebalanceRun};

use crate::tear_sheet::{TearSheetItem, TearSheetSection, TearSheetTable};
use crate::tear_sheet_pdf::Writer;

/// Shown instead of the tables when the run has no rebalances.
pub const NO_REBALANCES: &str =
    "No rebalances in this run: there are no weights, turnover or trades to show.";

/// A fraction as a percentage with two decimals (0.1234 -> "12.34%"), never "-0.00%".
fn pct(v: f64) -> String {
    let s = format!("{:.2}%", v * 100.0);
    if s == "-0.00%" {
        "0.00%".to_string()
    } else {
        s
    }
}

/// An amount with two decimals and thousands separators (12345.6 -> "12,345.60").
fn money(v: f64) -> String {
    let s = format!("{:.2}", v.abs());
    let (int_part, frac) = s.split_once('.').expect("two decimals always have a point");
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if v < 0.0 && s != "0.00" { "-" } else { "" };
    format!("{sign}{grouped}.{frac}")
}

fn reason(r: Reason) -> &'static str {
    match r {
        Reason::Initial => "Initial",
        Reason::Calendar => "Calendar",
        Reason::Drift => "Drift",
    }
}

fn item(label: &str, value: String) -> TearSheetItem {
    TearSheetItem { label: label.into(), value, sub: None, tone: None }
}

fn summary_section(run: &RebalanceRun, summary: &PortfolioBacktestSummary) -> TearSheetSection {
    let final_equity = run.equity.last().map_or_else(|| "-".to_string(), |(_, e)| money(*e));
    let mut items = vec![
        item("Total return", pct(summary.total_return)),
        item("Max drawdown", pct(summary.max_drawdown)),
        item("Final equity", final_equity),
        item("Rebalances", summary.rebalance_count.to_string()),
        item("Total turnover", pct(summary.total_turnover)),
        item("Total cost", money(summary.total_cost)),
    ];
    if let Some(config) = run.events.first().map(|e| &e.cost_config) {
        items.push(TearSheetItem {
            label: "Cost config".into(),
            value: format!("{} v{}", config.name, config.version),
            sub: Some(format!("slippage {} bps, minimum fee {}", config.slippage_bps, money(config.min_fee))),
            tone: None,
        });
    }
    TearSheetSection { category: "Portfolio summary".into(), items }
}

fn weights_table(run: &RebalanceRun) -> TearSheetTable {
    let rows = run
        .events
        .iter()
        .flat_map(|e| {
            e.weights
                .iter()
                .map(move |w| vec![e.date.to_string(), w.symbol.clone(), pct(w.before), pct(w.after)])
        })
        .collect();
    TearSheetTable {
        title: "Weights at each rebalance".into(),
        headers: vec!["Date".into(), "Symbol".into(), "Before".into(), "After".into()],
        rows,
    }
}

fn rebalances_table(run: &RebalanceRun) -> TearSheetTable {
    let rows = run
        .events
        .iter()
        .map(|e| {
            vec![
                e.date.to_string(),
                e.reasons.iter().map(|r| reason(*r)).collect::<Vec<_>>().join(", "),
                e.trades.len().to_string(),
                pct(e.turnover),
                money(e.cost),
            ]
        })
        .collect();
    TearSheetTable {
        title: "Turnover and cost per rebalance".into(),
        headers: vec!["Date".into(), "Reason".into(), "Trades".into(), "Turnover".into(), "Cost".into()],
        rows,
    }
}

fn trades_table(run: &RebalanceRun) -> TearSheetTable {
    let rows = run
        .events
        .iter()
        .flat_map(|e| {
            e.trades.iter().map(move |t| {
                vec![
                    e.date.to_string(),
                    t.symbol.clone(),
                    format!("{:+.4}", t.qty),
                    format!("{:.4}", t.price),
                    format!("{:.4}", t.exec_price),
                    money(t.slippage_cost),
                    money(t.fee),
                ]
            })
        })
        .collect();
    TearSheetTable {
        title: "Trades".into(),
        headers: vec![
            "Date".into(),
            "Symbol".into(),
            "Quantity".into(),
            "Price".into(),
            "Exec price".into(),
            "Slippage".into(),
            "Fee".into(),
        ],
        rows,
    }
}

/// Renders a rebalancing run and its summary as a PDF. `title` (e.g. the portfolio's name) heads
/// every page; the subtitle gives the run's dates and rebalance count.
#[must_use]
pub fn render(run: &RebalanceRun, summary: &PortfolioBacktestSummary, title: &str) -> Vec<u8> {
    let subtitle = match (run.equity.first(), run.equity.last()) {
        (Some((first, _)), Some((last, _))) => {
            format!("Portfolio report | {first} to {last} | {} rebalances", run.events.len())
        }
        _ => "Portfolio report | no prices".to_string(),
    };
    let mut w = Writer::new(title, &subtitle);
    w.section(&summary_section(run, summary));
    if run.events.is_empty() {
        w.note(NO_REBALANCES);
    } else {
        w.table(&weights_table(run));
        w.gap(16.0);
        w.table(&rebalances_table(run));
        w.gap(16.0);
        w.table(&trades_table(run));
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_backtest::portfolio::Holding;
    use spectra_backtest::portfolio_backtest::{run_portfolio_backtest, summarize};
    use spectra_backtest::rebalance::{Date, Frequency, RebalanceRule, TradeCostConfig};

    /// The text the PDF draws, with string escapes undone: every `(...) Tj` operand, one per line.
    fn drawn_text(pdf: &[u8]) -> Vec<String> {
        String::from_utf8_lossy(pdf)
            .lines()
            .filter_map(|line| {
                let start = line.find('(')?;
                let end = line.rfind(") Tj")?;
                (start < end).then(|| line[start + 1..end].replace("\\(", "(").replace("\\)", ")"))
            })
            .collect()
    }

    /// Two instruments, 10,000 cash, a flat 1.00 fee per trade and no slippage, rebalanced monthly:
    /// the initial allocation on Jan 2 and one calendar rebalance on Feb 2, after A rises 40%.
    fn two_instrument_run() -> (RebalanceRun, PortfolioBacktestSummary) {
        let symbols = vec!["A".to_string(), "B".to_string()];
        let targets =
            vec![Holding { symbol: "A".into(), weight: 0.5 }, Holding { symbol: "B".into(), weight: 0.5 }];
        let dates = [Date::new(2026, 1, 2), Date::new(2026, 2, 2)];
        let closes = vec![vec![100.0, 100.0], vec![140.0, 100.0]];
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None };
        let cost = TradeCostConfig { name: "flat-fee".into(), version: 1, slippage_bps: 0.0, min_fee: 1.0 };
        run_portfolio_backtest(&symbols, &targets, &dates, &closes, 10_000.0, rule, &cost).unwrap()
    }

    #[test]
    fn known_answer_two_instruments_one_rebalance() {
        // Jan 2 (initial): two trades cost 2.00, so 9,998 is invested, 4,999 each at 100:
        //   49.99 shares of A and of B, cash 0. Turnover 9,998 / 10,000 = 99.98%.
        // Feb 2 (calendar): A at 140. Equity 49.99 * 140 + 49.99 * 100 = 11,997.60.
        //   Weights before: A 6,998.60 / 11,997.60 = 58.33%, B 41.67%.
        //   Two trades cost 2.00, leaving 11,995.60, 5,997.80 per holding:
        //   A sells 1,000.80 / 140 = 7.1486, B buys 998.80 / 100 = 9.9880.
        //   Turnover 1,999.60 / 11,997.60 = 16.67%. Weights after: 50.00% each.
        // Totals: return 11,995.60 / 10,000 - 1 = 19.96%, turnover 116.65%, cost 4.00, drawdown 0.
        let (run, summary) = two_instrument_run();
        assert_eq!(run.events.len(), 2);
        assert!((run.events[1].weights[0].before - 6998.6 / 11997.6).abs() < 1e-12);
        assert!((run.equity[1].1 - 11_995.6).abs() < 1e-9);

        let text = drawn_text(&render(&run, &summary, "Two-asset portfolio"));
        let has = |s: &str| text.iter().any(|t| t == s);
        let row =
            |cells: &[&str]| text.windows(cells.len()).any(|w| w.iter().zip(cells).all(|(a, b)| a == b));

        assert!(has("Two-asset portfolio"));
        assert!(has("Portfolio report | 2026-01-02 to 2026-02-02 | 2 rebalances"));
        for (label, value) in [
            ("Total return", "19.96%"),
            ("Max drawdown", "0.00%"),
            ("Final equity", "11,995.60"),
            ("Rebalances", "2"),
            ("Total turnover", "116.65%"),
            ("Total cost", "4.00"),
        ] {
            assert!(row(&[label, value]), "summary {label} = {value}");
        }
        // The cost config's sub-line is drawn between its label and value.
        assert!(row(&["Cost config", "slippage 0 bps, minimum fee 1.00", "flat-fee v1"]));
        // Weights over time.
        assert!(row(&["2026-01-02", "A", "0.00%", "50.00%"]));
        assert!(row(&["2026-02-02", "A", "58.33%", "50.00%"]));
        assert!(row(&["2026-02-02", "B", "41.67%", "50.00%"]));
        // Turnover and cost per rebalance.
        assert!(row(&["2026-01-02", "Initial", "2", "99.98%", "2.00"]));
        assert!(row(&["2026-02-02", "Calendar", "2", "16.67%", "2.00"]));
        // Trades with their recorded costs.
        assert!(row(&["2026-02-02", "A", "-7.1486", "140.0000", "140.0000", "0.00", "1.00"]));
        assert!(row(&["2026-02-02", "B", "+9.9880", "100.0000", "100.0000", "0.00", "1.00"]));
    }

    #[test]
    fn the_same_inputs_give_byte_identical_reports() {
        // Two independent simulations of the same inputs, each rendered.
        let (run1, s1) = two_instrument_run();
        let (run2, s2) = two_instrument_run();
        let first = render(&run1, &s1, "Two-asset portfolio");
        let second = render(&run2, &s2, "Two-asset portfolio");
        assert_eq!(first, second);
        assert_eq!(first, render(&run1, &s1, "Two-asset portfolio"), "rendering again changes nothing");
        // And the bytes do depend on the inputs.
        assert_ne!(first, render(&run1, &s1, "Another name"));
    }

    #[test]
    fn a_run_without_rebalances_still_renders_and_says_so() {
        let run = RebalanceRun { events: vec![], equity: vec![], total_cost: 0.0, total_turnover: 0.0 };
        let summary = summarize(&run, 10_000.0);
        let pdf = render(&run, &summary, "Empty portfolio");
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        let text = drawn_text(&pdf);
        assert!(text.iter().any(|t| t == NO_REBALANCES), "the report says there are no rebalances");
        assert!(text.iter().any(|t| t == "Portfolio report | no prices"));
        assert!(!text.iter().any(|t| t == "TRADES"), "no empty tables are drawn");
        assert_eq!(pdf, render(&run, &summary, "Empty portfolio"), "deterministic when empty too");
    }

    #[test]
    fn a_long_run_breaks_onto_more_pages_and_keeps_every_rebalance() {
        let symbols: Vec<String> = ["A", "B", "C"].iter().map(|s| (*s).to_string()).collect();
        let targets: Vec<Holding> =
            symbols.iter().map(|s| Holding { symbol: s.clone(), weight: 1.0 / 3.0 }).collect();
        let dates: Vec<Date> = (0..36).map(|i| Date::new(2024 + i / 12, (i % 12) as u32 + 1, 2)).collect();
        let closes: Vec<Vec<f64>> =
            (0..36).map(|i| vec![100.0 + f64::from(i), 50.0 + f64::from(i % 5), 20.0]).collect();
        let rule = RebalanceRule { calendar: Some(Frequency::Monthly), drift_threshold: None };
        let cost =
            TradeCostConfig { name: "paper-default".into(), version: 1, slippage_bps: 5.0, min_fee: 1.0 };
        let (run, summary) =
            run_portfolio_backtest(&symbols, &targets, &dates, &closes, 100_000.0, rule, &cost).unwrap();

        let pdf = render(&run, &summary, "Three-asset portfolio");
        let s = String::from_utf8_lossy(&pdf);
        assert!(!s.contains("/Count 1 "), "36 rebalances do not fit on one page");
        let text = drawn_text(&pdf);
        for e in &run.events {
            assert!(text.iter().any(|t| *t == e.date.to_string()), "rebalance {} is in the report", e.date);
        }
    }

    #[test]
    fn number_formats() {
        assert_eq!(pct(0.1234), "12.34%");
        assert_eq!(pct(-0.000_01), "0.00%");
        assert_eq!(money(1_234_567.891), "1,234,567.89");
        assert_eq!(money(999.995), "1,000.00");
        assert_eq!(money(-12.5), "-12.50");
        assert_eq!(money(-0.001), "0.00");
    }
}
