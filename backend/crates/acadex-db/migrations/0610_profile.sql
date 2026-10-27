-- F4: the student profile (architecture.md §9.5). Sections: admission,
-- personal, parent/guardian, contact, education, documents. Contact, parent,
-- education and bank details are the student's to edit; name, date of birth,
-- admission number, category, blood group and Aadhaar change only through an
-- edit request the registrar (an administrator) approves. Aadhaar and bank
-- account numbers are sealed by the API with envelope encryption; only the
-- last four digits are stored in the clear, for masked display.

CREATE SCHEMA profile;
GRANT USAGE ON SCHEMA profile TO acadex_app;

CREATE TABLE profile.admission (
    student_id   uuid PRIMARY KEY REFERENCES people.students ON DELETE CASCADE,
    admission_no text NOT NULL UNIQUE CHECK (admission_no ~ '^[A-Z0-9/-]{4,30}$'),
    admitted_on  date NOT NULL,
    quota        text NOT NULL CHECK (quota IN ('merit', 'management', 'nri', 'lateral_entry'))
);

CREATE TABLE profile.personal_info (
    student_id          uuid PRIMARY KEY REFERENCES people.students ON DELETE CASCADE,
    date_of_birth       date CHECK (date_of_birth BETWEEN '1950-01-01' AND '2015-12-31'),
    gender              text CHECK (gender IN ('female', 'male', 'other', 'undisclosed')),
    blood_group         text CHECK (blood_group IN ('A+', 'A-', 'B+', 'B-', 'AB+', 'AB-', 'O+', 'O-')),
    category            text CHECK (category IN ('general', 'obc', 'sc', 'st', 'ews')),
    nationality         text CHECK (length(nationality) <= 60),
    mother_tongue       text CHECK (length(mother_tongue) <= 60),
    -- Sealed by the API (associated data: "<student id>:aadhaar").
    aadhaar_sealed      bytea,
    aadhaar_last4       text CHECK (aadhaar_last4 ~ '^[0-9]{4}$'),
    -- Sealed likewise ("<student id>:bank_account").
    bank_account_sealed bytea,
    bank_account_last4  text CHECK (bank_account_last4 ~ '^[0-9]{4}$'),
    bank_ifsc           text CHECK (bank_ifsc ~ '^[A-Z]{4}0[A-Z0-9]{6}$'),
    bank_name           text CHECK (length(bank_name) <= 100),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CHECK ((aadhaar_sealed IS NULL) = (aadhaar_last4 IS NULL)),
    CHECK ((bank_account_sealed IS NULL) = (bank_account_last4 IS NULL))
);

CREATE TABLE profile.parent_info (
    student_id        uuid PRIMARY KEY REFERENCES people.students ON DELETE CASCADE,
    father_name       text CHECK (length(father_name) <= 100),
    father_occupation text CHECK (length(father_occupation) <= 100),
    father_phone      text CHECK (father_phone ~ '^[0-9]{10}$'),
    mother_name       text CHECK (length(mother_name) <= 100),
    mother_occupation text CHECK (length(mother_occupation) <= 100),
    mother_phone      text CHECK (mother_phone ~ '^[0-9]{10}$'),
    guardian_name     text CHECK (length(guardian_name) <= 100),
    guardian_relation text CHECK (length(guardian_relation) <= 50),
    guardian_phone    text CHECK (guardian_phone ~ '^[0-9]{10}$'),
    annual_income     text CHECK (annual_income IN ('below_2_5l', '2_5l_to_5l', '5l_to_10l', 'above_10l')),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE profile.contact_info (
    student_id             uuid PRIMARY KEY REFERENCES people.students ON DELETE CASCADE,
    phone                  text CHECK (phone ~ '^[0-9]{10}$'),
    personal_email         text CHECK (personal_email ~ '^[^@\s]+@[^@\s]+\.[^@\s]+$' AND length(personal_email) <= 200),
    correspondence_address text CHECK (length(correspondence_address) <= 500),
    permanent_address      text CHECK (length(permanent_address) <= 500),
    emergency_name         text CHECK (length(emergency_name) <= 100),
    emergency_relation     text CHECK (length(emergency_relation) <= 50),
    emergency_phone        text CHECK (emergency_phone ~ '^[0-9]{10}$'),
    updated_at             timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE profile.education (
    student_id  uuid NOT NULL REFERENCES people.students ON DELETE CASCADE,
    level       text NOT NULL CHECK (level IN ('class_10', 'class_12', 'diploma', 'entrance')),
    board       text NOT NULL CHECK (length(board) BETWEEN 1 AND 100),
    institution text NOT NULL CHECK (length(institution) BETWEEN 1 AND 200),
    year        smallint NOT NULL CHECK (year BETWEEN 1990 AND 2030),
    -- A percentage, CGPA or rank, as the board or exam reports it.
    score       text NOT NULL CHECK (length(score) BETWEEN 1 AND 30),
    PRIMARY KEY (student_id, level)
);

-- Uploaded documents. A file is quarantined ('scanning') until the scanner
-- has checked its real type and run ClamAV over it; only 'clean' files can be
-- downloaded, and the registrar then verifies them.
CREATE TABLE profile.documents (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id   uuid NOT NULL REFERENCES people.students ON DELETE CASCADE,
    doc_type     text NOT NULL CHECK (doc_type IN ('class_10_marksheet', 'class_12_marksheet', 'transfer_certificate',
                                                   'aadhaar', 'caste_certificate', 'migration_certificate', 'other')),
    object       text NOT NULL UNIQUE,
    name         text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    content_type text NOT NULL CHECK (content_type IN ('application/pdf', 'image/png', 'image/jpeg')),
    size_bytes   integer NOT NULL CHECK (size_bytes > 0),
    scan_status  text NOT NULL DEFAULT 'scanning' CHECK (scan_status IN ('scanning', 'clean', 'infected', 'type_mismatch')),
    scan_detail  text CHECK (length(scan_detail) <= 200),
    scanned_at   timestamptz,
    verification text NOT NULL DEFAULT 'pending' CHECK (verification IN ('pending', 'verified', 'rejected')),
    verified_by  uuid REFERENCES identity.users,
    verified_at  timestamptz,
    verify_note  text CHECK (length(verify_note) <= 500),
    uploaded_at  timestamptz NOT NULL DEFAULT now(),
    CHECK (verification = 'pending' OR scan_status = 'clean')
);
CREATE INDEX ON profile.documents (student_id, doc_type, uploaded_at DESC);
CREATE INDEX ON profile.documents (verification, scan_status);

-- Changes to fields the student can't edit directly.
CREATE TABLE profile.edit_requests (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id   uuid NOT NULL REFERENCES people.students ON DELETE CASCADE,
    field        text NOT NULL CHECK (field IN ('full_name', 'date_of_birth', 'admission_no', 'category', 'blood_group', 'aadhaar')),
    -- What the registrar compares; Aadhaar shows masked in both.
    old_display  text CHECK (length(old_display) <= 200),
    new_display  text NOT NULL CHECK (length(new_display) BETWEEN 1 AND 200),
    -- The new value: plain for ordinary fields, sealed for Aadhaar.
    new_value    text CHECK (length(new_value) <= 200),
    new_sealed   bytea,
    reason       text NOT NULL CHECK (length(reason) BETWEEN 10 AND 1000),
    document_id  uuid REFERENCES profile.documents,
    status       text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'withdrawn')),
    decided_by   uuid REFERENCES identity.users,
    decided_at   timestamptz,
    decision_note text CHECK (length(decision_note) <= 500),
    created_at   timestamptz NOT NULL DEFAULT now(),
    CHECK ((field = 'aadhaar') = (new_sealed IS NOT NULL)),
    CHECK ((field = 'aadhaar') = (new_value IS NULL))
);
CREATE UNIQUE INDEX one_pending_per_field ON profile.edit_requests (student_id, field) WHERE status = 'pending';

-- Purpose-bound log of staff reads of student PII (architecture.md §13).
CREATE TABLE audit.data_access_log (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_id   uuid NOT NULL REFERENCES identity.users,
    actor_role text NOT NULL,
    student_id uuid NOT NULL REFERENCES people.students,
    -- 'profile' for a view (masked), or the field revealed in full.
    scope      text NOT NULL CHECK (scope IN ('profile', 'aadhaar', 'bank_account', 'document')),
    purpose    text NOT NULL CHECK (length(purpose) BETWEEN 3 AND 200),
    trace_id   text,
    at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON audit.data_access_log (student_id, at DESC);

-- ── Who reads a profile ─────────────────────────────────────────────────────
-- The student; administrators (the registrar) and support, institution-wide;
-- an HOD, their department. Staff reads are logged by the API.
CREATE FUNCTION profile.may_read(p_student uuid) RETURNS boolean
LANGUAGE sql STABLE
AS $$
    SELECT CASE identity.current_app_role()
        WHEN 'student' THEN p_student = identity.current_user_id()
        WHEN 'admin' THEN true
        WHEN 'super_admin' THEN true
        WHEN 'support_agent' THEN true
        WHEN 'hod' THEN people.in_my_department(p_student)
        WHEN 'worker' THEN true
        ELSE false END
$$;
GRANT EXECUTE ON FUNCTION profile.may_read(uuid) TO acadex_app;

DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['admission', 'personal_info', 'parent_info', 'contact_info', 'education', 'documents', 'edit_requests'] LOOP
        EXECUTE format('GRANT SELECT ON profile.%I TO acadex_app', t);
        EXECUTE format('ALTER TABLE profile.%I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY readers ON profile.%I FOR SELECT TO acadex_app USING (profile.may_read(student_id))', t);
    END LOOP;
END $$;

-- The student's own direct edits. Personal info: only the fields not behind
-- an edit request (column grants), on their own row.
GRANT INSERT (student_id) ON profile.personal_info TO acadex_app;
GRANT UPDATE (gender, nationality, mother_tongue, bank_account_sealed, bank_account_last4, bank_ifsc, bank_name, updated_at)
    ON profile.personal_info TO acadex_app;
GRANT INSERT, UPDATE ON profile.parent_info, profile.contact_info TO acadex_app;
GRANT INSERT, UPDATE, DELETE ON profile.education TO acadex_app;
DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['personal_info', 'parent_info', 'contact_info', 'education'] LOOP
        EXECUTE format($p$CREATE POLICY student_writes_own ON profile.%I FOR ALL TO acadex_app
                          USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id())
                          WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id())$p$, t);
    END LOOP;
END $$;

-- Documents: the student uploads; the worker records the scan; the
-- registrar verifies clean ones.
GRANT INSERT (id, student_id, doc_type, object, name, content_type, size_bytes) ON profile.documents TO acadex_app;
GRANT UPDATE (scan_status, scan_detail, scanned_at, verification, verified_by, verified_at, verify_note) ON profile.documents TO acadex_app;
CREATE POLICY student_uploads ON profile.documents FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY worker_scans ON profile.documents FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'worker' AND scan_status = 'scanning')
    WITH CHECK (identity.current_app_role() = 'worker' AND verification = 'pending');
CREATE POLICY registrar_verifies ON profile.documents FOR UPDATE TO acadex_app
    USING (identity.current_app_role() IN ('admin', 'super_admin') AND scan_status = 'clean')
    WITH CHECK (identity.current_app_role() IN ('admin', 'super_admin') AND verified_by = identity.current_user_id());

-- Edit requests: the student files and withdraws; the registrar decides
-- through profile.decide_edit_request, which applies an approval.
GRANT INSERT (student_id, field, old_display, new_display, new_value, new_sealed, reason, document_id)
    ON profile.edit_requests TO acadex_app;
GRANT UPDATE (status) ON profile.edit_requests TO acadex_app;
CREATE POLICY student_requests ON profile.edit_requests FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
                AND (document_id IS NULL OR document_id IN (SELECT id FROM profile.documents
                                                            WHERE student_id = identity.current_user_id())));
CREATE POLICY student_withdraws ON profile.edit_requests FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id() AND status = 'pending')
    WITH CHECK (status = 'withdrawn');

CREATE FUNCTION profile.decide_edit_request(p_request uuid, p_approve boolean, p_note text)
RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    r profile.edit_requests;
BEGIN
    IF identity.current_app_role() NOT IN ('admin', 'super_admin') THEN
        RAISE EXCEPTION 'the registrar decides edit requests' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT * INTO r FROM profile.edit_requests WHERE id = p_request AND status = 'pending' FOR UPDATE;
    IF NOT FOUND THEN
        RETURN false;
    END IF;
    IF p_approve THEN
        CASE r.field
            WHEN 'full_name' THEN
                UPDATE identity.users SET display_name = r.new_value WHERE id = r.student_id;
            WHEN 'admission_no' THEN
                UPDATE profile.admission SET admission_no = r.new_value WHERE student_id = r.student_id;
            WHEN 'date_of_birth' THEN
                INSERT INTO profile.personal_info (student_id, date_of_birth) VALUES (r.student_id, r.new_value::date)
                ON CONFLICT (student_id) DO UPDATE SET date_of_birth = EXCLUDED.date_of_birth, updated_at = now();
            WHEN 'category' THEN
                INSERT INTO profile.personal_info (student_id, category) VALUES (r.student_id, r.new_value)
                ON CONFLICT (student_id) DO UPDATE SET category = EXCLUDED.category, updated_at = now();
            WHEN 'blood_group' THEN
                INSERT INTO profile.personal_info (student_id, blood_group) VALUES (r.student_id, r.new_value)
                ON CONFLICT (student_id) DO UPDATE SET blood_group = EXCLUDED.blood_group, updated_at = now();
            WHEN 'aadhaar' THEN
                INSERT INTO profile.personal_info (student_id, aadhaar_sealed, aadhaar_last4)
                VALUES (r.student_id, r.new_sealed, right(r.new_display, 4))
                ON CONFLICT (student_id) DO UPDATE SET aadhaar_sealed = EXCLUDED.aadhaar_sealed,
                                                       aadhaar_last4 = EXCLUDED.aadhaar_last4, updated_at = now();
        END CASE;
    END IF;
    UPDATE profile.edit_requests
    SET status = CASE WHEN p_approve THEN 'approved' ELSE 'rejected' END,
        decided_by = identity.current_user_id(), decided_at = now(), decision_note = p_note
    WHERE id = p_request;
    RETURN true;
END $$;
REVOKE EXECUTE ON FUNCTION profile.decide_edit_request(uuid, boolean, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION profile.decide_edit_request(uuid, boolean, text) TO acadex_app;

-- The access log: staff append their own reads; students see who read
-- theirs; administrators see all.
GRANT SELECT, INSERT ON audit.data_access_log TO acadex_app;
ALTER TABLE audit.data_access_log ENABLE ROW LEVEL SECURITY;
CREATE POLICY staff_logs_own_reads ON audit.data_access_log FOR INSERT TO acadex_app
    WITH CHECK (actor_id = identity.current_user_id() AND actor_role = identity.current_app_role());
CREATE POLICY student_sees_reads_of_self ON audit.data_access_log FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY admin_sees_all ON audit.data_access_log FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('admin', 'super_admin'));
