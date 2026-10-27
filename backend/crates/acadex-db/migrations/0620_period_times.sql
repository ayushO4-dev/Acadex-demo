-- When each teaching period starts, and how long periods are: shown with
-- attendance sessions (configuration an administrator changes, A4).

ALTER TABLE org.institution_settings
    ADD COLUMN period_starts  text[] NOT NULL
        DEFAULT '{09:00,10:00,11:00,12:00,14:00,15:00,16:00,17:00}'
        CHECK (cardinality(period_starts) = 8
               AND array_to_string(period_starts, ',') ~ '^([01][0-9]|2[0-3]):[0-5][0-9](,([01][0-9]|2[0-3]):[0-5][0-9]){7}$'),
    ADD COLUMN period_minutes smallint NOT NULL DEFAULT 55 CHECK (period_minutes BETWEEN 30 AND 120);
GRANT UPDATE (period_starts, period_minutes) ON org.institution_settings TO acadex_app;
