-- M1 Data Studio, task 13: market data provider configs and dataset refresh schedules.

-- Market data provider configurations per workspace.
CREATE TABLE data_provider_configs (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    provider     text NOT NULL CHECK (provider IN ('fake', 'alpaca')),
    secret_id    uuid REFERENCES secrets (id) ON DELETE SET NULL,
    settings     jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);
CREATE INDEX data_provider_configs_ws ON data_provider_configs (workspace_id);

-- Dataset scheduled refresh configurations.
CREATE TABLE dataset_refresh_schedules (
    id                 uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id       uuid NOT NULL,
    dataset_id         uuid NOT NULL,
    provider_config_id uuid NOT NULL,
    symbols            text[] NOT NULL CHECK (cardinality(symbols) > 0),
    interval_seconds   bigint NOT NULL DEFAULT 86400 CHECK (interval_seconds >= 60),
    lookback_bars      integer NOT NULL DEFAULT 5 CHECK (lookback_bars >= 0),
    enabled            boolean NOT NULL DEFAULT true,
    last_run_at        timestamptz,
    next_run_at        timestamptz NOT NULL DEFAULT now(),
    created_by         uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (workspace_id, dataset_id) REFERENCES datasets (workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, provider_config_id) REFERENCES data_provider_configs (workspace_id, id) ON DELETE CASCADE,
    UNIQUE (workspace_id, dataset_id),
    UNIQUE (workspace_id, id)
);
CREATE INDEX dataset_refresh_schedules_due ON dataset_refresh_schedules (enabled, next_run_at) WHERE enabled = true;
CREATE INDEX dataset_refresh_schedules_ws ON dataset_refresh_schedules (workspace_id);
