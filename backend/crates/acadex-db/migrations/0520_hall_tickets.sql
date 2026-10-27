-- Hall tickets (architecture.md §9.3): generated in bulk by the worker before
-- release, one per seated student, each with a QR code to /verify/{id}.

CREATE TABLE exams.hall_tickets (
    -- Also the verification code in the QR: random, so it can't be guessed.
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id   uuid NOT NULL REFERENCES exams.exam_sessions,
    student_id   uuid NOT NULL REFERENCES people.students,
    pdf_object   text NOT NULL,
    generated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (session_id, student_id)
);

GRANT SELECT, INSERT ON exams.hall_tickets TO acadex_app;
ALTER TABLE exams.hall_tickets ENABLE ROW LEVEL SECURITY;
CREATE POLICY student_reads_own_released ON exams.hall_tickets FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'student' AND student_id = identity.current_user_id()
           AND session_id IN (SELECT id FROM exams.exam_sessions WHERE hall_tickets_released_at IS NOT NULL));
CREATE POLICY staff_reads_all ON exams.hall_tickets FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin', 'worker'));
CREATE POLICY worker_issues ON exams.hall_tickets FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() = 'worker');

-- What an invigilator's phone shows after scanning a ticket. No sign-in, so a
-- definer function returning just what's printed on the ticket anyway, and
-- only once tickets are released.
CREATE FUNCTION exams.verify_ticket(p_ticket uuid)
RETURNS TABLE (student_name text, roll_no text, program text, session_name text,
               venue text, seat_no smallint, papers jsonb)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT u.display_name, st.roll_no, p.name, x.name, v.name, se.seat_no,
           coalesce((SELECT jsonb_agg(jsonb_build_object('code', s.code, 'name', s.name,
                                                         'exam_date', sc.exam_date, 'starts_at', sc.starts_at)
                                      ORDER BY sc.exam_date, sc.starts_at)
                     FROM exams.exam_forms f
                     JOIN exams.exam_form_subjects fs ON fs.form_id = f.id
                     JOIN academic.subjects s ON s.id = fs.subject_id
                     JOIN exams.schedule sc ON sc.session_id = f.session_id AND sc.subject_id = s.id
                     WHERE f.session_id = t.session_id AND f.student_id = t.student_id AND f.status = 'submitted'),
                    '[]'::jsonb)
    FROM exams.hall_tickets t
    JOIN exams.exam_sessions x ON x.id = t.session_id AND x.hall_tickets_released_at IS NOT NULL
    JOIN people.students st ON st.user_id = t.student_id
    JOIN identity.users u ON u.id = t.student_id
    JOIN org.programs p ON p.id = st.program_id
    LEFT JOIN exams.seating se ON se.session_id = t.session_id AND se.student_id = t.student_id
    LEFT JOIN exams.venues v ON v.id = se.venue_id
    WHERE t.id = p_ticket
$$;
REVOKE EXECUTE ON FUNCTION exams.verify_ticket(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION exams.verify_ticket(uuid) TO acadex_app;

-- Release: the exam cell, once every seated student has a ticket.
GRANT UPDATE (hall_tickets_released_at, released_by, hall_tickets_generated_at) ON exams.exam_sessions TO acadex_app;
