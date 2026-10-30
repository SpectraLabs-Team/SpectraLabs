-- params: uuid, uuid, bigint
-- A holdout's access log (denied reads, granted and refused evaluations), newest first.
-- $1 workspace, $2 holdout id, $3 limit.
SELECT id, holdout_id, action, purpose, evaluation_num, user_id, at
  FROM holdout_access_log
 WHERE workspace_id = $1 AND holdout_id = $2
 ORDER BY at DESC, id DESC
 LIMIT $3
