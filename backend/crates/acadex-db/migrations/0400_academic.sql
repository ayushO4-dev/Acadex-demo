-- F2, Academic (architecture.md §9.1): subject types, the academic calendar,
-- attendance corrections, the internal pass mark, and MOOCs.

-- ── Subjects ────────────────────────────────────────────────────────────────
CREATE TYPE academic.subject_kind AS ENUM ('theory', 'lab', 'elective');
ALTER TABLE academic.subjects ADD COLUMN kind academic.subject_kind NOT NULL DEFAULT 'theory';
UPDATE academic.subjects SET kind = 'lab' WHERE credits = 1;

-- ── Academic calendar ───────────────────────────────────────────────────────
CREATE TYPE academic.event_kind AS ENUM ('holiday', 'exam', 'deadline', 'event', 'class_suspension');

CREATE TABLE academic.calendar_events (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    institution_id smallint NOT NULL REFERENCES org.institutions,
    -- NULL: everyone. Otherwise only that program.
    program_id     smallint REFERENCES org.programs,
    kind           academic.event_kind NOT NULL,
    title          text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description    text CHECK (length(description) <= 2000),
    starts_on      date NOT NULL,
    ends_on        date NOT NULL CHECK (ends_on >= starts_on),
    created_by     uuid REFERENCES identity.users,
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON academic.calendar_events (starts_on);

-- ── Attendance corrections ──────────────────────────────────────────────────
-- A student disputes one session's mark; the HOD of their department decides.
-- Approving rewrites the attendance row, in the same transaction.
CREATE TABLE academic.attendance_corrections (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    program_id       smallint NOT NULL,
    academic_year    smallint NOT NULL,
    student_id       uuid NOT NULL REFERENCES people.students (user_id),
    offering_id      uuid NOT NULL REFERENCES academic.offerings,
    session_date     date NOT NULL,
    slot             smallint NOT NULL,
    current_status   academic.attendance_status NOT NULL,
    requested_status academic.attendance_status NOT NULL CHECK (requested_status <> current_status),
    reason           text NOT NULL CHECK (length(reason) BETWEEN 5 AND 1000),
    status           text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    decided_by       uuid REFERENCES identity.users,
    decided_at       timestamptz,
    decision_note    text CHECK (length(decision_note) <= 1000),
    created_at       timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'pending') = (decided_by IS NULL))
);
-- One open request per session at a time.
CREATE UNIQUE INDEX attendance_corrections_one_pending
    ON academic.attendance_corrections (student_id, offering_id, session_date, slot)
    WHERE status = 'pending';
CREATE INDEX ON academic.attendance_corrections (program_id, status);

-- ── Internal pass mark ──────────────────────────────────────────────────────
-- Course Status reports internal and external pass/fail separately; a subject
-- is cleared only with both.
ALTER TABLE exams.grading_policy
    ADD COLUMN internal_min_pct numeric(5, 2) NOT NULL DEFAULT 40 CHECK (internal_min_pct BETWEEN 0 AND 100);

-- ── MOOCs ───────────────────────────────────────────────────────────────────
CREATE TABLE academic.mooc_courses (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    institution_id smallint NOT NULL REFERENCES org.institutions,
    provider       text NOT NULL CHECK (provider IN ('NPTEL', 'SWAYAM', 'Coursera')),
    code           text NOT NULL,
    title          text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    weeks          smallint NOT NULL CHECK (weeks BETWEEN 1 AND 52),
    credits        smallint NOT NULL CHECK (credits BETWEEN 1 AND 6),
    url            text CHECK (url ~ '^https://'),
    starts_on      date,
    active         boolean NOT NULL DEFAULT true,
    UNIQUE (provider, code)
);

-- enrolled → certificate uploaded → credit requested → approved | rejected
CREATE TABLE academic.mooc_enrolments (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    course_id          uuid NOT NULL REFERENCES academic.mooc_courses,
    student_id         uuid NOT NULL REFERENCES people.students (user_id),
    status             text NOT NULL DEFAULT 'enrolled'
                       CHECK (status IN ('enrolled', 'credit_requested', 'approved', 'rejected')),
    certificate_object text,
    certificate_name   text,
    score_pct          numeric(5, 2) CHECK (score_pct BETWEEN 0 AND 100),
    enrolled_at        timestamptz NOT NULL DEFAULT now(),
    requested_at       timestamptz,
    decided_by         uuid REFERENCES identity.users,
    decided_at         timestamptz,
    decision_note      text CHECK (length(decision_note) <= 1000),
    UNIQUE (course_id, student_id),
    CHECK (status = 'enrolled' OR certificate_object IS NOT NULL)
);
