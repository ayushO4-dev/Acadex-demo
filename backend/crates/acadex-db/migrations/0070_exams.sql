-- Grading is configuration, not code (architecture.md A4): the scale and the
-- pass rule are rows, so a university's change is an UPDATE, not a release.
CREATE TABLE exams.grade_scale (
    institution_id smallint NOT NULL REFERENCES org.institutions,
    min_pct        numeric(5, 2) NOT NULL CHECK (min_pct BETWEEN 0 AND 100),
    grade          text NOT NULL,
    points         smallint NOT NULL CHECK (points BETWEEN 0 AND 10),
    PRIMARY KEY (institution_id, min_pct)
);

CREATE TABLE exams.grading_policy (
    institution_id   smallint PRIMARY KEY REFERENCES org.institutions,
    -- A subject is passed only with at least this share of the external exam,
    -- whatever the total.
    external_min_pct numeric(5, 2) NOT NULL CHECK (external_min_pct BETWEEN 0 AND 100)
);

-- The unit of publication: one program's results for one semester.
CREATE TABLE exams.result_publications (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    program_id    smallint NOT NULL REFERENCES org.programs,
    semester      smallint NOT NULL CHECK (semester BETWEEN 1 AND 8),
    academic_year smallint NOT NULL,
    kind          text NOT NULL DEFAULT 'regular' CHECK (kind IN ('regular', 'backlog', 'revaluation')),
    staged_at     timestamptz NOT NULL DEFAULT now(),
    -- NULL while staged: nobody but the exam cell can see the marks.
    published_at  timestamptz,
    published_by  uuid REFERENCES identity.users,
    UNIQUE (program_id, semester, academic_year, kind),
    CHECK ((published_at IS NULL) = (published_by IS NULL))
);

-- Internal assessment is configured per offering (§9.3): e.g. two CCAs and a
-- midterm for a theory subject, lab assessments and a viva for a lab.
CREATE TABLE exams.internal_components (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    offering_id uuid NOT NULL REFERENCES academic.offerings,
    code        text NOT NULL,
    max_marks   smallint NOT NULL CHECK (max_marks > 0),
    position    smallint NOT NULL,
    UNIQUE (offering_id, code)
);

CREATE TABLE exams.internal_marks (
    student_id   uuid NOT NULL REFERENCES people.students,
    component_id uuid NOT NULL REFERENCES exams.internal_components,
    marks        numeric(5, 1) NOT NULL CHECK (marks >= 0),
    PRIMARY KEY (student_id, component_id)
);

CREATE TABLE exams.external_marks (
    student_id     uuid NOT NULL REFERENCES people.students,
    offering_id    uuid NOT NULL REFERENCES academic.offerings,
    publication_id uuid NOT NULL REFERENCES exams.result_publications,
    marks          numeric(5, 1) NOT NULL CHECK (marks >= 0),
    max_marks      smallint NOT NULL CHECK (max_marks > 0),
    PRIMARY KEY (student_id, offering_id),
    CHECK (marks <= max_marks)
);
CREATE INDEX ON exams.external_marks (publication_id);

-- Has this offering's result been published? Runs with the caller's
-- privileges, so RLS on result_publications applies inside it too.
CREATE FUNCTION exams.offering_published(p_offering uuid) RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM academic.offerings o
        JOIN academic.subjects s ON s.id = o.subject_id
        JOIN exams.result_publications rp
          ON rp.program_id = s.program_id AND rp.semester = s.semester
         AND rp.academic_year = o.academic_year
        WHERE o.id = p_offering AND rp.published_at IS NOT NULL)
$$;
