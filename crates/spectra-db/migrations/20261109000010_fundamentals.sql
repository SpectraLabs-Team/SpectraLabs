-- M34: fundamentals loaded from a provider (SEC EDGAR), stored point in time.
--
-- fundamental_facts is append-only: a refresh adds revisions and never changes a stored value.
-- Rows go away only with their workspace. Quarantined rows live in their own table, which readers
-- never query.

CREATE TABLE IF NOT EXISTS fundamental_loads (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL CHECK (symbol <> '' AND symbol = upper(btrim(symbol))),
    source TEXT NOT NULL CHECK (source <> ''),
    -- Content-addressed version of everything the provider returned that passed the checks.
    version_id TEXT NOT NULL CHECK (version_id ~ '^fund_v_[0-9a-f]{12}$'),
    accepted INTEGER NOT NULL CHECK (accepted >= 0),
    new_rows INTEGER NOT NULL CHECK (new_rows >= 0 AND new_rows <= accepted),
    quarantined INTEGER NOT NULL CHECK (quarantined >= 0),
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    loaded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_fundamental_loads_symbol
    ON fundamental_loads(workspace_id, symbol, loaded_at DESC);

CREATE TABLE IF NOT EXISTS fundamental_facts (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    load_id UUID NOT NULL REFERENCES fundamental_loads(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL CHECK (symbol <> '' AND symbol = upper(btrim(symbol))),
    period TEXT NOT NULL CHECK (period <> ''),
    fiscal_year SMALLINT NOT NULL CHECK (fiscal_year BETWEEN 1900 AND 2200),
    fiscal_quarter SMALLINT NOT NULL CHECK (fiscal_quarter BETWEEN 0 AND 4),
    metric TEXT NOT NULL CHECK (metric <> ''),
    -- Finite: in Postgres NaN sorts above Infinity, so this refuses both.
    value DOUBLE PRECISION NOT NULL CHECK (abs(value) < 'Infinity'::float8),
    currency TEXT NOT NULL,
    -- When the value became public; readers only see rows with known_at <= their as-of time.
    known_at TIMESTAMPTZ NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    filing_ref TEXT NOT NULL CHECK (filing_ref <> ''),
    source TEXT NOT NULL CHECK (source <> ''),
    loaded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    -- A filing reports one value per period and metric: reloading it adds nothing.
    CONSTRAINT uq_fundamental_facts_filing UNIQUE (workspace_id, symbol, period, metric, filing_ref)
);

CREATE INDEX IF NOT EXISTS idx_fundamental_facts_as_of
    ON fundamental_facts(workspace_id, symbol, period, metric, known_at DESC, revision DESC);

CREATE OR REPLACE FUNCTION fundamental_facts_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'fundamental_facts is append-only: add a revision instead of changing a value'
        USING ERRCODE = 'insufficient_privilege';
END $$;

DROP TRIGGER IF EXISTS fundamental_facts_no_update ON fundamental_facts;
CREATE TRIGGER fundamental_facts_no_update BEFORE UPDATE ON fundamental_facts
    FOR EACH ROW EXECUTE FUNCTION fundamental_facts_append_only();

CREATE TABLE IF NOT EXISTS fundamental_quarantine (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    load_id UUID NOT NULL REFERENCES fundamental_loads(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL,
    period TEXT NOT NULL,
    metric TEXT NOT NULL,
    -- As the provider sent it: may be NaN or infinite.
    value DOUBLE PRECISION NOT NULL,
    known_at TIMESTAMPTZ NOT NULL,
    filing_ref TEXT NOT NULL,
    category TEXT NOT NULL,
    reason TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT uq_fundamental_quarantine_filing UNIQUE (workspace_id, symbol, period, metric, filing_ref)
);

CREATE INDEX IF NOT EXISTS idx_fundamental_quarantine_symbol
    ON fundamental_quarantine(workspace_id, symbol, recorded_at DESC);

-- One refresh schedule per workspace; the worker enqueues a refresh when next_run_at passes.
CREATE TABLE IF NOT EXISTS fundamental_refresh_schedules (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
    symbols TEXT[] NOT NULL CHECK (cardinality(symbols) BETWEEN 1 AND 100),
    interval_seconds INTEGER NOT NULL CHECK (interval_seconds BETWEEN 3600 AND 2592000),
    next_run_at TIMESTAMPTZ NOT NULL,
    last_enqueued_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_fundamental_refresh_schedules_due
    ON fundamental_refresh_schedules(next_run_at);
