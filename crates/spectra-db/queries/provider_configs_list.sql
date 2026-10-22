-- params: uuid
-- $1 workspace_id
SELECT id, workspace_id, name, provider, secret_id, settings, created_by, created_at, updated_at
FROM data_provider_configs
WHERE workspace_id = $1
ORDER BY name ASC;
