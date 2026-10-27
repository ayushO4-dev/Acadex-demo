-- F4: activities (architecture.md §9.2). Four types share one base table, so
-- listings and approvals are uniform; each type's own fields live in its own
-- table. Workflow: draft → submitted → hod_review → approved | rejected.
-- Faculty of the student's program forward a submission to the HOD (or
-- return it to the student); the HOD approves with points, rejects, or
-- returns it.

CREATE SCHEMA activities;
GRANT USAGE ON SCHEMA activities TO acadex_app;

CREATE TABLE activities.activities (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    student_id   uuid NOT NULL REFERENCES people.students,
    kind         text NOT NULL CHECK (kind IN ('general', 'seminar', 'workshop', 'industrial_visit')),
    -- General: the title; seminar: the topic; workshop: its title; visit: the company.
    title        text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description  text NOT NULL DEFAULT '' CHECK (length(description) <= 4000),
    occurred_on  date NOT NULL,
    status       text NOT NULL DEFAULT 'draft'
                 CHECK (status IN ('draft', 'submitted', 'hod_review', 'approved', 'rejected')),
    points       smallint CHECK (points BETWEEN 0 AND 20),
    -- The latest reviewer's note to the student (why it was returned, rejected...).
    review_note  text CHECK (length(review_note) <= 1000),
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    submitted_at timestamptz,
    decided_at   timestamptz,
    CHECK ((status = 'approved') = (points IS NOT NULL))
);
CREATE INDEX ON activities.activities (student_id, kind);
CREATE INDEX ON activities.activities (status, submitted_at);

CREATE TABLE activities.general (
    activity_id uuid PRIMARY KEY REFERENCES activities.activities ON DELETE CASCADE,
    category    text NOT NULL CHECK (category IN ('sports', 'cultural', 'technical', 'social_service', 'other')),
    organiser   text NOT NULL CHECK (length(organiser) BETWEEN 1 AND 200),
    role        text NOT NULL CHECK (role IN ('participant', 'organiser', 'volunteer', 'winner', 'runner_up')),
    level       text NOT NULL CHECK (level IN ('college', 'state', 'national', 'international')),
    award       text CHECK (length(award) <= 200)
);

CREATE TABLE activities.seminars (
    activity_id    uuid PRIMARY KEY REFERENCES activities.activities ON DELETE CASCADE,
    speaker        text NOT NULL CHECK (length(speaker) BETWEEN 1 AND 200),
    organisation   text NOT NULL CHECK (length(organisation) BETWEEN 1 AND 200),
    mode           text NOT NULL CHECK (mode IN ('online', 'offline')),
    duration_hours numeric(4, 1) NOT NULL CHECK (duration_hours > 0 AND duration_hours <= 100)
);

CREATE TABLE activities.workshops (
    activity_id     uuid PRIMARY KEY REFERENCES activities.activities ON DELETE CASCADE,
    conducting_body text NOT NULL CHECK (length(conducting_body) BETWEEN 1 AND 200),
    to_on           date NOT NULL,
    hours           numeric(5, 1) NOT NULL CHECK (hours > 0 AND hours <= 500),
    skills          text[] NOT NULL DEFAULT '{}' CHECK (cardinality(skills) <= 20),
    certificate     boolean NOT NULL DEFAULT false
);

CREATE TABLE activities.industrial_visits (
    activity_id          uuid PRIMARY KEY REFERENCES activities.activities ON DELETE CASCADE,
    location             text NOT NULL CHECK (length(location) BETWEEN 1 AND 200),
    sector               text NOT NULL CHECK (length(sector) BETWEEN 1 AND 100),
    accompanying_faculty text NOT NULL CHECK (length(accompanying_faculty) BETWEEN 1 AND 200),
    report               text NOT NULL DEFAULT '' CHECK (length(report) <= 8000)
);

-- Evidence: certificates, photos, letters. Up to five per activity.
CREATE TABLE activities.attachments (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    activity_id  uuid NOT NULL REFERENCES activities.activities ON DELETE CASCADE,
    object       text NOT NULL,
    name         text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    content_type text NOT NULL CHECK (content_type IN ('application/pdf', 'image/png', 'image/jpeg')),
    size_bytes   integer NOT NULL CHECK (size_bytes > 0),
    uploaded_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON activities.attachments (activity_id);

-- Every move through the workflow, by whom, with the note.
CREATE TABLE activities.reviews (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    activity_id uuid NOT NULL REFERENCES activities.activities ON DELETE CASCADE,
    actor_id    uuid NOT NULL REFERENCES identity.users,
    from_status text NOT NULL,
    to_status   text NOT NULL,
    points      smallint,
    note        text CHECK (length(note) <= 1000),
    at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON activities.reviews (activity_id, at);

-- Suggested points, which the HOD may change on approval.
CREATE FUNCTION activities.suggested_points(p_activity uuid) RETURNS smallint
LANGUAGE sql STABLE
AS $$
    SELECT CASE a.kind
        WHEN 'general' THEN CASE g.level WHEN 'international' THEN 10 WHEN 'national' THEN 8
                                         WHEN 'state' THEN 5 ELSE 2 END
                            + CASE WHEN g.role IN ('winner', 'runner_up') THEN 2 ELSE 0 END
        WHEN 'seminar' THEN 2
        WHEN 'workshop' THEN least(8, 2 + floor(w.hours / 8))::int
        ELSE 3 END::smallint
    FROM activities.activities a
    LEFT JOIN activities.general g ON g.activity_id = a.id
    LEFT JOIN activities.workshops w ON w.activity_id = a.id
    WHERE a.id = p_activity
$$;

-- ── Who may move an activity where ──────────────────────────────────────────
-- Row-level security decides which rows each role sees and may update; this
-- trigger checks the move itself, so no column grant can be used to skip a
-- step: students edit drafts and submit them; faculty forward or return
-- submissions; HODs approve, reject or return. Points only on approval.
CREATE FUNCTION activities.check_transition() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_role text := identity.current_app_role();
    v_content_changed boolean := (NEW.title, NEW.description, NEW.occurred_on, NEW.kind, NEW.student_id)
                                 IS DISTINCT FROM (OLD.title, OLD.description, OLD.occurred_on, OLD.kind, OLD.student_id);
BEGIN
    IF v_role = 'student' THEN
        IF OLD.status <> 'draft' OR NEW.status NOT IN ('draft', 'submitted')
           OR NEW.points IS DISTINCT FROM OLD.points OR NEW.review_note IS DISTINCT FROM OLD.review_note THEN
            RAISE EXCEPTION 'students edit and submit drafts only' USING ERRCODE = 'check_violation';
        END IF;
    ELSIF v_role IN ('faculty', 'hod', 'super_admin') THEN
        IF v_content_changed THEN
            RAISE EXCEPTION 'reviewers don''t edit the activity' USING ERRCODE = 'check_violation';
        END IF;
        IF NOT ((OLD.status = 'submitted' AND NEW.status IN ('hod_review', 'draft') AND v_role IN ('faculty', 'hod', 'super_admin'))
                OR (OLD.status = 'hod_review' AND NEW.status IN ('approved', 'rejected', 'draft') AND v_role IN ('hod', 'super_admin'))) THEN
            RAISE EXCEPTION 'that move isn''t allowed from %', OLD.status USING ERRCODE = 'check_violation';
        END IF;
        IF NEW.status <> 'approved' AND NEW.points IS NOT NULL THEN
            RAISE EXCEPTION 'points are given on approval' USING ERRCODE = 'check_violation';
        END IF;
    ELSE
        RAISE EXCEPTION 'not allowed' USING ERRCODE = 'insufficient_privilege';
    END IF;
    NEW.updated_at := now();
    IF NEW.status = 'submitted' AND OLD.status = 'draft' THEN NEW.submitted_at := now(); END IF;
    IF NEW.status IN ('approved', 'rejected') THEN NEW.decided_at := now(); END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER check_transition BEFORE UPDATE ON activities.activities
    FOR EACH ROW EXECUTE FUNCTION activities.check_transition();

-- Is the acting faculty member in this student's program?
CREATE FUNCTION people.in_my_program(p_student uuid) RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT EXISTS (SELECT 1 FROM people.students s JOIN people.faculty f ON f.program_id = s.program_id
                     WHERE s.user_id = p_student AND f.user_id = identity.current_user_id()) $$;
GRANT EXECUTE ON FUNCTION people.in_my_program(uuid) TO acadex_app;
GRANT EXECUTE ON FUNCTION activities.suggested_points(uuid) TO acadex_app;

-- ── Access ──────────────────────────────────────────────────────────────────
GRANT SELECT, INSERT, DELETE ON activities.activities TO acadex_app;
GRANT UPDATE (title, description, occurred_on, status, points, review_note) ON activities.activities TO acadex_app;
ALTER TABLE activities.activities ENABLE ROW LEVEL SECURITY;

CREATE POLICY student_owns ON activities.activities FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id());
CREATE POLICY student_creates_drafts ON activities.activities FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
                AND status = 'draft' AND points IS NULL);
CREATE POLICY student_edits_drafts ON activities.activities FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id() AND status = 'draft')
    WITH CHECK (student_id = identity.current_user_id());
CREATE POLICY student_deletes_drafts ON activities.activities FOR DELETE TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id() AND status = 'draft');

-- Faculty see their program's activities once submitted (including ones
-- returned to the student, which stay drafts until resubmitted), and act on
-- submissions. A never-submitted draft is the student's alone.
CREATE POLICY faculty_reads_program ON activities.activities FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'faculty' AND submitted_at IS NOT NULL AND people.in_my_program(student_id));
CREATE POLICY faculty_forwards ON activities.activities FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'faculty' AND status = 'submitted' AND people.in_my_program(student_id))
    WITH CHECK (people.in_my_program(student_id));

-- HODs see their department's, and act on submissions and on what's forwarded.
CREATE POLICY hod_reads_department ON activities.activities FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'hod' AND submitted_at IS NOT NULL AND people.in_my_department(student_id));
CREATE POLICY hod_decides ON activities.activities FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'hod' AND status IN ('submitted', 'hod_review') AND people.in_my_department(student_id))
    WITH CHECK (people.in_my_department(student_id));

CREATE POLICY super_admin_all ON activities.activities FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'super_admin') WITH CHECK (identity.current_app_role() = 'super_admin');

-- Type tables and attachments follow their activity: visible when it is,
-- written only while it's the student's own draft.
DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['general', 'seminars', 'workshops', 'industrial_visits', 'attachments'] LOOP
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON activities.%I TO acadex_app', t);
        EXECUTE format('ALTER TABLE activities.%I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format($p$CREATE POLICY follows_activity ON activities.%I FOR SELECT TO acadex_app
                          USING (activity_id IN (SELECT id FROM activities.activities))$p$, t);
        EXECUTE format($p$CREATE POLICY owner_edits_draft ON activities.%I FOR ALL TO acadex_app
                          USING (activity_id IN (SELECT id FROM activities.activities
                                                 WHERE status = 'draft' AND student_id = identity.current_user_id()))
                          WITH CHECK (activity_id IN (SELECT id FROM activities.activities
                                                      WHERE status = 'draft' AND student_id = identity.current_user_id()))$p$, t);
    END LOOP;
END $$;

GRANT SELECT, INSERT ON activities.reviews TO acadex_app;
ALTER TABLE activities.reviews ENABLE ROW LEVEL SECURITY;
CREATE POLICY follows_activity ON activities.reviews FOR SELECT TO acadex_app
    USING (activity_id IN (SELECT id FROM activities.activities));
CREATE POLICY actor_records ON activities.reviews FOR INSERT TO acadex_app
    WITH CHECK (actor_id = identity.current_user_id() AND activity_id IN (SELECT id FROM activities.activities));
