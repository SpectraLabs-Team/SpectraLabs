-- Strategy drafts, versions DSL code, and parameter sets (M2b, Strategy Studio full).

-- 1. Add code column to strategy_versions if not present
ALTER TABLE strategy_versions
    ADD COLUMN IF NOT EXISTS code TEXT;

-- 2. Mutable draft per strategy (autosaved by the editor)
CREATE TABLE IF NOT EXISTS strategy_drafts (
    strategy_id UUID PRIMARY KEY REFERENCES strategies(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    code TEXT NOT NULL DEFAULT '',
    parameters JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_strategy_drafts_ws
    ON strategy_drafts(workspace_id, updated_at DESC);

-- 3. Named parameter sets per strategy version
CREATE TABLE IF NOT EXISTS strategy_parameter_sets (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    strategy_version_id UUID NOT NULL REFERENCES strategy_versions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    parameters JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    CONSTRAINT strategy_param_sets_unique UNIQUE (strategy_version_id, name)
);

CREATE INDEX IF NOT EXISTS idx_strategy_param_sets_version
    ON strategy_parameter_sets(workspace_id, strategy_version_id);
