-- M12 Alpha Workbench: Signal Library and versioning.
CREATE TABLE IF NOT EXISTS signals (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_signals_workspace_name UNIQUE (workspace_id, name)
);

CREATE TABLE IF NOT EXISTS signal_versions (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    signal_id UUID NOT NULL REFERENCES signals(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    signal_type TEXT NOT NULL DEFAULT 'builtin',
    parameters JSONB NOT NULL DEFAULT '{}'::jsonb,
    threshold JSONB,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_signal_versions_version UNIQUE (signal_id, version)
);

CREATE INDEX IF NOT EXISTS idx_signals_workspace
    ON signals(workspace_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_signal_versions_signal
    ON signal_versions(signal_id, version DESC);
