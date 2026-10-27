-- F3c: re-evaluation and re-total requests (architecture.md §9.3), and grade
-- cards made by the worker when results are published.

-- Fees per subject, set by the administrator with the appeal window.
ALTER TABLE org.institution_settings
    ADD COLUMN revaluation_fee_paise bigint NOT NULL DEFAULT 50000 CHECK (revaluation_fee_paise >= 0),
    ADD COLUMN retotal_fee_paise     bigint NOT NULL DEFAULT 20000 CHECK (retotal_fee_paise >= 0);
GRANT UPDATE (revaluation_fee_paise, retotal_fee_paise) ON org.institution_settings TO acadex_app;

-- Re-evaluation results come in several sets a year (one per batch of
-- decisions), so only regular and backlog sets stay one per semester and year.
ALTER TABLE exams.result_publications DROP CONSTRAINT result_publications_program_id_semester_academic_year_kind_key;
CREATE UNIQUE INDEX result_publications_one_per_term
    ON exams.result_publications (program_id, semester, academic_year, kind) WHERE kind <> 'revaluation';

-- One request per published result. A changed mark becomes a new attempt in
-- a staged re-evaluation set, published with the usual two-person approval.
CREATE TABLE exams.revaluation_requests (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id     uuid NOT NULL,
    offering_id    uuid NOT NULL,
    attempt        smallint NOT NULL,
    kind           text NOT NULL CHECK (kind IN ('revaluation', 'retotal')),
    reason         text NOT NULL CHECK (length(reason) BETWEEN 10 AND 1000),
    fee_paise      bigint NOT NULL CHECK (fee_paise >= 0),
    charge_id      bigint REFERENCES fees.ledger,
    status         text NOT NULL CHECK (status IN ('payment_pending', 'submitted', 'decided')),
    submitted_at   timestamptz,
    old_marks      numeric(5, 1) NOT NULL,
    new_marks      numeric(5, 1) CHECK (new_marks >= 0),
    -- The re-evaluation set holding the changed mark; NULL if unchanged.
    publication_id uuid REFERENCES exams.result_publications,
    remarks        text CHECK (length(remarks) <= 500),
    decided_by     uuid REFERENCES identity.users,
    decided_at     timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (student_id, offering_id, attempt) REFERENCES exams.external_marks,
    UNIQUE (student_id, offering_id, attempt),
    CHECK ((status = 'payment_pending') = (submitted_at IS NULL)),
    CHECK ((status = 'decided') = (decided_at IS NOT NULL AND new_marks IS NOT NULL AND decided_by IS NOT NULL))
);
CREATE INDEX ON exams.revaluation_requests (status, submitted_at);

GRANT SELECT ON exams.revaluation_requests TO acadex_app;
GRANT UPDATE (status, new_marks, publication_id, remarks, decided_by, decided_at) ON exams.revaluation_requests TO acadex_app;
ALTER TABLE exams.revaluation_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY student_reads_own ON exams.revaluation_requests FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY exam_cell_reads_all ON exams.revaluation_requests FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin'));
CREATE POLICY exam_cell_decides ON exams.revaluation_requests FOR UPDATE TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin') AND status = 'submitted')
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'super_admin')
                AND status = 'decided' AND decided_by = identity.current_user_id());

-- The student applies. Like exam forms, a definer function: only the student
-- themselves, only for a published regular or backlog result that's still
-- their latest for the subject, only inside the appeal window, and the fee
-- taken from settings, never the caller. Charges it to the ledger.
CREATE FUNCTION exams.request_revaluation(p_offering uuid, p_attempt smallint, p_kind text, p_reason text)
RETURNS TABLE (request_id uuid, status text, charge_id bigint)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_student  uuid := identity.current_user_id();
    v_settings org.institution_settings;
    v_marks    numeric;
    v_code     text;
    v_fee      bigint;
    v_year     smallint;
    v_charge   bigint;
    v_request  uuid;
BEGIN
    IF identity.current_app_role() <> 'student' OR v_student IS NULL THEN
        RAISE EXCEPTION 'only students apply for re-evaluation' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF p_kind NOT IN ('revaluation', 'retotal') THEN
        RAISE EXCEPTION 'choose re-evaluation or re-total' USING ERRCODE = 'check_violation';
    END IF;
    SELECT * INTO v_settings FROM org.institution_settings LIMIT 1;

    SELECT em.marks, s.code INTO v_marks, v_code
    FROM exams.external_marks em
    JOIN exams.result_publications rp ON rp.id = em.publication_id
    JOIN academic.offerings o ON o.id = em.offering_id
    JOIN academic.subjects s ON s.id = o.subject_id
    WHERE em.student_id = v_student AND em.offering_id = p_offering AND em.attempt = p_attempt
      AND rp.kind IN ('regular', 'backlog') AND rp.published_at IS NOT NULL
      AND rp.published_at + make_interval(days => coalesce(v_settings.results_appeal_days, 15)) >= now()
      AND NOT EXISTS (SELECT 1 FROM exams.external_marks later
                      WHERE later.student_id = em.student_id AND later.offering_id = em.offering_id
                        AND later.attempt > em.attempt);
    IF NOT FOUND THEN
        RAISE EXCEPTION 'that result can''t be appealed (not yours, not published, superseded, or past the window)'
            USING ERRCODE = 'check_violation';
    END IF;

    v_fee := CASE p_kind WHEN 'revaluation' THEN coalesce(v_settings.revaluation_fee_paise, 50000)
                         ELSE coalesce(v_settings.retotal_fee_paise, 20000) END;
    v_year := CASE WHEN extract(month FROM current_date) >= coalesce(v_settings.academic_year_start_month, 7)
                   THEN extract(year FROM current_date) ELSE extract(year FROM current_date) - 1 END;
    IF v_fee > 0 THEN
        INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, fee_head, due_on)
        VALUES (v_student, v_year, 'charge',
                CASE p_kind WHEN 'revaluation' THEN 'Re-evaluation fee — ' ELSE 'Re-total fee — ' END || v_code,
                v_fee, 'revaluation', current_date)
        RETURNING id INTO v_charge;
    END IF;

    INSERT INTO exams.revaluation_requests (student_id, offering_id, attempt, kind, reason, fee_paise, charge_id,
                                            status, submitted_at, old_marks)
    VALUES (v_student, p_offering, p_attempt, p_kind, p_reason, v_fee, v_charge,
            CASE WHEN v_charge IS NULL THEN 'submitted' ELSE 'payment_pending' END,
            CASE WHEN v_charge IS NULL THEN now() END, v_marks)
    RETURNING id INTO v_request;
    RETURN QUERY SELECT v_request, CASE WHEN v_charge IS NULL THEN 'submitted' ELSE 'payment_pending' END, v_charge;
END $$;

-- Marks the student's requests submitted once their fee is paid; called by
-- the payment webhook (as `gateway`), like exams.complete_paid_forms.
CREATE FUNCTION exams.complete_paid_revaluations(p_student uuid) RETURNS SETOF uuid
LANGUAGE sql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    UPDATE exams.revaluation_requests r SET status = 'submitted', submitted_at = now()
    WHERE r.student_id = p_student AND r.status = 'payment_pending'
      AND identity.current_app_role() = 'gateway'
      AND fees.pending_of(r.charge_id) <= 0
    RETURNING r.id
$$;

REVOKE EXECUTE ON FUNCTION exams.request_revaluation(uuid, smallint, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION exams.request_revaluation(uuid, smallint, text, text) TO acadex_app;
REVOKE EXECUTE ON FUNCTION exams.complete_paid_revaluations(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION exams.complete_paid_revaluations(uuid) TO acadex_app;

-- ── Grade cards ─────────────────────────────────────────────────────────────
-- One per student per semester, remade whenever a result for that semester
-- is published (regular, backlog or re-evaluation): the latest wins.
CREATE TABLE exams.grade_cards (
    student_id     uuid NOT NULL REFERENCES people.students,
    semester       smallint NOT NULL CHECK (semester BETWEEN 1 AND 8),
    publication_id uuid NOT NULL REFERENCES exams.result_publications,
    pdf_object     text NOT NULL,
    generated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (student_id, semester)
);
GRANT SELECT, INSERT, UPDATE ON exams.grade_cards TO acadex_app;
ALTER TABLE exams.grade_cards ENABLE ROW LEVEL SECURITY;
CREATE POLICY student_reads_own ON exams.grade_cards FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY staff_reads_all ON exams.grade_cards FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin', 'worker'));
CREATE POLICY hod_reads_department ON exams.grade_cards FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod'
           AND student_id IN (SELECT user_id FROM people.students WHERE program_id IN (SELECT org.my_department_programs())));
CREATE POLICY worker_writes ON exams.grade_cards FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'worker');
CREATE POLICY worker_updates ON exams.grade_cards FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'worker') WITH CHECK (identity.current_app_role() = 'worker');
