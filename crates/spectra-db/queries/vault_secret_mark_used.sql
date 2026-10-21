-- params: uuid, uuid
UPDATE secrets
SET last_used_at = now()
WHERE workspace_id = $1 AND id = $2;
