-- M19 task 3: workspaces and membership.
-- Every user has one personal workspace; teams are workspaces with several members. All research
-- data (datasets, runs, jobs) will be owned by a workspace, never directly by a user.

CREATE TABLE workspaces (
  id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  name              text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 60),
  slug              text        NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 72),
  created_by        uuid        REFERENCES users (id) ON DELETE SET NULL,
  -- Set only on personal workspaces: one per user, deleted with the account.
  personal_owner_id uuid        UNIQUE REFERENCES users (id) ON DELETE CASCADE,
  created_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
  workspace_id uuid        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  -- viewer: read only · editor: create and run · owner: members, settings, delete
  role         text        NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
  created_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (workspace_id, user_id)
);

CREATE INDEX workspace_members_user_id_idx ON workspace_members (user_id);

-- Active workspace of a session. If the workspace is deleted the session falls back to the
-- user's personal workspace on its next request.
ALTER TABLE sessions
  ADD CONSTRAINT sessions_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES workspaces (id) ON DELETE SET NULL;

-- Backfill: a personal workspace (owner role) for every existing user.
WITH created AS (
  INSERT INTO workspaces (name, slug, created_by, personal_owner_id)
  SELECT
    'Personal',
    coalesce(nullif(btrim(left(regexp_replace(lower(split_part(u.email, '@', 1)), '[^a-z0-9]+', '-', 'g'), 40), '-'), ''), 'user')
      || '-' || substr(replace(gen_random_uuid()::text, '-', ''), 1, 6),
    u.id,
    u.id
  FROM users u
  WHERE NOT EXISTS (SELECT 1 FROM workspaces w WHERE w.personal_owner_id = u.id)
  RETURNING id, personal_owner_id
)
INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT id, personal_owner_id, 'owner' FROM created;

-- Existing sessions start in their user's personal workspace.
UPDATE sessions s SET workspace_id = w.id FROM workspaces w WHERE w.personal_owner_id = s.user_id AND s.workspace_id IS NULL;
