-- params: uuid, uuid, text, text, text, text, jsonb
-- Append to the audit log. $1 workspace, $2 actor (nullable), $3 action ("upload.created",
-- "dataset.version_created", ...), $4 target type, $5 target id, $6 request id, $7 details.
WITH ins AS (
    INSERT INTO audit_events (workspace_id, actor_id, action, target_type, target_id, request_id, details)
    VALUES ($1, $2, $3, $4, $5, $6, $7)
    RETURNING id
)
SELECT id FROM ins
