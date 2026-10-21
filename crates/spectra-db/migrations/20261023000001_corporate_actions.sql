-- M1 Data Studio, task 11: corporate actions, adjustments and derived datasets.

-- Corporate action sets: workspace-scoped bundles of split and dividend events.
CREATE TABLE corporate_action_sets (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    description  text NOT NULL DEFAULT '',
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

-- Individual corporate actions (splits or cash dividends) belonging to a set.
CREATE TABLE corporate_actions (
    id              uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id    uuid NOT NULL,
    set_id          uuid NOT NULL,
    symbol          text NOT NULL CHECK (length(btrim(symbol)) > 0),
    ex_date         timestamptz NOT NULL,
    action_kind     text NOT NULL CHECK (action_kind IN ('split', 'dividend')),
    split_from      integer NOT NULL DEFAULT 1 CHECK (split_from > 0),
    split_to        integer NOT NULL DEFAULT 1 CHECK (split_to > 0),
    dividend_amount numeric(18, 8) NOT NULL DEFAULT 0.0 CHECK (dividend_amount >= 0),
    created_at      timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (workspace_id, set_id) REFERENCES corporate_action_sets (workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX corporate_actions_set_symbol_date ON corporate_actions (set_id, symbol, ex_date);

-- Dataset version lineage: link a derived version to the version it was derived from.
ALTER TABLE dataset_versions ADD COLUMN derived_from_id uuid REFERENCES dataset_versions (id) ON DELETE SET NULL;
ALTER TABLE dataset_versions ADD COLUMN derivation_kind text;
ALTER TABLE dataset_versions ADD COLUMN derivation_params jsonb NOT NULL DEFAULT '{}'::jsonb;
CREATE INDEX dataset_versions_derived_from ON dataset_versions (derived_from_id) WHERE derived_from_id IS NOT NULL;
