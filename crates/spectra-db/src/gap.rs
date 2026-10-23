//! Backtest-to-Paper Gap database operations (M10).

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// Stored gap report row.
#[derive(Debug, Clone, FromRow, serde::Serialize, serde::Deserialize)]
pub struct GapReportRecord {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub backtest_run_id: Option<Uuid>,
    pub paper_account_id: Option<Uuid>,
    pub period_start: Option<chrono::DateTime<chrono::Utc>>,
    pub period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub total_pnl_gap: f64,
    pub cost_gap: f64,
    pub slippage_gap: f64,
    pub timing_delay_gap: f64,
    pub missed_fills_gap: f64,
    pub tracking_error_pct: f64,
    pub mean_slippage_drift_bps: f64,
    pub mean_timing_latency_ms: f64,
    pub fill_ratio: f64,
    pub alert_status: String,
    pub report_json: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Parameters for creating a gap report record.
#[derive(Debug, Clone)]
pub struct NewGapReport {
    pub workspace_id: Uuid,
    pub strategy_id: Uuid,
    pub backtest_run_id: Option<Uuid>,
    pub paper_account_id: Option<Uuid>,
    pub period_start: Option<chrono::DateTime<chrono::Utc>>,
    pub period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub total_pnl_gap: f64,
    pub cost_gap: f64,
    pub slippage_gap: f64,
    pub timing_delay_gap: f64,
    pub missed_fills_gap: f64,
    pub tracking_error_pct: f64,
    pub mean_slippage_drift_bps: f64,
    pub mean_timing_latency_ms: f64,
    pub fill_ratio: f64,
    pub alert_status: String,
    pub report_json: serde_json::Value,
}

/// Insert a new gap analysis report.
///
/// # Errors
/// Returns an error if the database query fails.
pub async fn insert_gap_report(pool: &PgPool, rep: &NewGapReport) -> Result<GapReportRecord, sqlx::Error> {
    sqlx::query_as::<_, GapReportRecord>(
        r"
        INSERT INTO backtest_paper_gap_reports (
            workspace_id, strategy_id, backtest_run_id, paper_account_id,
            period_start, period_end, total_pnl_gap, cost_gap, slippage_gap,
            timing_delay_gap, missed_fills_gap, tracking_error_pct,
            mean_slippage_drift_bps, mean_timing_latency_ms, fill_ratio,
            alert_status, report_json
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17
        )
        RETURNING *
        ",
    )
    .bind(rep.workspace_id)
    .bind(rep.strategy_id)
    .bind(rep.backtest_run_id)
    .bind(rep.paper_account_id)
    .bind(rep.period_start)
    .bind(rep.period_end)
    .bind(rep.total_pnl_gap)
    .bind(rep.cost_gap)
    .bind(rep.slippage_gap)
    .bind(rep.timing_delay_gap)
    .bind(rep.missed_fills_gap)
    .bind(rep.tracking_error_pct)
    .bind(rep.mean_slippage_drift_bps)
    .bind(rep.mean_timing_latency_ms)
    .bind(rep.fill_ratio)
    .bind(&rep.alert_status)
    .bind(&rep.report_json)
    .fetch_one(pool)
    .await
}

/// List gap reports for a workspace and optional strategy.
///
/// # Errors
/// Returns an error if querying reports fails.
pub async fn list_gap_reports(
    pool: &PgPool,
    workspace_id: Uuid,
    strategy_id: Option<Uuid>,
) -> Result<Vec<GapReportRecord>, sqlx::Error> {
    if let Some(strat) = strategy_id {
        sqlx::query_as::<_, GapReportRecord>(
            r"
            SELECT * FROM backtest_paper_gap_reports
            WHERE workspace_id = $1 AND strategy_id = $2
            ORDER BY created_at DESC
            LIMIT 50
            ",
        )
        .bind(workspace_id)
        .bind(strat)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, GapReportRecord>(
            r"
            SELECT * FROM backtest_paper_gap_reports
            WHERE workspace_id = $1
            ORDER BY created_at DESC
            LIMIT 50
            ",
        )
        .bind(workspace_id)
        .fetch_all(pool)
        .await
    }
}

/// Fetch a single gap report by id.
///
/// # Errors
/// Returns an error if querying the report fails.
pub async fn get_gap_report(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<GapReportRecord>, sqlx::Error> {
    sqlx::query_as::<_, GapReportRecord>(
        r"
        SELECT * FROM backtest_paper_gap_reports
        WHERE workspace_id = $1 AND id = $2
        ",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}
