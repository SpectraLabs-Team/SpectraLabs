-- params: timestamptz, bigint
-- $1 as_of, $2 limit
SELECT id, workspace_id, dataset_id, provider_config_id, symbols, interval_seconds, lookback_bars, enabled, last_run_at, next_run_at, created_by, created_at, updated_at
FROM dataset_refresh_schedules
WHERE enabled = true AND next_run_at <= $1
ORDER BY next_run_at ASC
LIMIT $2;
