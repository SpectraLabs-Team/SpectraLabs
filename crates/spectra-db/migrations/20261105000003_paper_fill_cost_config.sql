-- M26 Task 9: every paper fill records the cost configuration that priced it.
-- Written on every new fill (manual fills: the named paper-default config; strategy runs: the
-- run's config preset). Fills recorded before this migration keep NULL: their costs were not
-- stored and are not reconstructed.

ALTER TABLE paper_fills ADD COLUMN cost_config jsonb;
