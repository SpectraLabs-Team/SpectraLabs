-- Reports (M16): every server-rendered report, stored when it is created, versioned and tied to
-- the run it came from.
--
-- One table for every report type. Each row is one immutable version. The versions of a report
-- (the same type for the same run) share a `report_id` and are numbered 1, 2, ... in order;
-- exporting again adds a version and never overwrites one. The bytes are stored inline (reports
-- are small) with their SHA-256, which the table checks against the content.
--
-- `run_kind` says what `run_id` points at: an optimization sweep, a backtest run, or a
-- backtest-to-paper gap report. There is no foreign key because those live in different tables.

CREATE TABLE reports (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    report_id    uuid NOT NULL,
    report_type  text NOT NULL CHECK (report_type IN ('validation', 'tear_sheet', 'gap')),
    run_kind     text NOT NULL CHECK (run_kind IN ('sweep', 'backtest', 'paper_gap')),
    run_id       uuid NOT NULL,
    version      integer NOT NULL CHECK (version >= 1),
    format       text NOT NULL CHECK (format IN ('pdf', 'csv')),
    content_type text NOT NULL CHECK (content_type <> ''),
    sha256       sha256_hex NOT NULL,
    size_bytes   bigint NOT NULL,
    content      bytea NOT NULL,
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    CHECK (size_bytes = octet_length(content)),
    CHECK (size_bytes <= 10485760),
    CHECK (sha256 = encode(sha256(content), 'hex')),
    UNIQUE (report_id, version),
    -- One numbering per (type, run): a second report_id for the same run would need version 1 again.
    UNIQUE (workspace_id, report_type, run_kind, run_id, version)
);

-- The reports of a run, newest first.
CREATE INDEX reports_run ON reports (workspace_id, run_kind, run_id, report_type, version DESC);

CREATE FUNCTION reports_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'report versions are immutable';
END $$;

CREATE TRIGGER reports_no_update
    BEFORE UPDATE ON reports
    FOR EACH ROW EXECUTE FUNCTION reports_immutable();
