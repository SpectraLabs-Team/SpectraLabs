-- params: uuid, uuid
-- $1 workspace_id, $2 id
SELECT id, workspace_id, name, provider, secret_id, settings, created_by, created_at, updated_at
FROM data_provider_configs
WHERE workspace_id = $1 AND id = $2;
