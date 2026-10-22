-- params: uuid, uuid, text
SELECT id, workspace_id, kind, ciphertext, hint
FROM secrets
WHERE workspace_id = $1
  AND (($2::uuid IS NOT NULL AND id = $2) OR ($2::uuid IS NULL AND ($3::text IS NULL OR kind = $3)))
LIMIT 1;
