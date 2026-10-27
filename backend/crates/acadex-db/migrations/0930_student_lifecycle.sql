-- The student lifecycle after admission: correcting a user's details,
-- promoting a program's students to the next semester at term end, and
-- graduating its final-semester students.

-- How many semesters a program runs (B.Tech 8, M.Tech 4, ...).
ALTER TABLE org.programs ADD COLUMN semesters smallint NOT NULL DEFAULT 8 CHECK (semesters BETWEEN 2 AND 8);

-- A graduate keeps their account and records (grade cards, receipts) but
-- leaves every roster, fee run and semester list.
ALTER TABLE people.students
    ADD COLUMN graduated_on       date,
    ADD COLUMN semester_changed_at timestamptz;
CREATE INDEX students_enrolled ON people.students (program_id, current_semester) WHERE graduated_on IS NULL;

-- One row per promotion run, so a second run by mistake is caught.
CREATE TABLE academic.promotions (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    program_id  smallint NOT NULL REFERENCES org.programs,
    promoted    integer NOT NULL,
    graduated   integer NOT NULL,
    held        integer NOT NULL,
    promoted_by uuid NOT NULL REFERENCES identity.users,
    promoted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON academic.promotions (program_id, promoted_at DESC);
GRANT SELECT ON academic.promotions TO acadex_app;
ALTER TABLE academic.promotions ENABLE ROW LEVEL SECURITY;
CREATE POLICY managers_read ON academic.promotions FOR SELECT TO acadex_app
    USING (academic.may_manage_program(program_id));

-- Moves every enrolled, active student of the program up one semester in a
-- single statement (so nobody moves twice), and graduates those in its last
-- semester. Students in p_hold stay where they are (detained: attendance
-- shortage, or a year back). Heads of department for their programs;
-- administration for all.
CREATE FUNCTION academic.promote_program(p_program smallint, p_hold uuid[])
RETURNS TABLE (promoted integer, graduated integer, held integer)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_last smallint;
    v_promoted integer;
    v_graduated integer;
    v_held integer;
BEGIN
    IF NOT academic.may_manage_program(p_program) THEN
        RAISE EXCEPTION 'not your program' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT semesters INTO v_last FROM org.programs WHERE id = p_program FOR UPDATE;
    IF v_last IS NULL THEN
        RAISE EXCEPTION 'no such program' USING ERRCODE = 'no_data_found';
    END IF;

    WITH moved AS (
        UPDATE people.students st
        SET current_semester    = CASE WHEN st.current_semester < v_last THEN st.current_semester + 1
                                       ELSE st.current_semester END,
            graduated_on        = CASE WHEN st.current_semester >= v_last THEN current_date END,
            semester_changed_at = now()
        FROM identity.users u
        WHERE u.id = st.user_id AND u.active
          AND st.program_id = p_program AND st.graduated_on IS NULL
          AND st.user_id <> ALL (coalesce(p_hold, '{}'))
        RETURNING st.graduated_on
    )
    SELECT count(*) FILTER (WHERE graduated_on IS NULL), count(*) FILTER (WHERE graduated_on IS NOT NULL)
    INTO v_promoted, v_graduated FROM moved;

    SELECT count(*) INTO v_held FROM people.students st JOIN identity.users u ON u.id = st.user_id
    WHERE u.active AND st.program_id = p_program AND st.graduated_on IS NULL
      AND st.user_id = ANY (coalesce(p_hold, '{}'));

    INSERT INTO academic.promotions (program_id, promoted, graduated, held, promoted_by)
    VALUES (p_program, v_promoted, v_graduated, v_held, identity.current_user_id());
    RETURN QUERY SELECT v_promoted, v_graduated, v_held;
END $$;
REVOKE EXECUTE ON FUNCTION academic.promote_program(smallint, uuid[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION academic.promote_program(smallint, uuid[]) TO acadex_app;

-- Everything an administrator can correct about a user, for the edit form.
CREATE FUNCTION identity.admin_user_detail(p_user uuid)
RETURNS TABLE (id uuid, email text, display_name text, role text, active boolean, signed_in boolean,
               program_id smallint, roll_no text, batch_year smallint, semester smallint, section text,
               graduated_on date, designation text)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    PERFORM identity.require_admin();
    RETURN QUERY
        SELECT u.id, u.email, u.display_name, u.role::text, u.active,
               EXISTS (SELECT 1 FROM identity.user_identities i WHERE i.user_id = u.id),
               coalesce(s.program_id, f.program_id), s.roll_no, s.batch_year, s.current_semester, s.section,
               s.graduated_on, f.designation
        FROM identity.users u
        LEFT JOIN people.students s ON s.user_id = u.id
        LEFT JOIN people.faculty f ON f.user_id = u.id
        WHERE u.id = p_user;
END $$;

-- Corrects a user's details. The email can change only until they first sign
-- in: after that their login is tied to it, and it changes in Auth0 first.
-- Fields that don't apply to the user's role are ignored.
CREATE FUNCTION identity.admin_update_user(
    p_user        uuid,
    p_name        text,
    p_email       text,
    p_program     smallint,
    p_roll_no     text,
    p_batch_year  smallint,
    p_semester    smallint,
    p_section     text,
    p_designation text
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_role text;
    v_email text;
    v_last smallint;
BEGIN
    SELECT role::text, email INTO v_role, v_email FROM identity.users WHERE id = p_user FOR UPDATE;
    PERFORM identity.require_admin(v_role IN ('admin', 'super_admin'));
    IF v_role IS NULL THEN
        RAISE EXCEPTION 'no such user' USING ERRCODE = 'no_data_found';
    END IF;
    IF lower(trim(p_email)) <> v_email
       AND EXISTS (SELECT 1 FROM identity.user_identities WHERE user_id = p_user) THEN
        RAISE EXCEPTION 'the email can''t change after the first sign-in' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE identity.users SET display_name = trim(p_name), email = lower(trim(p_email)) WHERE id = p_user;

    IF v_role = 'student' THEN
        SELECT semesters INTO v_last FROM org.programs WHERE id = p_program;
        IF v_last IS NULL OR p_semester NOT BETWEEN 1 AND v_last THEN
            RAISE EXCEPTION 'that program runs semesters 1 to %', coalesce(v_last, 8) USING ERRCODE = 'check_violation';
        END IF;
        UPDATE people.students
        SET program_id = p_program, roll_no = upper(trim(p_roll_no)), batch_year = p_batch_year,
            section = upper(p_section),
            semester_changed_at = CASE WHEN current_semester <> p_semester THEN now() ELSE semester_changed_at END,
            current_semester = p_semester
        WHERE user_id = p_user;
    ELSIF EXISTS (SELECT 1 FROM people.faculty WHERE user_id = p_user) THEN
        UPDATE people.faculty SET program_id = p_program, designation = trim(p_designation) WHERE user_id = p_user;
    END IF;
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'identity.admin_user_detail(uuid)',
        'identity.admin_update_user(uuid, text, text, smallint, text, smallint, smallint, text, text)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;

GRANT SELECT (semesters) ON org.programs TO acadex_app;
GRANT SELECT (graduated_on, semester_changed_at) ON people.students TO acadex_app;
