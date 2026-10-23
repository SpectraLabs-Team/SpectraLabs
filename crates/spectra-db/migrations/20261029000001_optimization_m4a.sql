-- Module 4a: Link sweeps to experiments for child run lineage

ALTER TABLE optimization_sweeps
    ADD COLUMN IF NOT EXISTS experiment_id UUID REFERENCES experiments(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_opt_sweeps_experiment ON optimization_sweeps(experiment_id) WHERE experiment_id IS NOT NULL;
