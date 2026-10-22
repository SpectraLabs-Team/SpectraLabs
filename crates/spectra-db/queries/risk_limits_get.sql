-- params: uuid, uuid
-- Get risk limits for a workspace and optional strategy ($2 may be null).
SELECT *
  FROM risk_limits
 WHERE workspace_id = $1
   AND (strategy_id IS NOT DISTINCT FROM $2);
