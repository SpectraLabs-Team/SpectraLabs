-- params: uuid, uuid, uuid, bigint, bigint, double precision, double precision
-- Record a step equity snapshot for equity history and drawdown analysis.
WITH ins AS (
    INSERT INTO equity_snapshots (workspace_id, strategy_id, run_id, step, ts_ns, equity, cash, created_at)
    VALUES ($1, $2, $3, $4, $5, $6, $7, now())
    RETURNING id, workspace_id, strategy_id, run_id, step, ts_ns, equity, cash, created_at
)
SELECT * FROM ins;
