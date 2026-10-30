-- M24.1 task 1: one instrument record for every tradable type. Type-specific
-- fields are nullable, and each type's own rules are checked here as well as
-- in spectra-core, so a bad row cannot be written by any client.

CREATE TABLE instruments (
    id             uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id   uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    symbol         text NOT NULL CHECK (length(btrim(symbol)) > 0),
    instrument_type text NOT NULL CHECK (instrument_type IN ('equity', 'future', 'bond', 'option', 'swap')),
    currency       char(3) NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    multiplier     numeric(20, 8) NOT NULL DEFAULT 1 CHECK (multiplier > 0),
    tick_size      numeric(20, 8) NOT NULL CHECK (tick_size > 0),
    lot_size       numeric(20, 8) NOT NULL DEFAULT 1 CHECK (lot_size > 0),
    expiry         timestamptz,
    strike         numeric(20, 8),
    maturity       timestamptz,
    coupon_rate    numeric(12, 8),
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, symbol),
    UNIQUE (workspace_id, id),
    CONSTRAINT instruments_equity_fields CHECK (
        instrument_type <> 'equity'
        OR (expiry IS NULL AND strike IS NULL AND maturity IS NULL AND coupon_rate IS NULL AND multiplier = 1)
    ),
    CONSTRAINT instruments_future_fields CHECK (
        instrument_type <> 'future'
        OR (expiry IS NOT NULL AND strike IS NULL AND maturity IS NULL AND coupon_rate IS NULL)
    ),
    CONSTRAINT instruments_option_fields CHECK (
        instrument_type <> 'option'
        OR (expiry IS NOT NULL AND strike IS NOT NULL AND strike > 0 AND maturity IS NULL AND coupon_rate IS NULL)
    ),
    CONSTRAINT instruments_bond_fields CHECK (
        instrument_type <> 'bond'
        OR (maturity IS NOT NULL AND coupon_rate IS NOT NULL AND expiry IS NULL AND strike IS NULL)
    )
);

-- Backfill: every symbol the workspace already uses becomes an equity with
-- multiplier 1. Existing equity results do not depend on this table (the
-- engine reads symbols from datasets), so this only gives the API a record.
INSERT INTO instruments (workspace_id, symbol, instrument_type, currency, multiplier, tick_size, lot_size)
SELECT DISTINCT ca.workspace_id, btrim(ca.symbol), 'equity', 'USD', 1, 0.01, 1
FROM corporate_actions ca
ON CONFLICT (workspace_id, symbol) DO NOTHING;

INSERT INTO instruments (workspace_id, symbol, instrument_type, currency, multiplier, tick_size, lot_size)
SELECT DISTINCT um.workspace_id, btrim(um.symbol), 'equity', 'USD', 1, 0.01, 1
FROM universe_members um
ON CONFLICT (workspace_id, symbol) DO NOTHING;
