#![allow(dead_code)]

//!
//! The frontend renders these sections as JSON, and the stored PDF is rendered from the same
//! values, so the on-screen sheet and the PDF cannot disagree. The formatting rules mirror
//! `web/frontend/components/studio/backtests/format.ts`; keep the two in step.

use serde::Serialize;
use serde_json::Value;

/// One metric card on the sheet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TearSheetItem {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<&'static str>,
}

/// A titled group of metric cards.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TearSheetSection {
    pub category: String,
    pub items: Vec<TearSheetItem>,
}

/// The strategy allocation table, shown only when the run has strategies.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TearSheetTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Everything the sheet shows below its header.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TearSheet {
    pub sections: Vec<TearSheetSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<TearSheetTable>,
}

const DASH: &str = "—";

/// JavaScript `toFixed`: exact halves round away from zero, unlike Rust's round-half-to-even.
fn js_fixed(v: f64, digits: usize) -> String {
    let abs = v.abs();
    // A long exact expansion of the binary value, so a tie (…5000…) is detected exactly.
    let exact = format!("{abs:.1100}");
    let (int_part, frac) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let tie = frac.as_bytes().get(digits) == Some(&b'5') && frac[digits + 1..].bytes().all(|b| b == b'0');
    let body = if tie { round_up(int_part, &frac[..digits]) } else { format!("{abs:.digits$}") };
    if v < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// Adds one unit in the last place to `int_part.frac`, carrying as needed.
fn round_up(int_part: &str, frac: &str) -> String {
    let mut d: Vec<u8> = int_part.bytes().chain(frac.bytes()).collect();
    let mut i = d.len();
    loop {
        if i == 0 {
            d.insert(0, b'1');
            break;
        }
        i -= 1;
        if d[i] == b'9' {
            d[i] = b'0';
        } else {
            d[i] += 1;
            break;
        }
    }
    let int_len = d.len() - frac.len();
    let mut out = String::from_utf8(d[..int_len].to_vec()).unwrap_or_default();
    if !frac.is_empty() {
        out.push('.');
        out.push_str(std::str::from_utf8(&d[int_len..]).unwrap_or_default());
    }
    out
}

/// Inserts thousands separators into a run of digits, as `en-US` does.
fn group(digits: &str) -> String {
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}

/// `pct`: 12.34% / -5.25%. Negatives use ASCII '-', as `format.ts` `pct` does (`toFixed`).
pub fn pct(v: Option<f64>, digits: usize) -> String {
    finite(v).map_or(DASH.to_string(), |x| format!("{}%", js_fixed(x * 100.0, digits)))
}

/// `signedPct`: +4.20% / −1.05%. The minus sign is U+2212, as in the frontend.
pub fn signed_pct(v: Option<f64>, digits: usize) -> String {
    match finite(v) {
        None => DASH.to_string(),
        Some(x) => {
            let s = format!("{}%", js_fixed(x.abs() * 100.0, digits));
            if x > 0.0 {
                format!("+{s}")
            } else if x < 0.0 {
                format!("−{s}")
            } else {
                s
            }
        }
    }
}

/// `ratio`: a plain number with `digits` decimals.
pub fn ratio(v: Option<f64>, digits: usize) -> String {
    finite(v).map_or(DASH.to_string(), |x| js_fixed(x, digits))
}

/// `int`: rounds half up (as `Math.round`), then groups thousands.
pub fn int(v: Option<f64>) -> String {
    finite(v).map_or(DASH.to_string(), |x| {
        let r = (x + 0.5).floor();
        let sign = if r < 0.0 { "-" } else { "" };
        format!("{sign}{}", group(&format!("{:.0}", r.abs())))
    })
}

/// `cash`: money with thousands separators and a fixed number of decimals.
pub fn cash(v: Option<f64>, digits: usize) -> String {
    finite(v).map_or(DASH.to_string(), |x| {
        let fixed = js_fixed(x, digits);
        let (sign, rest) = fixed.strip_prefix('-').map_or(("", fixed.as_str()), |r| ("-", r));
        match rest.split_once('.') {
            Some((i, f)) => format!("{sign}{}.{f}", group(i)),
            None => format!("{sign}{}", group(rest)),
        }
    })
}

/// `tone`: profit, loss or neutral for a signed number.
#[allow(clippy::unnecessary_wraps)]
pub fn tone(v: Option<f64>) -> Option<&'static str> {
    Some(match finite(v) {
        Some(x) if x > 0.0 => "profit",
        Some(x) if x < 0.0 => "loss",
        _ => "neutral",
    })
}

/// Reads a number at a JSON path. Numbers may be stored as strings (decimal values), as the frontend's `Number(...)` allows.
fn num(v: &Value, path: &str) -> Option<f64> {
    match v.pointer(path)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn item(
    label: impl Into<String>,
    value: String,
    sub: Option<String>,
    tone: Option<&'static str>,
) -> TearSheetItem {
    TearSheetItem { label: label.into(), value, sub, tone }
}

/// Builds the sections the frontend shows for one backtest, from its stored `summary`.
pub fn build(summary: &Value) -> TearSheet {
    let n = |p: &str| num(summary, p);
    let total_return = n("/metrics/total_return");
    let cagr = n("/performance/cagr");
    let realized = n("/activity/realized_pnl");
    let max_dd = n("/drawdown/max_drawdown");
    let skew_level = n("/tail/level");

    let sections = vec![
        TearSheetSection {
            category: "Return & Capital Dynamics".into(),
            items: vec![
                item(
                    "Total Return",
                    signed_pct(total_return, 2),
                    Some(format!("{} trading periods", int(n("/metrics/periods")))),
                    tone(total_return),
                ),
                item(
                    "Compound Annual Growth (CAGR)",
                    signed_pct(cagr, 2),
                    Some(format!("Annualized σ {}", pct(n("/metrics/annualized_volatility"), 2))),
                    tone(cagr),
                ),
                item(
                    "Initial Capital",
                    cash(n("/run/config/initial_cash"), 0),
                    Some("Starting cash".into()),
                    None,
                ),
                item(
                    "Final Equity",
                    cash(n("/activity/final_equity"), 0),
                    Some(format!("Realized PnL {}", cash(realized, 0))),
                    tone(realized),
                ),
            ],
        },
        TearSheetSection {
            category: "Risk-Adjusted Performance Ratios".into(),
            items: vec![
                item(
                    "Sharpe Ratio",
                    ratio(n("/metrics/sharpe"), 2),
                    Some("Annualized excess return / total risk".into()),
                    None,
                ),
                item(
                    "Sortino Ratio",
                    ratio(n("/performance/sortino"), 2),
                    Some("Excess return / downside deviation".into()),
                    None,
                ),
                item(
                    "Calmar Ratio",
                    ratio(n("/performance/calmar"), 2),
                    Some("CAGR / maximum drawdown".into()),
                    None,
                ),
                item(
                    "Maximum Drawdown",
                    pct(max_dd, 2),
                    Some(format!(
                        "Longest underwater: {} periods",
                        int(n("/drawdown/longest_underwater_periods"))
                    )),
                    Some("loss"),
                ),
            ],
        },
        TearSheetSection {
            category: "Trading Execution & Distribution".into(),
            items: vec![
                item(
                    "Total Completed Trades",
                    int(n("/trades/count")),
                    Some(format!("{} individual fills", int(n("/activity/fill_count")))),
                    None,
                ),
                item(
                    "Win Rate",
                    pct(n("/trades/win_rate"), 1),
                    Some(format!("Profit factor: {}", ratio(n("/trades/profit_factor"), 2))),
                    None,
                ),
                item(
                    "Payoff Ratio (Win / Loss)",
                    ratio(n("/trades/payoff_ratio"), 2),
                    Some(format!("Expectancy: {}", cash(n("/trades/expectancy"), 2))),
                    None,
                ),
                item(
                    "Commission & Fees Paid",
                    cash(n("/activity/total_fees"), 2),
                    Some(format!("Financing: {}", cash(n("/activity/financing"), 2))),
                    None,
                ),
            ],
        },
        TearSheetSection {
            category: "Tail Risk & Statistical Moments".into(),
            items: vec![
                item(
                    format!("Value at Risk ({})", pct(skew_level, 0)),
                    pct(n("/tail/var"), 2),
                    Some("Historical simulation VaR".into()),
                    Some("loss"),
                ),
                item(
                    "Expected Shortfall (CVaR)",
                    pct(n("/tail/expected_shortfall"), 2),
                    Some("Average loss in tail regime".into()),
                    Some("loss"),
                ),
                item(
                    "Return Skewness",
                    ratio(n("/performance/skewness"), 2),
                    Some("Asymmetry of return distribution".into()),
                    None,
                ),
                item(
                    "Excess Kurtosis",
                    ratio(n("/performance/excess_kurtosis"), 2),
                    Some("Fat-tail indicator".into()),
                    None,
                ),
            ],
        },
    ];

    TearSheet { sections, table: strategy_table(summary) }
}

/// The per-strategy table. Labels are the raw strategy names: the frontend's catalog
/// labels are not available on the server.
fn strategy_table(summary: &Value) -> Option<TearSheetTable> {
    let list = summary.pointer("/strategies/list")?.as_array()?;
    if list.is_empty() {
        return None;
    }
    let rows = list
        .iter()
        .map(|s| {
            let f = |k: &str| {
                s.get(k).and_then(|v| match v {
                    Value::Number(n) => n.as_f64(),
                    Value::String(t) => t.parse::<f64>().ok(),
                    _ => None,
                })
            };
            vec![
                s.get("strategy").and_then(Value::as_str).unwrap_or(DASH).to_string(),
                cash(f("allocation"), 0),
                cash(f("final_equity"), 0),
                signed_pct(f("total_return"), 2),
                signed_pct(f("contribution"), 2),
                ratio(f("sharpe"), 2),
                pct(f("max_drawdown"), 2),
            ]
        })
        .collect();
    Some(TearSheetTable {
        title: "Strategy Sub-Component Allocation & Attribution".into(),
        headers: ["Strategy", "Allocation", "Final Equity", "Return", "Contribution", "Sharpe", "Max DD"]
            .map(String::from)
            .to_vec(),
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_fixed_rounds_exact_ties_away_from_zero() {
        // JS: (0.125).toFixed(2) === "0.13"; Rust's format! would give "0.12".
        assert_eq!(js_fixed(0.125, 2), "0.13");
        assert_eq!(js_fixed(2.5, 0), "3");
        assert_eq!(js_fixed(0.5, 0), "1");
        assert_eq!(js_fixed(-0.125, 2), "-0.13");
        assert_eq!(js_fixed(9.995, 2), "9.99"); // not an exact tie in binary
        assert_eq!(js_fixed(1.005, 2), "1.00"); // not an exact tie in binary
    }

    #[test]
    fn percentages_and_signs() {
        assert_eq!(pct(Some(0.042), 2), "4.20%");
        assert_eq!(pct(Some(-0.0525), 2), "-5.25%");
        assert_eq!(signed_pct(Some(0.042), 2), "+4.20%");
        assert_eq!(signed_pct(Some(-0.0105), 2), "−1.05%");
        assert_eq!(signed_pct(None, 2), "—");
        assert_eq!(pct(Some(f64::NAN), 2), "—");
    }

    #[test]
    fn ratios_ints_and_cash() {
        assert_eq!(ratio(Some(1.5), 2), "1.50");
        assert_eq!(int(Some(1_234_567.4)), "1,234,567");
        assert_eq!(int(Some(2.5)), "3");
        assert_eq!(int(Some(-2.5)), "-2");
        assert_eq!(cash(Some(1234.5), 2), "1,234.50");
        assert_eq!(cash(Some(-9876.0), 0), "-9,876");
        assert_eq!(cash(None, 2), "—");
    }

    #[test]
    fn tones() {
        assert_eq!(tone(Some(0.1)), Some("profit"));
        assert_eq!(tone(Some(-0.1)), Some("loss"));
        assert_eq!(tone(Some(0.0)), Some("neutral"));
        assert_eq!(tone(None), Some("neutral"));
    }

    #[test]
    fn builds_the_frontend_sections_from_a_summary() {
        let summary = json!({
            "metrics": {"total_return": 0.042, "periods": 252, "annualized_volatility": 0.15, "sharpe": 1.2},
            "performance": {"cagr": 0.04, "sortino": 1.5, "calmar": 0.8, "skewness": -0.3, "excess_kurtosis": 2.1},
            "drawdown": {"max_drawdown": -0.0525, "longest_underwater_periods": 40},
            "run": {"config": {"initial_cash": "100000"}},
            "activity": {"final_equity": 104_200.0, "realized_pnl": 4200.0, "fill_count": 310,
                         "total_fees": 12.5, "financing": 0.0},
            "trades": {"count": 80, "win_rate": 0.55, "profit_factor": 1.4, "payoff_ratio": 1.1, "expectancy": 52.5},
            "tail": {"level": 0.95, "var": -0.021, "expected_shortfall": -0.034}
        });
        let sheet = build(&summary);
        assert_eq!(sheet.sections.len(), 4);
        let ret = &sheet.sections[0].items[0];
        assert_eq!(ret.label, "Total Return");
        assert_eq!(ret.value, "+4.20%");
        assert_eq!(ret.sub.as_deref(), Some("252 trading periods"));
        assert_eq!(ret.tone, Some("profit"));
        assert_eq!(sheet.sections[0].items[0].sub.as_deref(), Some("252 trading periods"));
        assert_eq!(sheet.sections[0].items[2].value, "100,000");
        assert_eq!(sheet.sections[1].items[3].value, "-5.25%");
        assert_eq!(sheet.sections[3].items[0].label, "Value at Risk (95%)");
        assert!(sheet.table.is_none());
    }

    #[test]
    fn strategy_table_appears_only_with_strategies() {
        let summary = json!({"strategies": {"list": [
            {"strategy": "momentum", "allocation": 50000, "final_equity": 52000,
             "total_return": 0.04, "contribution": 0.02, "sharpe": 1.1, "max_drawdown": -0.03}
        ]}});
        let t = build(&summary).table.expect("table");
        assert_eq!(t.rows[0], ["momentum", "50,000", "52,000", "+4.00%", "+2.00%", "1.10", "-3.00%"]);
    }
}
