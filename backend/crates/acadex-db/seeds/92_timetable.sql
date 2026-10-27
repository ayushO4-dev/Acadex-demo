-- This year's timetable for semester 5, matching how its attendance was
-- marked: subject n meets in period n, odd-numbered subjects on Monday,
-- Wednesday and Friday, even ones on Tuesday, Thursday and Saturday (the lab
-- subject's periods are practicals). Theory subjects with a practical part
-- have it on Thursday, in period 7 (the first) or 8 (the third).

BEGIN;

INSERT INTO academic.timetable (offering_id, weekday, slot, session_kind, room)
SELECT o.id, d, n, CASE WHEN s.kind = 'lab' THEN 'practical' ELSE 'theory' END,
       CASE s.program_id WHEN 11 THEN 'E-' ELSE 'C-' END || CASE WHEN s.kind = 'lab' THEN 'Lab 2' ELSE '305' END
FROM academic.subjects s
JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = 2026
CROSS JOIN LATERAL (SELECT right(s.code, 1)::int AS n) x
CROSS JOIN LATERAL unnest(CASE WHEN x.n % 2 = 1 THEN ARRAY[1, 3, 5] ELSE ARRAY[2, 4, 6] END) d
WHERE s.semester = 5;

INSERT INTO academic.timetable (offering_id, weekday, slot, session_kind, room)
SELECT o.id, 4, CASE WHEN s.code LIKE '%01' THEN 7 ELSE 8 END, 'practical',
       CASE s.program_id WHEN 11 THEN 'E-Lab 1' ELSE 'C-Lab 1' END
FROM academic.subjects s
JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = 2026
WHERE s.semester = 5 AND s.kind = 'theory' AND s.has_practical;

COMMIT;
