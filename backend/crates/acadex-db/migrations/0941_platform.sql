-- Acadex is sold to universities: the platform operator (the vendor) and a
-- university's own accounts are different things. A platform operator runs
-- the deployment (operations, dead letters, the audit chain) and onboards the
-- university (its profile and administrators), and sees none of its records:
-- identity.is_admin() is false for them, so every university policy refuses.
-- University administrators, in turn, can't see or change platform accounts.

CREATE FUNCTION identity.is_platform() RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT identity.current_user_id() IS NOT NULL
             AND identity.current_app_role() = 'platform_admin' $$;

-- The university's public profile, kept by the platform operator.
ALTER TABLE org.institutions
    ADD COLUMN short_name    text,
    ADD COLUMN city          text,
    ADD COLUMN website       text,
    ADD COLUMN contact_email text,
    ADD COLUMN updated_at    timestamptz NOT NULL DEFAULT now();

-- Only a platform operator (or a migration or seed, which acts as no app
-- role) may create, change or touch a platform account.
CREATE FUNCTION identity.guard_platform_accounts() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF identity.current_app_role() IS NOT NULL
       AND identity.current_app_role() <> 'platform_admin'
       AND (NEW.role = 'platform_admin' OR (TG_OP = 'UPDATE' AND OLD.role = 'platform_admin')) THEN
        RAISE EXCEPTION 'platform accounts are managed by the platform' USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER guard_platform_accounts BEFORE INSERT OR UPDATE ON identity.users
    FOR EACH ROW EXECUTE FUNCTION identity.guard_platform_accounts();

-- University administrators list university users only.
CREATE OR REPLACE FUNCTION identity.admin_list_users(
    p_search     text,
    p_role       text,
    p_after_name text,
    p_after_id   uuid,
    p_limit      integer
) RETURNS TABLE (id uuid, email text, display_name text, role text, active boolean,
                 signed_in boolean, created_at timestamptz, detail text)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_admin();
    RETURN QUERY
        SELECT u.id, u.email, u.display_name, u.role::text, u.active,
               EXISTS (SELECT 1 FROM identity.user_identities i WHERE i.user_id = u.id),
               u.created_at,
               coalesce(s.roll_no || ' · ' || sp.name || ' · section ' || s.section,
                        f.designation || ' · ' || fp.name)
        FROM identity.users u
        LEFT JOIN people.students s ON s.user_id = u.id
        LEFT JOIN org.programs sp ON sp.id = s.program_id
        LEFT JOIN people.faculty f ON f.user_id = u.id
        LEFT JOIN org.programs fp ON fp.id = f.program_id
        WHERE u.role <> 'platform_admin'
          AND (p_search IS NULL
               OR u.display_name ILIKE '%' || p_search || '%'
               OR u.email ILIKE '%' || p_search || '%'
               OR s.roll_no ILIKE p_search || '%')
          AND (p_role IS NULL OR u.role::text = p_role)
          AND (p_after_name IS NULL OR (u.display_name, u.id) > (p_after_name, p_after_id))
        ORDER BY u.display_name, u.id
        LIMIT least(p_limit, 200);
END $$;

-- Operations move from university administration to the platform.
DROP POLICY admin_handles ON ops.dead_letters;
CREATE POLICY platform_handles ON ops.dead_letters FOR ALL TO acadex_app
    USING (identity.is_platform())
    WITH CHECK (identity.is_platform() AND handled_by = identity.current_user_id());
DROP POLICY admin_reads ON audit.chain_verifications;
CREATE POLICY platform_reads ON audit.chain_verifications FOR SELECT TO acadex_app USING (identity.is_platform());

-- ── The platform console's functions ────────────────────────────────────────

CREATE FUNCTION identity.require_platform() RETURNS void
LANGUAGE plpgsql STABLE
AS $$
BEGIN
    IF NOT identity.is_platform() THEN
        RAISE EXCEPTION 'platform operators only' USING ERRCODE = 'insufficient_privilege';
    END IF;
END $$;

CREATE FUNCTION identity.platform_update_institution(
    p_name text, p_short_name text, p_city text, p_website text, p_contact_email text
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_platform();
    UPDATE org.institutions
    SET name = trim(p_name), short_name = nullif(trim(p_short_name), ''), city = nullif(trim(p_city), ''),
        website = nullif(trim(p_website), ''), contact_email = nullif(lower(trim(p_contact_email)), ''),
        updated_at = now()
    WHERE id = (SELECT institution_id FROM identity.users WHERE id = identity.current_user_id());
END $$;

-- The university's administrators (and its break-glass account).
CREATE FUNCTION identity.platform_list_admins()
RETURNS TABLE (id uuid, email text, display_name text, role text, active boolean, signed_in boolean,
               created_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_platform();
    RETURN QUERY
        SELECT u.id, u.email, u.display_name, u.role::text, u.active,
               EXISTS (SELECT 1 FROM identity.user_identities i WHERE i.user_id = u.id), u.created_at
        FROM identity.users u
        WHERE u.role IN ('admin', 'super_admin')
        ORDER BY u.role, u.display_name;
END $$;

CREATE FUNCTION identity.platform_create_admin(p_email text, p_name text) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_id uuid;
BEGIN
    PERFORM identity.require_platform();
    INSERT INTO identity.users (institution_id, email, display_name, role)
    SELECT u.institution_id, lower(trim(p_email)), trim(p_name), 'admin'
    FROM identity.users u WHERE u.id = identity.current_user_id()
    RETURNING id INTO v_id;
    RETURN v_id;
END $$;

CREATE FUNCTION identity.platform_set_admin_active(p_user uuid, p_active boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_platform();
    UPDATE identity.users
    SET active = p_active, deactivated_at = CASE WHEN p_active THEN NULL ELSE now() END
    WHERE id = p_user AND role IN ('admin', 'super_admin');
    IF NOT FOUND THEN
        RAISE EXCEPTION 'no such administrator' USING ERRCODE = 'no_data_found';
    END IF;
END $$;

-- An administrator's login, for linking a new Auth0 account and for invitations.
CREATE FUNCTION identity.platform_admin_login(p_user uuid)
RETURNS TABLE (subject text, email text, display_name text, active boolean)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_platform();
    RETURN QUERY
        SELECT i.provider_subject, u.email, u.display_name, u.active
        FROM identity.users u LEFT JOIN identity.user_identities i ON i.user_id = u.id
        WHERE u.id = p_user AND u.role IN ('admin', 'super_admin');
END $$;

CREATE FUNCTION identity.platform_link_login(p_user uuid, p_subject text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_platform();
    IF NOT EXISTS (SELECT 1 FROM identity.users WHERE id = p_user AND role IN ('admin', 'super_admin')) THEN
        RAISE EXCEPTION 'no such administrator' USING ERRCODE = 'no_data_found';
    END IF;
    INSERT INTO identity.user_identities (provider_subject, user_id) VALUES (p_subject, p_user);
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'identity.platform_update_institution(text, text, text, text, text)',
        'identity.platform_list_admins()',
        'identity.platform_create_admin(text, text)',
        'identity.platform_set_admin_active(uuid, boolean)',
        'identity.platform_admin_login(uuid)',
        'identity.platform_link_login(uuid, text)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;

-- Accounts per role, for the platform's view of the deployment (licensing):
-- counts only, no names.
CREATE FUNCTION identity.platform_usage() RETURNS TABLE (role text, users bigint, active bigint)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_platform();
    RETURN QUERY
        SELECT u.role::text, count(*), count(*) FILTER (WHERE u.active)
        FROM identity.users u WHERE u.role <> 'platform_admin'
        GROUP BY u.role ORDER BY u.role;
END $$;
REVOKE EXECUTE ON FUNCTION identity.platform_usage() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.platform_usage() TO acadex_app;
