-- An exam session can be cancelled before any student registers (a
-- registration carries a fee charge the accounts office would have to
-- settle). A cancelled session stays on record, with who cancelled it and
-- why, and takes no more registrations, timetable changes, seating or hall
-- tickets. Venues can be renamed or resized, and removed when unused.

ALTER TABLE exams.exam_sessions
    ADD COLUMN cancelled_at  timestamptz,
    ADD COLUMN cancelled_by  uuid REFERENCES identity.users,
    ADD COLUMN cancel_reason text CHECK (length(cancel_reason) BETWEEN 3 AND 500);

-- The backstop for the API's own checks.
CREATE FUNCTION exams.refuse_cancelled_session() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM exams.exam_sessions WHERE id = NEW.session_id AND cancelled_at IS NOT NULL) THEN
        RAISE EXCEPTION 'that exam session is cancelled' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER refuse_cancelled_session BEFORE INSERT ON exams.exam_forms
    FOR EACH ROW EXECUTE FUNCTION exams.refuse_cancelled_session();
CREATE TRIGGER refuse_cancelled_session BEFORE INSERT ON exams.seating
    FOR EACH ROW EXECUTE FUNCTION exams.refuse_cancelled_session();
CREATE TRIGGER refuse_cancelled_session BEFORE INSERT ON exams.schedule
    FOR EACH ROW EXECUTE FUNCTION exams.refuse_cancelled_session();

GRANT DELETE ON exams.venues TO acadex_app;
