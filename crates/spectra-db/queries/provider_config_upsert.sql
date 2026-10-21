-- params: uuid, text, text, uuid, jsonb, uuid
-- $1 workspace_id, $2 name, $3 provider, $4 secret_id, $5 settings, $6 created_by
INSERT INTO data_provider_configs (workspace_id, name, provider, secret_id, settings, created_by, updated_at)
VALUES ($1, btrim($2), $3, $4, $5, $6, now())
ON CONFLICT (workspace_id, name) DO UPDATE SET
    provider = EXCLUDED.provider,
    secret_id = EXCLUDED.secret_id,
    settings = EXCLUDED.settings,
    updated_at = now()
RETURNING id, workspace_id, name, provider, secret_id, settings, created_by, created_at, updated_at;
