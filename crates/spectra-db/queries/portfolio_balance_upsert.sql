-- params: uuid, uuid, double precision, double precision, double precision, double precision
-- Upsert portfolio cash, equity, peak equity, and daily start equity.
WITH upsert AS (
    INSERT INTO portfolio_balances (workspace_id, strategy_id, cash, equity, peak_equity, daily_start_equity, updated_at)
    VALUES ($1, $2, $3, $4, $5, $6, now())
    ON CONFLICT (workspace_id, strategy_id) DO UPDATE SET
        cash = EXCLUDED.cash,
        equity = EXCLUDED.equity,
        peak_equity = EXCLUDED.peak_equity,
        daily_start_equity = EXCLUDED.daily_start_equity,
        updated_at = now()
    RETURNING id, workspace_id, strategy_id, cash, equity, peak_equity, daily_start_equity, updated_at
)
SELECT * FROM upsert;
