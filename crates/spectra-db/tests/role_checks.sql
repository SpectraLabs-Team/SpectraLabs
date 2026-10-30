-- Role and privilege checks for service role separation (M20 hardening):
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f crates/spectra-db/tests/role_checks.sql
-- Everything runs in one transaction that is rolled back.

BEGIN;

-- 1. Create a temporary test application role
CREATE ROLE test_spectra_app WITH LOGIN;

-- 2. Grant service permissions
GRANT USAGE ON SCHEMA public TO test_spectra_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO test_spectra_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO test_spectra_app;

-- 3. Enforce read-only restriction on user/workspace tables
REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON users, workspaces, workspace_members, schema_migrations, _sqlx_migrations FROM test_spectra_app;
GRANT SELECT ON users, workspaces, workspace_members, schema_migrations, _sqlx_migrations TO test_spectra_app;

-- 4. Verify privilege boundaries
DO $$
BEGIN
    -- test_spectra_app CAN read workspaces
    IF NOT has_table_privilege('test_spectra_app', 'workspaces', 'SELECT') THEN
        RAISE EXCEPTION 'test_spectra_app cannot SELECT from workspaces';
    END IF;

    -- test_spectra_app CAN write to jobs and datasets
    IF NOT has_table_privilege('test_spectra_app', 'jobs', 'INSERT') THEN
        RAISE EXCEPTION 'test_spectra_app cannot INSERT into jobs';
    END IF;
    IF NOT has_table_privilege('test_spectra_app', 'datasets', 'INSERT') THEN
        RAISE EXCEPTION 'test_spectra_app cannot INSERT into datasets';
    END IF;

    -- test_spectra_app CANNOT write to users or workspaces
    IF has_table_privilege('test_spectra_app', 'users', 'INSERT') THEN
        RAISE EXCEPTION 'test_spectra_app unexpectedly has INSERT on users';
    END IF;
    IF has_table_privilege('test_spectra_app', 'workspaces', 'DELETE') THEN
        RAISE EXCEPTION 'test_spectra_app unexpectedly has DELETE on workspaces';
    END IF;
    IF has_table_privilege('test_spectra_app', 'schema_migrations', 'UPDATE') THEN
        RAISE EXCEPTION 'test_spectra_app unexpectedly has UPDATE on schema_migrations';
    END IF;

    -- 5. Create and test backup role (read-only for pg_dump)
    EXECUTE 'CREATE ROLE test_spectra_backup WITH LOGIN';
    EXECUTE 'GRANT USAGE ON SCHEMA public TO test_spectra_backup';
    EXECUTE 'GRANT SELECT ON ALL TABLES IN SCHEMA public TO test_spectra_backup';
    EXECUTE 'GRANT SELECT ON ALL SEQUENCES IN SCHEMA public TO test_spectra_backup';
    EXECUTE 'REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON ALL TABLES IN SCHEMA public FROM test_spectra_backup';

    -- test_spectra_backup CAN read all tables
    IF NOT has_table_privilege('test_spectra_backup', 'workspaces', 'SELECT') THEN
        RAISE EXCEPTION 'test_spectra_backup cannot SELECT from workspaces';
    END IF;
    IF NOT has_table_privilege('test_spectra_backup', 'users', 'SELECT') THEN
        RAISE EXCEPTION 'test_spectra_backup cannot SELECT from users';
    END IF;
    IF NOT has_table_privilege('test_spectra_backup', 'jobs', 'SELECT') THEN
        RAISE EXCEPTION 'test_spectra_backup cannot SELECT from jobs';
    END IF;

    -- test_spectra_backup CANNOT write anywhere
    IF has_table_privilege('test_spectra_backup', 'jobs', 'INSERT') THEN
        RAISE EXCEPTION 'test_spectra_backup unexpectedly has INSERT on jobs';
    END IF;
    IF has_table_privilege('test_spectra_backup', 'users', 'INSERT') THEN
        RAISE EXCEPTION 'test_spectra_backup unexpectedly has INSERT on users';
    END IF;
    IF has_table_privilege('test_spectra_backup', 'workspaces', 'DELETE') THEN
        RAISE EXCEPTION 'test_spectra_backup unexpectedly has DELETE on workspaces';
    END IF;
END $$;

\echo 'service role separation checks passed'
ROLLBACK;
