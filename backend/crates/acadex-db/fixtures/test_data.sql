-- Small, fixed dataset for the Rust tests. Applied as the owner after migrations.
--
--   A, B   BTech ECE students          C   BTech CSE student
--   F      ECE faculty, teaches EC501   G   ECE faculty, teaches EC502
--   E, E2  exam cell (publishing needs two of them)
--   Auth0 subjects auth0|a, auth0|b, auth0|f, auth0|e, auth0|e2 are already linked;
--   C, G and unlinked@acadex.test have never signed in.

INSERT INTO org.institutions VALUES (1, 'Test Institute');
INSERT INTO org.programs VALUES (11, 1, 'btech_ece', 'B.Tech ECE'), (12, 1, 'btech_cse', 'B.Tech CSE');

INSERT INTO identity.users (id, institution_id, email, display_name, role) VALUES
    ('a0000000-0000-0000-0000-00000000000a', 1, 'a@acadex.test', 'Student A', 'student'),
    ('b0000000-0000-0000-0000-00000000000b', 1, 'b@acadex.test', 'Student B', 'student'),
    ('c0000000-0000-0000-0000-00000000000c', 1, 'c@acadex.test', 'Student C', 'student'),
    ('f0000000-0000-0000-0000-00000000000f', 1, 'f@acadex.test', 'Faculty F', 'faculty'),
    ('90000000-0000-0000-0000-000000000009', 1, 'g@acadex.test', 'Faculty G', 'faculty'),
    ('e0000000-0000-0000-0000-00000000000e', 1, 'e@acadex.test', 'Exam Cell', 'exam_cell'),
    ('e2000000-0000-0000-0000-0000000000e2', 1, 'e2@acadex.test', 'Controller E2', 'exam_cell'),
    ('d0000000-0000-0000-0000-00000000000d', 1, 'unlinked@acadex.test', 'Not Yet Signed In', 'student');

INSERT INTO identity.user_identities (provider_subject, user_id) VALUES
    ('auth0|a', 'a0000000-0000-0000-0000-00000000000a'),
    ('auth0|b', 'b0000000-0000-0000-0000-00000000000b'),
    ('auth0|f', 'f0000000-0000-0000-0000-00000000000f'),
    ('auth0|e', 'e0000000-0000-0000-0000-00000000000e'),
    ('auth0|e2', 'e2000000-0000-0000-0000-0000000000e2');

INSERT INTO people.students (user_id, program_id, roll_no, batch_year, current_semester) VALUES
    ('a0000000-0000-0000-0000-00000000000a', 11, 'ECE24001', 2024, 5),
    ('b0000000-0000-0000-0000-00000000000b', 11, 'ECE24002', 2024, 5),
    ('c0000000-0000-0000-0000-00000000000c', 12, 'CSE24001', 2024, 5),
    ('d0000000-0000-0000-0000-00000000000d', 11, 'ECE24003', 2024, 5);

INSERT INTO people.faculty (user_id, program_id, designation) VALUES
    ('f0000000-0000-0000-0000-00000000000f', 11, 'Professor'),
    ('90000000-0000-0000-0000-000000000009', 11, 'Assistant Professor');

INSERT INTO academic.subjects (id, program_id, code, name, credits, semester) VALUES
    ('50000000-0000-0000-0000-000000000501', 11, 'EC501', 'Digital Communication', 4, 5),
    ('50000000-0000-0000-0000-000000000502', 11, 'EC502', 'Microprocessors', 4, 5),
    ('50000000-0000-0000-0000-000000000301', 11, 'EC301', 'Electronic Devices', 4, 3),
    ('50000000-0000-0000-0000-000000000c51', 12, 'CS501', 'Operating Systems', 4, 5);

INSERT INTO academic.offerings (id, subject_id, academic_year, faculty_id) VALUES
    ('0f000000-0000-0000-0000-000000000501', '50000000-0000-0000-0000-000000000501', 2026, 'f0000000-0000-0000-0000-00000000000f'),
    ('0f000000-0000-0000-0000-000000000502', '50000000-0000-0000-0000-000000000502', 2026, '90000000-0000-0000-0000-000000000009'),
    ('0f000000-0000-0000-0000-000000000301', '50000000-0000-0000-0000-000000000301', 2025, 'f0000000-0000-0000-0000-00000000000f'),
    ('0f000000-0000-0000-0000-000000000c51', '50000000-0000-0000-0000-000000000c51', 2026, '90000000-0000-0000-0000-000000000009');

SELECT academic.ensure_attendance_partition(p, y) FROM (VALUES (11, 2025), (11, 2026), (12, 2026)) v(p, y);

-- A and B: 4 sessions of EC501 and EC502 in 2026 (A attends all, B misses half),
-- plus EC301 in 2025. C: CS501 in 2026.
INSERT INTO academic.attendance (program_id, academic_year, student_id, offering_id, session_date, slot, status, marked_by)
SELECT st.program_id, o.academic_year, st.user_id, o.id, date '2026-07-13' + d, 1,
       CASE WHEN st.roll_no = 'ECE24002' AND d % 2 = 1 THEN 'absent' ELSE 'present' END::academic.attendance_status,
       o.faculty_id
FROM people.students st
JOIN academic.offerings o ON true
JOIN academic.subjects sub ON sub.id = o.subject_id AND sub.program_id = st.program_id
CROSS JOIN generate_series(0, 3) d
WHERE st.roll_no IN ('ECE24001', 'ECE24002', 'CSE24001');

-- Notices: one of each visibility case, all by the exam cell.
INSERT INTO comms.notices (id, institution_id, program_id, severity, title, body, published_at, expires_at, created_by) VALUES
    ('70000000-0000-0000-0000-000000000001', 1, NULL, 'general',   'Campus open day', 'All welcome.',      now() - interval '2 days', NULL,                     'e0000000-0000-0000-0000-00000000000e'),
    ('70000000-0000-0000-0000-000000000002', 1, 11,   'important', 'ECE lab moved',   'Now in block C.',   now() - interval '1 day',  NULL,                     'e0000000-0000-0000-0000-00000000000e'),
    ('70000000-0000-0000-0000-000000000003', 1, 12,   'critical',  'CSE exam moved',  'New date Monday.',  now() - interval '1 day',  NULL,                     'e0000000-0000-0000-0000-00000000000e'),
    ('70000000-0000-0000-0000-000000000004', 1, NULL, 'general',   'Old notice',      'Expired.',          now() - interval '9 days', now() - interval '1 day', 'e0000000-0000-0000-0000-00000000000e'),
    ('70000000-0000-0000-0000-000000000005', 1, NULL, 'general',   'Next week',       'Scheduled.',        now() + interval '7 days', NULL,                     'e0000000-0000-0000-0000-00000000000e');

-- Results. A 10-point absolute scale; passing needs 40% of the external exam.
INSERT INTO exams.grade_scale VALUES
    (1, 90, 'O', 10), (1, 80, 'A+', 9), (1, 70, 'A', 8), (1, 60, 'B+', 7),
    (1, 50, 'B', 6), (1, 45, 'C', 5), (1, 40, 'P', 4), (1, 0, 'F', 0);
INSERT INTO exams.grading_policy VALUES (1, 40);

-- EC401, semester 4 (AY 2025), taught by G. Added after the attendance rows,
-- so it has none.
INSERT INTO academic.subjects (id, program_id, code, name, credits, semester) VALUES
    ('50000000-0000-0000-0000-000000000401', 11, 'EC401', 'Analog Circuits', 3, 4);
INSERT INTO academic.offerings (id, subject_id, academic_year, faculty_id) VALUES
    ('0f000000-0000-0000-0000-000000000401', '50000000-0000-0000-0000-000000000401', 2025, '90000000-0000-0000-0000-000000000009');

-- ECE semester 3 is published; semester 4 is staged.
INSERT INTO exams.result_publications (id, program_id, semester, academic_year, requested_by, requested_at, published_at, published_by) VALUES
    ('7e000000-0000-0000-0000-000000000003', 11, 3, 2025, 'e2000000-0000-0000-0000-0000000000e2', now() - interval '31 days',
     now() - interval '30 days', 'e0000000-0000-0000-0000-00000000000e'),
    ('7e000000-0000-0000-0000-000000000004', 11, 4, 2025, NULL, NULL, NULL, NULL);

INSERT INTO exams.internal_components (id, offering_id, code, max_marks, position) VALUES
    ('c0000000-0000-0000-0000-000000000301', '0f000000-0000-0000-0000-000000000301', 'CCA1', 10, 1),
    ('c0000000-0000-0000-0000-000000000302', '0f000000-0000-0000-0000-000000000301', 'MID',  30, 2),
    ('c0000000-0000-0000-0000-000000000401', '0f000000-0000-0000-0000-000000000401', 'CCA1', 20, 1),
    ('c0000000-0000-0000-0000-000000000402', '0f000000-0000-0000-0000-000000000401', 'CCA2', 20, 2);

-- A: EC301 9 + 27 + 54 = 90 (O); EC401 15 + 15 + 45 = 75 (A).
-- B: EC301 5 + 15 + 18 = 38, and 18/60 is under the external minimum (F);
--    EC401 10 + 10 + 30 = 50 (B).
INSERT INTO exams.internal_marks (student_id, component_id, marks) VALUES
    ('a0000000-0000-0000-0000-00000000000a', 'c0000000-0000-0000-0000-000000000301', 9),
    ('a0000000-0000-0000-0000-00000000000a', 'c0000000-0000-0000-0000-000000000302', 27),
    ('a0000000-0000-0000-0000-00000000000a', 'c0000000-0000-0000-0000-000000000401', 15),
    ('a0000000-0000-0000-0000-00000000000a', 'c0000000-0000-0000-0000-000000000402', 15),
    ('b0000000-0000-0000-0000-00000000000b', 'c0000000-0000-0000-0000-000000000301', 5),
    ('b0000000-0000-0000-0000-00000000000b', 'c0000000-0000-0000-0000-000000000302', 15),
    ('b0000000-0000-0000-0000-00000000000b', 'c0000000-0000-0000-0000-000000000401', 10),
    ('b0000000-0000-0000-0000-00000000000b', 'c0000000-0000-0000-0000-000000000402', 10);
INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks) VALUES
    ('a0000000-0000-0000-0000-00000000000a', '0f000000-0000-0000-0000-000000000301', '7e000000-0000-0000-0000-000000000003', 54, 60),
    ('a0000000-0000-0000-0000-00000000000a', '0f000000-0000-0000-0000-000000000401', '7e000000-0000-0000-0000-000000000004', 45, 60),
    ('b0000000-0000-0000-0000-00000000000b', '0f000000-0000-0000-0000-000000000301', '7e000000-0000-0000-0000-000000000003', 18, 60),
    ('b0000000-0000-0000-0000-00000000000b', '0f000000-0000-0000-0000-000000000401', '7e000000-0000-0000-0000-000000000004', 30, 60);

-- Fees, AY 2026. Ledger ids are fixed so tests can name the charges.
INSERT INTO fees.ledger (id, student_id, academic_year, kind, particulars, amount_paise, due_on) OVERRIDING SYSTEM VALUE VALUES
    (1001, 'a0000000-0000-0000-0000-00000000000a', 2026, 'charge', 'Tuition fee — Semester 5',     6250000, '2026-07-31'),
    (1002, 'a0000000-0000-0000-0000-00000000000a', 2026, 'charge', 'Examination fee — Semester 5',  240000, '2026-10-20'),
    (1003, 'b0000000-0000-0000-0000-00000000000b', 2026, 'charge', 'Tuition fee — Semester 5',     6250000, '2026-07-31');
INSERT INTO fees.ledger (id, student_id, academic_year, kind, charge_id, amount_paise, remarks) OVERRIDING SYSTEM VALUE VALUES
    (1004, 'b0000000-0000-0000-0000-00000000000b', 2026, 'concession', 1003, 1562500, 'Merit scholarship (25%)');
SELECT setval(pg_get_serial_sequence('fees.ledger', 'id'), 2000);

-- Roles beyond student, faculty and exam cell (architecture.md §7.3).
--   H  HOD of ECE (also on the faculty)   M  admin   S  super admin   K  accounts   P  support
--   V  the platform operator (the vendor, not the university)
INSERT INTO identity.users (id, institution_id, email, display_name, role) VALUES
    ('40000000-0000-0000-0000-00000000004d', 1, 'h@acadex.test', 'HOD H', 'hod'),
    ('ad000000-0000-0000-0000-0000000000ad', 1, 'm@acadex.test', 'Admin M', 'admin'),
    ('5a000000-0000-0000-0000-00000000005a', 1, 's@acadex.test', 'Super S', 'super_admin'),
    ('ac000000-0000-0000-0000-0000000000ac', 1, 'k@acadex.test', 'Accounts K', 'accounts'),
    ('5e000000-0000-0000-0000-00000000005e', 1, 'p@acadex.test', 'Support P', 'support_agent'),
    ('fa000000-0000-0000-0000-0000000000fa', 1, 'v@vendor.test', 'Platform V', 'platform_admin');
INSERT INTO identity.user_identities (provider_subject, user_id) VALUES
    ('auth0|h', '40000000-0000-0000-0000-00000000004d'),
    ('auth0|m', 'ad000000-0000-0000-0000-0000000000ad'),
    ('auth0|s', '5a000000-0000-0000-0000-00000000005a'),
    ('auth0|k', 'ac000000-0000-0000-0000-0000000000ac'),
    ('auth0|p', '5e000000-0000-0000-0000-00000000005e'),
    ('auth0|v', 'fa000000-0000-0000-0000-0000000000fa');
INSERT INTO people.faculty (user_id, program_id, designation) VALUES
    ('40000000-0000-0000-0000-00000000004d', 11, 'Professor & HOD');

INSERT INTO org.departments (id, institution_id, code, name, hod_id) VALUES
    (1, 1, 'ece', 'Electronics & Communication', '40000000-0000-0000-0000-00000000004d'),
    (2, 1, 'cse', 'Computer Science', NULL);
UPDATE org.programs SET department_id = CASE id WHEN 11 THEN 1 ELSE 2 END;
UPDATE people.students SET section = 'B' WHERE roll_no = 'ECE24002';

-- No attendance lock in the tests' fixed dates (a test turns it on).
INSERT INTO org.institution_settings (institution_id, attendance_lock_days) VALUES (1, 0);

-- F2. Calendar: one event for everyone, one per program.
INSERT INTO academic.calendar_events (institution_id, program_id, kind, title, starts_on, ends_on) VALUES
    (1, NULL, 'holiday', 'Diwali break', '2026-10-18', '2026-10-22'),
    (1, 11,   'event',   'ECE project expo', '2026-10-05', '2026-10-05'),
    (1, 12,   'exam',    'CSE lab exam', '2026-10-07', '2026-10-07');

INSERT INTO academic.mooc_courses (id, institution_id, provider, code, title, weeks, credits, url) VALUES
    ('3c000000-0000-0000-0000-000000000001', 1, 'NPTEL', 'noc26-ee01', 'Digital Signal Processing', 12, 3,
     'https://onlinecourses.nptel.ac.in/');

-- F3. What each fee charge is for; an exam session whose form window is open
-- now; two small rooms; the timetable.
UPDATE fees.ledger SET fee_head = CASE id WHEN 1002 THEN 'exam' ELSE 'tuition' END WHERE kind = 'charge';
INSERT INTO exams.exam_sessions (id, institution_id, name, academic_year, form_opens_on, form_closes_on, regular_fee_paise, backlog_fee_paise) VALUES
    ('e5000000-0000-0000-0000-000000000001', 1, 'Nov–Dec 2026', 2026, current_date - 1, current_date + 10, 240000, 100000);
INSERT INTO exams.venues (id, institution_id, name, capacity) VALUES (1, 1, 'Room 101', 2), (2, 1, 'Room 102', 2);
INSERT INTO exams.schedule (session_id, subject_id, exam_date, starts_at, duration_minutes) VALUES
    ('e5000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000501', current_date + 40, '10:00', 180),
    ('e5000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000502', current_date + 42, '10:00', 180),
    ('e5000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000301', current_date + 44, '14:00', 180);
