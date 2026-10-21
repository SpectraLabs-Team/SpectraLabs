-- params: uuid, uuid
-- Fetch one corporate action set by id in the workspace. $1 workspace, $2 set id.
SELECT id, name, description, created_at
  FROM corporate_action_sets
 WHERE workspace_id = $1 AND id = $2;
