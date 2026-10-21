-- M6: workspace-scoped survival presets and durable stress reports.
CREATE TABLE survival_presets (
    id uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 80),
    config jsonb NOT NULL,
    created_by uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

CREATE TABLE survival_reports (
    id uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    run_id uuid NOT NULL,
    preset_id uuid,
    job_id uuid UNIQUE,
    created_by uuid REFERENCES users (id) ON DELETE SET NULL,
    status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    config jsonb NOT NULL,
    worst_verdict text CHECK (worst_verdict IS NULL OR worst_verdict IN ('pass', 'warn', 'fail', 'insufficient')),
    verdicts jsonb NOT NULL DEFAULT '{}'::jsonb,
    results jsonb,
    text_sha256 text,
    text_path text,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    FOREIGN KEY (workspace_id, run_id) REFERENCES runs (workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (job_id) REFERENCES jobs (id) ON DELETE SET NULL,
    CHECK ((status IN ('succeeded', 'failed', 'cancelled')) = (finished_at IS NOT NULL))
);
CREATE INDEX survival_reports_run_created ON survival_reports (workspace_id, run_id, created_at DESC, id DESC);
CREATE INDEX survival_reports_verdict_created ON survival_reports (workspace_id, worst_verdict, created_at DESC) WHERE worst_verdict IS NOT NULL;
