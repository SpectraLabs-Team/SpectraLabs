-- params: uuid, text
SELECT *
  FROM holdout_vault
 WHERE workspace_id = $1 AND name = $2;
