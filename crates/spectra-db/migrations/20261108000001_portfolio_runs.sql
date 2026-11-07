-- Portfolio backtest runs (M15 Task 6): a saved portfolio run through the rebalancing simulator on
-- a dataset version, with its inputs and result stored like a backtest's.
--
-- `request` records every input the run used, including a snapshot of the portfolio's name and
-- holdings at the time, so a run stays readable and reproducible after the portfolio is edited or
-- deleted. Deleting the portfolio keeps its runs and clears `portfolio_id`.
--
-- A run is `running` while it is computed and `succeeded` or `failed` once it ends; `result` is
-- set only on success, `error` only on failure.

CREATE TABLE portfolio_runs (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    portfolio_id uuid,
    status       text NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    request      jsonb NOT NULL,
    result       jsonb,
    error        text,
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    finished_at  timestamptz,
    -- The portfolio must be in the run's workspace; deleting it clears only `portfolio_id`.
    FOREIGN KEY (portfolio_id, workspace_id) REFERENCES portfolios (id, workspace_id)
        ON DELETE SET NULL (portfolio_id),
    CHECK ((status = 'succeeded') = (result IS NOT NULL)),
    CHECK ((status = 'failed') = (error IS NOT NULL)),
    CHECK ((status = 'running') = (finished_at IS NULL))
);

-- A portfolio's runs, newest first (ids are UUIDv7, so id order is creation order).
CREATE INDEX portfolio_runs_portfolio ON portfolio_runs (workspace_id, portfolio_id, id DESC);

-- Portfolio reports are stored through M16 like every other report.
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_report_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_report_type_check
    CHECK (report_type IN ('validation', 'tear_sheet', 'gap', 'signal_test', 'portfolio'));

ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_run_kind_check;
ALTER TABLE reports ADD CONSTRAINT reports_run_kind_check
    CHECK (run_kind IN ('sweep', 'backtest', 'paper_gap', 'signal_test', 'portfolio_backtest'));
