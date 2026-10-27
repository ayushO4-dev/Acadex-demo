-- Theory and practical attendance logged separately. In semesters 3 and 5
-- the first and third theory subjects of each program have a practical
-- component, held on Thursdays in periods 7 and 8. Each student's practical
-- attendance follows their own theory attendance in that subject, give or
-- take. (Lab subjects were already all practical: migration 0720.)
--
-- For the demo: Sanika's EC501 practicals are in shortage although her theory
-- attendance there is fine, so the separate threshold shows.

BEGIN;
SELECT setseed(0.9009);

UPDATE academic.subjects s SET has_practical = true
FROM (SELECT id, row_number() OVER (PARTITION BY program_id, semester ORDER BY code) AS n
      FROM academic.subjects WHERE semester IN (3, 5) AND kind = 'theory') t
WHERE s.id = t.id AND t.n IN (1, 3);

-- Every Thursday of each term so far: period 7 for the first subject, 8 for the third.
CREATE TEMP TABLE seed_practicals AS
SELECT st.program_id, o.academic_year, st.user_id AS student_id, o.id AS offering_id, day::date AS session_date,
       CASE WHEN s.code LIKE '%01' THEN 7 ELSE 8 END::smallint AS slot, o.faculty_id,
       coalesce((SELECT avg(CASE WHEN a.status IN ('present', 'late') THEN 1.0 ELSE 0.0 END)
                 FROM academic.attendance a
                 WHERE a.program_id = st.program_id AND a.academic_year = o.academic_year
                   AND a.student_id = st.user_id AND a.offering_id = o.id), 0.85) AS habit,
       st.roll_no, s.code
FROM academic.subjects s
JOIN academic.offerings o ON o.subject_id = s.id
JOIN people.students st ON st.program_id = s.program_id AND st.batch_year + (s.semester - 1) / 2 = o.academic_year
CROSS JOIN LATERAL generate_series(
    CASE o.academic_year WHEN 2025 THEN date '2025-07-17' ELSE date '2026-07-16' END,
    CASE o.academic_year WHEN 2025 THEN date '2025-11-06' ELSE date '2026-10-01' END,
    interval '7 days') day
WHERE s.has_practical AND o.academic_year IN (2025, 2026);

INSERT INTO academic.attendance (program_id, academic_year, student_id, offering_id, session_date, slot, status,
                                 marked_by, marked_at, session_kind)
SELECT program_id, academic_year, student_id, offering_id, session_date, slot,
       CASE WHEN roll_no = 'ECE24002' AND code = 'EC501' THEN
                CASE WHEN random() < 0.55 THEN 'present' ELSE 'absent' END
            WHEN random() < least(0.99, habit + (random() - 0.5) * 0.1) THEN 'present'
            ELSE 'absent' END::academic.attendance_status,
       faculty_id, session_date + time '18:00', 'practical'
FROM seed_practicals
ON CONFLICT DO NOTHING;

COMMIT;
