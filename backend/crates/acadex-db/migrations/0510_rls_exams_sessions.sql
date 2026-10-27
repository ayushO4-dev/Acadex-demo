-- Access for 0500_exams_sessions.sql. Same conventions as 0200_rls_policies.sql.

GRANT SELECT (fee_head) ON fees.ledger TO acadex_app;
GRANT INSERT (fee_head) ON fees.ledger TO acadex_app;

-- ── Sessions, venues, timetable: everyone reads; the exam cell manages ──────
GRANT SELECT, INSERT, UPDATE ON exams.exam_sessions, exams.venues TO acadex_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON exams.schedule TO acadex_app;
ALTER TABLE exams.exam_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE exams.venues ENABLE ROW LEVEL SECURITY;
ALTER TABLE exams.schedule ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON exams.exam_sessions FOR SELECT TO acadex_app USING (true);
CREATE POLICY everyone_reads ON exams.venues FOR SELECT TO acadex_app USING (true);
CREATE POLICY everyone_reads ON exams.schedule FOR SELECT TO acadex_app USING (true);
CREATE POLICY exam_cell_manages ON exams.exam_sessions FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin'))
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin'));
CREATE POLICY exam_cell_manages ON exams.venues FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin'))
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin'));
CREATE POLICY exam_cell_manages ON exams.schedule FOR ALL TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin'))
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin'));
-- The worker records when hall tickets are generated.
CREATE POLICY worker_marks_generated ON exams.exam_sessions FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'worker') WITH CHECK (identity.current_app_role() = 'worker');

-- ── Forms: created only through exams.create_form ───────────────────────────
GRANT SELECT ON exams.exam_forms, exams.exam_form_subjects, exams.exam_form_charges TO acadex_app;
GRANT UPDATE (ack_pdf_object) ON exams.exam_forms TO acadex_app;
ALTER TABLE exams.exam_forms ENABLE ROW LEVEL SECURITY;
ALTER TABLE exams.exam_form_subjects ENABLE ROW LEVEL SECURITY;
ALTER TABLE exams.exam_form_charges ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_own ON exams.exam_forms FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY hod_reads_department ON exams.exam_forms FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND people.in_my_department(student_id));
CREATE POLICY staff_reads_all ON exams.exam_forms FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin', 'accounts', 'worker'));
CREATE POLICY worker_attaches_ack ON exams.exam_forms FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'worker') WITH CHECK (identity.current_app_role() = 'worker');

-- Children follow their form's visibility (the subquery is RLS-filtered).
CREATE POLICY follows_form ON exams.exam_form_subjects FOR SELECT TO acadex_app
    USING (form_id IN (SELECT id FROM exams.exam_forms));
CREATE POLICY follows_form ON exams.exam_form_charges FOR SELECT TO acadex_app
    USING (form_id IN (SELECT id FROM exams.exam_forms));

-- ── Seating: a student sees their seat once hall tickets are released ───────
GRANT SELECT ON exams.seating TO acadex_app;
ALTER TABLE exams.seating ENABLE ROW LEVEL SECURITY;
CREATE POLICY student_reads_own_released ON exams.seating FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
           AND session_id IN (SELECT id FROM exams.exam_sessions WHERE hall_tickets_released_at IS NOT NULL));
CREATE POLICY staff_reads_all ON exams.seating FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin', 'worker'));
