-- How a saved portfolio's weights were built (M15 Task 2): fixed weights as given, equal weight
-- (1/N), or inverse volatility. Existing rows were fixed-weight saves.
ALTER TABLE portfolios ADD COLUMN method text NOT NULL DEFAULT 'fixed'
    CHECK (method IN ('fixed', 'equal', 'inverse_volatility'));
