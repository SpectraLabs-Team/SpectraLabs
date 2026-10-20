-- params: uuid, text, text, uuid, integer
-- A page of a workspace's jobs of one kind, newest first. $1 workspace, $2 kind,
-- $3 status filter (nullable), $4 cursor: jobs older than this id (nullable), $5 page size.
SELECT * FROM jobs
 WHERE workspace_id = $1
   AND kind = $2
   AND ($3::text IS NULL OR status = $3::text)
   AND ($4::uuid IS NULL OR id < $4::uuid)
 ORDER BY id DESC
 LIMIT $5
