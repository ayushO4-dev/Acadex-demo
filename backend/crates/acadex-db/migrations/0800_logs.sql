-- Logs (architecture.md §9.8): authentication events, and filtering by date.

-- Authentication events are kept in the hash-chained action log with
-- everything else, so they can't be quietly edited either. This view is how
-- to ask for them alone; security_invoker keeps the log's row-level security.
CREATE VIEW audit.auth_events WITH (security_invoker = true) AS
    SELECT id, occurred_at, actor_id, action, outcome, ip, user_agent, metadata
    FROM audit.action_log
    WHERE action LIKE 'auth.%' OR action = 'identity.linked';
GRANT SELECT ON audit.auth_events TO acadex_app;

-- Sign-in failures, lockouts and password changes happen at Auth0, not here.
-- A worker job pulls Auth0's tenant log and records each event against the
-- user it concerns; this is where it got to.
CREATE TABLE audit.auth0_log_cursor (
    singleton   boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    last_log_id text NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now()
);
GRANT SELECT, INSERT, UPDATE ON audit.auth0_log_cursor TO acadex_app;
ALTER TABLE audit.auth0_log_cursor ENABLE ROW LEVEL SECURITY;
CREATE POLICY worker_only ON audit.auth0_log_cursor FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'worker')
    WITH CHECK (identity.current_app_role() = 'worker');

-- Each Auth0 event is recorded once, even if the job is interrupted between
-- recording it and moving the cursor.
CREATE UNIQUE INDEX action_log_auth0_event ON audit.action_log ((metadata->>'auth0_log_id'))
    WHERE metadata ? 'auth0_log_id';

-- Administration filters everyone's actions by date range.
CREATE INDEX ON audit.action_log (occurred_at);
