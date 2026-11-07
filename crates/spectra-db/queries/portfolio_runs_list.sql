-- params: uuid, uuid, uuid, integer
-- A page of a portfolio's runs, newest first. $1 workspace, $2 portfolio,
-- $3 cursor: runs older than this id (nullable), $4 page size.
SELECT * FROM portfolio_runs
 WHERE workspace_id = $1
   AND portfolio_id = $2
   AND ($3::uuid IS NULL OR id < $3::uuid)
 ORDER BY id DESC
 LIMIT $4
