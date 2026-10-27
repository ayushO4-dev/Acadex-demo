CREATE TABLE academic.subjects (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    program_id smallint NOT NULL REFERENCES org.programs,
    code       text NOT NULL UNIQUE,
    name       text NOT NULL,
    credits    smallint NOT NULL CHECK (credits > 0),
    semester   smallint NOT NULL CHECK (semester BETWEEN 1 AND 8)
);

-- A subject taught in one academic year by one faculty member.
CREATE TABLE academic.offerings (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    subject_id    uuid NOT NULL REFERENCES academic.subjects,
    academic_year smallint NOT NULL,  -- 2026 = AY 2026-27
    faculty_id    uuid NOT NULL REFERENCES people.faculty (user_id),
    UNIQUE (subject_id, academic_year)
);
CREATE INDEX ON academic.offerings (faculty_id);
