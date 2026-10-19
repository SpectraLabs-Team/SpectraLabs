-- M19 task 17: API tokens for programmatic access (scripts, notebooks, CI) to /api/engine.
--   personal   acts as the person who made it, in one workspace, and never with more than their
--              current role there (a removed member's tokens stop working at once).
--   workspace  belongs to the workspace (made by an owner); keeps working if its creator leaves.
-- Scopes: 'read' (viewer-level reads) and 'run' (editor-level computations; always with 'read').
-- Nothing owner-level (members, keys, settings) is ever reachable with a token.
-- The token is shown once; only a keyed hash is stored. Every token expires (at most a year).

CREATE TABLE api_tokens (
  id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  kind         text        NOT NULL CHECK (kind IN ('personal', 'workspace')),
  workspace_id uuid        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  -- Personal tokens: whose. Deleting the account deletes its tokens.
  user_id      uuid        REFERENCES users (id) ON DELETE CASCADE,
  created_by   uuid        REFERENCES users (id) ON DELETE SET NULL,
  name         text        NOT NULL CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 60),
  scopes       text[]      NOT NULL CHECK (scopes = ARRAY['read'] OR scopes = ARRAY['read', 'run']),
  token_hash   bytea       NOT NULL UNIQUE,
  -- Last 4 characters, so people can tell their tokens apart.
  hint         text        NOT NULL CHECK (length(hint) = 4),
  created_at   timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL,
  last_used_at timestamptz,
  last_used_ip text,
  revoked_at   timestamptz,
  revoked_by   uuid        REFERENCES users (id) ON DELETE SET NULL,
  CHECK ((kind = 'personal') = (user_id IS NOT NULL)),
  CHECK (expires_at > created_at AND expires_at <= created_at + interval '366 days')
);

CREATE INDEX api_tokens_workspace_idx ON api_tokens (workspace_id, created_at DESC);
CREATE INDEX api_tokens_user_idx      ON api_tokens (user_id) WHERE user_id IS NOT NULL;
