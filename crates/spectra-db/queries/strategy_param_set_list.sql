-- params: uuid, uuid
-- List parameter sets for a strategy version.
-- $1 workspace_id, $2 strategy_version_id
SELECT id, workspace_id, strategy_version_id, name, description, parameters, created_at, created_by
  FROM strategy_parameter_sets
 WHERE workspace_id = $1 AND strategy_version_id = $2
 ORDER BY name ASC
