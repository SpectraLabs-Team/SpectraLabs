-- M36: Chart drawings and saved layout templates

CREATE TABLE IF NOT EXISTS chart_drawings (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chart_id TEXT NOT NULL CHECK (length(chart_id) <= 128),
    dataset_version_id TEXT,
    tool TEXT NOT NULL CHECK (tool IN ('trendline', 'horizontal_level', 'fibonacci', 'text')),
    points JSONB NOT NULL,
    style JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_chart_drawings_lookup
    ON chart_drawings(workspace_id, chart_id, deleted_at);

CREATE TABLE IF NOT EXISTS chart_templates (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0 AND length(name) <= 100),
    version INT NOT NULL DEFAULT 1 CHECK (version >= 1),
    indicators JSONB NOT NULL DEFAULT '[]'::jsonb,
    drawings JSONB NOT NULL DEFAULT '[]'::jsonb,
    comparisons JSONB NOT NULL DEFAULT '[]'::jsonb,
    dataset_versions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_chart_templates_workspace
    ON chart_templates(workspace_id, name);
