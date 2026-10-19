-- M19 task 1: auth data moves from .data/*.json to Postgres.
-- Emails are stored trimmed and lower-cased; the CHECKs make sure nothing else slips in.

CREATE TABLE users (
  id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
  email               text        NOT NULL UNIQUE CHECK (email = lower(btrim(email)) AND email <> ''),
  name                text        NOT NULL DEFAULT '',
  password_hash       text        NOT NULL,
  platform_role       text        NOT NULL DEFAULT 'user' CHECK (platform_role IN ('admin', 'user')),
  status              text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
  -- Ask for an emailed code at every login. Admins always have it on (enforced in authStore).
  two_factor          boolean     NOT NULL DEFAULT true,
  created_at          timestamptz NOT NULL DEFAULT now(),
  -- Sessions issued before either of these are rejected.
  password_changed_at timestamptz,
  -- "Sign out everywhere" for the current signed-cookie sessions. Task 2 (database-backed
  -- sessions) replaces this with per-session revocation.
  sessions_revoked_at timestamptz
);

-- One-time 6-digit codes for sign-up, password reset and two-step login. Only a SHA-256 of the
-- code is stored. One live code per (kind, email); a new code replaces the old one.
CREATE TABLE email_codes (
  kind          text        NOT NULL CHECK (kind IN ('signup', 'reset', 'login')),
  email         text        NOT NULL CHECK (email = lower(btrim(email)) AND email <> ''),
  code_hash     text        NOT NULL,
  expires_at    timestamptz NOT NULL,
  attempts      integer     NOT NULL DEFAULT 0,
  -- Sign-up only: the account to create once the code is confirmed.
  password_hash text,
  name          text,
  two_factor    boolean,
  created_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (kind, email)
);

CREATE INDEX email_codes_expires_at_idx ON email_codes (expires_at);
