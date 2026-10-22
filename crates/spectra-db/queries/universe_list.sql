-- params: uuid
-- Lists universes in a workspace with their member count. $1 workspace.
SELECT u.id, u.name, u.description, u.created_at,
       count(m.id)::bigint AS member_count
  FROM universes u
  LEFT JOIN universe_members m ON m.workspace_id = u.workspace_id AND m.universe_id = u.id
 WHERE u.workspace_id = $1
 GROUP BY u.id, u.name, u.description, u.created_at
 ORDER BY u.name ASC;
