-- Checks the SQL the dashboard's Postgres auth backend runs (web/frontend/lib/server/auth/sql.ts).
-- Run after the migrations, with the statements prepared first:
--   node scripts/print-auth-sql.mjs > /tmp/prep.sql   (from web/frontend)
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f /tmp/prep.sql -f crates/spectra-db/tests/auth_sql_checks.sql
-- psql keeps prepared statements per session, so both files must run in one psql call, as above.
-- Runs in a transaction that is rolled back.

BEGIN;

-- Sign-up creates the user, a personal workspace and the owner membership; emails are normalized.
EXECUTE insert_user('  New.User@Example.com ', 'Ada', 'salt:hash', 'user', true);
DO $$ BEGIN
    IF (SELECT count(*) FROM users WHERE email = 'new.user@example.com') <> 1 THEN RAISE EXCEPTION 'user not created'; END IF;
    IF (SELECT w.name FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id
          JOIN users u ON u.id = m.user_id WHERE u.email = 'new.user@example.com' AND m.role = 'owner') <> 'Ada''s workspace' THEN
        RAISE EXCEPTION 'personal workspace missing or misnamed';
    END IF;
END $$;
-- A second sign-up with the same email creates nothing.
EXECUTE insert_user('new.user@example.com', 'X', 'h', 'user', false);
DO $$ BEGIN
    IF (SELECT name FROM users WHERE email = 'new.user@example.com') <> 'Ada' THEN RAISE EXCEPTION 'duplicate sign-up overwrote the user'; END IF;
    IF (SELECT count(*) FROM workspaces WHERE created_by = (SELECT id FROM users WHERE email = 'new.user@example.com')) <> 1 THEN
        RAISE EXCEPTION 'duplicate sign-up created a workspace';
    END IF;
END $$;

-- findUser returns the default workspace and millisecond timestamps.
CREATE TEMP TABLE found AS EXECUTE find_user('NEW.USER@example.com');
DO $$ BEGIN
    IF (SELECT count(*) FROM found WHERE workspace_id IS NOT NULL AND created_at_ms > 1.7e12 AND password_changed_at_ms IS NULL) <> 1 THEN
        RAISE EXCEPTION 'find_user shape';
    END IF;
END $$;

-- Updates: only the given fields change.
EXECUTE update_user('new.user@example.com', NULL, NULL, false);
EXECUTE update_user('new.user@example.com', 'salt:new', 1760000000000, NULL);
DO $$ BEGIN
    IF (SELECT row(password_hash, two_factor, (extract(epoch FROM password_changed_at) * 1000)::bigint)::text FROM users
         WHERE email = 'new.user@example.com') <> '(salt:new,f,1760000000000)' THEN
        RAISE EXCEPTION 'update_user changed the wrong fields';
    END IF;
END $$;

-- Codes: upsert resets attempts; peek; bump; delete; purge.
EXECUTE put_code('signup', 'Code@Example.com', repeat('a', 64), 'salt:h', 'C', true, extract(epoch FROM now() + interval '5 min') * 1000);
EXECUTE bump_attempts('signup', 'code@example.com');
EXECUTE put_code('signup', 'code@example.com', repeat('b', 64), 'salt:h', 'C', true, extract(epoch FROM now() + interval '5 min') * 1000);
CREATE TEMP TABLE peeked AS EXECUTE peek_code('signup', 'code@example.com');
DO $$ BEGIN
    IF (SELECT row(code_hash = repeat('b', 64), attempts, expired)::text FROM peeked) <> '(t,0,f)' THEN
        RAISE EXCEPTION 'put_code did not replace the code and reset attempts';
    END IF;
END $$;
EXECUTE lock_code('signup', 'code@example.com');
EXECUTE put_code('reset', 'old@example.com', repeat('c', 64), '', '', NULL, extract(epoch FROM now() - interval '1 min') * 1000);
EXECUTE purge_expired_codes;
EXECUTE delete_code('signup', 'code@example.com');
DO $$ BEGIN
    IF (SELECT count(*) FROM auth_codes WHERE email IN ('code@example.com', 'old@example.com')) <> 0 THEN
        RAISE EXCEPTION 'delete/purge left codes behind';
    END IF;
END $$;

CREATE TEMP TABLE listed AS EXECUTE list_users;

\echo 'auth sql checks passed'
ROLLBACK;
