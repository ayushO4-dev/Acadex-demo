-- F3c: marks entry, attempts (regular, then backlog) and two-person approval
-- of result publication (architecture.md §9.3).

-- A subject's external result can have several attempts: the regular one,
-- then backlog attempts. The latest published one counts.
ALTER TABLE exams.external_marks ADD COLUMN attempt smallint NOT NULL DEFAULT 1 CHECK (attempt BETWEEN 1 AND 10);
ALTER TABLE exams.external_marks DROP CONSTRAINT external_marks_pkey;
ALTER TABLE exams.external_marks ADD PRIMARY KEY (student_id, offering_id, attempt);

-- Two-person approval: one exam-cell user asks to publish, a different one
-- publishes. NOT VALID: results published before this rule keep their record.
ALTER TABLE exams.result_publications
    ADD COLUMN requested_by uuid REFERENCES identity.users,
    ADD COLUMN requested_at timestamptz;
ALTER TABLE exams.result_publications
    ADD CONSTRAINT publication_needs_two_people
    CHECK (published_at IS NULL OR (requested_by IS NOT NULL AND published_by <> requested_by)) NOT VALID;

-- ── Access ──────────────────────────────────────────────────────────────────
-- The exam cell creates result sets and moves them through request → publish.
GRANT INSERT ON exams.result_publications TO acadex_app;
GRANT UPDATE (requested_by, requested_at) ON exams.result_publications TO acadex_app;
CREATE POLICY exam_cell_creates ON exams.result_publications FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin') AND published_at IS NULL);
DROP POLICY exam_cell_publishes ON exams.result_publications;
CREATE POLICY exam_cell_moves ON exams.result_publications FOR UPDATE TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin') AND published_at IS NULL)
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin')
                AND (published_by IS NULL OR published_by = identity.current_user_id())
                AND (requested_by IS NULL OR requested_by = identity.current_user_id() OR published_by = identity.current_user_id()));

-- Internal components: set by the subject's teacher or the exam cell, until
-- its result is published.
CREATE FUNCTION academic.teaches(p_offering uuid) RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT EXISTS (SELECT 1 FROM academic.offerings WHERE id = p_offering AND faculty_id = identity.current_user_id()) $$;
GRANT EXECUTE ON FUNCTION academic.teaches(uuid) TO acadex_app;

GRANT INSERT, UPDATE, DELETE ON exams.internal_components TO acadex_app;
ALTER TABLE exams.internal_components ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON exams.internal_components FOR SELECT TO acadex_app USING (true);
CREATE POLICY setters_write ON exams.internal_components FOR ALL TO acadex_app
    USING ((academic.teaches(offering_id) OR identity.current_app_role() IN ('exam_cell', 'super_admin'))
           AND NOT exams.offering_published(offering_id))
    WITH CHECK ((academic.teaches(offering_id) OR identity.current_app_role() IN ('exam_cell', 'super_admin'))
                AND NOT exams.offering_published(offering_id));

-- Internal marks: entered by the teacher (or exam cell) until published; the
-- teacher also reads them back.
GRANT INSERT, UPDATE ON exams.internal_marks TO acadex_app;
CREATE POLICY teacher_reads ON exams.internal_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'faculty'
           AND academic.teaches((SELECT offering_id FROM exams.internal_components WHERE id = component_id)));
CREATE POLICY setters_write ON exams.internal_marks FOR INSERT TO acadex_app
    WITH CHECK ((identity.current_app_role() IN ('exam_cell', 'super_admin')
                 OR academic.teaches((SELECT offering_id FROM exams.internal_components WHERE id = component_id)))
                AND NOT exams.offering_published((SELECT offering_id FROM exams.internal_components WHERE id = component_id)));
CREATE POLICY setters_update ON exams.internal_marks FOR UPDATE TO acadex_app
    USING ((identity.current_app_role() IN ('exam_cell', 'super_admin')
            OR academic.teaches((SELECT offering_id FROM exams.internal_components WHERE id = component_id)))
           AND NOT exams.offering_published((SELECT offering_id FROM exams.internal_components WHERE id = component_id)));

-- External marks: the exam cell, into a staged result set only.
GRANT INSERT, UPDATE ON exams.external_marks TO acadex_app;
CREATE POLICY exam_cell_enters_staged ON exams.external_marks FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin')
                AND publication_id IN (SELECT id FROM exams.result_publications WHERE published_at IS NULL));
CREATE POLICY exam_cell_corrects_staged ON exams.external_marks FOR UPDATE TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin')
           AND publication_id IN (SELECT id FROM exams.result_publications WHERE published_at IS NULL));

-- The worker reads results for grade cards.
CREATE POLICY worker_reads ON exams.external_marks FOR SELECT TO acadex_app USING (identity.current_app_role() = 'worker');
CREATE POLICY worker_reads ON exams.internal_marks FOR SELECT TO acadex_app USING (identity.current_app_role() = 'worker');
CREATE POLICY worker_reads ON exams.result_publications FOR SELECT TO acadex_app USING (identity.current_app_role() = 'worker');

-- A published offering no longer means every attempt is: a backlog attempt
-- stays hidden until its own result set is published.
DROP POLICY student_reads_own_published ON exams.external_marks;
CREATE POLICY student_reads_own_published ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student'
           AND student_id = identity.current_user_id()
           AND publication_id IN (SELECT id FROM exams.result_publications WHERE published_at IS NOT NULL));
DROP POLICY hod_reads_department_published ON exams.external_marks;
CREATE POLICY hod_reads_department_published ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod'
           AND publication_id IN (SELECT id FROM exams.result_publications WHERE published_at IS NOT NULL)
           AND offering_id IN (SELECT o.id FROM academic.offerings o
                               JOIN academic.subjects s ON s.id = o.subject_id
                               WHERE s.program_id IN (SELECT org.my_department_programs())));

-- The super admin acts as the exam cell too.
CREATE POLICY super_admin_reads_all ON exams.result_publications FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'super_admin');
CREATE POLICY super_admin_reads_all ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'super_admin');
CREATE POLICY super_admin_reads_all ON exams.internal_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'super_admin');
