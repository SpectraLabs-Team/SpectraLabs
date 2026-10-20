-- params: uuid, uuid
-- Mirror a signed-in M19 identity into the engine's tables (M1 task 1): the user and the
-- workspace get rows with the same ids as in the frontend database, so `workspace_id` and
-- `created_by` foreign keys hold. Rows are created once and then left alone (an existing row
-- is never updated), so this is safe to run on every request.
--   $1 user id, or NULL when the caller has none (an API token without a user).
--   $2 workspace id.
-- The engine never receives the user's email or the workspace's name, so mirrored rows get
-- placeholders: a reserved .invalid address, no usable password ('!'), and slug ws-<uuid hex>.
WITH mirrored_user AS (
    INSERT INTO users (id, email, password_hash)
    SELECT $1, 'u-' || replace($1::text, '-', '') || '@mirror.invalid', '!'
     WHERE $1 IS NOT NULL
    ON CONFLICT DO NOTHING
    RETURNING id
)
INSERT INTO workspaces (id, slug, name, created_by)
SELECT $2, 'ws-' || replace($2::text, '-', ''), 'Workspace ' || left(replace($2::text, '-', ''), 8),
       -- A user created just now, or one that already existed: either way it is in users.
       (SELECT id FROM mirrored_user UNION ALL SELECT id FROM users WHERE id = $1 LIMIT 1)
ON CONFLICT DO NOTHING
