-- M19 task 2: database-backed sessions. The cookie holds only a random 32-byte token; the server
-- keeps this row. Allows signing out one device, "sign out everywhere" and idle timeouts.

CREATE TABLE sessions (
  id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  -- HMAC-SHA256(AUTH_SECRET, token). The token itself is never stored, so a copy of this table
  -- can't be replayed as cookies.
  token_hash   bytea       NOT NULL UNIQUE,
  user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  -- Active workspace. Task 3 adds the workspaces table and the foreign key.
  workspace_id uuid,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL,
  ip           text,
  user_agent   text,
  revoked_at   timestamptz
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id) WHERE revoked_at IS NULL;
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

-- Replaced by per-session revocation (sessions.revoked_at).
ALTER TABLE users DROP COLUMN sessions_revoked_at;
