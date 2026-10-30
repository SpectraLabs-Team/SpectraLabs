-- M24 decision 2: multi-currency with point-in-time FX conversion.

-- The currency a workspace keeps its books in. A workspace without a row
-- uses USD (spectra_core::DEFAULT_BASE_CURRENCY).
CREATE TABLE workspace_currency (
    workspace_id  uuid PRIMARY KEY REFERENCES workspaces (id) ON DELETE CASCADE,
    base_currency char(3) NOT NULL CHECK (base_currency ~ '^[A-Z]{3}$'),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- FX observations: from as_of on, one unit of `currency` is worth `rate`
-- units of `base_currency`. Twelve decimals keep small rates (JPY in USD)
-- exact. Rows are kept per base currency, so changing a workspace's base
-- never reuses rates quoted against the old one.
CREATE TABLE fx_rates (
    id            uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    currency      char(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    base_currency char(3) NOT NULL CHECK (base_currency ~ '^[A-Z]{3}$'),
    as_of         timestamptz NOT NULL,
    rate          numeric(30, 12) NOT NULL CHECK (rate > 0),
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (currency <> base_currency),
    UNIQUE (workspace_id, base_currency, currency, as_of)
);
CREATE INDEX fx_rates_lookup ON fx_rates (workspace_id, base_currency, currency, as_of);
