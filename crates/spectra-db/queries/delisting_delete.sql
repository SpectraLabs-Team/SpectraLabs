-- params: uuid, uuid
-- Deletes an instrument delisting by id. $1 workspace, $2 id.
DELETE FROM instrument_delistings
 WHERE workspace_id = $1 AND id = $2
RETURNING id;
