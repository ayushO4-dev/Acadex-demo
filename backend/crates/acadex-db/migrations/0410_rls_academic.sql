-- Access for 0400_academic.sql. Same conventions as 0200_rls_policies.sql.

GRANT SELECT (kind) ON academic.subjects TO acadex_app;
GRANT SELECT (internal_min_pct) ON exams.grading_policy TO acadex_app;
GRANT UPDATE (internal_min_pct) ON exams.grading_policy TO acadex_app;

-- Is this student in the acting HOD's department?
CREATE FUNCTION people.in_my_department(p_student uuid) RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT EXISTS (SELECT 1 FROM people.students s
                     WHERE s.user_id = p_student
                       AND s.program_id IN (SELECT org.my_department_programs())) $$;
GRANT EXECUTE ON FUNCTION people.in_my_department(uuid) TO acadex_app;

-- ── Calendar: readable by everyone it concerns; admin and exam cell manage ──
GRANT SELECT, INSERT, UPDATE, DELETE ON academic.calendar_events TO acadex_app;
ALTER TABLE academic.calendar_events ENABLE ROW LEVEL SECURITY;

CREATE POLICY reads_institution_wide ON academic.calendar_events FOR SELECT TO acadex_app
    USING (program_id IS NULL AND identity.current_user_id() IS NOT NULL);
CREATE POLICY reads_own_program ON academic.calendar_events FOR SELECT TO acadex_app
    USING (program_id IN (SELECT program_id FROM people.students WHERE user_id = identity.current_user_id()
                          UNION ALL
                          SELECT program_id FROM people.faculty WHERE user_id = identity.current_user_id()));
CREATE POLICY managers_read_all ON academic.calendar_events FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));
CREATE POLICY managers_write ON academic.calendar_events FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'))
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin')
                AND created_by = identity.current_user_id());

-- ── Attendance corrections ──────────────────────────────────────────────────
GRANT SELECT, INSERT ON academic.attendance_corrections TO acadex_app;
GRANT UPDATE (status, decided_by, decided_at, decision_note) ON academic.attendance_corrections TO acadex_app;
ALTER TABLE academic.attendance_corrections ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_requests ON academic.attendance_corrections FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
                AND status = 'pending');
CREATE POLICY student_reads_own ON academic.attendance_corrections FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY faculty_reads_assigned ON academic.attendance_corrections FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'faculty'
           AND offering_id IN (SELECT id FROM academic.offerings WHERE faculty_id = identity.current_user_id()));
CREATE POLICY hod_reads_department ON academic.attendance_corrections FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs()));
CREATE POLICY hod_decides ON academic.attendance_corrections FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs())
           AND status = 'pending')
    WITH CHECK (decided_by = identity.current_user_id());

-- An approved correction rewrites the mark, as the HOD.
CREATE POLICY hod_corrects_department ON academic.attendance FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs()))
    WITH CHECK (identity.current_app_role() = 'hod' AND marked_by = identity.current_user_id());

-- ── MOOCs ───────────────────────────────────────────────────────────────────
GRANT SELECT, INSERT, UPDATE ON academic.mooc_courses TO acadex_app;
ALTER TABLE academic.mooc_courses ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON academic.mooc_courses FOR SELECT TO acadex_app
    USING (identity.current_user_id() IS NOT NULL);
CREATE POLICY managers_write ON academic.mooc_courses FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('hod', 'admin', 'super_admin'))
    WITH CHECK (identity.current_app_role() IN ('hod', 'admin', 'super_admin'));

GRANT SELECT, INSERT ON academic.mooc_enrolments TO acadex_app;
GRANT UPDATE (status, certificate_object, certificate_name, score_pct, requested_at,
              decided_by, decided_at, decision_note) ON academic.mooc_enrolments TO acadex_app;
ALTER TABLE academic.mooc_enrolments ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_owns ON academic.mooc_enrolments FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id())
    -- A student moves their own enrolment only as far as requesting credit.
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
                AND status IN ('enrolled', 'credit_requested') AND decided_by IS NULL);
CREATE POLICY hod_reviews_department ON academic.mooc_enrolments FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND people.in_my_department(student_id));
CREATE POLICY hod_decides ON academic.mooc_enrolments FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'hod' AND people.in_my_department(student_id))
    WITH CHECK (decided_by = identity.current_user_id());
CREATE POLICY staff_reads_all ON academic.mooc_enrolments FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));
