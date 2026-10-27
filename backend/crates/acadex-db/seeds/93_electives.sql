-- Professional Elective I for semester 5 in both programs: two options each,
-- meeting together on Monday and Wednesday in period 7. Most students have
-- chosen (about half each way); Sanika Deshpande (student.b) and a few
-- others haven't yet, so choosing shows in the demo. Atharva Joshi
-- (student.a) took Embedded Systems.

BEGIN;
SELECT setseed(0.9393);

-- VLSI Design and Software Engineering were marked electives with no
-- alternative, so everyone took them: they are core subjects.
UPDATE academic.subjects SET kind = 'theory', elective_group = NULL WHERE code IN ('EC505', 'CS505');

INSERT INTO academic.subjects (program_id, code, name, credits, semester, kind, elective_group) VALUES
    (11, 'EC507A', 'Embedded Systems',            3, 5, 'elective', 'Professional Elective I'),
    (11, 'EC507B', 'Optical Fibre Communication', 3, 5, 'elective', 'Professional Elective I'),
    (12, 'CS507A', 'Machine Learning',            3, 5, 'elective', 'Professional Elective I'),
    (12, 'CS507B', 'Cloud Computing',             3, 5, 'elective', 'Professional Elective I');

-- Taught by the program's second and fourth faculty members, free then.
INSERT INTO academic.offerings (subject_id, academic_year, faculty_id)
SELECT s.id, 2026, u.id
FROM academic.subjects s
JOIN identity.users u
  ON u.email = CASE s.program_id WHEN 11 THEN 'ece' ELSE 'cse' END
               || '.faculty' || CASE WHEN s.code LIKE '%A' THEN 2 ELSE 4 END || '@northgate.example'
WHERE s.code IN ('EC507A', 'EC507B', 'CS507A', 'CS507B');

INSERT INTO academic.timetable (offering_id, weekday, slot, session_kind, room)
SELECT o.id, d, 7, 'theory',
       CASE s.program_id WHEN 11 THEN 'E-' ELSE 'C-' END || CASE WHEN s.code LIKE '%A' THEN '401' ELSE '402' END
FROM academic.subjects s
JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = 2026
CROSS JOIN unnest(ARRAY[1, 3]) d
WHERE s.code IN ('EC507A', 'EC507B', 'CS507A', 'CS507B');

INSERT INTO academic.elective_choices (student_id, subject_id, semester, elective_group, chosen_at)
SELECT st.user_id,
       (SELECT s.id FROM academic.subjects s
        WHERE s.program_id = st.program_id AND s.semester = 5 AND s.kind = 'elective'
          AND s.code LIKE CASE WHEN st.roll_no = 'ECE24001' OR (st.roll_no <> 'ECE24001' AND r < 0.48) THEN '%A' ELSE '%B' END),
       5, 'Professional Elective I', timestamptz '2026-07-08 10:00+05:30' + r * interval '5 days'
FROM (SELECT user_id, program_id, roll_no, random() AS r FROM people.students
      WHERE current_semester = 5 AND graduated_on IS NULL) st
WHERE st.roll_no <> 'ECE24002' AND (st.roll_no = 'ECE24001' OR st.r < 0.92);

COMMIT;
