-- Administration sets the support desk's response and resolution targets
-- per priority. New tickets take them; open ones keep the deadlines they
-- were given.
GRANT UPDATE (first_response_minutes, resolve_minutes) ON support.sla_policy TO acadex_app;
ALTER TABLE support.sla_policy ENABLE ROW LEVEL SECURITY;
CREATE POLICY everyone_reads ON support.sla_policy FOR SELECT TO acadex_app USING (true);
CREATE POLICY admin_sets ON support.sla_policy FOR UPDATE TO acadex_app
    USING (identity.is_admin()) WITH CHECK (identity.is_admin());
