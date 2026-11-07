-- Saved portfolios (M15 Task 1): a named set of symbols with target weights, stored per workspace.
--
-- A portfolio's weights must be finite, non-negative, unique by symbol, and sum to 1 within
-- 1e-6. The sum is checked by a deferred constraint trigger, so it is checked at commit: a save
-- whose weights do not sum to 1 is refused, and the whole portfolio is rolled back. The same rule
-- is checked by the application (spectra_backtest::portfolio::validate_weights) before the save.

CREATE TABLE portfolios (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 120),
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace_id, name),
    -- Lets holdings reference the portfolio with its workspace, so they cannot cross workspaces.
    UNIQUE (id, workspace_id)
);

CREATE TABLE portfolio_holdings (
    portfolio_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    symbol       text NOT NULL CHECK (btrim(symbol) <> ''),
    weight       double precision NOT NULL CHECK (weight >= 0 AND weight <= 1),
    PRIMARY KEY (portfolio_id, symbol),
    FOREIGN KEY (portfolio_id, workspace_id) REFERENCES portfolios (id, workspace_id) ON DELETE CASCADE
);

CREATE FUNCTION portfolio_weights_sum_to_one() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    total double precision;
    holdings integer;
BEGIN
    SELECT coalesce(sum(weight), 0), count(*) INTO total, holdings
      FROM portfolio_holdings WHERE portfolio_id = NEW.portfolio_id;
    IF holdings = 0 THEN
        RAISE EXCEPTION 'portfolio % has no holdings', NEW.portfolio_id USING ERRCODE = 'check_violation';
    END IF;
    IF abs(total - 1) > 0.000001 THEN
        RAISE EXCEPTION 'portfolio % weights sum to %, not 1 within 0.000001', NEW.portfolio_id, total
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER portfolio_holdings_sum
    AFTER INSERT OR UPDATE ON portfolio_holdings
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION portfolio_weights_sum_to_one();
