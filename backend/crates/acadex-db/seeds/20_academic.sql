-- F2: subject types, the AY 2026-27 calendar, MOOCs, and an attendance
-- correction waiting for the ECE HOD.

BEGIN;

INSERT INTO academic.calendar_events (institution_id, program_id, kind, title, description, starts_on, ends_on, created_by)
SELECT 1, program_id, kind::academic.event_kind, title, description, starts_on, ends_on, u.id
FROM (VALUES
    (NULL::smallint, 'event',    'Semester 5 classes begin', NULL::text, date '2026-07-13', date '2026-07-13'),
    (NULL,           'holiday',  'Independence Day', NULL, '2026-08-15', '2026-08-15'),
    (NULL,           'exam',     'Mid-semester examinations', 'Timetable on the notice board.', '2026-09-07', '2026-09-12'),
    (NULL,           'holiday',  'Ganesh Chaturthi', NULL, '2026-09-14', '2026-09-14'),
    (NULL,           'holiday',  'Gandhi Jayanti', NULL, '2026-10-02', '2026-10-02'),
    (NULL,           'deadline', 'Semester 5 examination forms', 'Late forms attract a late fee.', '2026-10-12', '2026-10-20'),
    (NULL,           'holiday',  'Diwali break', NULL, '2026-10-18', '2026-10-22'),
    (12,             'class_suspension', 'Classes suspended: placement drive', 'CSE classes resume the next day.', '2026-10-27', '2026-10-27'),
    (11,             'event',    'ECE project expo', 'Semester 5 mini projects, seminar hall.', '2026-10-30', '2026-10-30'),
    (NULL,           'deadline', 'Last day of instruction', NULL, '2026-11-07', '2026-11-07'),
    (NULL,           'exam',     'Semester-end examinations', 'Hall tickets from 9 November.', '2026-11-16', '2026-12-04'),
    (NULL,           'holiday',  'Christmas', NULL, '2026-12-25', '2026-12-25'),
    (NULL,           'event',    'Semester 6 classes begin', NULL, '2027-01-04', '2027-01-04'),
    (NULL,           'holiday',  'Republic Day', NULL, '2027-01-26', '2027-01-26'),
    (NULL,           'event',    'Acadex Techfest', 'Three days of competitions and talks.', '2027-02-12', '2027-02-14')
) e(program_id, kind, title, description, starts_on, ends_on)
CROSS JOIN identity.users u WHERE u.email = 'examcell@northgate.example';

INSERT INTO academic.mooc_courses (institution_id, provider, code, title, weeks, credits, url, starts_on) VALUES
    (1, 'NPTEL',    'noc26-ee41', 'Digital Signal Processing and its Applications', 12, 3, 'https://onlinecourses.nptel.ac.in/', '2026-07-20'),
    (1, 'NPTEL',    'noc26-cs62', 'Introduction to Machine Learning', 12, 3, 'https://onlinecourses.nptel.ac.in/', '2026-07-20'),
    (1, 'NPTEL',    'noc26-ec15', 'VLSI Physical Design', 8, 2, 'https://onlinecourses.nptel.ac.in/', '2026-08-03'),
    (1, 'NPTEL',    'noc26-cs91', 'Cloud Computing', 8, 2, 'https://onlinecourses.nptel.ac.in/', '2026-08-03'),
    (1, 'SWAYAM',   'sw26-hs03', 'Professional Ethics', 4, 1, 'https://swayam.gov.in/', '2026-07-27'),
    (1, 'SWAYAM',   'sw26-mg11', 'Entrepreneurship', 8, 2, 'https://swayam.gov.in/', '2026-08-10'),
    (1, 'Coursera', 'crs-gcp-01', 'Google Cloud Fundamentals: Core Infrastructure', 4, 1, 'https://www.coursera.org/', NULL),
    (1, 'Coursera', 'crs-dl-01', 'Neural Networks and Deep Learning', 4, 2, 'https://www.coursera.org/', NULL);

-- Atharva's DSP credit is approved; Sanika's ML request waits for her HOD.
INSERT INTO academic.mooc_enrolments
    (course_id, student_id, status, certificate_object, certificate_name, score_pct, enrolled_at, requested_at, decided_by, decided_at, decision_note)
SELECT c.id, st.user_id, x.status, 'seed/sample-certificate.pdf', 'NPTEL certificate.pdf', x.score,
       timestamptz '2026-07-21 10:00+05:30', x.requested::timestamptz,
       CASE WHEN x.status = 'approved' THEN h.id END,
       CASE WHEN x.status = 'approved' THEN timestamptz '2026-09-25 12:00+05:30' END,
       CASE WHEN x.status = 'approved' THEN 'Elite certificate; counts as an open elective.' END
FROM (VALUES ('ECE24001', 'noc26-ee41', 'approved', 86.0, '2026-09-22 18:00+05:30'),
             ('ECE24002', 'noc26-cs62', 'credit_requested', 71.0, '2026-10-01 20:15+05:30')) x(roll, code, status, score, requested)
JOIN people.students st ON st.roll_no = x.roll
JOIN academic.mooc_courses c ON c.code = x.code
CROSS JOIN identity.users h WHERE h.email = 'hod.ece@northgate.example';

-- Sanika says she was at the inter-college quiz for one EC501 session she
-- was marked absent for.
INSERT INTO academic.attendance_corrections
    (program_id, academic_year, student_id, offering_id, session_date, slot, current_status, requested_status, reason, created_at)
SELECT a.program_id, a.academic_year, a.student_id, a.offering_id, a.session_date, a.slot, a.status, 'excused',
       'Represented the college at the inter-college technical quiz; the letter from the organisers is with the department office.',
       timestamptz '2026-09-29 11:30+05:30'
FROM academic.attendance a
JOIN people.students st ON st.user_id = a.student_id AND st.roll_no = 'ECE24002'
JOIN academic.offerings o ON o.id = a.offering_id
JOIN academic.subjects s ON s.id = o.subject_id AND s.code = 'EC501'
WHERE a.academic_year = 2026 AND a.status = 'absent'
ORDER BY a.session_date DESC
LIMIT 1;

COMMIT;
