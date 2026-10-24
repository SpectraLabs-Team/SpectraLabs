-- Backtest-to-Paper Gap Analysis (M10):
-- Stores trade-by-trade comparison, attribution breakdown (cost, slippage, delay, missed fills)
-- and alert breach verdicts.

CREATE TABLE IF NOT EXISTS backtest_paper_gap_reports (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    strategy_id UUID NOT NULL REFERENCES strategies(id) ON DELETE CASCADE,
    backtest_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    paper_account_id UUID,
    period_start TIMESTAMPTZ,
    period_end TIMESTAMPTZ,
    total_pnl_gap DOUBLE PRECISION NOT NULL,
    cost_gap DOUBLE PRECISION NOT NULL,
    slippage_gap DOUBLE PRECISION NOT NULL,
    timing_delay_gap DOUBLE PRECISION NOT NULL,
    missed_fills_gap DOUBLE PRECISION NOT NULL,
    tracking_error_pct DOUBLE PRECISION NOT NULL,
    mean_slippage_drift_bps DOUBLE PRECISION NOT NULL,
    mean_timing_latency_ms DOUBLE PRECISION NOT NULL,
    fill_ratio DOUBLE PRECISION NOT NULL,
    alert_status TEXT NOT NULL CHECK (alert_status IN ('normal', 'warning', 'breach')),
    report_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_gap_reports_workspace_strategy
    ON backtest_paper_gap_reports(workspace_id, strategy_id, created_at DESC);
