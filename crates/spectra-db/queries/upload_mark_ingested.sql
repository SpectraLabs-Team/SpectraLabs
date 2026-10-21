-- params: uuid, uuid
-- An upload became a dataset version: keep it (as the version's source) past its expiry.
-- $1 workspace, $2 upload id.
UPDATE uploads SET ingested_at = COALESCE(ingested_at, now()) WHERE workspace_id = $1 AND id = $2
