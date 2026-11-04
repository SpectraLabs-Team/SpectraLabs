-- M26 Task 6b: a decision can be recorded without a price.
-- A kill-switch rejection of a market order with no known price used to insert price 0.0,
-- which failed CHECK (price > 0), so the decision was never recorded. NULL now means
-- "no price known"; the CHECK still rejects zero and negative prices.

ALTER TABLE risk_decisions ALTER COLUMN price DROP NOT NULL;
