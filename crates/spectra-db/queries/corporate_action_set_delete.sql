-- params: uuid, uuid
-- Delete a corporate action set from the workspace. $1 workspace, $2 set id.
DELETE FROM corporate_action_sets
 WHERE workspace_id = $1 AND id = $2
RETURNING id;
