-- params: uuid
SELECT wrapped_key, master_version
FROM workspace_keys
WHERE workspace_id = $1;
