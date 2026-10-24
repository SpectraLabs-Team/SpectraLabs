//! Backtest-to-Paper Gap Analysis (M10).
//!
//! Re-runs or compares backtest fills against paper trading executions on the same period:
//! - Compares fills, slippage, timing and P&L trade by trade.
//! - Explains the gap by cause: transaction costs, slippage/spread, execution latency, missed fills.
//! - Evaluates configurable alert thresholds with automated breach detection.
//!
//! Written with the Rust standard library only, ensuring deterministic calculation.

use spectra_core::Json;

/// Side of an executed trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapTradeSide {
    Buy,
    Sell,
}

impl GapTradeSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

/// A fill record from paper trading.
#[derive(Debug, Clone)]
pub struct PaperFillRecord {
    pub symbol: String,
    pub side: GapTradeSide,
    pub qty: f64,
    pub price: f64,
    pub timestamp_ns: i64,
    pub fee: f64,
    pub slippage_bps: f64,
    pub order_id: Option<String>,
}

/// A fill record from the backtest run.
#[derive(Debug, Clone)]
pub struct BacktestFillRecord {
    pub symbol: String,
    pub side: GapTradeSide,
    pub qty: f64,
    pub price: f64,
    pub timestamp_ns: i64,
    pub fee: f64,
    pub slippage_bps: f64,
    pub bar_price: f64,
    pub order_id: Option<String>,
}

/// Trade-by-trade comparison between paper execution and backtest fill.
#[derive(Debug, Clone)]
pub struct MatchedTradeGap {
    pub symbol: String,
    pub side: GapTradeSide,
    pub qty: f64,
    pub paper_price: f64,
    pub backtest_price: f64,
    pub price_delta: f64,
    pub timing_latency_ms: f64,
    pub slippage_delta_bps: f64,
    pub fee_delta: f64,
    pub pnl_gap: f64,
}

/// Decomposition of the gap by cause.
#[derive(Debug, Clone, Default)]
pub struct GapAttribution {
    /// Difference from commissions, financing, exchange and borrow costs ($).
    pub cost_gap: f64,
    /// Difference from realized bid-ask spread and price impact ($).
    pub slippage_gap: f64,
    /// Difference caused by timing latency between bar signal and paper fill ($).
    pub timing_delay_gap: f64,
    /// Difference from orders filled in backtest but unfilled/rejected in paper ($).
    pub missed_fills_gap: f64,
    /// Total dollar discrepancy (Paper P&L - Backtest P&L).
    pub total_pnl_gap: f64,
}

/// Alert status for gap tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapAlertStatus {
    Normal,
    Warning,
    Breach,
}

impl GapAlertStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Breach => "breach",
        }
    }
}

/// Configurable thresholds for alerting.
#[derive(Debug, Clone)]
pub struct GapThresholds {
    /// Maximum acceptable slippage drift in basis points.
    pub max_slippage_drift_bps: f64,
    /// Maximum acceptable execution latency in milliseconds.
    pub max_timing_latency_ms: f64,
    /// Maximum acceptable annualized/period tracking error in percent.
    pub max_tracking_error_pct: f64,
    /// Minimum acceptable fill ratio (matched paper volume / backtest volume).
    pub min_fill_ratio: f64,
}

impl Default for GapThresholds {
    fn default() -> Self {
        Self {
            max_slippage_drift_bps: 10.0,
            max_timing_latency_ms: 1000.0,
            max_tracking_error_pct: 3.0,
            min_fill_ratio: 0.95,
        }
    }
}

/// An individual active gap alert.
#[derive(Debug, Clone)]
pub struct GapAlert {
    pub code: String,
    pub severity: GapAlertStatus,
    pub message: String,
    pub metric_name: String,
    pub threshold: f64,
    pub actual_value: f64,
}

/// Summary metrics of the gap analysis.
#[derive(Debug, Clone, Default)]
pub struct GapMetrics {
    pub mean_slippage_drift_bps: f64,
    pub mean_timing_latency_ms: f64,
    pub tracking_error_pct: f64,
    pub fill_ratio: f64,
    pub matched_trades: usize,
    pub unmatched_paper_trades: usize,
    pub unmatched_backtest_trades: usize,
}

/// Complete Backtest-to-Paper Gap report.
#[derive(Debug, Clone)]
pub struct GapReport {
    pub matched_trades: Vec<MatchedTradeGap>,
    pub attribution: GapAttribution,
    pub metrics: GapMetrics,
    pub alert_status: GapAlertStatus,
    pub alerts: Vec<GapAlert>,
}

fn num(v: f64) -> Json {
    Json::number(v).unwrap_or(Json::Null)
}

impl GapReport {
    /// Convert the report into JSON structure.
    pub fn to_json(&self) -> Json {
        let mut trades_arr = Vec::new();
        for t in &self.matched_trades {
            trades_arr.push(
                Json::object()
                    .with("symbol", Json::string(&t.symbol))
                    .with("side", Json::string(t.side.as_str()))
                    .with("qty", num(t.qty))
                    .with("paper_price", num(t.paper_price))
                    .with("backtest_price", num(t.backtest_price))
                    .with("price_delta", num(t.price_delta))
                    .with("timing_latency_ms", num(t.timing_latency_ms))
                    .with("slippage_delta_bps", num(t.slippage_delta_bps))
                    .with("fee_delta", num(t.fee_delta))
                    .with("pnl_gap", num(t.pnl_gap)),
            );
        }

        let mut alerts_arr = Vec::new();
        for a in &self.alerts {
            alerts_arr.push(
                Json::object()
                    .with("code", Json::string(&a.code))
                    .with("severity", Json::string(a.severity.as_str()))
                    .with("message", Json::string(&a.message))
                    .with("metric_name", Json::string(&a.metric_name))
                    .with("threshold", num(a.threshold))
                    .with("actual_value", num(a.actual_value)),
            );
        }

        Json::object()
            .with(
                "attribution",
                Json::object()
                    .with("cost_gap", num(self.attribution.cost_gap))
                    .with("slippage_gap", num(self.attribution.slippage_gap))
                    .with("timing_delay_gap", num(self.attribution.timing_delay_gap))
                    .with("missed_fills_gap", num(self.attribution.missed_fills_gap))
                    .with("total_pnl_gap", num(self.attribution.total_pnl_gap)),
            )
            .with(
                "metrics",
                Json::object()
                    .with("mean_slippage_drift_bps", num(self.metrics.mean_slippage_drift_bps))
                    .with("mean_timing_latency_ms", num(self.metrics.mean_timing_latency_ms))
                    .with("tracking_error_pct", num(self.metrics.tracking_error_pct))
                    .with("fill_ratio", num(self.metrics.fill_ratio))
                    .with("matched_trades", num(self.metrics.matched_trades as f64))
                    .with("unmatched_paper_trades", num(self.metrics.unmatched_paper_trades as f64))
                    .with("unmatched_backtest_trades", num(self.metrics.unmatched_backtest_trades as f64)),
            )
            .with("alert_status", Json::string(self.alert_status.as_str()))
            .with("alerts", Json::Array(alerts_arr))
            .with("matched_trades", Json::Array(trades_arr))
    }
}

/// Compute backtest-to-paper gap analysis.
///
/// Matches paper fills and backtest fills trade-by-trade in FIFO order per symbol and side.
/// Decomposes the P&L divergence into cost, slippage, timing latency, and missed fill gaps.
pub fn compute_backtest_to_paper_gap(
    mut paper_fills: Vec<PaperFillRecord>,
    mut backtest_fills: Vec<BacktestFillRecord>,
    thresholds: &GapThresholds,
) -> GapReport {
    paper_fills.sort_by_key(|f| f.timestamp_ns);
    backtest_fills.sort_by_key(|f| f.timestamp_ns);

    let mut matched_trades = Vec::new();
    let mut total_backtest_qty = 0.0;
    let mut total_matched_qty = 0.0;

    let mut cost_gap = 0.0;
    let mut slippage_gap = 0.0;
    let mut timing_delay_gap = 0.0;
    let mut missed_fills_gap = 0.0;

    let mut sum_slippage_drift = 0.0;
    let mut sum_timing_latency = 0.0;

    // Track unmatched backtest fills
    let mut bt_matched = vec![false; backtest_fills.len()];

    for p in &paper_fills {
        // Find first matching unmatched backtest fill for same symbol and side
        let mut found_idx = None;
        for (i, b) in backtest_fills.iter().enumerate() {
            if !bt_matched[i] && b.symbol == p.symbol && b.side == p.side {
                found_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = found_idx {
            bt_matched[idx] = true;
            let b = &backtest_fills[idx];
            let qty = p.qty.min(b.qty);
            total_matched_qty += qty;
            total_backtest_qty += b.qty;

            let price_delta = match p.side {
                GapTradeSide::Buy => p.price - b.price,
                GapTradeSide::Sell => b.price - p.price,
            };

            let timing_latency_ms = (p.timestamp_ns.saturating_sub(b.timestamp_ns)) as f64 / 1_000_000.0;
            let slippage_delta_bps = p.slippage_bps - b.slippage_bps;
            let fee_delta = p.fee - b.fee;

            // Attribution breakdown for this trade:
            // 1. Cost gap: fee delta
            cost_gap += fee_delta;

            // 2. Slippage gap: price difference from modeled vs realized slippage bps
            let slip_component = qty * b.price * (slippage_delta_bps / 10_000.0);
            slippage_gap += slip_component;

            // 3. Timing latency gap: remaining price delta beyond slippage
            let delay_component = (price_delta * qty) - slip_component;
            timing_delay_gap += delay_component;

            let trade_pnl_gap = -(fee_delta + (price_delta * qty));

            sum_slippage_drift += slippage_delta_bps;
            sum_timing_latency += timing_latency_ms.abs();

            matched_trades.push(MatchedTradeGap {
                symbol: p.symbol.clone(),
                side: p.side,
                qty,
                paper_price: p.price,
                backtest_price: b.price,
                price_delta,
                timing_latency_ms,
                slippage_delta_bps,
                fee_delta,
                pnl_gap: trade_pnl_gap,
            });
        } else {
            // Paper trade executed with no backtest counterpart (unexpected trade)
            missed_fills_gap += p.qty * p.price;
        }
    }

    // Account for backtest trades that were never filled in paper
    let mut unmatched_bt_count = 0;
    for (i, b) in backtest_fills.iter().enumerate() {
        if !bt_matched[i] {
            unmatched_bt_count += 1;
            total_backtest_qty += b.qty;
            missed_fills_gap -= b.qty * b.price;
        }
    }

    let unmatched_paper_count = paper_fills.len().saturating_sub(matched_trades.len());
    let matched_count = matched_trades.len();

    let mean_slippage_drift_bps =
        if matched_count > 0 { sum_slippage_drift / (matched_count as f64) } else { 0.0 };

    let mean_timing_latency_ms =
        if matched_count > 0 { sum_timing_latency / (matched_count as f64) } else { 0.0 };

    let fill_ratio =
        if total_backtest_qty > 0.0 { (total_matched_qty / total_backtest_qty).min(1.0) } else { 1.0 };

    // Tracking error approximation: standard deviation of trade pnl gaps relative to trade value
    let tracking_error_pct = if matched_count > 0 {
        let mean_gap = matched_trades.iter().map(|t| t.pnl_gap).sum::<f64>() / (matched_count as f64);
        let var = matched_trades
            .iter()
            .map(|t| {
                let diff = t.pnl_gap - mean_gap;
                diff * diff
            })
            .sum::<f64>()
            / (matched_count as f64);
        var.sqrt() / 100.0
    } else {
        0.0
    };

    let total_pnl_gap = -(cost_gap + slippage_gap + timing_delay_gap) + missed_fills_gap;

    // Check alert thresholds
    let mut alerts = Vec::new();
    let mut status = GapAlertStatus::Normal;

    if mean_slippage_drift_bps.abs() > thresholds.max_slippage_drift_bps {
        let sev = if mean_slippage_drift_bps.abs() > thresholds.max_slippage_drift_bps * 1.5 {
            GapAlertStatus::Breach
        } else {
            GapAlertStatus::Warning
        };
        if sev == GapAlertStatus::Breach || status == GapAlertStatus::Normal {
            status = sev;
        }
        alerts.push(GapAlert {
            code: "slippage_drift_exceeded".into(),
            severity: sev,
            message: format!(
                "Average slippage drift ({:.2} bps) exceeds threshold ({:.2} bps)",
                mean_slippage_drift_bps, thresholds.max_slippage_drift_bps
            ),
            metric_name: "mean_slippage_drift_bps".into(),
            threshold: thresholds.max_slippage_drift_bps,
            actual_value: mean_slippage_drift_bps,
        });
    }

    if mean_timing_latency_ms > thresholds.max_timing_latency_ms {
        let sev = if mean_timing_latency_ms > thresholds.max_timing_latency_ms * 2.0 {
            GapAlertStatus::Breach
        } else {
            GapAlertStatus::Warning
        };
        if sev == GapAlertStatus::Breach || status == GapAlertStatus::Normal {
            status = sev;
        }
        alerts.push(GapAlert {
            code: "execution_latency_high".into(),
            severity: sev,
            message: format!(
                "Mean execution latency ({:.1} ms) exceeds threshold ({:.1} ms)",
                mean_timing_latency_ms, thresholds.max_timing_latency_ms
            ),
            metric_name: "mean_timing_latency_ms".into(),
            threshold: thresholds.max_timing_latency_ms,
            actual_value: mean_timing_latency_ms,
        });
    }

    if fill_ratio < thresholds.min_fill_ratio {
        let sev = GapAlertStatus::Breach;
        status = sev;
        alerts.push(GapAlert {
            code: "fill_ratio_breach".into(),
            severity: sev,
            message: format!(
                "Fill execution ratio ({:.1}%) is below minimum threshold ({:.1}%)",
                fill_ratio * 100.0,
                thresholds.min_fill_ratio * 100.0
            ),
            metric_name: "fill_ratio".into(),
            threshold: thresholds.min_fill_ratio,
            actual_value: fill_ratio,
        });
    }

    if tracking_error_pct > thresholds.max_tracking_error_pct {
        let sev = GapAlertStatus::Warning;
        if status == GapAlertStatus::Normal {
            status = sev;
        }
        alerts.push(GapAlert {
            code: "tracking_error_warning".into(),
            severity: sev,
            message: format!(
                "Tracking error ({:.2}%) exceeds threshold ({:.2}%)",
                tracking_error_pct, thresholds.max_tracking_error_pct
            ),
            metric_name: "tracking_error_pct".into(),
            threshold: thresholds.max_tracking_error_pct,
            actual_value: tracking_error_pct,
        });
    }

    GapReport {
        matched_trades,
        attribution: GapAttribution {
            cost_gap,
            slippage_gap,
            timing_delay_gap,
            missed_fills_gap,
            total_pnl_gap,
        },
        metrics: GapMetrics {
            mean_slippage_drift_bps,
            mean_timing_latency_ms,
            tracking_error_pct,
            fill_ratio,
            matched_trades: matched_count,
            unmatched_paper_trades: unmatched_paper_count,
            unmatched_backtest_trades: unmatched_bt_count,
        },
        alert_status: status,
        alerts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_match_yields_zero_gap_and_normal_status() {
        let paper = vec![
            PaperFillRecord {
                symbol: "AAPL".into(),
                side: GapTradeSide::Buy,
                qty: 100.0,
                price: 150.0,
                timestamp_ns: 1_000_000,
                fee: 1.0,
                slippage_bps: 2.0,
                order_id: Some("p1".into()),
            },
            PaperFillRecord {
                symbol: "AAPL".into(),
                side: GapTradeSide::Sell,
                qty: 100.0,
                price: 155.0,
                timestamp_ns: 2_000_000,
                fee: 1.0,
                slippage_bps: 2.0,
                order_id: Some("p2".into()),
            },
        ];

        let backtest = vec![
            BacktestFillRecord {
                symbol: "AAPL".into(),
                side: GapTradeSide::Buy,
                qty: 100.0,
                price: 150.0,
                timestamp_ns: 1_000_000,
                fee: 1.0,
                slippage_bps: 2.0,
                bar_price: 150.0,
                order_id: Some("b1".into()),
            },
            BacktestFillRecord {
                symbol: "AAPL".into(),
                side: GapTradeSide::Sell,
                qty: 100.0,
                price: 155.0,
                timestamp_ns: 2_000_000,
                fee: 1.0,
                slippage_bps: 2.0,
                bar_price: 155.0,
                order_id: Some("b2".into()),
            },
        ];

        let report = compute_backtest_to_paper_gap(paper, backtest, &GapThresholds::default());
        assert_eq!(report.alert_status, GapAlertStatus::Normal);
        assert_eq!(report.matched_trades.len(), 2);
        assert_eq!(report.metrics.fill_ratio, 1.0);
        assert!((report.attribution.cost_gap).abs() < 1e-6);
        assert!((report.attribution.slippage_gap).abs() < 1e-6);
        assert!((report.attribution.timing_delay_gap).abs() < 1e-6);
        assert!(report.alerts.is_empty());
    }

    #[test]
    fn realistic_gap_decomposes_attribution_and_triggers_warning() {
        let paper = vec![PaperFillRecord {
            symbol: "NVDA".into(),
            side: GapTradeSide::Buy,
            qty: 50.0,
            price: 501.20,             // Paid 501.20 (higher slippage and delay)
            timestamp_ns: 500_000_000, // 500 ms latency
            fee: 2.50,
            slippage_bps: 15.0, // 15 bps slippage vs 5 bps modeled
            order_id: Some("p1".into()),
        }];

        let backtest = vec![BacktestFillRecord {
            symbol: "NVDA".into(),
            side: GapTradeSide::Buy,
            qty: 50.0,
            price: 500.25,
            timestamp_ns: 0,
            fee: 1.00,
            slippage_bps: 5.0,
            bar_price: 500.00,
            order_id: Some("b1".into()),
        }];

        let thresholds = GapThresholds {
            max_slippage_drift_bps: 8.0,
            max_timing_latency_ms: 200.0,
            max_tracking_error_pct: 5.0,
            min_fill_ratio: 0.90,
        };

        let report = compute_backtest_to_paper_gap(paper, backtest, &thresholds);
        assert_ne!(report.alert_status, GapAlertStatus::Normal);
        assert_eq!(report.matched_trades.len(), 1);
        assert!(report.attribution.cost_gap > 0.0);
        assert!(report.attribution.slippage_gap > 0.0);
        assert!(report.metrics.mean_timing_latency_ms >= 500.0);

        let json = report.to_json();
        assert!(json.dump().contains("attribution"));
        assert!(json.dump().contains("slippage_drift_exceeded"));
    }
}
