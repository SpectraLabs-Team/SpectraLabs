-- M24: swap records. A fixed-for-floating swap stores its maturity, its fixed
-- rate (coupon_rate, which may be negative) and its fixed-leg frequency
-- (coupon_frequency). It has no day count, expiry, strike or underlying.
ALTER TABLE instruments DROP CONSTRAINT instruments_coupon_terms;
ALTER TABLE instruments ADD CONSTRAINT instruments_coupon_terms CHECK (
    (instrument_type = 'bond' AND coupon_frequency IS NOT NULL AND day_count IS NOT NULL
        AND coupon_rate >= 0 AND coupon_rate <= 1)
    OR (instrument_type = 'swap' AND coupon_frequency IS NOT NULL AND day_count IS NULL
        AND coupon_rate >= -1 AND coupon_rate <= 1)
    OR (instrument_type NOT IN ('bond', 'swap') AND coupon_frequency IS NULL AND day_count IS NULL)
);
ALTER TABLE instruments ADD CONSTRAINT instruments_swap_fields CHECK (
    instrument_type <> 'swap'
    OR (maturity IS NOT NULL AND coupon_rate IS NOT NULL AND expiry IS NULL AND strike IS NULL
        AND underlying IS NULL)
);
