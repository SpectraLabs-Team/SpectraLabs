-- params: uuid
SELECT *
  FROM holdout_vault
 WHERE workspace_id = $1
 ORDER BY sealed_at DESC;
