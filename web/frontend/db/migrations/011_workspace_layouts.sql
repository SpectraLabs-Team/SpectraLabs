-- M18: Workspace dashboard layouts and server-side layout sharing.
-- Supports user custom layouts and workspace-wide default layouts (user_id IS NULL).

CREATE TABLE IF NOT EXISTS workspace_layouts (
  id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id uuid        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  user_id      uuid        REFERENCES users (id) ON DELETE CASCADE,
  layout       jsonb       NOT NULL,
  updated_at   timestamptz NOT NULL DEFAULT now()
);

-- One custom layout per user per workspace
CREATE UNIQUE INDEX IF NOT EXISTS workspace_layouts_user_idx
  ON workspace_layouts (workspace_id, user_id)
  WHERE user_id IS NOT NULL;

-- Exactly one workspace-wide default layout per workspace
CREATE UNIQUE INDEX IF NOT EXISTS workspace_layouts_default_idx
  ON workspace_layouts (workspace_id)
  WHERE user_id IS NULL;
