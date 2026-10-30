-- Behaviour of the M4a holdout guard: the database never hands out a sealed holdout's checksum
-- except through a logged, budgeted evaluation grant.
--   { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/holdout_checks.sql; } \
--     | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
-- Runs in a transaction that is rolled back.

BEGIN;
SET LOCAL client_min_messages = warning;

INSERT INTO users (id, email, password_hash) VALUES ('00000000-0000-7000-8000-000000000a11', 'hold@example.com', '!');
INSERT INTO workspaces (id, slug, name) VALUES
    ('00000000-0000-7000-8000-0000000000a1', 'holda', 'A'),
    ('00000000-0000-7000-8000-0000000000b1', 'holdb', 'B');

CREATE FUNCTION pg_temp.check(ok boolean, what text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN IF ok IS NOT TRUE THEN RAISE EXCEPTION 'holdout check failed: %', what; END IF; END $$;

\set ws '00000000-0000-7000-8000-0000000000a1'
\set other '00000000-0000-7000-8000-0000000000b1'
\set usr '00000000-0000-7000-8000-000000000a11'
\set sum1 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
\set sum2 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'

CREATE TEMP TABLE research AS EXECUTE dataset_version_register(:'ws', 'research', :'sum1', 100, 1000, '{}', NULL, NULL, NULL);
CREATE TEMP TABLE hold AS EXECUTE dataset_version_register(:'ws', 'holdout', :'sum2', 50, 500, '{}', NULL, NULL, NULL);
SELECT id AS research_id FROM research \gset
SELECT id AS hold_id FROM hold \gset

-- Before sealing, both resolve with their checksum.
CREATE TEMP TABLE r0 AS EXECUTE dataset_version_resolve(:'ws', :'hold_id', 'test', NULL);
SELECT pg_temp.check((SELECT checksum FROM r0) = :'sum2' AND (SELECT sealed_holdout FROM r0) IS NULL, 'unsealed resolves');

INSERT INTO holdout_vault (id, workspace_id, name, dataset_version_id, budget)
VALUES ('00000000-0000-7000-8000-00000000f001', :'ws', 'final-test', :'hold_id', 2);
\set h '00000000-0000-7000-8000-00000000f001'

-- Sealed: no checksum, the holdout is named, and the attempt is logged.
CREATE TEMP TABLE r1 AS EXECUTE dataset_version_resolve(:'ws', :'hold_id', 'bars', :'usr');
SELECT pg_temp.check((SELECT count(*) FROM r1) = 1, 'sealed version still resolves as a row');
SELECT pg_temp.check((SELECT checksum FROM r1) IS NULL AND (SELECT blob_sha256 FROM r1) IS NULL, 'no checksum for sealed');
SELECT pg_temp.check((SELECT sealed_holdout FROM r1) = 'final-test', 'names the holdout');
SELECT pg_temp.check(
    (SELECT count(*) FROM holdout_access_log WHERE holdout_id = :'h' AND action = 'denied' AND purpose = 'bars'
        AND user_id = :'usr') = 1,
    'denied read is logged');

-- Unsealed data is unaffected and logs nothing.
CREATE TEMP TABLE r2 AS EXECUTE dataset_version_resolve(:'ws', :'research_id', 'job', NULL);
SELECT pg_temp.check((SELECT checksum FROM r2) = :'sum1', 'research data resolves');
SELECT pg_temp.check((SELECT count(*) FROM holdout_access_log WHERE holdout_id = :'h') = 1, 'nothing logged for research data');

-- Another workspace cannot see it at all.
CREATE TEMP TABLE r3 AS EXECUTE dataset_version_resolve(:'other', :'hold_id', 'job', NULL);
SELECT pg_temp.check((SELECT count(*) FROM r3) = 0, 'other workspace');

-- A grant spends budget, logs first, and returns the checksum.
CREATE TEMP TABLE g1 AS EXECUTE holdout_grant(:'ws', :'h', 'evaluate', :'usr');
SELECT pg_temp.check((SELECT evaluation_num FROM g1) = 1 AND (SELECT budget FROM g1) = 2, 'first grant');
SELECT pg_temp.check((SELECT checksum FROM g1) = :'sum2' AND (SELECT dataset_version_id FROM g1) = :'hold_id', 'grant returns the data');
SELECT pg_temp.check((SELECT evaluations_used FROM holdout_vault WHERE id = :'h') = 1, 'budget spent');
SELECT pg_temp.check(
    (SELECT count(*) FROM holdout_access_log WHERE holdout_id = :'h' AND action = 'granted' AND evaluation_num = 1) = 1,
    'grant logged');

CREATE TEMP TABLE g2 AS EXECUTE holdout_grant(:'ws', :'h', 'evaluate', :'usr');
SELECT pg_temp.check((SELECT evaluation_num FROM g2) = 2, 'second grant');

-- Spent: no data, and the refusal is logged.
CREATE TEMP TABLE g3 AS EXECUTE holdout_grant(:'ws', :'h', 'evaluate', :'usr');
SELECT pg_temp.check((SELECT count(*) FROM g3) = 0, 'exhausted grant returns nothing');
SELECT pg_temp.check((SELECT evaluations_used FROM holdout_vault WHERE id = :'h') = 2, 'budget not overspent');
SELECT pg_temp.check(
    (SELECT count(*) FROM holdout_access_log WHERE holdout_id = :'h' AND action = 'exhausted') = 1,
    'exhausted logged');

-- Another workspace cannot be granted it.
CREATE TEMP TABLE g4 AS EXECUTE holdout_grant(:'other', :'h', 'evaluate', NULL);
SELECT pg_temp.check((SELECT count(*) FROM g4) = 0, 'no grant across workspaces');

-- The access log, newest first.
CREATE TEMP TABLE log1 AS EXECUTE holdout_access_log(:'ws', :'h', 10);
SELECT pg_temp.check((SELECT count(*) FROM log1) = 4, 'log lists denied, 2 granted, exhausted');

-- Budget cannot be refunded, a holdout cannot be moved, and the log cannot be rewritten.
DO $$
BEGIN
    BEGIN
        UPDATE holdout_vault SET evaluations_used = 0 WHERE id = '00000000-0000-7000-8000-00000000f001';
        RAISE EXCEPTION 'refund allowed';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        UPDATE holdout_vault SET dataset_version_id = (SELECT id FROM research)
         WHERE id = '00000000-0000-7000-8000-00000000f001';
        RAISE EXCEPTION 'move allowed';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        UPDATE holdout_access_log SET action = 'granted' WHERE action = 'denied';
        RAISE EXCEPTION 'log rewrite allowed';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;

ROLLBACK;
