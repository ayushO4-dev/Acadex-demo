-- The outbox dispatcher: a third system context (see 0230_rls_fees.sql),
-- the only one that reads pending events and marks them sent.
GRANT SELECT, UPDATE (published_at) ON ops.outbox TO acadex_app;

CREATE POLICY dispatcher_reads ON ops.outbox FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'dispatcher');
CREATE POLICY dispatcher_marks_sent ON ops.outbox FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'dispatcher')
    WITH CHECK (identity.current_app_role() = 'dispatcher');
