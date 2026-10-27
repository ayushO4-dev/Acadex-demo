-- Curriculum management: heads of department maintain their department's
-- subjects and assign faculty to teach them each academic year;
-- administration can do so for any program. Everyone signed in still reads.

-- A subject in use can't be deleted (attendance and marks point at it); it is
-- retired instead, and no longer offered.
ALTER TABLE academic.subjects
    ADD COLUMN active     boolean NOT NULL DEFAULT true,
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now(),
    ADD CONSTRAINT subject_code_format CHECK (code ~ '^[A-Z]{2,4}[0-9]{3}[A-Z]?$') NOT VALID,
    ADD CONSTRAINT subject_name_length CHECK (length(name) BETWEEN 3 AND 120) NOT VALID,
    ADD CONSTRAINT subject_credits_range CHECK (credits BETWEEN 1 AND 10) NOT VALID;

GRANT SELECT (active, updated_at) ON academic.subjects TO acadex_app;
GRANT INSERT (program_id, code, name, credits, semester, kind, has_practical) ON academic.subjects TO acadex_app;
GRANT UPDATE (name, credits, semester, kind, has_practical, active, updated_at) ON academic.subjects TO acadex_app;
GRANT INSERT (subject_id, academic_year, faculty_id), UPDATE (faculty_id), DELETE ON academic.offerings TO acadex_app;

-- Whether anything was recorded against an offering (attendance, marks,
-- exam registrations, corrections): such an offering, and its subject's
-- semester, are history and stay as they are. Sees past row-level security,
-- since administration can't read attendance itself.
CREATE FUNCTION academic.offering_has_records(p_offering uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT EXISTS (SELECT 1 FROM academic.attendance WHERE offering_id = p_offering)
        OR EXISTS (SELECT 1 FROM exams.internal_components WHERE offering_id = p_offering)
        OR EXISTS (SELECT 1 FROM exams.external_marks WHERE offering_id = p_offering)
        OR EXISTS (SELECT 1 FROM exams.exam_form_subjects WHERE offering_id = p_offering)
        OR EXISTS (SELECT 1 FROM academic.attendance_corrections WHERE offering_id = p_offering)
$$;
GRANT EXECUTE ON FUNCTION academic.offering_has_records(uuid) TO acadex_app;

-- Who may change a program's curriculum.
CREATE FUNCTION academic.may_manage_program(p_program smallint) RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT identity.is_admin()
        OR (identity.current_app_role() = 'hod' AND p_program IN (SELECT org.my_department_programs()))
$$;

ALTER TABLE academic.subjects ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON academic.subjects FOR SELECT TO acadex_app USING (true);
CREATE POLICY managers_add ON academic.subjects FOR INSERT TO acadex_app
    WITH CHECK (academic.may_manage_program(program_id));
CREATE POLICY managers_change ON academic.subjects FOR UPDATE TO acadex_app
    USING (academic.may_manage_program(program_id))
    WITH CHECK (academic.may_manage_program(program_id));

ALTER TABLE academic.offerings ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON academic.offerings FOR SELECT TO acadex_app USING (true);
CREATE POLICY managers_assign ON academic.offerings FOR ALL TO acadex_app
    USING (academic.may_manage_program((SELECT program_id FROM academic.subjects WHERE id = subject_id)))
    WITH CHECK (academic.may_manage_program((SELECT program_id FROM academic.subjects WHERE id = subject_id)));
