-- Development seed: an admin user and a "dev" workspace they own. Idempotent.
-- `spectra-db seed` runs the same statements with $1 = the admin email (ADMIN_EMAIL).
-- The admin gets no usable password ('!'): set one with "Forgot password" in the app.
INSERT INTO users (email, name, password_hash, role)
VALUES (lower(btrim($1)), 'Admin', '!', 'admin')
ON CONFLICT (email) DO UPDATE SET role = 'admin';

INSERT INTO workspaces (slug, name, created_by)
SELECT 'dev', 'Development', id FROM users WHERE email = lower(btrim($1))
ON CONFLICT (slug) DO NOTHING;

INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, u.id, 'owner' FROM workspaces w, users u
 WHERE w.slug = 'dev' AND u.email = lower(btrim($1))
ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = 'owner';
