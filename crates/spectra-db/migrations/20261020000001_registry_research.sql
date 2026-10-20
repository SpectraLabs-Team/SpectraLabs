-- M5a: complete the relational projections used by the Postgres experiment registry.
-- The hash-chained registry_events rows remain the source of tamper evidence;
-- these columns and tables are query-friendly projections written in the same transaction.

ALTER TABLE experiments
    ADD COLUMN tags text[] NOT NULL DEFAULT '{}',
    ADD COLUMN parent_id uuid,
    ADD COLUMN archived_at timestamptz,
    ADD COLUMN registry_id text NOT NULL DEFAULT spectra_uuidv7()::text,
    ADD CONSTRAINT experiments_parent_same_workspace_fk
        FOREIGN KEY (workspace_id, parent_id) REFERENCES experiments (workspace_id, id),
    ADD CONSTRAINT experiments_registry_id_nonempty CHECK (length(btrim(registry_id)) > 0),
    ADD CONSTRAINT experiments_workspace_registry_id_unique UNIQUE (workspace_id, registry_id);

CREATE INDEX experiments_workspace_created ON experiments (workspace_id, created_at DESC, id DESC);
CREATE INDEX experiments_tags_gin ON experiments USING gin (tags);
CREATE INDEX experiments_parent ON experiments (workspace_id, parent_id) WHERE parent_id IS NOT NULL;

CREATE TABLE experiment_notes (
    id            uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id  uuid NOT NULL,
    experiment_id uuid NOT NULL,
    author_id     uuid REFERENCES users (id) ON DELETE SET NULL,
    body          text NOT NULL CHECK (length(btrim(body)) > 0),
    created_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (workspace_id, experiment_id) REFERENCES experiments (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX experiment_notes_workspace_experiment_created
    ON experiment_notes (workspace_id, experiment_id, created_at, id);

ALTER TABLE runs
    ADD COLUMN dataset_id text NOT NULL DEFAULT '',
    ADD COLUMN strategy text NOT NULL DEFAULT '',
    ADD COLUMN dataset_bars_sha256 text NOT NULL DEFAULT '',
    ADD COLUMN strategy_sha256 text NOT NULL DEFAULT '',
    ADD COLUMN config jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN config_sha256 text NOT NULL DEFAULT '',
    ADD COLUMN result_sha256 text NOT NULL DEFAULT '',
    ADD COLUMN sharpe_per_period double precision,
    ADD COLUMN n_obs bigint NOT NULL DEFAULT 0 CHECK (n_obs >= 0),
    ADD COLUMN bar_count bigint NOT NULL DEFAULT 0 CHECK (bar_count >= 0),
    ADD COLUMN counts_as_trial boolean NOT NULL DEFAULT true,
    ADD COLUMN registry_id text,
    ADD CONSTRAINT runs_bars_sha256_format CHECK (dataset_bars_sha256 = '' OR dataset_bars_sha256 ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT runs_strategy_sha256_format CHECK (strategy_sha256 = '' OR strategy_sha256 ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT runs_config_sha256_format CHECK (config_sha256 = '' OR config_sha256 ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT runs_result_sha256_format CHECK (result_sha256 = '' OR result_sha256 ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT runs_registry_id_nonempty CHECK (registry_id IS NULL OR length(btrim(registry_id)) > 0),
    ADD CONSTRAINT runs_finished_at_matches_status
        CHECK ((status IN ('succeeded', 'failed', 'cancelled')) = (finished_at IS NOT NULL)),
    ADD CONSTRAINT runs_running_has_started_at CHECK (status <> 'running' OR started_at IS NOT NULL);

CREATE UNIQUE INDEX runs_workspace_registry_id_unique ON runs (workspace_id, registry_id)
    WHERE registry_id IS NOT NULL;
CREATE INDEX runs_workspace_strategy ON runs (workspace_id, strategy, created_at DESC);
CREATE INDEX runs_workspace_dataset ON runs (workspace_id, dataset_id, created_at DESC);
CREATE INDEX runs_workspace_experiment_created ON runs (workspace_id, experiment_id, created_at DESC, id DESC)
    WHERE experiment_id IS NOT NULL;
CREATE INDEX runs_strategy_version ON runs (workspace_id, strategy_version_id, created_at DESC)
    WHERE strategy_version_id IS NOT NULL;
CREATE INDEX runs_dataset_version ON runs (workspace_id, dataset_version_id, created_at DESC)
    WHERE dataset_version_id IS NOT NULL;
CREATE INDEX runs_workspace_kind_status_created ON runs (workspace_id, kind, status, created_at DESC, id DESC);
CREATE INDEX runs_sharpe ON runs (workspace_id, ((metrics->>'sharpe')::double precision))
    WHERE metrics ? 'sharpe';

ALTER TABLE run_artifacts
    ADD COLUMN registry_id text NOT NULL DEFAULT spectra_uuidv7()::text,
    ADD COLUMN summary jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD CONSTRAINT run_artifacts_registry_id_nonempty CHECK (length(btrim(registry_id)) > 0),
    ADD CONSTRAINT run_artifacts_workspace_registry_id_unique UNIQUE (workspace_id, registry_id);

-- A chain is unique within its workspace, not across the database: exporting a
-- workspace and importing it into another (a restore or a copy on the same
-- server) must keep the original hashes.
ALTER TABLE registry_events DROP CONSTRAINT registry_events_hash_key;
ALTER TABLE registry_events ADD CONSTRAINT registry_events_workspace_hash_key UNIQUE (workspace_id, hash);

-- Serialize all appends inside a workspace. The original M20 trigger validated
-- the tail but two simultaneous writers could both observe the same tail.
CREATE OR REPLACE FUNCTION registry_events_check_append() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    last registry_events%ROWTYPE;
BEGIN
    PERFORM 1 FROM workspaces WHERE id = NEW.workspace_id FOR UPDATE;
    SELECT * INTO last FROM registry_events
     WHERE workspace_id = NEW.workspace_id ORDER BY seq DESC LIMIT 1;
    IF NOT FOUND THEN
        IF NEW.seq <> 0 OR NEW.prev_hash <> repeat('0', 64) THEN
            RAISE EXCEPTION 'registry chain must start at seq 0 with the zero hash';
        END IF;
    ELSIF NEW.seq <> last.seq + 1 OR NEW.prev_hash <> last.hash THEN
        RAISE EXCEPTION 'registry chain broken: expected seq % after hash %', last.seq + 1, last.hash;
    END IF;
    RETURN NEW;
END $$;

CREATE TABLE run_notes (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL,
    run_id       uuid NOT NULL,
    author_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    body         text NOT NULL CHECK (length(btrim(body)) > 0),
    created_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (workspace_id, run_id) REFERENCES runs (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX run_notes_workspace_run_created ON run_notes (workspace_id, run_id, created_at, id);

CREATE FUNCTION runs_enforce_lifecycle() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- Registry runs (those under an experiment) go queued -> running -> finished.
        -- Plain job results (benchmark, risk, stress) are recorded once, already
        -- finished, by spectra_db::jobs::insert_run; they become immutable below.
        IF NEW.experiment_id IS NOT NULL AND NEW.status <> 'queued' THEN
            RAISE EXCEPTION 'registry runs must be created in queued status';
        END IF;
        RETURN NEW;
    END IF;

    IF OLD.status IN ('succeeded', 'failed', 'cancelled') THEN
        RAISE EXCEPTION 'finished runs are immutable';
    END IF;

    IF NEW.status <> OLD.status AND NOT (
        (OLD.status = 'queued' AND NEW.status IN ('running', 'failed', 'cancelled')) OR
        (OLD.status = 'running' AND NEW.status IN ('succeeded', 'failed', 'cancelled'))
    ) THEN
        RAISE EXCEPTION 'invalid run status transition: % -> %', OLD.status, NEW.status;
    END IF;

    IF NEW.status <> 'succeeded' AND (
        NEW.dataset_bars_sha256 IS DISTINCT FROM OLD.dataset_bars_sha256 OR
        NEW.strategy_sha256 IS DISTINCT FROM OLD.strategy_sha256 OR
        NEW.config IS DISTINCT FROM OLD.config OR
        NEW.config_sha256 IS DISTINCT FROM OLD.config_sha256 OR
        NEW.result_sha256 IS DISTINCT FROM OLD.result_sha256 OR
        NEW.sharpe_per_period IS DISTINCT FROM OLD.sharpe_per_period OR
        NEW.n_obs IS DISTINCT FROM OLD.n_obs OR
        NEW.bar_count IS DISTINCT FROM OLD.bar_count OR
        NEW.metrics IS DISTINCT FROM OLD.metrics OR
        NEW.params IS DISTINCT FROM OLD.params OR
        NEW.seed IS DISTINCT FROM OLD.seed OR
        NEW.engine_version IS DISTINCT FROM OLD.engine_version OR
        NEW.registry_id IS DISTINCT FROM OLD.registry_id
    ) THEN
        RAISE EXCEPTION 'run results may only be written when the run succeeds';
    END IF;

    IF OLD.status = 'running' AND NEW.status = 'succeeded' AND
       (NEW.result_sha256 = '' OR NEW.config_sha256 = '' OR NEW.dataset_bars_sha256 = '') THEN
        RAISE EXCEPTION 'a successful run requires data, config, and result fingerprints';
    END IF;

    RETURN NEW;
END $$;

CREATE TRIGGER runs_lifecycle_guard BEFORE INSERT OR UPDATE ON runs
    FOR EACH ROW EXECUTE FUNCTION runs_enforce_lifecycle();

CREATE FUNCTION run_notes_forbid_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'run notes are append-only';
END $$;
CREATE TRIGGER run_notes_no_update BEFORE UPDATE OR DELETE ON run_notes
    FOR EACH ROW EXECUTE FUNCTION run_notes_forbid_change();
CREATE TRIGGER run_notes_no_truncate BEFORE TRUNCATE ON run_notes
    FOR EACH STATEMENT EXECUTE FUNCTION run_notes_forbid_change();
CREATE TRIGGER experiment_notes_no_update BEFORE UPDATE OR DELETE ON experiment_notes
    FOR EACH ROW EXECUTE FUNCTION run_notes_forbid_change();
CREATE TRIGGER experiment_notes_no_truncate BEFORE TRUNCATE ON experiment_notes
    FOR EACH STATEMENT EXECUTE FUNCTION run_notes_forbid_change();
