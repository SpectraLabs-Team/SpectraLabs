-- params: uuid, uuid
-- One strategy version of a workspace, by id (what a backtest's strategy_version_id names).
-- $1 workspace, $2 version id.
SELECT v.id, v.strategy_id, s.name AS strategy_name, v.version, v.builtin, v.source_hash, v.config_hash,
       v.parameter_schema, v.parameters, v.code, v.created_by, v.created_at
  FROM strategy_versions v JOIN strategies s ON s.id = v.strategy_id
 WHERE v.workspace_id = $1 AND v.id = $2
