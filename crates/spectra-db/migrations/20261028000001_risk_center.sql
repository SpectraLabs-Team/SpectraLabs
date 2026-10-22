-- M8: Risk Center and Enforcement
-- 1. Risk limits per workspace and optional strategy
CREATE TABLE risk_limits (
    id                     uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id           uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id            uuid REFERENCES strategies (id) ON DELETE CASCADE,
    max_gross_exposure     double precision NOT NULL DEFAULT 1000000.0 CHECK (max_gross_exposure >= 0),
    max_net_exposure       double precision NOT NULL DEFAULT 500000.0 CHECK (max_net_exposure >= 0),
    max_position_size      double precision NOT NULL DEFAULT 100000.0 CHECK (max_position_size >= 0),
    max_concentration_pct  double precision NOT NULL DEFAULT 0.25 CHECK (max_concentration_pct >= 0 AND max_concentration_pct <= 1.0),
    max_leverage           double precision NOT NULL DEFAULT 3.0 CHECK (max_leverage >= 0),
    max_drawdown_pct       double precision NOT NULL DEFAULT 0.15 CHECK (max_drawdown_pct >= 0 AND max_drawdown_pct <= 1.0),
    max_daily_loss         double precision NOT NULL DEFAULT 25000.0 CHECK (max_daily_loss >= 0),
    max_var_pct            double precision NOT NULL DEFAULT 0.05 CHECK (max_var_pct >= 0 AND max_var_pct <= 1.0),
    size_reduction_enabled boolean NOT NULL DEFAULT true,
    updated_by             uuid REFERENCES users (id) ON DELETE SET NULL,
    updated_at             timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, strategy_id)
);

CREATE INDEX idx_risk_limits_workspace ON risk_limits (workspace_id);

-- 2. Kill switches (workspace-wide or per-strategy)
CREATE TABLE risk_kill_switches (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id  uuid REFERENCES strategies (id) ON DELETE CASCADE,
    active       boolean NOT NULL DEFAULT true,
    reason       text NOT NULL DEFAULT '',
    triggered_by uuid REFERENCES users (id) ON DELETE SET NULL,
    triggered_at timestamptz NOT NULL DEFAULT now(),
    cleared_at   timestamptz,
    cleared_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (workspace_id, strategy_id)
);

CREATE INDEX idx_risk_kill_switches_workspace ON risk_kill_switches (workspace_id);

-- 3. Pre-trade risk decisions & audit trail
CREATE TABLE risk_decisions (
    id            uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id   uuid REFERENCES strategies (id) ON DELETE SET NULL,
    symbol        text NOT NULL,
    side          text NOT NULL CHECK (side IN ('buy', 'sell')),
    requested_qty double precision NOT NULL CHECK (requested_qty > 0),
    price         double precision NOT NULL CHECK (price > 0),
    approved_qty  double precision NOT NULL DEFAULT 0 CHECK (approved_qty >= 0),
    verdict       text NOT NULL CHECK (verdict IN ('approved', 'rejected', 'reduced')),
    breaches      jsonb NOT NULL DEFAULT '[]'::jsonb,
    reasons       text[] NOT NULL DEFAULT '{}'::text[],
    metrics       jsonb NOT NULL DEFAULT '{}'::jsonb,
    actor_id      uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_risk_decisions_workspace ON risk_decisions (workspace_id, created_at DESC);
CREATE INDEX idx_risk_decisions_strategy ON risk_decisions (workspace_id, strategy_id, created_at DESC);
