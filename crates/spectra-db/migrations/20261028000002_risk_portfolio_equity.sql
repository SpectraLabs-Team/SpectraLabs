-- M8: Server-built portfolio snapshots and equity history tracking for risk enforcement.
-- Tables: portfolio_positions, portfolio_balances, equity_snapshots

CREATE TABLE portfolio_positions (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id  uuid REFERENCES strategies (id) ON DELETE CASCADE,
    symbol       text NOT NULL CHECK (length(btrim(symbol)) > 0),
    qty          double precision NOT NULL,
    price        double precision NOT NULL CHECK (price >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT portfolio_positions_ws_strat_symbol_unique UNIQUE NULLS NOT DISTINCT (workspace_id, strategy_id, symbol)
);

CREATE INDEX portfolio_positions_ws_strat ON portfolio_positions (workspace_id, strategy_id);

CREATE TABLE portfolio_balances (
    id                 uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id       uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id        uuid REFERENCES strategies (id) ON DELETE CASCADE,
    cash               double precision NOT NULL DEFAULT 0.0,
    equity             double precision NOT NULL DEFAULT 0.0,
    peak_equity        double precision NOT NULL DEFAULT 0.0,
    daily_start_equity double precision NOT NULL DEFAULT 0.0,
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT portfolio_balances_ws_strat_unique UNIQUE NULLS NOT DISTINCT (workspace_id, strategy_id)
);

CREATE INDEX portfolio_balances_ws_strat ON portfolio_balances (workspace_id, strategy_id);

CREATE TABLE equity_snapshots (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id  uuid REFERENCES strategies (id) ON DELETE CASCADE,
    run_id       uuid REFERENCES runs (id) ON DELETE CASCADE,
    step         bigint NOT NULL DEFAULT 0,
    ts_ns        bigint NOT NULL,
    equity       double precision NOT NULL,
    cash         double precision NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX equity_snapshots_ws_strat_ts ON equity_snapshots (workspace_id, strategy_id, ts_ns ASC);
