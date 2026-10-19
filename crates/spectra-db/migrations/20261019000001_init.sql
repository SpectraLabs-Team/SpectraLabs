-- Spectra Labs: first schema (M20 step 3).
--
-- Conventions
--   * ids are UUIDv7 (time-ordered). The application generates them; the column defaults use
--     spectra_uuidv7() so rows inserted by hand or by scripts get one too.
--   * timestamps are timestamptz, always written and read as UTC.
--   * every table holding user data carries workspace_id, and every query filters by it.
--   * hashes are lower-case hex SHA-256 (64 characters).

-- 16 random bytes without needing the pgcrypto extension.
CREATE FUNCTION gen_random_bytes_compat() RETURNS bytea
LANGUAGE sql VOLATILE PARALLEL SAFE AS $$
    SELECT decode(replace(gen_random_uuid()::text, '-', ''), 'hex')
$$;

-- UUIDv7 (RFC 9562): 48-bit Unix milliseconds, version 7, variant 10, random bits.
-- Postgres 18 ships uuidv7(); this keeps 16/17 working.
CREATE FUNCTION spectra_uuidv7() RETURNS uuid
LANGUAGE plpgsql VOLATILE PARALLEL SAFE AS $$
DECLARE
    b bytea := gen_random_bytes_compat();
    ms bigint := floor(extract(epoch FROM clock_timestamp()) * 1000);
BEGIN
    b := overlay(b PLACING substring(int8send(ms) FROM 3) FROM 1 FOR 6);
    b := set_byte(b, 6, (get_byte(b, 6) & 15) | 112);  -- version 7
    b := set_byte(b, 8, (get_byte(b, 8) & 63) | 128);  -- variant 10
    RETURN encode(b, 'hex')::uuid;
END $$;

CREATE DOMAIN sha256_hex AS text CHECK (VALUE ~ '^[0-9a-f]{64}$');

-- ---------------------------------------------------------------------------
-- Identity
-- ---------------------------------------------------------------------------

CREATE TABLE users (
    id                  uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    email               text NOT NULL UNIQUE CHECK (email = lower(btrim(email)) AND email LIKE '%_@_%'),
    name                text NOT NULL DEFAULT '',
    -- "salt:hash" from the frontend's PBKDF2 (authStore.ts). '!' = no usable password yet.
    password_hash       text NOT NULL,
    role                text NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    two_factor          boolean NOT NULL DEFAULT false,
    created_at          timestamptz NOT NULL DEFAULT now(),
    -- Sessions issued before this are rejected.
    password_changed_at timestamptz
);

-- Pending emailed codes (sign-up, password reset, 2FA login). Only the SHA-256 of a code is kept.
CREATE TABLE auth_codes (
    kind          text NOT NULL CHECK (kind IN ('signup', 'reset', 'login')),
    email         text NOT NULL CHECK (email = lower(btrim(email))),
    code_hash     sha256_hex NOT NULL,
    -- Sign-up only: the account details to create once the code is confirmed.
    password_hash text NOT NULL DEFAULT '',
    name          text NOT NULL DEFAULT '',
    two_factor    boolean,
    attempts      integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    expires_at    timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (kind, email)
);
CREATE INDEX auth_codes_expires_at ON auth_codes (expires_at);

CREATE TABLE workspaces (
    id         uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    slug       text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name       text NOT NULL,
    -- Per-workspace artifact storage quota (step 7); NULL = no limit.
    artifact_quota_bytes bigint CHECK (artifact_quota_bytes IS NULL OR artifact_quota_bytes >= 0),
    created_by uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         text NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user ON workspace_members (user_id);

-- ---------------------------------------------------------------------------
-- Research objects
-- ---------------------------------------------------------------------------

CREATE TABLE datasets (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    description  text NOT NULL DEFAULT '',
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

CREATE TABLE dataset_versions (
    id             uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id   uuid NOT NULL,
    dataset_id     uuid NOT NULL,
    version        integer NOT NULL CHECK (version >= 1),
    -- Content address in the checksummed store (spectra-data DataStore id).
    checksum       sha256_hex NOT NULL,
    row_count      bigint NOT NULL CHECK (row_count >= 0),
    byte_size      bigint NOT NULL CHECK (byte_size >= 0),
    quality_report jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_by     uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (dataset_id, version),
    UNIQUE (dataset_id, checksum),
    UNIQUE (workspace_id, id),
    -- The version must belong to a dataset in the same workspace.
    FOREIGN KEY (workspace_id, dataset_id) REFERENCES datasets (workspace_id, id) ON DELETE CASCADE
);

CREATE TABLE strategies (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    description  text NOT NULL DEFAULT '',
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

CREATE TABLE strategy_versions (
    id               uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id     uuid NOT NULL,
    strategy_id      uuid NOT NULL,
    version          integer NOT NULL CHECK (version >= 1),
    source_hash      sha256_hex NOT NULL,
    config_hash      sha256_hex NOT NULL,
    parameter_schema jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (strategy_id, version),
    UNIQUE (strategy_id, source_hash, config_hash),
    UNIQUE (workspace_id, id),
    FOREIGN KEY (workspace_id, strategy_id) REFERENCES strategies (workspace_id, id) ON DELETE CASCADE
);

CREATE TABLE experiments (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    hypothesis   text NOT NULL DEFAULT '',
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

-- ---------------------------------------------------------------------------
-- Job queue (claimed with FOR UPDATE SKIP LOCKED; see step 6)
-- ---------------------------------------------------------------------------

CREATE TABLE jobs (
    id               uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id     uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    kind             text NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_.]*$'),
    payload          jsonb NOT NULL DEFAULT '{}'::jsonb,
    status           text NOT NULL DEFAULT 'queued'
                     CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    priority         integer NOT NULL DEFAULT 0,
    attempts         integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts     integer NOT NULL DEFAULT 3 CHECK (max_attempts >= 1),
    progress         real NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
    cancel_requested boolean NOT NULL DEFAULT false,
    run_after        timestamptz NOT NULL DEFAULT now(),
    heartbeat_at     timestamptz,
    started_at       timestamptz,
    finished_at      timestamptz,
    worker_id        text,
    error            text,
    result           jsonb,
    created_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, id),
    CHECK ((status IN ('succeeded', 'failed', 'cancelled')) = (finished_at IS NOT NULL))
);
-- The claim query: queued jobs that are due, highest priority first, oldest first.
CREATE INDEX jobs_claim ON jobs (priority DESC, run_after, id) WHERE status = 'queued';
-- Stale-heartbeat sweep.
CREATE INDEX jobs_running ON jobs (heartbeat_at) WHERE status = 'running';
CREATE INDEX jobs_workspace_status ON jobs (workspace_id, status, id DESC);

CREATE TABLE job_events (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id       uuid NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    kind         text NOT NULL,  -- queued, claimed, progress, retried, succeeded, failed, cancelled, ...
    message      text NOT NULL DEFAULT '',
    data         jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX job_events_job ON job_events (job_id, id);

-- ---------------------------------------------------------------------------
-- Runs and their artifacts
-- ---------------------------------------------------------------------------

CREATE TABLE runs (
    id                  uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id        uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    experiment_id       uuid,
    strategy_version_id uuid,
    dataset_version_id  uuid,
    job_id              uuid REFERENCES jobs (id) ON DELETE SET NULL,
    kind                text NOT NULL DEFAULT 'backtest',
    status              text NOT NULL DEFAULT 'queued'
                        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    params              jsonb NOT NULL DEFAULT '{}'::jsonb,
    metrics             jsonb NOT NULL DEFAULT '{}'::jsonb,
    seed                bigint,
    engine_version      text NOT NULL DEFAULT '',
    error               text,
    created_by          uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    started_at          timestamptz,
    finished_at         timestamptz,
    UNIQUE (workspace_id, id),
    FOREIGN KEY (workspace_id, experiment_id) REFERENCES experiments (workspace_id, id),
    FOREIGN KEY (workspace_id, strategy_version_id) REFERENCES strategy_versions (workspace_id, id),
    FOREIGN KEY (workspace_id, dataset_version_id) REFERENCES dataset_versions (workspace_id, id)
);
CREATE INDEX runs_workspace_created ON runs (workspace_id, id DESC);
CREATE INDEX runs_experiment ON runs (experiment_id) WHERE experiment_id IS NOT NULL;

CREATE TABLE run_artifacts (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL,
    run_id       uuid NOT NULL,
    kind         text NOT NULL,  -- equity_curve, trades, report, ...
    sha256       sha256_hex NOT NULL,
    -- Path relative to ARTIFACT_DIR (content-addressed: derived from sha256).
    path         text NOT NULL,
    size_bytes   bigint NOT NULL CHECK (size_bytes >= 0),
    content_type text NOT NULL DEFAULT 'application/octet-stream',
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (run_id, kind, sha256),
    FOREIGN KEY (workspace_id, run_id) REFERENCES runs (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX run_artifacts_workspace_sha ON run_artifacts (workspace_id, sha256);

-- ---------------------------------------------------------------------------
-- Hash-chained registry log, one chain per workspace (same format as spectra-registry):
-- hash = SHA-256(prev_hash || '\n' || dump({"seq","type","ts_ns","payload"})).
-- payload is `json`, not `jsonb`: jsonb reorders keys, which would change the dump and
-- break the chain. Rows can be appended but never changed or removed.
-- ---------------------------------------------------------------------------

CREATE TABLE registry_events (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE RESTRICT,
    seq          bigint NOT NULL CHECK (seq >= 0),
    type         text NOT NULL,
    ts_ns        bigint NOT NULL,
    payload      json NOT NULL,
    prev_hash    sha256_hex NOT NULL,
    hash         sha256_hex NOT NULL UNIQUE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, seq)
);

CREATE FUNCTION registry_events_check_append() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    last registry_events%ROWTYPE;
BEGIN
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

CREATE FUNCTION registry_events_forbid_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'registry_events is append-only';
END $$;

CREATE TRIGGER registry_events_append BEFORE INSERT ON registry_events
    FOR EACH ROW EXECUTE FUNCTION registry_events_check_append();
CREATE TRIGGER registry_events_no_update BEFORE UPDATE OR DELETE ON registry_events
    FOR EACH ROW EXECUTE FUNCTION registry_events_forbid_change();
CREATE TRIGGER registry_events_no_truncate BEFORE TRUNCATE ON registry_events
    FOR EACH STATEMENT EXECUTE FUNCTION registry_events_forbid_change();

-- ---------------------------------------------------------------------------
-- Audit log
-- ---------------------------------------------------------------------------

CREATE TABLE audit_events (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces (id) ON DELETE SET NULL,
    actor_id     uuid REFERENCES users (id) ON DELETE SET NULL,
    action       text NOT NULL,  -- e.g. "job.created", "user.signed_in"
    target_type  text NOT NULL DEFAULT '',
    target_id    text NOT NULL DEFAULT '',
    request_id   text NOT NULL DEFAULT '',
    details      jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_workspace ON audit_events (workspace_id, id DESC);
CREATE INDEX audit_events_actor ON audit_events (actor_id, id DESC);
