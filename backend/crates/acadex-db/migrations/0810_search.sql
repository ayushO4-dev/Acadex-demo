-- Universal search (architecture.md §8.2): trigram indexes, so a substring
-- match ('%term%') uses an index instead of scanning. What each user may
-- find is still decided by row-level security on the tables themselves.
CREATE INDEX subjects_name_trgm ON academic.subjects USING gin (name gin_trgm_ops);
CREATE INDEX notices_title_trgm ON comms.notices USING gin (title gin_trgm_ops);
CREATE INDEX tickets_subject_trgm ON support.tickets USING gin (subject gin_trgm_ops);
CREATE INDEX users_display_name_trgm ON identity.users USING gin (display_name gin_trgm_ops);
CREATE INDEX students_roll_no_trgm ON people.students USING gin (roll_no gin_trgm_ops);
CREATE INDEX receipts_no_trgm ON fees.receipts USING gin (receipt_no gin_trgm_ops);
