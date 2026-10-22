-- params: uuid, uuid, uuid, text, text, jsonb, uuid
-- Create a named parameter set for a strategy version.
-- $1 id, $2 workspace_id, $3 strategy_version_id, $4 name, $5 description, $6 parameters, $7 created_by
INSERT INTO strategy_parameter_sets (id, workspace_id, strategy_version_id, name, description, parameters, created_by)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING id, workspace_id, strategy_version_id, name, description, parameters, created_at, created_by
