-- The API's privileges. Its login (acadex_api locally; a Cloud SQL IAM user in
-- production) is a member of acadex_app, which owns nothing and lacks BYPASSRLS,
-- so row-level security always applies. Grants are in 0200_rls_policies.sql.
-- Concurrent test databases may race to create it, hence both exceptions.
DO $$
BEGIN
    CREATE ROLE acadex_app NOLOGIN;
EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
END $$;

CREATE SCHEMA org;       -- institution, programs
CREATE SCHEMA identity;  -- users, Auth0 links, session helpers
CREATE SCHEMA people;    -- students, faculty
CREATE SCHEMA academic;  -- subjects, offerings, attendance
CREATE SCHEMA exams;     -- results (Phase 2)
CREATE SCHEMA fees;      -- ledger, receipts (Phase 2)
CREATE SCHEMA comms;     -- notices (Phase 2)
CREATE SCHEMA audit;     -- hash-chained action log
CREATE SCHEMA ops;       -- outbox, idempotency keys (Phase 2)

-- The acting user, set per transaction by the API with
-- set_config('app.user_id', ..., true). NULL when unset, so policies fail closed;
-- nullif covers the '' a pooled connection reports once a setting has been reset.
CREATE FUNCTION identity.current_user_id() RETURNS uuid
LANGUAGE sql STABLE
AS $$ SELECT nullif(current_setting('app.user_id', true), '')::uuid $$;

CREATE FUNCTION identity.current_app_role() RETURNS text
LANGUAGE sql STABLE
AS $$ SELECT nullif(current_setting('app.role', true), '') $$;
