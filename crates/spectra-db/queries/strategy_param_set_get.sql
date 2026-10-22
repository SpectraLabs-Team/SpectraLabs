-- params: uuid, uuid
-- Get a parameter set by id.
-- $1 workspace_id, $2 id
SELECT id, workspace_id, strategy_version_id, name, description, parameters, created_at, created_by
  FROM strategy_parameter_sets
 WHERE workspace_id = $1 AND id = $2
