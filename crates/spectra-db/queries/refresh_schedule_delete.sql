-- params: uuid, uuid
-- $1 workspace_id, $2 id
DELETE FROM dataset_refresh_schedules
WHERE workspace_id = $1 AND id = $2
RETURNING id;
