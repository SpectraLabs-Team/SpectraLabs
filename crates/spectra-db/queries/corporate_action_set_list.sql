-- params: uuid
-- Lists corporate action sets in a workspace with their action counts. $1 workspace.
SELECT s.id, s.name, s.description, s.created_at,
       count(a.id)::bigint AS action_count
  FROM corporate_action_sets s
  LEFT JOIN corporate_actions a ON a.workspace_id = s.workspace_id AND a.set_id = s.id
 WHERE s.workspace_id = $1
 GROUP BY s.id, s.name, s.description, s.created_at
 ORDER BY s.name ASC;
