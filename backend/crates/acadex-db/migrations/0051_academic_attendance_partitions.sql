CREATE TYPE academic.attendance_status AS ENUM ('present', 'absent', 'late', 'excused');

-- The highest-volume table, partitioned by program and then by academic year
-- (architecture.md §6.3), so "my attendance this year" scans one partition.
CREATE TABLE academic.attendance (
    program_id    smallint NOT NULL REFERENCES org.programs,
    academic_year smallint NOT NULL,
    student_id    uuid NOT NULL REFERENCES people.students (user_id),
    offering_id   uuid NOT NULL REFERENCES academic.offerings,
    session_date  date NOT NULL,
    slot          smallint NOT NULL,
    status        academic.attendance_status NOT NULL,
    marked_by     uuid NOT NULL REFERENCES identity.users,
    marked_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (program_id, academic_year, student_id, offering_id, session_date, slot)
) PARTITION BY LIST (program_id);

CREATE INDEX ON academic.attendance (offering_id, session_date);

-- Creates the program partition and its academic-year sub-partition if missing,
-- e.g. academic.attendance_btech_ece_2026. Called by the seeds now and by the
-- weekly partition.maintain job later. Partitions get no grants of their own, so
-- the app role can only reach rows through the parent, where RLS applies.
CREATE FUNCTION academic.ensure_attendance_partition(p_program_id integer, p_academic_year integer)
RETURNS text
LANGUAGE plpgsql
AS $$
DECLARE
    v_program_table text;
    v_year_table    text;
BEGIN
    SELECT 'attendance_' || code INTO STRICT v_program_table FROM org.programs WHERE id = p_program_id;
    v_year_table := v_program_table || '_' || p_academic_year;

    IF to_regclass('academic.' || v_program_table) IS NULL THEN
        EXECUTE format(
            'CREATE TABLE academic.%I PARTITION OF academic.attendance
                 FOR VALUES IN (%s) PARTITION BY RANGE (academic_year)',
            v_program_table, p_program_id);
    END IF;

    IF to_regclass('academic.' || v_year_table) IS NULL THEN
        EXECUTE format(
            'CREATE TABLE academic.%I PARTITION OF academic.%I FOR VALUES FROM (%s) TO (%s)',
            v_year_table, v_program_table, p_academic_year, p_academic_year + 1);
    END IF;

    RETURN 'academic.' || v_year_table;
END $$;
