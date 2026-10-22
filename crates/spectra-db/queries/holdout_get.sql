-- params: uuid, uuid
SELECT *
  FROM holdout_vault
 WHERE workspace_id = $1 AND id = $2;
