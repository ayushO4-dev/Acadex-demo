-- Electives: an elective subject belongs to a group ("Professional
-- Elective I"), and each student takes one subject per group in their
-- semester. Until now every student of the semester was on every elective's
-- roster; academic.takes() is the rule rosters, exam forms and course status
-- now follow. A student chooses (and may change their mind until the class
-- they leave has records); the HOD or administration can assign for them.

ALTER TABLE academic.subjects
    ADD COLUMN elective_group text CHECK (length(elective_group) BETWEEN 2 AND 60);
UPDATE academic.subjects SET elective_group = 'Elective' WHERE kind = 'elective' AND elective_group IS NULL;
ALTER TABLE academic.subjects
    ADD CONSTRAINT electives_are_grouped CHECK ((kind = 'elective') = (elective_group IS NOT NULL));
GRANT SELECT (elective_group), INSERT (elective_group), UPDATE (elective_group) ON academic.subjects TO acadex_app;

CREATE TABLE academic.elective_choices (
    student_id     uuid NOT NULL REFERENCES people.students (user_id) ON DELETE CASCADE,
    subject_id     uuid NOT NULL REFERENCES academic.subjects,
    -- Copied from the subject, so one choice per group per semester holds.
    semester       smallint NOT NULL,
    elective_group text NOT NULL,
    chosen_at      timestamptz NOT NULL DEFAULT now(),
    chosen_by      uuid REFERENCES identity.users,
    PRIMARY KEY (student_id, semester, elective_group)
);
CREATE INDEX ON academic.elective_choices (subject_id);
GRANT SELECT ON academic.elective_choices TO acadex_app;
ALTER TABLE academic.elective_choices ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_choices ON academic.elective_choices FOR SELECT TO acadex_app
    USING (student_id = identity.current_user_id());
CREATE POLICY staff_read ON academic.elective_choices FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('faculty', 'hod', 'exam_cell', 'admin', 'super_admin', 'worker'));

-- Whether a student takes a subject: every core subject of their semester,
-- and the electives they chose.
CREATE FUNCTION academic.takes(p_student uuid, p_subject uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT NOT EXISTS (SELECT 1 FROM academic.subjects WHERE id = p_subject AND kind = 'elective')
        OR EXISTS (SELECT 1 FROM academic.elective_choices WHERE student_id = p_student AND subject_id = p_subject)
$$;
GRANT EXECUTE ON FUNCTION academic.takes(uuid, uuid) TO acadex_app;

-- Chooses (or changes) a student's elective in a subject's group. A student
-- chooses for themselves in their current semester; a manager of the
-- program for anyone. Leaving a class that already has records is refused.
CREATE FUNCTION academic.choose_elective(p_student uuid, p_subject uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_subject  academic.subjects;
    v_student  people.students;
    v_previous uuid;
    v_year     smallint;
BEGIN
    SELECT * INTO v_subject FROM academic.subjects WHERE id = p_subject;
    SELECT * INTO v_student FROM people.students WHERE user_id = p_student;
    IF v_subject.id IS NULL OR v_student.user_id IS NULL THEN
        RAISE EXCEPTION 'no such subject or student' USING ERRCODE = 'no_data_found';
    END IF;
    IF p_student = identity.current_user_id() AND identity.current_app_role() = 'student' THEN
        IF v_subject.semester <> v_student.current_semester THEN
            RAISE EXCEPTION 'you choose electives for your current semester' USING ERRCODE = 'check_violation';
        END IF;
    ELSIF NOT academic.may_manage_program(v_student.program_id) THEN
        RAISE EXCEPTION 'not your program' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF v_subject.kind <> 'elective' OR NOT v_subject.active OR v_subject.program_id <> v_student.program_id THEN
        RAISE EXCEPTION 'that is not an elective of the student''s program' USING ERRCODE = 'check_violation';
    END IF;

    SELECT c.subject_id INTO v_previous FROM academic.elective_choices c
    WHERE c.student_id = p_student AND c.semester = v_subject.semester AND c.elective_group = v_subject.elective_group
    FOR UPDATE;
    IF v_previous = p_subject THEN
        RETURN 'unchanged';
    END IF;
    IF v_previous IS NOT NULL THEN
        v_year := v_student.batch_year + (v_subject.semester - 1) / 2;
        IF EXISTS (SELECT 1 FROM academic.attendance a JOIN academic.offerings o ON o.id = a.offering_id
                   WHERE o.subject_id = v_previous AND o.academic_year = v_year AND a.student_id = p_student)
           OR EXISTS (SELECT 1 FROM exams.internal_marks m JOIN exams.internal_components ic ON ic.id = m.component_id
                      JOIN academic.offerings o ON o.id = ic.offering_id
                      WHERE o.subject_id = v_previous AND m.student_id = p_student) THEN
            RAISE EXCEPTION 'attendance or marks are recorded in the current choice' USING ERRCODE = 'check_violation';
        END IF;
    END IF;
    INSERT INTO academic.elective_choices (student_id, subject_id, semester, elective_group, chosen_by)
    VALUES (p_student, p_subject, v_subject.semester, v_subject.elective_group, identity.current_user_id())
    ON CONFLICT (student_id, semester, elective_group)
    DO UPDATE SET subject_id = EXCLUDED.subject_id, chosen_at = now(), chosen_by = EXCLUDED.chosen_by;
    RETURN CASE WHEN v_previous IS NULL THEN 'chosen' ELSE 'changed' END;
END $$;
REVOKE EXECUTE ON FUNCTION academic.choose_elective(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION academic.choose_elective(uuid, uuid) TO acadex_app;
