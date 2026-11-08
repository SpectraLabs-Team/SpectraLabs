-- Risk parity as a fourth way to build a saved portfolio's weights (M15 follow-on): equal risk
-- contributions from the covariance of the holdings' daily closes in a dataset version.
ALTER TABLE portfolios DROP CONSTRAINT portfolios_method_check;
ALTER TABLE portfolios ADD CONSTRAINT portfolios_method_check
    CHECK (method IN ('fixed', 'equal', 'inverse_volatility', 'risk_parity'));
