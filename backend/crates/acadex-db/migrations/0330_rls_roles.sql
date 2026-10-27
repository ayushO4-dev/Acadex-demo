-- Row-level access for the roles added in 0300 (architecture.md §7.3). Same
-- conventions as 0200_rls_policies.sql. `super_admin` gets what `admin` gets;
-- its every request is also audited by the API.

-- Programs in the acting HOD's department.
CREATE FUNCTION org.my_department_programs() RETURNS SETOF smallint
LANGUAGE sql STABLE
AS $$
    SELECT p.id FROM org.programs p
    JOIN org.departments d ON d.id = p.department_id
    WHERE d.hod_id = identity.current_user_id()
$$;
GRANT EXECUTE ON FUNCTION org.my_department_programs() TO acadex_app;
GRANT EXECUTE ON FUNCTION identity.is_admin() TO acadex_app;

-- ── Reference data and settings ─────────────────────────────────────────────
GRANT SELECT ON org.departments, org.institution_settings TO acadex_app;
GRANT INSERT, UPDATE ON org.departments TO acadex_app;
GRANT UPDATE (attendance_threshold_pct, academic_year_start_month, results_appeal_days,
              updated_at, updated_by) ON org.institution_settings TO acadex_app;
GRANT INSERT, UPDATE, DELETE ON exams.grade_scale TO acadex_app;
GRANT UPDATE (external_min_pct) ON exams.grading_policy TO acadex_app;
GRANT SELECT (active) ON identity.users TO acadex_app;

-- Programs: readable by all; only administration moves one to a department.
GRANT UPDATE (department_id) ON org.programs TO acadex_app;
ALTER TABLE org.programs ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON org.programs FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_updates ON org.programs FOR UPDATE TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin());

ALTER TABLE org.departments ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON org.departments FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_manages ON org.departments FOR INSERT TO acadex_app WITH CHECK (identity.is_admin());
CREATE POLICY admin_updates ON org.departments FOR UPDATE TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin());

ALTER TABLE org.institution_settings ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON org.institution_settings FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_updates ON org.institution_settings FOR UPDATE TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin() AND updated_by = identity.current_user_id());

ALTER TABLE exams.grade_scale ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON exams.grade_scale FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_manages ON exams.grade_scale FOR ALL TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin());
ALTER TABLE exams.grading_policy ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON exams.grading_policy FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_updates ON exams.grading_policy FOR UPDATE TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin());

-- ── People ──────────────────────────────────────────────────────────────────
CREATE POLICY hod_reads_department ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs()));
-- Accounts, support and administration look students up across the institution.
CREATE POLICY staff_reads_all ON people.students FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('accounts', 'support_agent', 'admin', 'super_admin'));

-- ── Attendance and results: an HOD reads their department ───────────────────
CREATE POLICY hod_reads_department ON academic.attendance FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs()));

CREATE POLICY hod_reads_department_published ON exams.external_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod'
           AND exams.offering_published(offering_id)
           AND offering_id IN (SELECT o.id FROM academic.offerings o
                               JOIN academic.subjects s ON s.id = o.subject_id
                               WHERE s.program_id IN (SELECT org.my_department_programs())));
CREATE POLICY hod_reads_department_published ON exams.internal_marks FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod'
           AND component_id IN (SELECT c.id FROM exams.internal_components c
                                JOIN academic.offerings o ON o.id = c.offering_id
                                JOIN academic.subjects s ON s.id = o.subject_id
                                WHERE s.program_id IN (SELECT org.my_department_programs())
                                  AND exams.offering_published(o.id)));

-- ── Fees: the accounts office reads every ledger ────────────────────────────
CREATE POLICY accounts_reads_all ON fees.ledger FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'accounts');
CREATE POLICY accounts_reads_all ON fees.payments FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'accounts');
CREATE POLICY accounts_reads_all ON fees.receipts FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'accounts');
CREATE POLICY accounts_reads_all ON fees.payment_attempts FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'accounts');

-- ── Notices: administration publishes too ───────────────────────────────────
CREATE POLICY admin_reads_all ON comms.notices FOR SELECT TO acadex_app USING (identity.is_admin());
CREATE POLICY admin_publishes ON comms.notices FOR INSERT TO acadex_app
    WITH CHECK (identity.is_admin() AND created_by = identity.current_user_id());

-- ── Audit: institution-wide reading belongs to administration (§9.8) ────────
-- The exam cell read everyone's log in Phase 2; architecture.md gives that
-- to admin. Everyone still reads their own.
DROP POLICY exam_cell_reads_all ON audit.action_log;
CREATE POLICY admin_reads_all ON audit.action_log FOR SELECT TO acadex_app USING (identity.is_admin());
