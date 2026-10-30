-- M24.3: what a bond needs to pay coupons and accrue interest.
ALTER TABLE instruments
    ADD COLUMN coupon_frequency smallint CHECK (coupon_frequency IS NULL OR coupon_frequency IN (1, 2, 4, 12)),
    ADD COLUMN day_count text CHECK (day_count IS NULL OR day_count IN ('act_act_icma', '30_360', 'act_360', 'act_365f'));

-- Bonds state both; nothing else may carry them.
ALTER TABLE instruments ADD CONSTRAINT instruments_coupon_terms CHECK (
    (instrument_type = 'bond' AND coupon_frequency IS NOT NULL AND day_count IS NOT NULL
        AND coupon_rate >= 0 AND coupon_rate <= 1)
    OR (instrument_type <> 'bond' AND coupon_frequency IS NULL AND day_count IS NULL)
);
