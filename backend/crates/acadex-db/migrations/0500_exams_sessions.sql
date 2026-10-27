-- F3, Examination (architecture.md §9.3): sessions, timetable, venues,
-- seating, and exam forms paid through the fee ledger.

-- What a ledger charge is for, so a form can find its fee without matching
-- on text. Charges made before this have NULL until a seed or job sets them.
ALTER TABLE fees.ledger
    ADD COLUMN fee_head text CHECK (fee_head IN ('tuition', 'library', 'activities', 'exam', 'backlog_exam', 'revaluation', 'other'));

CREATE TABLE exams.exam_sessions (
    id                        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    institution_id            smallint NOT NULL REFERENCES org.institutions,
    name                      text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    academic_year             smallint NOT NULL,
    form_opens_on             date NOT NULL,
    form_closes_on            date NOT NULL CHECK (form_closes_on >= form_opens_on),
    regular_fee_paise         bigint NOT NULL CHECK (regular_fee_paise >= 0),
    backlog_fee_paise         bigint NOT NULL CHECK (backlog_fee_paise >= 0),   -- per subject
    hall_tickets_generated_at timestamptz,
    hall_tickets_released_at  timestamptz,
    released_by               uuid REFERENCES identity.users,
    created_by                uuid REFERENCES identity.users,
    created_at                timestamptz NOT NULL DEFAULT now(),
    CHECK (hall_tickets_released_at IS NULL OR hall_tickets_generated_at IS NOT NULL)
);

CREATE TABLE exams.venues (
    id             smallint PRIMARY KEY,
    institution_id smallint NOT NULL REFERENCES org.institutions,
    name           text NOT NULL UNIQUE,
    capacity       smallint NOT NULL CHECK (capacity BETWEEN 1 AND 500)
);

-- The timetable: one paper per subject per session.
CREATE TABLE exams.schedule (
    session_id       uuid NOT NULL REFERENCES exams.exam_sessions ON DELETE CASCADE,
    subject_id       uuid NOT NULL REFERENCES academic.subjects,
    exam_date        date NOT NULL,
    starts_at        time NOT NULL,
    duration_minutes smallint NOT NULL CHECK (duration_minutes BETWEEN 30 AND 360),
    PRIMARY KEY (session_id, subject_id)
);

-- Immutable once submitted: the record of what a student registered for.
CREATE TABLE exams.exam_forms (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id     uuid NOT NULL REFERENCES exams.exam_sessions,
    student_id     uuid NOT NULL REFERENCES people.students,
    kind           text NOT NULL CHECK (kind IN ('regular', 'backlog')),
    fee_paise      bigint NOT NULL CHECK (fee_paise >= 0),
    status         text NOT NULL CHECK (status IN ('payment_pending', 'submitted')),
    submitted_at   timestamptz,
    ack_pdf_object text,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (session_id, student_id, kind),
    CHECK ((status = 'submitted') = (submitted_at IS NOT NULL))
);

CREATE TABLE exams.exam_form_subjects (
    form_id     uuid NOT NULL REFERENCES exams.exam_forms ON DELETE CASCADE,
    subject_id  uuid NOT NULL REFERENCES academic.subjects,
    -- The offering whose result this attempt counts towards.
    offering_id uuid NOT NULL REFERENCES academic.offerings,
    PRIMARY KEY (form_id, subject_id)
);

-- The ledger charges that pay for a form. When none is pending, the form is submitted.
CREATE TABLE exams.exam_form_charges (
    form_id   uuid NOT NULL REFERENCES exams.exam_forms ON DELETE CASCADE,
    charge_id bigint NOT NULL REFERENCES fees.ledger,
    PRIMARY KEY (form_id, charge_id)
);

CREATE TABLE exams.seating (
    session_id uuid NOT NULL REFERENCES exams.exam_sessions ON DELETE CASCADE,
    student_id uuid NOT NULL REFERENCES people.students,
    venue_id   smallint NOT NULL REFERENCES exams.venues,
    seat_no    smallint NOT NULL CHECK (seat_no > 0),
    PRIMARY KEY (session_id, student_id),
    UNIQUE (session_id, venue_id, seat_no)
);

-- Pending amount of one ledger charge (charges minus concessions and net payments).
CREATE FUNCTION fees.pending_of(p_charge bigint) RETURNS bigint
LANGUAGE sql STABLE
AS $$
    SELECT c.amount_paise
           - coalesce(sum(e.amount_paise) FILTER (WHERE e.kind IN ('concession', 'payment')), 0)
           + coalesce(sum(e.amount_paise) FILTER (WHERE e.kind = 'refund'), 0)
    FROM fees.ledger c LEFT JOIN fees.ledger e ON e.charge_id = c.id
    WHERE c.id = p_charge
    GROUP BY c.id, c.amount_paise
$$;

-- Registers the acting student for a session. The API has already checked
-- eligibility and which subjects qualify (it holds the grading rules); this
-- enforces the rest: only the student themselves, only while the window is
-- open, once per kind, and fees taken from the session, never the caller.
-- Charges the fee to the ledger (reusing an existing exam-fee charge for the
-- year) and submits at once if nothing is left to pay.
CREATE FUNCTION exams.create_form(p_session uuid, p_kind text, p_subjects uuid[], p_offerings uuid[])
RETURNS TABLE (form_id uuid, status text, pending_charges bigint[])
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_student uuid := identity.current_user_id();
    v_session exams.exam_sessions;
    v_form    uuid;
    v_charge  bigint;
    v_fee     bigint := 0;
    v_pending bigint[];
    i         integer;
BEGIN
    IF identity.current_app_role() <> 'student' OR v_student IS NULL THEN
        RAISE EXCEPTION 'only students register for exams' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT * INTO v_session FROM exams.exam_sessions WHERE id = p_session;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'no such exam session' USING ERRCODE = 'no_data_found';
    END IF;
    IF current_date NOT BETWEEN v_session.form_opens_on AND v_session.form_closes_on THEN
        RAISE EXCEPTION 'the form window is closed' USING ERRCODE = 'check_violation';
    END IF;
    IF p_kind NOT IN ('regular', 'backlog') OR coalesce(array_length(p_subjects, 1), 0) = 0
       OR array_length(p_subjects, 1) <> array_length(p_offerings, 1) THEN
        RAISE EXCEPTION 'choose at least one subject' USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO exams.exam_forms (session_id, student_id, kind, fee_paise, status)
    VALUES (p_session, v_student, p_kind, 0, 'payment_pending')
    RETURNING id INTO v_form;
    FOR i IN 1 .. array_length(p_subjects, 1) LOOP
        INSERT INTO exams.exam_form_subjects (form_id, subject_id, offering_id)
        VALUES (v_form, p_subjects[i], p_offerings[i]);
    END LOOP;

    IF p_kind = 'regular' THEN
        -- The year's exam fee may already be on the ledger (and even paid).
        SELECT id INTO v_charge FROM fees.ledger
        WHERE student_id = v_student AND kind = 'charge' AND fee_head = 'exam'
          AND academic_year = v_session.academic_year
        ORDER BY id LIMIT 1;
        IF v_charge IS NULL AND v_session.regular_fee_paise > 0 THEN
            INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, fee_head, due_on)
            VALUES (v_student, v_session.academic_year, 'charge', 'Examination fee — ' || v_session.name,
                    v_session.regular_fee_paise, 'exam', v_session.form_closes_on)
            RETURNING id INTO v_charge;
        END IF;
        IF v_charge IS NOT NULL THEN
            INSERT INTO exams.exam_form_charges VALUES (v_form, v_charge);
            v_fee := (SELECT amount_paise FROM fees.ledger WHERE id = v_charge);
        END IF;
    ELSIF v_session.backlog_fee_paise > 0 THEN
        FOR i IN 1 .. array_length(p_subjects, 1) LOOP
            INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, fee_head, due_on)
            SELECT v_student, v_session.academic_year, 'charge', 'Backlog examination fee — ' || s.code,
                   v_session.backlog_fee_paise, 'backlog_exam', v_session.form_closes_on
            FROM academic.subjects s WHERE s.id = p_subjects[i]
            RETURNING id INTO v_charge;
            INSERT INTO exams.exam_form_charges VALUES (v_form, v_charge);
            v_fee := v_fee + v_session.backlog_fee_paise;
        END LOOP;
    END IF;

    SELECT array_agg(fc.charge_id ORDER BY fc.charge_id) INTO v_pending
    FROM exams.exam_form_charges fc WHERE fc.form_id = v_form AND fees.pending_of(fc.charge_id) > 0;

    UPDATE exams.exam_forms f
    SET fee_paise = v_fee,
        status = CASE WHEN v_pending IS NULL THEN 'submitted' ELSE 'payment_pending' END,
        submitted_at = CASE WHEN v_pending IS NULL THEN now() END
    WHERE f.id = v_form;
    RETURN QUERY SELECT v_form, CASE WHEN v_pending IS NULL THEN 'submitted' ELSE 'payment_pending' END, coalesce(v_pending, '{}');
END $$;

-- Marks the acting student's forms submitted once nothing on them is
-- pending; called by the payment webhook (as `gateway`). Returns them.
CREATE FUNCTION exams.complete_paid_forms(p_student uuid) RETURNS SETOF uuid
LANGUAGE sql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    UPDATE exams.exam_forms f SET status = 'submitted', submitted_at = now()
    WHERE f.student_id = p_student AND f.status = 'payment_pending'
      AND identity.current_app_role() = 'gateway'
      AND NOT EXISTS (SELECT 1 FROM exams.exam_form_charges fc
                      WHERE fc.form_id = f.id AND fees.pending_of(fc.charge_id) > 0)
    RETURNING f.id
$$;

-- Seats every student with a submitted form, program then roll number, room
-- by room in name order. Exam cell only, and only before hall tickets are
-- generated. Returns the number seated, or fails if the rooms are too small.
CREATE FUNCTION exams.allocate_seats(p_session uuid) RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_needed integer;
    v_capacity integer;
BEGIN
    IF identity.current_app_role() NOT IN ('exam_cell', 'super_admin') THEN
        RAISE EXCEPTION 'exam cell only' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF EXISTS (SELECT 1 FROM exams.exam_sessions WHERE id = p_session AND hall_tickets_generated_at IS NOT NULL) THEN
        RAISE EXCEPTION 'hall tickets are already generated for this session' USING ERRCODE = 'check_violation';
    END IF;
    SELECT count(DISTINCT student_id) INTO v_needed FROM exams.exam_forms WHERE session_id = p_session AND status = 'submitted';
    SELECT coalesce(sum(capacity), 0) INTO v_capacity FROM exams.venues;
    IF v_needed > v_capacity THEN
        RAISE EXCEPTION 'the rooms seat %, but % students are registered', v_capacity, v_needed USING ERRCODE = 'check_violation';
    END IF;

    DELETE FROM exams.seating WHERE session_id = p_session;
    WITH students AS (
        SELECT DISTINCT f.student_id, s.program_id, s.roll_no
        FROM exams.exam_forms f JOIN people.students s ON s.user_id = f.student_id
        WHERE f.session_id = p_session AND f.status = 'submitted'
    ), ordered AS (
        SELECT student_id, row_number() OVER (ORDER BY program_id, roll_no) AS n FROM students
    ), rooms AS (
        SELECT id, capacity,
               coalesce(sum(capacity) OVER (ORDER BY name ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), 0) AS before
        FROM exams.venues
    )
    INSERT INTO exams.seating (session_id, student_id, venue_id, seat_no)
    SELECT p_session, o.student_id, r.id, (o.n - r.before)::smallint
    FROM ordered o JOIN rooms r ON o.n > r.before AND o.n <= r.before + r.capacity;
    RETURN v_needed;
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'exams.create_form(uuid, text, uuid[], uuid[])',
        'exams.complete_paid_forms(uuid)',
        'exams.allocate_seats(uuid)',
        'fees.pending_of(bigint)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;
