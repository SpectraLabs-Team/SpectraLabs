-- params: double precision
-- Workers that sent a heartbeat in the last $1 seconds (and prunes rows silent for a day).
WITH gone AS (DELETE FROM workers WHERE heartbeat_at < now() - interval '1 day')
SELECT count(*) AS alive FROM workers WHERE heartbeat_at >= now() - make_interval(secs => $1)
