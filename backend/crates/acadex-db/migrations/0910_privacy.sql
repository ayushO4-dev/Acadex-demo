-- The Digital Personal Data Protection Act, 2023 (architecture.md §13):
-- consent records, and erasure that honours statutory retention of academic
-- and financial records. (The data-subject export reads existing tables as
-- the student, so it needs nothing here.)

CREATE SCHEMA privacy;
GRANT USAGE ON SCHEMA privacy TO acadex_app;

-- Consent for the optional purposes, as an append-only history: what was
-- agreed (or withdrawn), under which version of the privacy notice, when.
-- The current choice is the latest row per purpose.
CREATE TABLE privacy.consents (
    id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id        uuid NOT NULL REFERENCES identity.users,
    purpose        text NOT NULL CHECK (purpose IN ('email_notices', 'placement_profile', 'alumni_contact', 'research_analytics')),
    granted        boolean NOT NULL,
    notice_version text NOT NULL,
    recorded_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON privacy.consents (user_id, purpose, id DESC);

GRANT SELECT, INSERT ON privacy.consents TO acadex_app;
ALTER TABLE privacy.consents ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_consents ON privacy.consents FOR ALL TO acadex_app
    USING (user_id = identity.current_user_id())
    WITH CHECK (user_id = identity.current_user_id());
CREATE POLICY admin_reads ON privacy.consents FOR SELECT TO acadex_app USING (identity.is_admin());

-- A student asks for their data to be erased; administration (the data
-- protection officer) approves, which erases at once, or refuses with a reason.
CREATE TABLE privacy.erasure_requests (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id    uuid NOT NULL REFERENCES people.students,
    reason        text NOT NULL CHECK (length(reason) BETWEEN 10 AND 1000),
    status        text NOT NULL DEFAULT 'requested' CHECK (status IN ('requested', 'completed', 'refused', 'withdrawn')),
    requested_at  timestamptz NOT NULL DEFAULT now(),
    decided_by    uuid REFERENCES identity.users,
    decided_at    timestamptz,
    decision_note text CHECK (length(decision_note) <= 1000),
    -- What was erased and what was kept, and why: shown to both sides.
    summary       jsonb
);
CREATE UNIQUE INDEX one_open_erasure ON privacy.erasure_requests (student_id) WHERE status = 'requested';

GRANT SELECT, INSERT ON privacy.erasure_requests TO acadex_app;
GRANT UPDATE (status, decided_by, decided_at, decision_note, summary) ON privacy.erasure_requests TO acadex_app;
ALTER TABLE privacy.erasure_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY student_requests ON privacy.erasure_requests FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id() AND status = 'requested');
CREATE POLICY student_reads_own ON privacy.erasure_requests FOR SELECT TO acadex_app
    USING (student_id = identity.current_user_id());
CREATE POLICY student_withdraws ON privacy.erasure_requests FOR UPDATE TO acadex_app
    USING (student_id = identity.current_user_id() AND status = 'requested')
    WITH CHECK (student_id = identity.current_user_id() AND status = 'withdrawn' AND decided_by IS NULL);
CREATE POLICY admin_decides ON privacy.erasure_requests FOR ALL TO acadex_app
    USING (identity.is_admin())
    WITH CHECK (identity.is_admin());

-- Erases one student's personal data, keeping what law or academic record
-- keeping requires. Administration only. Returns what was erased and kept,
-- and the storage objects for the caller to delete (files live outside the
-- database).
CREATE FUNCTION privacy.erase_student(p_student uuid, p_notice_version text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_objects  text[];
    v_counts   jsonb := '{}';
    v_n        bigint;
BEGIN
    IF NOT identity.is_admin() THEN
        RAISE EXCEPTION 'erasure is for administration' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM people.students WHERE user_id = p_student) THEN
        RAISE EXCEPTION 'no such student' USING ERRCODE = 'no_data_found';
    END IF;

    -- Documents that are personal rather than academic records, and support
    -- attachments the student uploaded: rows here, files by the caller.
    SELECT coalesce(array_agg(object), '{}') INTO v_objects
    FROM (SELECT d.object FROM profile.documents d
          WHERE d.student_id = p_student AND d.doc_type IN ('aadhaar', 'other')
          UNION ALL
          SELECT a.object FROM support.attachments a WHERE a.uploader_id = p_student) o;

    DELETE FROM profile.edit_requests WHERE student_id = p_student;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('profile_change_requests', v_n);
    DELETE FROM profile.documents WHERE student_id = p_student AND doc_type IN ('aadhaar', 'other');
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('documents', v_n);

    UPDATE profile.personal_info
    SET blood_group = NULL, nationality = NULL, mother_tongue = NULL,
        aadhaar_sealed = NULL, aadhaar_last4 = NULL,
        bank_account_sealed = NULL, bank_account_last4 = NULL, bank_ifsc = NULL, bank_name = NULL,
        updated_at = now()
    WHERE student_id = p_student;
    DELETE FROM profile.parent_info WHERE student_id = p_student;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('parent_details', v_n);
    DELETE FROM profile.contact_info WHERE student_id = p_student;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('contact_details', v_n);

    UPDATE support.messages SET body = '[Erased at the student''s request]'
    WHERE author_id = p_student AND body <> '[Erased at the student''s request]';
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('support_messages', v_n);
    DELETE FROM support.attachments WHERE uploader_id = p_student;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_counts := v_counts || jsonb_build_object('support_attachments', v_n);

    DELETE FROM comms.notice_reads WHERE user_id = p_student;
    DELETE FROM identity.notification_prefs WHERE user_id = p_student;

    -- Every optional purpose withdrawn, as part of the consent history.
    INSERT INTO privacy.consents (user_id, purpose, granted, notice_version)
    SELECT p_student, p.purpose, false, p_notice_version
    FROM unnest(ARRAY['email_notices', 'placement_profile', 'alumni_contact', 'research_analytics']) AS p(purpose);

    -- The account can no longer sign in; the name stays on academic records.
    UPDATE identity.users SET active = false, deactivated_at = now() WHERE id = p_student;

    RETURN jsonb_build_object(
        'erased', v_counts || jsonb_build_object(
            'personal_details', 'Aadhaar, bank account, blood group, nationality and mother tongue',
            'other', 'notice read receipts, email preferences, optional consents (withdrawn)'),
        'retained', jsonb_build_array(
            'Name, roll number, program and admission record: needed to issue and verify degrees',
            'Date of birth, gender and category: printed on certificates and used for reservation audits',
            'Education history and academic documents (marksheets, certificates): statutory academic records',
            'Attendance, marks, results and examination records: statutory academic records',
            'Fee ledger, payments and receipts: financial records kept for 8 years under the Income Tax Act',
            'The audit log: tamper-evident security record'),
        'objects', to_jsonb(v_objects));
END $$;
GRANT EXECUTE ON FUNCTION privacy.erase_student(uuid, text) TO acadex_app;
