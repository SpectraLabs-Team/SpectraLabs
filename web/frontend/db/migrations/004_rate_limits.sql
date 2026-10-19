-- M19 task 7: rate limits and login lockouts that survive restarts and work across several
-- servers (they were in-memory per process).

-- Fixed-window counters. One row per key (e.g. "login:ip:203.0.113.7"); a row whose window has
-- ended is reset on its next hit and deleted by occasional housekeeping.
CREATE TABLE rate_limits (
  key      text        PRIMARY KEY,
  count    integer     NOT NULL,
  reset_at timestamptz NOT NULL
);

CREATE INDEX rate_limits_reset_at_idx ON rate_limits (reset_at);

-- Account lockout: 10 failed password attempts lock sign-in for 15 minutes (the owner is emailed).
-- A successful sign-in or a password reset clears it.
ALTER TABLE users
  ADD COLUMN failed_login_count   integer     NOT NULL DEFAULT 0,
  ADD COLUMN last_failed_login_at timestamptz,
  ADD COLUMN locked_until         timestamptz;
