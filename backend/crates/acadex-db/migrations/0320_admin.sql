-- User administration. SECURITY DEFINER, like resolve_subject: the app role
-- still can't read emails or write identity.users directly, and every
-- function checks that the acting user is an administrator.

CREATE FUNCTION identity.is_admin() RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT identity.current_user_id() IS NOT NULL
             AND identity.current_app_role() IN ('admin', 'super_admin') $$;

CREATE FUNCTION identity.require_admin(p_for_privileged boolean DEFAULT false) RETURNS void
LANGUAGE plpgsql STABLE
AS $$
BEGIN
    IF NOT identity.is_admin() THEN
        RAISE EXCEPTION 'administrators only' USING ERRCODE = 'insufficient_privilege';
    END IF;
    -- Making or changing an administrator takes a super administrator.
    IF p_for_privileged AND identity.current_app_role() <> 'super_admin' THEN
        RAISE EXCEPTION 'super administrators only' USING ERRCODE = 'insufficient_privilege';
    END IF;
END $$;

CREATE FUNCTION identity.admin_list_users(
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
        WHERE (p_search IS NULL
               OR u.display_name ILIKE '%' || p_search || '%'
               OR u.email ILIKE '%' || p_search || '%'
               OR s.roll_no ILIKE p_search || '%')
          AND (p_role IS NULL OR u.role::text = p_role)
          -- Keyset pagination by name (rule R2: no OFFSET).
          AND (p_after_name IS NULL OR (u.display_name, u.id) > (p_after_name, p_after_id))
        ORDER BY u.display_name, u.id
        LIMIT least(p_limit, 200);
END $$;

-- Creates a user, with the student or staff record their role needs.
CREATE FUNCTION identity.admin_create_user(
    p_email       text,
    p_name        text,
    p_role        text,
    p_program     smallint,   -- students: their program; faculty and HODs: home program
    p_roll_no     text,
    p_batch_year  smallint,
    p_semester    smallint,
    p_section     text,
    p_designation text,       -- faculty and HODs
    p_department  smallint    -- HODs: the department they head
) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_id uuid;
BEGIN
    PERFORM identity.require_admin(p_role IN ('admin', 'super_admin'));
    IF p_role = 'student' AND (p_program IS NULL OR p_roll_no IS NULL OR p_batch_year IS NULL
                               OR p_semester IS NULL OR p_section IS NULL) THEN
        RAISE EXCEPTION 'a student needs program, roll number, batch, semester and section'
            USING ERRCODE = 'check_violation';
    END IF;
    IF p_role IN ('faculty', 'hod') AND (p_program IS NULL OR p_designation IS NULL) THEN
        RAISE EXCEPTION 'faculty and HODs need a home program and a designation'
            USING ERRCODE = 'check_violation';
    END IF;
    IF p_role = 'hod' AND p_department IS NULL THEN
        RAISE EXCEPTION 'an HOD needs a department' USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO identity.users (institution_id, email, display_name, role)
    SELECT u.institution_id, lower(trim(p_email)), trim(p_name), p_role::identity.user_role
    FROM identity.users u WHERE u.id = identity.current_user_id()
    RETURNING id INTO v_id;

    IF p_role = 'student' THEN
        INSERT INTO people.students (user_id, program_id, roll_no, batch_year, current_semester, section)
        VALUES (v_id, p_program, upper(trim(p_roll_no)), p_batch_year, p_semester, upper(p_section));
    ELSIF p_role IN ('faculty', 'hod') THEN
        INSERT INTO people.faculty (user_id, program_id, designation) VALUES (v_id, p_program, p_designation);
    END IF;
    IF p_role = 'hod' THEN
        UPDATE org.departments SET hod_id = v_id WHERE id = p_department;
    END IF;
    RETURN v_id;
END $$;

-- Staff roles only: a student's role is never changed, and nobody becomes a
-- student this way (their records would be missing).
CREATE FUNCTION identity.admin_set_role(p_user uuid, p_role text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_current text;
BEGIN
    SELECT role::text INTO v_current FROM identity.users WHERE id = p_user FOR UPDATE;
    PERFORM identity.require_admin(p_role IN ('admin', 'super_admin')
                                   OR v_current IN ('admin', 'super_admin'));
    IF v_current IS NULL THEN
        RAISE EXCEPTION 'no such user' USING ERRCODE = 'no_data_found';
    END IF;
    IF p_user = identity.current_user_id() THEN
        RAISE EXCEPTION 'you can''t change your own role' USING ERRCODE = 'check_violation';
    END IF;
    IF v_current = 'student' OR p_role = 'student' THEN
        RAISE EXCEPTION 'student roles can''t be changed' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE identity.users SET role = p_role::identity.user_role WHERE id = p_user;
END $$;

CREATE FUNCTION identity.admin_set_active(p_user uuid, p_active boolean) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_role text;
BEGIN
    SELECT role::text INTO v_role FROM identity.users WHERE id = p_user FOR UPDATE;
    PERFORM identity.require_admin(v_role IN ('admin', 'super_admin'));
    IF v_role IS NULL THEN
        RAISE EXCEPTION 'no such user' USING ERRCODE = 'no_data_found';
    END IF;
    IF p_user = identity.current_user_id() THEN
        RAISE EXCEPTION 'you can''t deactivate yourself' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE identity.users
    SET active = p_active, deactivated_at = CASE WHEN p_active THEN NULL ELSE now() END
    WHERE id = p_user;
END $$;

-- An account created in Auth0 by the API is linked straight away, so the
-- user's first sign-in needs no email match.
CREATE FUNCTION identity.admin_link_login(p_user uuid, p_subject text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_admin();
    INSERT INTO identity.user_identities (provider_subject, user_id) VALUES (p_subject, p_user);
END $$;

-- The login to block or unblock in Auth0 when a user is deactivated or
-- reactivated: their linked subject, else their email.
CREATE FUNCTION identity.admin_login_of(p_user uuid) RETURNS TABLE (subject text, email text)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_admin();
    RETURN QUERY
        SELECT i.provider_subject, u.email
        FROM identity.users u LEFT JOIN identity.user_identities i ON i.user_id = u.id
        WHERE u.id = p_user;
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'identity.admin_list_users(text, text, text, uuid, integer)',
        'identity.admin_create_user(text, text, text, smallint, text, smallint, smallint, text, text, smallint)',
        'identity.admin_set_role(uuid, text)',
        'identity.admin_set_active(uuid, boolean)',
        'identity.admin_link_login(uuid, text)',
        'identity.admin_login_of(uuid)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;
