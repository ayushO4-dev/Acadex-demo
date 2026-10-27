-- The weekly class timetable: which class meets when and where. Heads of
-- department keep their programs' timetables (administration, any); everyone
-- reads. One entry is one class period: a subject's offering (for the whole
-- class, or one section), on a weekday and period, in a room. The API refuses
-- a clash: the same students, teacher or room twice in one period.

CREATE TABLE academic.timetable (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    offering_id  uuid NOT NULL REFERENCES academic.offerings ON DELETE CASCADE,
    -- NULL: the whole class; otherwise one section.
    section      text CHECK (section ~ '^[A-Z]$'),
    weekday      smallint NOT NULL CHECK (weekday BETWEEN 1 AND 6),   -- Monday = 1
    slot         smallint NOT NULL CHECK (slot BETWEEN 1 AND 8),
    session_kind text NOT NULL DEFAULT 'theory' CHECK (session_kind IN ('theory', 'practical')),
    room         text CHECK (length(room) BETWEEN 1 AND 40),
    created_by   uuid REFERENCES identity.users,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON academic.timetable (offering_id);
CREATE INDEX ON academic.timetable (weekday, slot);

GRANT SELECT, INSERT, DELETE ON academic.timetable TO acadex_app;
ALTER TABLE academic.timetable ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON academic.timetable FOR SELECT TO acadex_app USING (true);
CREATE POLICY managers_write ON academic.timetable FOR ALL TO acadex_app
    USING (academic.may_manage_program((SELECT s.program_id FROM academic.offerings o
                                        JOIN academic.subjects s ON s.id = o.subject_id WHERE o.id = offering_id)))
    WITH CHECK (academic.may_manage_program((SELECT s.program_id FROM academic.offerings o
                                             JOIN academic.subjects s ON s.id = o.subject_id WHERE o.id = offering_id)));
