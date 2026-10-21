-- Development seed: an admin account with its personal workspace, and a "dev" team workspace it
-- owns. Idempotent. `spectra-db seed` runs these statements with $1 = the admin email (ADMIN_EMAIL).
-- The tables are the dashboard's (web/frontend/db/migrations), and the rows match what its sign-up makes.
-- The admin gets no usable password ('!'): set one with "Forgot password" in the app.
-- Admins always use two-step sign-in, as the dashboard enforces.
INSERT INTO users (email, name, password_hash, platform_role, two_factor)
VALUES (lower(btrim($1)), 'Admin', '!', 'admin', true)
ON CONFLICT (email) DO UPDATE SET platform_role = 'admin', two_factor = true;

-- Every account has exactly one personal workspace (lib/server/workspaces.ts).
INSERT INTO workspaces (name, slug, created_by, personal_owner_id)
SELECT 'Personal',
       coalesce(nullif(btrim(left(regexp_replace(split_part(u.email, '@', 1), '[^a-z0-9]+', '-', 'g'), 40), '-'), ''), 'workspace')
         || '-' || substr(replace(gen_random_uuid()::text, '-', ''), 1, 6),
       u.id, u.id
  FROM users u WHERE u.email = lower(btrim($1))
ON CONFLICT (personal_owner_id) DO NOTHING;

INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, u.id, 'owner' FROM workspaces w JOIN users u ON w.personal_owner_id = u.id
 WHERE u.email = lower(btrim($1))
ON CONFLICT (workspace_id, user_id) DO NOTHING;

INSERT INTO workspaces (slug, name, created_by)
SELECT 'dev', 'Development', id FROM users WHERE email = lower(btrim($1))
ON CONFLICT (slug) DO NOTHING;

INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, u.id, 'owner' FROM workspaces w, users u
 WHERE w.slug = 'dev' AND u.email = lower(btrim($1))
ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'owner';
