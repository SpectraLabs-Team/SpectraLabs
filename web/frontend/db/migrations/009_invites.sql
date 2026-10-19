-- M19 task 12: team management. Invitations by email: a random single-use link, valid 7 days,
-- that only the invited email address can accept. Only a keyed hash of the token is stored.

CREATE TABLE workspace_invites (
  id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id uuid        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  email        text        NOT NULL CHECK (email = lower(btrim(email)) AND email <> ''),
  -- Owners join through an ownership transfer or a role change, not an invite.
  role         text        NOT NULL CHECK (role IN ('editor', 'viewer')),
  token_hash   bytea       NOT NULL UNIQUE,
  invited_by   uuid        REFERENCES users (id) ON DELETE SET NULL,
  created_at   timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL,
  accepted_at  timestamptz,
  accepted_by  uuid        REFERENCES users (id) ON DELETE SET NULL,
  revoked_at   timestamptz
);

-- At most one open invite per email per workspace (a new invite replaces the old one).
CREATE UNIQUE INDEX workspace_invites_open_idx ON workspace_invites (workspace_id, email) WHERE accepted_at IS NULL AND revoked_at IS NULL;
