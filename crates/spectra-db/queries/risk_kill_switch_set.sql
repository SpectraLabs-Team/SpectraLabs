-- params: uuid, uuid, boolean, text, uuid
-- Set or clear a kill switch for a workspace or strategy.
WITH upsert AS (
    INSERT INTO risk_kill_switches (
        workspace_id, strategy_id, active, reason, triggered_by, triggered_at, cleared_at, cleared_by
    )
    VALUES (
        $1, $2, $3, $4,
        CASE WHEN $3 THEN $5 ELSE NULL END,
        CASE WHEN $3 THEN now() ELSE now() END,
        CASE WHEN NOT $3 THEN now() ELSE NULL END,
        CASE WHEN NOT $3 THEN $5 ELSE NULL END
    )
    ON CONFLICT (workspace_id, strategy_id) DO UPDATE SET
        active       = EXCLUDED.active,
        reason       = EXCLUDED.reason,
        triggered_by = CASE WHEN EXCLUDED.active THEN EXCLUDED.triggered_by ELSE risk_kill_switches.triggered_by END,
        triggered_at = CASE WHEN EXCLUDED.active THEN now() ELSE risk_kill_switches.triggered_at END,
        cleared_at   = CASE WHEN NOT EXCLUDED.active THEN now() ELSE NULL END,
        cleared_by   = CASE WHEN NOT EXCLUDED.active THEN $5 ELSE NULL END
    RETURNING *
)
SELECT * FROM upsert;
