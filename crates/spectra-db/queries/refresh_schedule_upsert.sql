-- params: uuid, uuid, uuid, text[], bigint, integer, boolean, uuid
-- $1 workspace_id, $2 dataset_id, $3 provider_config_id, $4 symbols, $5 interval_seconds, $6 lookback_bars, $7 enabled, $8 created_by
WITH ins AS (
    INSERT INTO dataset_refresh_schedules (
        workspace_id, dataset_id, provider_config_id, symbols, interval_seconds, lookback_bars, enabled, created_by, updated_at
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
    ON CONFLICT (workspace_id, dataset_id) DO UPDATE SET
        provider_config_id = EXCLUDED.provider_config_id,
        symbols = EXCLUDED.symbols,
        interval_seconds = EXCLUDED.interval_seconds,
        lookback_bars = EXCLUDED.lookback_bars,
        enabled = EXCLUDED.enabled,
        updated_at = now()
    RETURNING id, workspace_id, dataset_id, provider_config_id, symbols, interval_seconds, lookback_bars, enabled, last_run_at, next_run_at, created_by, created_at, updated_at
)
SELECT id, workspace_id, dataset_id, provider_config_id, symbols, interval_seconds, lookback_bars, enabled, last_run_at, next_run_at, created_by, created_at, updated_at FROM ins;
