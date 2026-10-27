-- F3c: two-person publishing and backlog results.
--
-- Demo login added here — create it in Auth0 to sign in as it:
--   coe@northgate.example          Dr. Vikram Bhosale, Controller of Examinations
--                            (exam cell; approves what the exam cell office requests)
--
-- Published results get the record two-person approval now keeps: the exam
-- cell office asked, the Controller approved. Sanika failed EC104 in semester 1
-- and cleared it in the backlog exam the next year; EC304 is still a backlog.
-- Semester 4 stays staged for the live demo: examcell requests, coe approves.

BEGIN;

INSERT INTO identity.users (institution_id, email, display_name, role)
VALUES (1, 'coe@northgate.example', 'Dr. Vikram Bhosale', 'exam_cell')
ON CONFLICT (email) DO NOTHING;

UPDATE exams.result_publications rp
SET requested_by = office.id, requested_at = rp.published_at - interval '1 day', published_by = coe.id
FROM identity.users office, identity.users coe
WHERE office.email = 'examcell@northgate.example' AND coe.email = 'coe@northgate.example'
  AND rp.published_at IS NOT NULL AND rp.requested_by IS NULL;
ALTER TABLE exams.result_publications VALIDATE CONSTRAINT publication_needs_two_people;

-- Sanika's regular EC104: 17/60, under the external minimum.
UPDATE exams.external_marks em SET marks = 17
FROM people.students st, academic.offerings o, academic.subjects sub
WHERE st.roll_no = 'ECE24002' AND em.student_id = st.user_id AND em.attempt = 1
  AND o.id = em.offering_id AND sub.id = o.subject_id AND sub.code = 'EC104';

-- The 2025 backlog results for ECE semester 1, published in July 2025.
INSERT INTO exams.result_publications (program_id, semester, academic_year, kind, staged_at,
                                       requested_by, requested_at, published_at, published_by)
SELECT 11, 1, 2025, 'backlog', timestamptz '2025-07-01 10:00+05:30',
       office.id, timestamptz '2025-07-09 10:00+05:30', timestamptz '2025-07-10 10:00+05:30', coe.id
FROM identity.users office, identity.users coe
WHERE office.email = 'examcell@northgate.example' AND coe.email = 'coe@northgate.example';

INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks, attempt)
SELECT st.user_id, o.id, rp.id, 38, 60, 2
FROM people.students st
JOIN academic.subjects sub ON sub.program_id = st.program_id AND sub.code = 'EC104'
JOIN academic.offerings o ON o.subject_id = sub.id
JOIN exams.result_publications rp ON rp.program_id = 11 AND rp.semester = 1 AND rp.academic_year = 2025 AND rp.kind = 'backlog'
WHERE st.roll_no = 'ECE24002';

COMMIT;
