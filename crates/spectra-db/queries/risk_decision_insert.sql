-- params: uuid, uuid, text, text, double precision, double precision, double precision, text, jsonb, text[], jsonb, uuid
-- Record a pre-trade risk decision in the audit trail.
WITH ins AS (
    INSERT INTO risk_decisions (
        workspace_id, strategy_id, symbol, side, requested_qty, price,
        approved_qty, verdict, breaches, reasons, metrics, actor_id
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    RETURNING *
)
SELECT * FROM ins;
