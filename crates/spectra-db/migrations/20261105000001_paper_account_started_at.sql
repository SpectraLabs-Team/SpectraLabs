-- M26 Task 6b: when the current paper account started.
-- A reset clears positions and balance but keeps old fills, so P&L attribution needs the
-- start of the current account. Existing rows are backfilled to the epoch so their whole
-- fill history counts; new rows start now; a reset sets it to now().

ALTER TABLE portfolio_balances ADD COLUMN started_at timestamptz;
UPDATE portfolio_balances SET started_at = 'epoch'::timestamptz;
ALTER TABLE portfolio_balances ALTER COLUMN started_at SET DEFAULT now();
ALTER TABLE portfolio_balances ALTER COLUMN started_at SET NOT NULL;
