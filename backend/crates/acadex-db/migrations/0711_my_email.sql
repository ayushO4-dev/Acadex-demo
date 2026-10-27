-- The signed-in user's own email, for the sidebar. Emails stay column-
-- restricted (0200); this returns only the caller's.
CREATE FUNCTION identity.my_email() RETURNS text
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$ SELECT email FROM identity.users WHERE id = identity.current_user_id() $$;
REVOKE EXECUTE ON FUNCTION identity.my_email() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.my_email() TO acadex_app;
