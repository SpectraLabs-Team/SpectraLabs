-- Admin console: per-user daily active time, recorded from client heartbeats.
--
-- The browser sends a heartbeat about once a minute while the page is visible and the person has
-- used it recently (see components/activity/ActivityTracker.tsx). The server credits at most 60
-- seconds per heartbeat and nothing when the gap since the previous heartbeat is over 120 seconds,
-- so a closed laptop or a background tab does not count as active time.
--
-- Only daily totals are kept: no URLs, no page content, no per-click data. Rows older than the
-- retention window are removed by `npm run activity:purge` (default 400 days).

CREATE TABLE user_activity_daily (
  user_id           uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  day               date        NOT NULL,
  active_seconds    integer     NOT NULL DEFAULT 0 CHECK (active_seconds >= 0),
  heartbeats        integer     NOT NULL DEFAULT 0 CHECK (heartbeats >= 0),
  last_heartbeat_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, day)
);

CREATE INDEX user_activity_daily_day ON user_activity_daily (day);
