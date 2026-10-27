-- Everything the app role may touch, and the row-level policies that scope it.
-- Kept in one file so access can be reviewed in one place.
--
-- Policies read the acting user from identity.current_user_id() and
-- identity.current_app_role(), which are NULL outside a request transaction,
-- so a query that forgets to bind the user sees nothing.

GRANT USAGE ON SCHEMA org, identity, people, academic, audit TO acadex_app;

-- Reference data: readable by every signed-in user.
GRANT SELECT ON org.institutions, org.programs TO acadex_app;
GRANT SELECT ON academic.subjects, academic.offerings TO acadex_app;
GRANT SELECT ON people.faculty TO acadex_app;

-- Names and roles only; emails stay behind identity.resolve_subject().
GRANT SELECT (id, display_name, role) ON identity.users TO acadex_app;

-- SECURITY DEFINER functions: callable by the app role only, not by PUBLIC.
REVOKE EXECUTE ON FUNCTION identity.resolve_subject(text, text, boolean, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.resolve_subject(text, text, boolean, boolean) TO acadex_app;
REVOKE EXECUTE ON FUNCTION audit.record(text, text, text, text, inet, text, text, jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION audit.record(text, text, text, text, inet, text, text, jsonb) TO acadex_app;
REVOKE EXECUTE ON FUNCTION audit.verify_chain() FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION academic.ensure_attendance_partition(integer, integer) FROM PUBLIC;

-- ── people.students ─────────────────────────────────────────────────────────
GRANT SELECT ON people.students TO acadex_app;
ALTER TABLE people.students ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_self ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student'
           AND user_id = identity.current_user_id());

CREATE POLICY faculty_reads_taught_programs ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'faculty'
           AND program_id IN (SELECT s.program_id
                              FROM academic.offerings o
                              JOIN academic.subjects s ON s.id = o.subject_id
                              WHERE o.faculty_id = identity.current_user_id()));

CREATE POLICY exam_cell_reads_all ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');

-- ── academic.attendance ─────────────────────────────────────────────────────
GRANT SELECT, INSERT, UPDATE ON academic.attendance TO acadex_app;
ALTER TABLE academic.attendance ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_own ON academic.attendance FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student'
           AND student_id = identity.current_user_id());

CREATE POLICY faculty_reads_assigned ON academic.attendance FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'faculty'
           AND offering_id IN (SELECT id FROM academic.offerings
                               WHERE faculty_id = identity.current_user_id()));

CREATE POLICY faculty_marks_assigned ON academic.attendance FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'faculty'
                AND marked_by = identity.current_user_id()
                AND offering_id IN (SELECT id FROM academic.offerings
                                    WHERE faculty_id = identity.current_user_id()));

CREATE POLICY faculty_corrects_assigned ON academic.attendance FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'faculty'
           AND offering_id IN (SELECT id FROM academic.offerings
                               WHERE faculty_id = identity.current_user_id()))
    WITH CHECK (marked_by = identity.current_user_id());

CREATE POLICY exam_cell_reads_all ON academic.attendance FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');

-- ── audit.action_log ────────────────────────────────────────────────────────
-- Read-only for the app; rows arrive through audit.record().
GRANT SELECT ON audit.action_log TO acadex_app;
ALTER TABLE audit.action_log ENABLE ROW LEVEL SECURITY;

CREATE POLICY reads_own_actions ON audit.action_log FOR SELECT TO acadex_app
    USING (actor_id = identity.current_user_id());

CREATE POLICY exam_cell_reads_all ON audit.action_log FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');
