-- Withdrawing published results, when they turn out to be wrong. Like
-- publishing it takes two people: one exam-cell member asks, with a reason, and
-- a different one approves. Approval puts the set back to staged (students no
-- longer see it, and its marks can be corrected), takes down its grade cards
-- (they're made again when it's republished), and keeps a permanent record.
-- Refused while re-evaluations on those results are still open.

ALTER TABLE exams.result_publications
    ADD COLUMN withdrawal_requested_by uuid REFERENCES identity.users,
    ADD COLUMN withdrawal_requested_at timestamptz,
    ADD COLUMN withdrawal_reason       text CHECK (length(withdrawal_reason) BETWEEN 10 AND 500),
    ADD CONSTRAINT withdrawal_request_whole
        CHECK ((withdrawal_requested_by IS NULL) = (withdrawal_requested_at IS NULL)
               AND (withdrawal_requested_by IS NULL) = (withdrawal_reason IS NULL)),
    ADD CONSTRAINT withdrawal_only_when_published
        CHECK (withdrawal_requested_by IS NULL OR published_at IS NOT NULL);

-- Every withdrawal, kept after the set is republished.
CREATE TABLE exams.publication_withdrawals (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    publication_id uuid NOT NULL REFERENCES exams.result_publications,
    published_at   timestamptz NOT NULL,
    published_by   uuid REFERENCES identity.users,
    reason         text NOT NULL,
    requested_by   uuid NOT NULL REFERENCES identity.users,
    requested_at   timestamptz NOT NULL,
    approved_by    uuid NOT NULL REFERENCES identity.users,
    approved_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (approved_by <> requested_by)
);
GRANT SELECT ON exams.publication_withdrawals TO acadex_app;
ALTER TABLE exams.publication_withdrawals ENABLE ROW LEVEL SECURITY;
CREATE POLICY staff_reads ON exams.publication_withdrawals FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'super_admin', 'admin', 'hod'));

CREATE FUNCTION exams.require_publisher() RETURNS void
LANGUAGE plpgsql STABLE
AS $$
BEGIN
    IF identity.current_app_role() NOT IN ('exam_cell', 'super_admin') THEN
        RAISE EXCEPTION 'the exam cell only' USING ERRCODE = 'insufficient_privilege';
    END IF;
END $$;

-- Step one: ask, with a reason. Returns false if it isn't published or a
-- withdrawal is already asked for.
CREATE FUNCTION exams.request_withdrawal(p_id uuid, p_reason text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM exams.require_publisher();
    UPDATE exams.result_publications
    SET withdrawal_requested_by = identity.current_user_id(), withdrawal_requested_at = now(),
        withdrawal_reason = trim(p_reason)
    WHERE id = p_id AND published_at IS NOT NULL AND withdrawal_requested_by IS NULL;
    RETURN FOUND;
END $$;

-- The person who asked can take it back.
CREATE FUNCTION exams.cancel_withdrawal(p_id uuid) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM exams.require_publisher();
    UPDATE exams.result_publications
    SET withdrawal_requested_by = NULL, withdrawal_requested_at = NULL, withdrawal_reason = NULL
    WHERE id = p_id AND withdrawal_requested_by = identity.current_user_id();
    RETURN FOUND;
END $$;

-- Step two: someone else approves. 'withdrawn', 'not-requested',
-- 'same-person' or 'revaluations-open'.
CREATE FUNCTION exams.approve_withdrawal(p_id uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v exams.result_publications;
BEGIN
    PERFORM exams.require_publisher();
    SELECT * INTO v FROM exams.result_publications WHERE id = p_id FOR UPDATE;
    IF v.id IS NULL OR v.withdrawal_requested_by IS NULL THEN
        RETURN 'not-requested';
    END IF;
    IF v.withdrawal_requested_by = identity.current_user_id() THEN
        RETURN 'same-person';
    END IF;
    IF EXISTS (SELECT 1 FROM exams.revaluation_requests r
               JOIN exams.external_marks em
                 ON em.student_id = r.student_id AND em.offering_id = r.offering_id AND em.attempt = r.attempt
               WHERE em.publication_id = p_id AND r.status <> 'decided') THEN
        RETURN 'revaluations-open';
    END IF;

    INSERT INTO exams.publication_withdrawals
        (publication_id, published_at, published_by, reason, requested_by, requested_at, approved_by)
    VALUES (v.id, v.published_at, v.published_by, v.withdrawal_reason, v.withdrawal_requested_by,
            v.withdrawal_requested_at, identity.current_user_id());
    DELETE FROM exams.grade_cards WHERE publication_id = p_id;
    UPDATE exams.result_publications
    SET published_at = NULL, published_by = NULL, requested_by = NULL, requested_at = NULL,
        withdrawal_requested_by = NULL, withdrawal_requested_at = NULL, withdrawal_reason = NULL
    WHERE id = p_id;
    RETURN 'withdrawn';
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'exams.request_withdrawal(uuid, text)',
        'exams.cancel_withdrawal(uuid)',
        'exams.approve_withdrawal(uuid)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;
