-- Access to notices. Same conventions as 0200_rls_policies.sql: grants and
-- policies for a schema live together in one 02xx file.

GRANT USAGE ON SCHEMA comms TO acadex_app;

-- A new notice takes its institution from the author.
GRANT SELECT (institution_id) ON identity.users TO acadex_app;

-- ── comms.notices ───────────────────────────────────────────────────────────
GRANT SELECT, INSERT ON comms.notices TO acadex_app;
ALTER TABLE comms.notices ENABLE ROW LEVEL SECURITY;

-- Current institution-wide notices are public: this policy needs no acting
-- user, which is what lets the public notice board read them.
CREATE POLICY everyone_reads_current_general ON comms.notices FOR SELECT TO acadex_app
    USING (program_id IS NULL
           AND published_at <= now()
           AND (expires_at IS NULL OR expires_at > now()));

CREATE POLICY members_read_current_program ON comms.notices FOR SELECT TO acadex_app
    USING (published_at <= now()
           AND (expires_at IS NULL OR expires_at > now())
           AND program_id IN (SELECT program_id FROM people.students WHERE user_id = identity.current_user_id()
                              UNION ALL
                              SELECT program_id FROM people.faculty WHERE user_id = identity.current_user_id()));

-- The exam cell also sees scheduled and expired notices.
CREATE POLICY exam_cell_reads_all ON comms.notices FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'exam_cell');

CREATE POLICY exam_cell_publishes ON comms.notices FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'exam_cell'
                AND created_by = identity.current_user_id());

-- ── comms.notice_reads ──────────────────────────────────────────────────────
GRANT SELECT, INSERT ON comms.notice_reads TO acadex_app;
ALTER TABLE comms.notice_reads ENABLE ROW LEVEL SECURITY;

CREATE POLICY own_read_state ON comms.notice_reads FOR ALL TO acadex_app
    USING (user_id = identity.current_user_id())
    WITH CHECK (user_id = identity.current_user_id());
