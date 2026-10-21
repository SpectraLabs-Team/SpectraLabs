-- params: uuid, timestamptz, timestamptz
-- $1 id, $2 last_run_at, $3 next_run_at
UPDATE dataset_refresh_schedules
SET last_run_at = $2, next_run_at = $3, updated_at = now()
WHERE id = $1
RETURNING id;
