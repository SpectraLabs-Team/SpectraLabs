-- M19 task 10: authenticator-app codes (TOTP), recovery codes, and "confirm it's you" (step-up).

-- Authenticator secret, encrypted with the master key (AES-256-GCM, bound to the user id).
-- confirmed_at is set once the user proves the app works by entering a first code; until then the
-- row is a pending enrolment and isn't used at sign-in.
CREATE TABLE user_totp (
  user_id           uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
  secret_ciphertext bytea       NOT NULL,
  master_version    integer     NOT NULL,
  created_at        timestamptz NOT NULL DEFAULT now(),
  confirmed_at      timestamptz,
  -- Last time step accepted: a code can't be used twice (replay protection).
  last_step         bigint
);

-- Ten single-use codes for when the phone is lost. Only a keyed hash is stored.
CREATE TABLE recovery_codes (
  user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  code_hash  bytea       NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  used_at    timestamptz,
  PRIMARY KEY (user_id, code_hash)
);

-- Step-up: when this session last proved it's the account owner (sign-in, password or code).
-- Sensitive actions need this to be recent.
ALTER TABLE sessions ADD COLUMN reauth_at timestamptz;
UPDATE sessions SET reauth_at = created_at;

-- Sign-in with an authenticator: the password step issues a short-lived single-use challenge
-- (kind 'mfa') that the code step must present, so the second step can't be reached without the first.
ALTER TABLE email_codes DROP CONSTRAINT email_codes_kind_check;
ALTER TABLE email_codes ADD CONSTRAINT email_codes_kind_check CHECK (kind IN ('signup', 'reset', 'login', 'mfa'));
