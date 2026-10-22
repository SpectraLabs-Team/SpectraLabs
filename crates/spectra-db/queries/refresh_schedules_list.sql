-- params: uuid
-- $1 workspace_id
SELECT id, workspace_id, dataset_id, provider_config_id, symbols, interval_seconds, lookback_bars, enabled, last_run_at, next_run_at, created_by, created_at, updated_at
FROM dataset_refresh_schedules
WHERE workspace_id = $1
ORDER BY created_at DESC;
