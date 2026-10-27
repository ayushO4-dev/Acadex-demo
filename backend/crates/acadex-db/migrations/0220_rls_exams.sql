-- Access to results and the outbox. Same conventions as 0200_rls_policies.sql.
--
-- The rule that matters: a student sees their own marks, and only once the
-- exam cell has published them. It's enforced here, so no API bug can leak a
-- staged result.

GRANT USAGE ON SCHEMA exams, ops TO acadex_app;

-- Configuration and assessment structure: readable by every signed-in user.
GRANT SELECT ON exams.grade_scale, exams.grading_policy, exams.internal_components TO acadex_app;
GRANT EXECUTE ON FUNCTION exams.offering_published(uuid) TO acadex_app;

-- ── exams.result_publications ───────────────────────────────────────────────
GRANT SELECT, UPDATE (published_at, published_by) ON exams.result_publications TO acadex_app;
ALTER TABLE exams.result_publications ENABLE ROW LEVEL SECURITY;

CREATE POLICY everyone_reads_published ON exams.result_publications FOR SELECT TO acadex_app
    USING (published_at IS NOT NULL);

CREATE POLICY exam_cell_reads_all ON exams.result_publications FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');

CREATE POLICY exam_cell_publishes ON exams.result_publications FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'exam_cell')
    WITH CHECK (published_by = identity.current_user_id());

-- ── exams.internal_marks, exams.external_marks ──────────────────────────────
GRANT SELECT ON exams.internal_marks, exams.external_marks TO acadex_app;
ALTER TABLE exams.internal_marks ENABLE ROW LEVEL SECURITY;
ALTER TABLE exams.external_marks ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_reads_own_published ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student'
           AND student_id = identity.current_user_id()
           AND exams.offering_published(offering_id));

CREATE POLICY student_reads_own_published ON exams.internal_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student'
           AND student_id = identity.current_user_id()
           AND exams.offering_published((SELECT offering_id FROM exams.internal_components
                                         WHERE id = component_id)));

-- Staged results too: the exam cell reviews them before publishing.
CREATE POLICY exam_cell_reads_all ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');
CREATE POLICY exam_cell_reads_all ON exams.internal_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');

-- ── ops.outbox ──────────────────────────────────────────────────────────────
-- Append-only for the API; publishing reads it under its own role (Phase 2, async).
GRANT INSERT ON ops.outbox TO acadex_app;
ALTER TABLE ops.outbox ENABLE ROW LEVEL SECURITY;
CREATE POLICY app_appends ON ops.outbox FOR INSERT TO acadex_app WITH CHECK (true);
