-- Module 9: Paper Trading
-- Tables: paper_orders, paper_fills

CREATE TABLE paper_orders (
    id               uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id     uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id      uuid NOT NULL REFERENCES strategies (id) ON DELETE CASCADE,
    symbol           text NOT NULL CHECK (length(btrim(symbol)) > 0),
    side             text NOT NULL CHECK (side IN ('buy', 'sell')),
    order_type       text NOT NULL CHECK (order_type IN ('market', 'limit', 'stop', 'stop_limit')),
    qty              double precision NOT NULL CHECK (qty > 0),
    limit_price      double precision CHECK (limit_price IS NULL OR limit_price > 0),
    stop_price       double precision CHECK (stop_price IS NULL OR stop_price > 0),
    tif              text NOT NULL DEFAULT 'gtc' CHECK (tif IN ('gtc', 'day', 'ioc', 'fok')),
    status           text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'filled', 'partially_filled', 'rejected', 'cancelled')),
    filled_qty       double precision NOT NULL DEFAULT 0.0 CHECK (filled_qty >= 0),
    filled_avg_price double precision CHECK (filled_avg_price IS NULL OR filled_avg_price >= 0),
    reject_reason    text,
    created_by       uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX paper_orders_ws_strat ON paper_orders (workspace_id, strategy_id, created_at DESC);
CREATE INDEX paper_orders_ws_status ON paper_orders (workspace_id, status);

CREATE TABLE paper_fills (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    order_id     uuid NOT NULL REFERENCES paper_orders (id) ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    strategy_id  uuid NOT NULL REFERENCES strategies (id) ON DELETE CASCADE,
    symbol       text NOT NULL,
    side         text NOT NULL CHECK (side IN ('buy', 'sell')),
    qty          double precision NOT NULL CHECK (qty > 0),
    price        double precision NOT NULL CHECK (price >= 0),
    fee          double precision NOT NULL DEFAULT 0.0 CHECK (fee >= 0),
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX paper_fills_ws_order ON paper_fills (workspace_id, order_id);
CREATE INDEX paper_fills_ws_strat ON paper_fills (workspace_id, strategy_id, created_at DESC);
