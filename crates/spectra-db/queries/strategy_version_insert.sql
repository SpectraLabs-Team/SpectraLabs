-- params: uuid, uuid, text, text, text, jsonb, jsonb, uuid
-- Add the next version of a strategy (run after strategy_lock, in the same transaction).
-- $1 workspace, $2 strategy id, $3 built-in name, $4 source hash, $5 config hash,
-- $6 parameter schema, $7 parameters, $8 created by (nullable).
WITH v AS (
INSERT INTO strategy_versions (workspace_id, strategy_id, version, builtin, source_hash, config_hash,
                               parameter_schema, parameters, created_by)
SELECT $1, $2, coalesce((SELECT max(version) FROM strategy_versions WHERE strategy_id = $2), 0) + 1,
       $3, $4, $5, $6, $7, $8
RETURNING id, strategy_id, version, builtin, source_hash, config_hash, parameter_schema, parameters,
          created_by, created_at
)
SELECT v.id, v.strategy_id, s.name AS strategy_name, v.version, v.builtin, v.source_hash, v.config_hash,
       v.parameter_schema, v.parameters, v.created_by, v.created_at
  FROM v JOIN strategies s ON s.id = v.strategy_id
