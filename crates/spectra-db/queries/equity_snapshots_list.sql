-- params: uuid, uuid, integer
-- List historical equity snapshots for exposure and drawdown charting.
SELECT step, ts_ns, equity, cash, created_at
  FROM equity_snapshots
 WHERE workspace_id = $1
   AND (($2::uuid IS NULL AND strategy_id IS NULL) OR strategy_id = $2::uuid)
 ORDER BY ts_ns ASC
 LIMIT $3;
