-- A document of type "other" says what it is (an income certificate, a
-- gap-year affidavit…); the listed types don't need one.
ALTER TABLE profile.documents
    ADD COLUMN description text CHECK (length(description) BETWEEN 3 AND 80),
    ADD CHECK (doc_type = 'other' OR description IS NULL);
GRANT INSERT (description) ON profile.documents TO acadex_app;
