-- params: uuid, uuid
-- Check if a kill switch is active for workspace and optional strategy ($2 may be null).
SELECT *
  FROM risk_kill_switches
 WHERE workspace_id = $1
   AND (strategy_id IS NOT DISTINCT FROM $2);
