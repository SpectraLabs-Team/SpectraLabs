-- M24.4: swap backtests. A swap names the discount curve it is marked on
-- (instruments.underlying), and curve_quotes keeps each curve's dated
-- quotes: from as_of on, these deposits and par swaps define the curve.
-- Years and rates are exact decimals (8 fractional digits), as runs use them.
-- A swap stored before this migration has no curve and cannot be marked.
-- Stop with a clear message rather than guess one: set each swap's
-- underlying to its curve name (or delete it), then run the migration again.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM instruments WHERE instrument_type = 'swap' AND underlying IS NULL) THEN
        RAISE EXCEPTION 'swap instruments without a curve: set instruments.underlying to the curve name of each swap (or delete it), then migrate again';
    END IF;
END $$;
ALTER TABLE instruments DROP CONSTRAINT instruments_underlying_fields;
ALTER TABLE instruments ADD CONSTRAINT instruments_underlying_fields CHECK (
    instrument_type IN ('future', 'option', 'swap') OR underlying IS NULL
);
ALTER TABLE instruments DROP CONSTRAINT instruments_swap_fields;
ALTER TABLE instruments ADD CONSTRAINT instruments_swap_fields CHECK (
    instrument_type <> 'swap'
    OR (maturity IS NOT NULL AND coupon_rate IS NOT NULL AND expiry IS NULL AND strike IS NULL
        AND underlying IS NOT NULL)
);

CREATE TABLE curve_quotes (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    curve        text NOT NULL CHECK (length(btrim(curve)) > 0 AND curve = btrim(curve)),
    as_of        timestamptz NOT NULL,
    quote_type   text NOT NULL CHECK (quote_type IN ('deposit', 'swap')),
    years        numeric(20, 8) NOT NULL CHECK (years > 0),
    rate         numeric(20, 8) NOT NULL CHECK (rate > -1 AND rate < 1),
    frequency    smallint,
    created_at   timestamptz NOT NULL DEFAULT now(),
    CHECK ((quote_type = 'swap' AND frequency IN (1, 2, 4, 12)) OR (quote_type = 'deposit' AND frequency IS NULL)),
    UNIQUE (workspace_id, curve, as_of, years)
);
CREATE INDEX curve_quotes_lookup ON curve_quotes (workspace_id, curve, as_of);
