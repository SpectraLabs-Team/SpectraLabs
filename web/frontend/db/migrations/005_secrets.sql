-- M19 task 8: encrypted storage for broker API keys (envelope encryption).
--
-- Each workspace has one random 256-bit data key, stored only wrapped (AES-256-GCM) by a master
-- key that lives outside the database (SECRETS_MASTER_KEY). Secrets are encrypted with their
-- workspace's data key. A copy of the database alone can't decrypt anything, and rotating the
-- master key only re-wraps one small row per workspace.

CREATE TABLE workspace_keys (
  workspace_id   uuid        PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
  -- format byte | 12-byte IV | 16-byte GCM tag | encrypted data key
  wrapped_key    bytea       NOT NULL,
  -- Which master key wrapped it (see SECRETS_MASTER_KEY's "version:key" form).
  master_version integer     NOT NULL,
  created_at     timestamptz NOT NULL DEFAULT now(),
  rotated_at     timestamptz
);

CREATE TABLE secrets (
  id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id uuid        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  kind         text        NOT NULL CHECK (kind IN ('alpaca_paper')),
  -- format byte | 12-byte IV | 16-byte GCM tag | encrypted JSON; bound to (workspace, kind)
  ciphertext   bytea       NOT NULL,
  -- The last 4 characters of the key id: all the UI ever shows.
  hint         text        NOT NULL CHECK (length(hint) <= 4),
  created_by   uuid        REFERENCES users (id) ON DELETE SET NULL,
  created_at   timestamptz NOT NULL DEFAULT now(),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  last_used_at timestamptz,
  UNIQUE (workspace_id, kind)
);
