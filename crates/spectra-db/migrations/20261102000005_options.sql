-- M24.4: European, cash-settled options: a right (call or put) and the
-- underlying they settle against.
ALTER TABLE instruments
    ADD COLUMN option_right text CHECK (option_right IS NULL OR option_right IN ('call', 'put'));

ALTER TABLE instruments ADD CONSTRAINT instruments_option_terms CHECK (
    (instrument_type = 'option' AND option_right IS NOT NULL AND underlying IS NOT NULL)
    OR (instrument_type <> 'option' AND option_right IS NULL)
);
