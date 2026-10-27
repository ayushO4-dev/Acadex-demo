-- Administration adds the university's programs (until now only the demo
-- data could) and corrects them. A program's code names its attendance
-- partitions, so it never changes; its length can't drop below a semester
-- that students or subjects are already in.

CREATE FUNCTION org.create_program(p_code text, p_name text, p_semesters smallint, p_department smallint)
RETURNS smallint
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_id    smallint;
    v_start smallint;
    v_year  integer;
BEGIN
    PERFORM identity.require_admin();
    LOCK TABLE org.programs IN SHARE ROW EXCLUSIVE MODE;
    v_id := coalesce((SELECT max(id) FROM org.programs), 10) + 1;
    INSERT INTO org.programs (id, institution_id, code, name, semesters, department_id)
    SELECT v_id, u.institution_id, lower(trim(p_code)), trim(p_name), p_semesters, p_department
    FROM identity.users u WHERE u.id = identity.current_user_id();
    -- Attendance for this academic year and the next can be marked at once.
    v_start := coalesce((SELECT academic_year_start_month FROM org.institution_settings LIMIT 1), 7);
    v_year := extract(year FROM current_date)::integer
              - CASE WHEN extract(month FROM current_date) < v_start THEN 1 ELSE 0 END;
    PERFORM academic.ensure_attendance_partition(v_id, v_year);
    PERFORM academic.ensure_attendance_partition(v_id, v_year + 1);
    RETURN v_id;
END $$;

CREATE FUNCTION org.update_program(p_id smallint, p_name text, p_semesters smallint, p_department smallint)
RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_used smallint;
BEGIN
    PERFORM identity.require_admin();
    SELECT greatest((SELECT max(current_semester) FROM people.students WHERE program_id = p_id),
                    (SELECT max(semester) FROM academic.subjects WHERE program_id = p_id))
    INTO v_used;
    IF p_semesters < coalesce(v_used, 0) THEN
        RAISE EXCEPTION 'semester % is already in use', v_used USING ERRCODE = 'check_violation';
    END IF;
    UPDATE org.programs SET name = trim(p_name), semesters = p_semesters, department_id = p_department
    WHERE id = p_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'no such program' USING ERRCODE = 'no_data_found';
    END IF;
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'org.create_program(text, text, smallint, smallint)',
        'org.update_program(smallint, text, smallint, smallint)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;
