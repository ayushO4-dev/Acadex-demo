-- F6: the remaining jobs of architecture.md §11.2 and the dead-letter store
-- of §11.3. Each job runs in the worker's system context; where it needs
-- more than the app role may do (create partitions, read every audit row),
-- a SECURITY DEFINER function does exactly that and refuses anyone else.

-- ── Dead letters ─────────────────────────────────────────────────────────────
-- Pub/Sub redelivers a failed push with backoff. The worker counts failures
-- per message; after five, the message is parked here and acknowledged, so
-- one poison message can't retry forever. Administration replays (back into
-- the outbox) or discards it. On Google Cloud the subscription's own
-- dead-letter topic is a backstop behind this.
CREATE TABLE ops.delivery_failures (
    message_id text PRIMARY KEY,
    attempts   integer NOT NULL,
    last_error text NOT NULL,
    first_at   timestamptz NOT NULL DEFAULT now(),
    last_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE ops.dead_letters (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    message_id  text NOT NULL UNIQUE,
    event       text NOT NULL,
    payload     jsonb NOT NULL,
    attributes  jsonb NOT NULL DEFAULT '{}',
    attempts    integer NOT NULL,
    last_error  text NOT NULL,
    first_failed_at timestamptz NOT NULL,
    dead_at     timestamptz NOT NULL DEFAULT now(),
    status      text NOT NULL DEFAULT 'dead' CHECK (status IN ('dead', 'replayed', 'discarded')),
    handled_by  uuid REFERENCES identity.users,
    handled_at  timestamptz,
    note        text CHECK (length(note) <= 500)
);
CREATE INDEX ON ops.dead_letters (status, dead_at DESC);

GRANT SELECT, INSERT, UPDATE, DELETE ON ops.delivery_failures TO acadex_app;
ALTER TABLE ops.delivery_failures ENABLE ROW LEVEL SECURITY;
CREATE POLICY worker_counts ON ops.delivery_failures FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'worker')
    WITH CHECK (identity.current_app_role() = 'worker');

GRANT SELECT, INSERT ON ops.dead_letters TO acadex_app;
GRANT UPDATE (status, handled_by, handled_at, note) ON ops.dead_letters TO acadex_app;
ALTER TABLE ops.dead_letters ENABLE ROW LEVEL SECURITY;
CREATE POLICY worker_parks ON ops.dead_letters FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'worker');
CREATE POLICY worker_reads ON ops.dead_letters FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');
CREATE POLICY admin_handles ON ops.dead_letters FOR ALL TO acadex_app
    USING (identity.is_admin())
    WITH CHECK (identity.is_admin() AND handled_by = identity.current_user_id());
-- Replaying puts the event back in the outbox, which any role may append to.

-- ── audit.verify_chain, nightly ─────────────────────────────────────────────
-- Each run walks the whole chain and records where it ended. The next run
-- also checks that row is still there, unchanged: deleting the newest rows
-- (which the walk alone can't notice) shows as a break at the old head.
CREATE TABLE audit.chain_verifications (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    verified_at     timestamptz NOT NULL DEFAULT now(),
    rows_checked    bigint NOT NULL,
    head_id         bigint,
    head_hash       bytea,
    first_broken_id bigint,
    detail          text
);
GRANT SELECT ON audit.chain_verifications TO acadex_app;
ALTER TABLE audit.chain_verifications ENABLE ROW LEVEL SECURITY;
CREATE POLICY admin_reads ON audit.chain_verifications FOR SELECT TO acadex_app USING (identity.is_admin());

CREATE FUNCTION audit.verify_and_checkpoint() RETURNS audit.chain_verifications
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_prev   audit.chain_verifications;
    v_walk   record;
    v_head   audit.action_log;
    v_detail text;
    v_broken bigint;
    v_out    audit.chain_verifications;
BEGIN
    IF identity.current_app_role() IS DISTINCT FROM 'worker' THEN
        RAISE EXCEPTION 'audit.verify_and_checkpoint is for the worker' USING ERRCODE = 'insufficient_privilege';
    END IF;

    SELECT * INTO v_walk FROM audit.verify_chain();
    v_broken := v_walk.first_broken_id;
    IF v_broken IS NOT NULL THEN
        v_detail := 'row ' || v_broken || ' does not match its hash or back-link';
    END IF;

    SELECT * INTO v_prev FROM audit.chain_verifications
    WHERE first_broken_id IS NULL AND head_id IS NOT NULL ORDER BY id DESC LIMIT 1;
    IF v_broken IS NULL AND FOUND
       AND NOT EXISTS (SELECT 1 FROM audit.action_log l WHERE l.id = v_prev.head_id AND l.row_hash = v_prev.head_hash) THEN
        v_broken := v_prev.head_id;
        v_detail := 'the last verified row ' || v_prev.head_id || ' was removed or changed';
    END IF;

    SELECT * INTO v_head FROM audit.action_log ORDER BY id DESC LIMIT 1;
    INSERT INTO audit.chain_verifications (rows_checked, head_id, head_hash, first_broken_id, detail)
    VALUES (v_walk.rows_checked, v_head.id, v_head.row_hash, v_broken, v_detail)
    RETURNING * INTO v_out;
    RETURN v_out;
END $$;
GRANT EXECUTE ON FUNCTION audit.verify_and_checkpoint() TO acadex_app;

-- ── partition.maintain, weekly ──────────────────────────────────────────────
-- Attendance partitions for every program, this academic year and next,
-- created ahead so marking never fails on a missing partition.
CREATE FUNCTION academic.maintain_partitions(p_years integer[]) RETURNS text[]
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_created text[] := '{}';
    v_program integer;
    v_year    integer;
    v_table   text;
BEGIN
    IF identity.current_app_role() IS DISTINCT FROM 'worker' THEN
        RAISE EXCEPTION 'academic.maintain_partitions is for the worker' USING ERRCODE = 'insufficient_privilege';
    END IF;
    FOR v_program IN SELECT id FROM org.programs ORDER BY id LOOP
        FOREACH v_year IN ARRAY p_years LOOP
            SELECT 'attendance_' || code || '_' || v_year INTO v_table FROM org.programs WHERE id = v_program;
            IF to_regclass('academic.' || v_table) IS NULL THEN
                PERFORM academic.ensure_attendance_partition(v_program, v_year);
                v_created := v_created || v_table;
            END IF;
        END LOOP;
    END LOOP;
    RETURN v_created;
END $$;
GRANT EXECUTE ON FUNCTION academic.maintain_partitions(integer[]) TO acadex_app;

-- ── attendance.rollup, hourly and after marking ─────────────────────────────
-- Held and attended sessions per student, offering and kind, so a whole
-- department's shortages are one cheap query instead of an aggregate over
-- the largest table. A student's own page still reads live.
CREATE TABLE academic.attendance_rollup (
    program_id    smallint NOT NULL REFERENCES org.programs,
    academic_year smallint NOT NULL,
    student_id    uuid NOT NULL REFERENCES people.students (user_id) ON DELETE CASCADE,
    offering_id   uuid NOT NULL REFERENCES academic.offerings,
    session_kind  text NOT NULL CHECK (session_kind IN ('theory', 'practical')),
    conducted     integer NOT NULL,
    attended      integer NOT NULL,
    refreshed_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (student_id, offering_id, session_kind)
);
CREATE INDEX ON academic.attendance_rollup (program_id, academic_year);

GRANT SELECT ON academic.attendance_rollup TO acadex_app;
ALTER TABLE academic.attendance_rollup ENABLE ROW LEVEL SECURITY;
CREATE POLICY hod_reads_department ON academic.attendance_rollup FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND program_id IN (SELECT org.my_department_programs()));
CREATE POLICY staff_reads_all ON academic.attendance_rollup FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin', 'worker'));

CREATE FUNCTION academic.refresh_attendance_rollup(p_program_id integer, p_academic_year integer) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_rows bigint;
BEGIN
    IF identity.current_app_role() IS DISTINCT FROM 'worker' THEN
        RAISE EXCEPTION 'academic.refresh_attendance_rollup is for the worker' USING ERRCODE = 'insufficient_privilege';
    END IF;
    INSERT INTO academic.attendance_rollup (program_id, academic_year, student_id, offering_id, session_kind,
                                            conducted, attended, refreshed_at)
    SELECT a.program_id, a.academic_year, a.student_id, a.offering_id, a.session_kind,
           count(*), count(*) FILTER (WHERE a.status IN ('present', 'late')), now()
    FROM academic.attendance a
    WHERE a.program_id = p_program_id AND a.academic_year = p_academic_year
    GROUP BY a.program_id, a.academic_year, a.student_id, a.offering_id, a.session_kind
    ON CONFLICT (student_id, offering_id, session_kind)
    DO UPDATE SET conducted = EXCLUDED.conducted, attended = EXCLUDED.attended, refreshed_at = now();
    GET DIAGNOSTICS v_rows = ROW_COUNT;
    RETURN v_rows;
END $$;
GRANT EXECUTE ON FUNCTION academic.refresh_attendance_rollup(integer, integer) TO acadex_app;
