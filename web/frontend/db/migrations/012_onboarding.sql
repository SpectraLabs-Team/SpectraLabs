-- Guided website tour: per-user onboarding state, so the tour starts once per user and per tour
-- version, on any device. One row per user, removed with the account.
--   completed / skipped: the user finished or dismissed the tour whose number is in `version`.
--   last_step: the last step the user reached (for resuming), a short step id.
-- A user with no row has never seen the tour.

CREATE TABLE user_onboarding (
  user_id    uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
  completed  boolean     NOT NULL DEFAULT false,
  skipped    boolean     NOT NULL DEFAULT false,
  version    integer     NOT NULL DEFAULT 0 CHECK (version >= 0 AND version <= 1000),
  last_step  text        CHECK (last_step IS NULL OR last_step ~ '^[a-z0-9-]{1,64}$'),
  updated_at timestamptz NOT NULL DEFAULT now()
);
