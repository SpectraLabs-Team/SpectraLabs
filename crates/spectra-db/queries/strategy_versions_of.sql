-- params: uuid, uuid
-- Every version of a strategy, newest first. $1 workspace, $2 strategy id.
SELECT v.id, v.strategy_id, s.name AS strategy_name, v.version, v.builtin, v.source_hash, v.config_hash,
       v.parameter_schema, v.parameters, v.created_by, v.created_at
  FROM strategy_versions v JOIN strategies s ON s.id = v.strategy_id
 WHERE v.workspace_id = $1 AND v.strategy_id = $2
 ORDER BY v.version DESC
