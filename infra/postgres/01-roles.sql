-- Local database roles, mirroring what Terraform sets up on Cloud SQL.
-- Runs when the Postgres volume is first created, and again on `just db-reset`.
--
--   acadex      schema owner (superuser locally): migrations, seeds, tests
--   acadex_app  the API's privileges: no superuser, no BYPASSRLS, owns nothing,
--               so row-level security always applies
--   acadex_api  the API's login, a member of acadex_app
DO $$
BEGIN
    CREATE ROLE acadex_app NOLOGIN;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$
BEGIN
    CREATE ROLE acadex_api LOGIN PASSWORD 'acadex_api' IN ROLE acadex_app;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
