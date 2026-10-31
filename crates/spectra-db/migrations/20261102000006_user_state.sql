-- M25.1: per-user state moved from browser local storage to the server.
-- Keyed by (user_id, workspace_id). The user comes from the session in the
-- route layer, never from the request body. Decision 1 (per user), Decision 8
-- (paper_orders stays the only order book; sandbox orders get their own table).

-- Portfolio positions (was spectra-portfolio-<email>).
CREATE TABLE IF NOT EXISTS user_portfolio (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    csv          text NOT NULL DEFAULT '',
    source       text,
    version      integer NOT NULL DEFAULT 1 CHECK (version >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- Paper-account state (was spectra-paper-<email>).
CREATE TABLE IF NOT EXISTS user_paper_account (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    state        jsonb NOT NULL DEFAULT '{}'::jsonb,
    version      integer NOT NULL DEFAULT 1 CHECK (version >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- Risk parameters (was spectra-risk-params-<email>).
CREATE TABLE IF NOT EXISTS user_risk_params (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    params       jsonb NOT NULL DEFAULT '{}'::jsonb,
    version      integer NOT NULL DEFAULT 1 CHECK (version >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- Paper sandbox balance and positions (was spectra-paper-sandbox-v1, without orders).
CREATE TABLE IF NOT EXISTS user_paper_sandbox (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    cash         double precision NOT NULL DEFAULT 0.0,
    positions    jsonb NOT NULL DEFAULT '[]'::jsonb,
    version      integer NOT NULL DEFAULT 1 CHECK (version >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- Sandbox orders (was the orders array in spectra-paper-sandbox-v1).
-- Sandbox-scoped on purpose: paper_orders remains the only real order book.
CREATE TABLE IF NOT EXISTS user_sandbox_orders (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    payload      jsonb NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS user_sandbox_orders_user_ws
    ON user_sandbox_orders (user_id, workspace_id, created_at DESC);

-- Paper watchlist (was spectra-paper-watchlist).
CREATE TABLE IF NOT EXISTS user_paper_watchlist (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    symbols      jsonb NOT NULL DEFAULT '[]'::jsonb,
    version      integer NOT NULL DEFAULT 1 CHECK (version >= 0),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- One-time import tracking (Decision 3). A repeat import with the same key is a no-op.
CREATE TABLE IF NOT EXISTS user_state_imports (
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    import_key   text NOT NULL CHECK (length(btrim(import_key)) > 0),
    imported_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id, import_key)
);
