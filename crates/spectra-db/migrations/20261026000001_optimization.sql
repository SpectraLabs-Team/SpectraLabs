-- Module 4: Optimization, Walk-Forward Validation, Overfitting Statistics, and Holdout Vault

-- 1. Parameter sweeps (parent record for grid/random optimization jobs)
CREATE TABLE IF NOT EXISTS optimization_sweeps (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    strategy_id UUID REFERENCES strategies(id) ON DELETE SET NULL,
    strategy_version_id UUID REFERENCES strategy_versions(id) ON DELETE SET NULL,
    dataset_version_id UUID NOT NULL REFERENCES dataset_versions(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    method TEXT NOT NULL CHECK (method IN ('grid', 'random')),
    objective TEXT NOT NULL DEFAULT 'sharpe',
    param_grid JSONB NOT NULL,
    trial_count INT NOT NULL DEFAULT 0,
    best_trial_num INT,
    best_params JSONB,
    best_metrics JSONB,
    best_objective DOUBLE PRECISION,
    dsr DOUBLE PRECISION,
    psr DOUBLE PRECISION,
    min_trl DOUBLE PRECISION,
    pbo DOUBLE PRECISION,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_opt_sweeps_ws ON optimization_sweeps(workspace_id, created_at DESC);

-- 2. Individual trials belonging to a sweep
CREATE TABLE IF NOT EXISTS optimization_trials (
    id UUID PRIMARY KEY,
    sweep_id UUID NOT NULL REFERENCES optimization_sweeps(id) ON DELETE CASCADE,
    trial_num INT NOT NULL,
    params JSONB NOT NULL,
    metrics JSONB NOT NULL,
    objective_value DOUBLE PRECISION NOT NULL,
    returns_mean DOUBLE PRECISION,
    returns_vol DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT opt_trials_sweep_num UNIQUE (sweep_id, trial_num)
);

CREATE INDEX IF NOT EXISTS idx_opt_trials_sweep ON optimization_trials(sweep_id, objective_value DESC);

-- 3. Walk-Forward validation reports
CREATE TABLE IF NOT EXISTS walk_forward_reports (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    job_id UUID REFERENCES jobs(id) ON DELETE SET NULL,
    strategy_id UUID REFERENCES strategies(id) ON DELETE SET NULL,
    strategy_version_id UUID REFERENCES strategy_versions(id) ON DELETE SET NULL,
    dataset_version_id UUID NOT NULL REFERENCES dataset_versions(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    mode TEXT NOT NULL DEFAULT 'rolling' CHECK (mode IN ('rolling', 'anchored')),
    train_size INT NOT NULL,
    test_size INT NOT NULL,
    step_size INT NOT NULL,
    embargo INT NOT NULL DEFAULT 0,
    objective TEXT NOT NULL DEFAULT 'sharpe',
    param_grid JSONB NOT NULL DEFAULT '{}'::jsonb,
    folds JSONB NOT NULL DEFAULT '[]'::jsonb,
    efficiency_ratio DOUBLE PRECISION,
    in_sample_sharpe DOUBLE PRECISION,
    out_of_sample_sharpe DOUBLE PRECISION,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_wf_reports_ws ON walk_forward_reports(workspace_id, created_at DESC);

-- 4. Holdout Vault: sealed datasets with metered evaluation budgets
CREATE TABLE IF NOT EXISTS holdout_vault (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    dataset_version_id UUID NOT NULL REFERENCES dataset_versions(id) ON DELETE CASCADE,
    budget INT NOT NULL CHECK (budget > 0),
    evaluations_used INT NOT NULL DEFAULT 0 CHECK (evaluations_used <= budget),
    notes TEXT NOT NULL DEFAULT '',
    sealed_by UUID REFERENCES users(id) ON DELETE SET NULL,
    sealed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT holdout_vault_name_unique UNIQUE (workspace_id, name)
);

CREATE INDEX IF NOT EXISTS idx_holdout_vault_ws ON holdout_vault(workspace_id, sealed_at DESC);

-- 5. Holdout evaluations log (audited access)
CREATE TABLE IF NOT EXISTS holdout_evaluations (
    id UUID PRIMARY KEY,
    holdout_id UUID NOT NULL REFERENCES holdout_vault(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    strategy_version_id UUID REFERENCES strategy_versions(id) ON DELETE SET NULL,
    evaluation_num INT NOT NULL,
    metrics JSONB NOT NULL,
    dsr DOUBLE PRECISION,
    warnings JSONB NOT NULL DEFAULT '[]'::jsonb,
    evaluated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    evaluated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT holdout_eval_unique UNIQUE (holdout_id, evaluation_num)
);

CREATE INDEX IF NOT EXISTS idx_holdout_evals ON holdout_evaluations(holdout_id, evaluation_num ASC);
