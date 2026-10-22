-- params: uuid, uuid, double precision, double precision, double precision, double precision, double precision, double precision, double precision, double precision, boolean, uuid
-- Upsert risk limits for a workspace and optional strategy.
WITH upsert AS (
    INSERT INTO risk_limits (
        workspace_id, strategy_id, max_gross_exposure, max_net_exposure,
        max_position_size, max_concentration_pct, max_leverage,
        max_drawdown_pct, max_daily_loss, max_var_pct,
        size_reduction_enabled, updated_by, updated_at
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
    ON CONFLICT (workspace_id, strategy_id) DO UPDATE SET
        max_gross_exposure     = EXCLUDED.max_gross_exposure,
        max_net_exposure       = EXCLUDED.max_net_exposure,
        max_position_size      = EXCLUDED.max_position_size,
        max_concentration_pct  = EXCLUDED.max_concentration_pct,
        max_leverage           = EXCLUDED.max_leverage,
        max_drawdown_pct       = EXCLUDED.max_drawdown_pct,
        max_daily_loss         = EXCLUDED.max_daily_loss,
        max_var_pct            = EXCLUDED.max_var_pct,
        size_reduction_enabled = EXCLUDED.size_reduction_enabled,
        updated_by             = EXCLUDED.updated_by,
        updated_at             = now()
    RETURNING *
)
SELECT * FROM upsert;
