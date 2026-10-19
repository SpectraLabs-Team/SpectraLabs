-- params: uuid, text, uuid, integer
-- A page of a workspace's jobs, newest first. $1 workspace, $2 status filter (nullable),
-- $3 cursor: return jobs older than this id (nullable), $4 page size.
SELECT * FROM jobs
 WHERE workspace_id = $1
   AND ($2::text IS NULL OR status = $2::text)
   AND ($3::uuid IS NULL OR id < $3::uuid)
 ORDER BY id DESC
 LIMIT $4
