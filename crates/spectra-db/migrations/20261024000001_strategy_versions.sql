-- M2a (Strategy Studio, minimal), task 2: saved strategies and their versions.
--
-- `strategies` and `strategy_versions` came with the first schema; nothing used them until now.
-- In M2a a version is a built-in strategy (`builtin`, by name) with a full parameter set
-- (`parameters`, every parameter after defaults, in the strategy's own order):
--   source_hash  SHA-256 of the built-in's source file (the `strategy_sha256` its runs record)
--   config_hash  SHA-256 of the canonical parameter text
-- The existing unique (strategy_id, source_hash, config_hash) makes saving the same code and
-- parameters twice return the same version. A version never changes once written.

ALTER TABLE strategy_versions
    ADD COLUMN builtin text NOT NULL CHECK (builtin ~ '^[a-z][a-z0-9_]*$'),
    ADD COLUMN parameters jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(parameters) = 'object');

-- The version list of a strategy, newest first.
CREATE INDEX strategy_versions_strategy_version ON strategy_versions (strategy_id, version DESC);

CREATE FUNCTION strategy_versions_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'strategy versions are immutable';
END $$;

CREATE TRIGGER strategy_versions_no_update
    BEFORE UPDATE ON strategy_versions
    FOR EACH ROW EXECUTE FUNCTION strategy_versions_immutable();
