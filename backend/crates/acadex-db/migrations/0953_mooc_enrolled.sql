-- How many students have enrolled on a MOOC course, past row-level security
-- (a student sees only their own enrolment): once anyone has, its weeks and
-- credits are fixed.
CREATE FUNCTION academic.mooc_enrolled(p_course uuid) RETURNS bigint
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$ SELECT count(*) FROM academic.mooc_enrolments WHERE course_id = p_course $$;
GRANT EXECUTE ON FUNCTION academic.mooc_enrolled(uuid) TO acadex_app;
