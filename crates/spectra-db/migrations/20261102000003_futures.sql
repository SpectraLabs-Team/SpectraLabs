-- M24.2: a future's chain (contracts with the same underlying roll into each
-- other) and an option's underlying symbol.
ALTER TABLE instruments ADD COLUMN underlying text CHECK (underlying IS NULL OR length(btrim(underlying)) > 0);
ALTER TABLE instruments ADD CONSTRAINT instruments_underlying_fields CHECK (
    instrument_type IN ('future', 'option') OR underlying IS NULL
);
