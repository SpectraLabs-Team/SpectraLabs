-- M12 Alpha Workbench: Signal tests, decay, quantile analysis, and gate thresholds.
-- Enables versioned reports through M16 and stores signal validation runs.

ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_report_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_report_type_check
    CHECK (report_type IN ('validation', 'tear_sheet', 'gap', 'signal_test'));

ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_run_kind_check;
ALTER TABLE reports ADD CONSTRAINT reports_run_kind_check
    CHECK (run_kind IN ('sweep', 'backtest', 'paper_gap', 'signal_test'));

CREATE TABLE IF NOT EXISTS signal_tests (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    dataset_version_id UUID REFERENCES dataset_versions(id) ON DELETE SET NULL,
    signal_name TEXT NOT NULL,
    universe JSONB NOT NULL DEFAULT '[]'::jsonb,
    horizons JSONB NOT NULL DEFAULT '[1, 5, 10, 20]'::jsonb,
    primary_horizon INTEGER NOT NULL DEFAULT 5,
    period_start TIMESTAMPTZ,
    period_end TIMESTAMPTZ,
    mean_ic DOUBLE PRECISION NOT NULL,
    ic_stdev DOUBLE PRECISION NOT NULL,
    ic_t_stat DOUBLE PRECISION NOT NULL,
    half_life_days DOUBLE PRECISION NOT NULL,
    top_quantile_turnover DOUBLE PRECISION NOT NULL,
    top_bottom_spread DOUBLE PRECISION NOT NULL,
    threshold JSONB,
    passed_gate BOOLEAN NOT NULL DEFAULT true,
    tests_run_count INTEGER NOT NULL DEFAULT 1,
    report_json JSONB NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_signal_tests_workspace
    ON signal_tests(workspace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_signal_tests_gate
    ON signal_tests(workspace_id, signal_name, passed_gate, created_at DESC);
