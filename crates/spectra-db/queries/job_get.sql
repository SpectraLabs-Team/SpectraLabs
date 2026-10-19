-- params: uuid, uuid
-- One job of a workspace. $1 workspace, $2 job id.
SELECT * FROM jobs WHERE workspace_id = $1 AND id = $2
