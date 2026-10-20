-- params: uuid, uuid, text
-- The newest artifact of a kind for a run, if the run belongs to the workspace.
-- $1 workspace, $2 run id, $3 artifact kind.
SELECT sha256::text AS sha256, content_type, size_bytes
  FROM run_artifacts
 WHERE workspace_id = $1 AND run_id = $2 AND kind = $3
 ORDER BY created_at DESC, id DESC
 LIMIT 1
