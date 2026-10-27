-- Demo dataset: 1 institution, 2 programs, 500 students, 12 faculty, 60 subjects
-- across semesters 1-5, two academic years of attendance (~240k rows), and
-- results for semesters 1-4 (1-3 published, 4 staged for the exam cell to
-- publish in the demo), this year's fees, and notices. Run with `just seed`; later files in this folder add to it.
-- Deterministic: setseed() makes every run produce the same data.
--
-- Demo logins — create these users in Auth0 to sign in as them:
--   student.a@northgate.example    Atharva Joshi, BTech ECE, comfortably above 75%
--   student.b@northgate.example    Sanika Deshpande, BTech ECE, in attendance shortage, with a backlog in EC304
--   faculty.ece@northgate.example  Dr. Revati Gokhale, teaches EC501 this year (and subject 1 of each earlier semester)
--   examcell@northgate.example     Exam Cell Office
--   coe@northgate.example          Controller of Examinations, approves publishing

BEGIN;
SELECT setseed(0.2026);

INSERT INTO org.institutions (id, name, short_name, city, website)
VALUES (1, 'Northgate University', 'NGU', 'Pune', 'https://northgate.example');
UPDATE org.institutions SET email_domains = ARRAY['northgate.example'] WHERE id = 1;
INSERT INTO org.programs VALUES
    (11, 1, 'btech_ece', 'B.Tech Electronics & Communication Engineering'),
    (12, 1, 'btech_cse', 'B.Tech Computer Science & Engineering');

-- Names: slot k gets first[k mod 160] and a surname stepping through all 110,
-- so no full name repeats, no first name is used more than 4 times and no
-- surname more than 8 (98_names_check.sql holds every seed to at most 10).
CREATE TEMP TABLE names AS SELECT
    ARRAY['Aadesh','Aakash','Aboli','Abhay','Aditi','Advait','Ajinkya','Akshata','Alok','Amruta','Anagha',
          'Aniket','Anjali','Ankita','Anushka','Apurva','Archana','Arnav','Ashwini','Atul','Avantika','Ayush',
          'Bhagyashree','Bhakti','Bhushan','Chaitanya','Chetan','Darshan','Deepali','Devendra','Dhanashree',
          'Dhruv','Disha','Gargi','Gauri','Gautam','Girish','Gunjan','Harsh','Harshada','Hemant','Hrishikesh',
          'Indrajit','Ira','Jai','Janhavi','Jayant','Juhi','Kalyani','Kanak','Ketaki','Ketan','Kiran','Komal',
          'Kunal','Lalit','Madhura','Mahesh','Manasi','Mandar','Manish','Mayur','Megha','Mihir','Mohit',
          'Mrunal','Mukta','Nachiket','Namrata','Narendra','Neel','Nikita','Nilesh','Nishant','Ojas','Onkar',
          'Pallavi','Pankaj','Parag','Parth','Payal','Pooja','Pradnya','Pranav','Prasad','Prathamesh',
          'Pratiksha','Purva','Rachana','Radhika','Rahul','Rajat','Rajeshwari','Rakesh','Ravi','Renuka',
          'Rhea','Rishabh','Ritika','Rohit','Rucha','Rupali','Sachin','Sagar','Sakshi','Saloni','Samruddhi',
          'Sanket','Sanjana','Santosh','Saurabh','Sayali','Shantanu','Sharvari','Shekhar','Shivani',
          'Shraddha','Shreyas','Shruti','Siddhesh','Simran','Snehal','Soham','Sonal','Suhas','Sujata',
          'Sumedh','Sunil','Supriya','Swapnil','Swara','Tanmay','Tejas','Tejaswini','Trupti','Tushar','Uday',
          'Urvi','Vaibhav','Vaishnavi','Varad','Vedant','Vidya','Vinay','Vineet','Vishakha','Vishal','Yash',
          'Yogesh','Zainab','Farhan','Imran','Ayesha','Kabir','Karan','Nikhil','Sneha','Tanvi','Varun',
          'Rohan'] AS first,
    ARRAY['Agarwal','Apte','Athavale','Bapat','Barve','Bhagwat','Bhalerao','Bhide','Bhosale','Borkar',
          'Chandorkar','Chaudhari','Chitale','Dalvi','Damle','Dandekar','Date','Deo','Deshmukh','Deshpande',
          'Dhamdhere','Dixit','Gadgil','Gaikwad','Gandhi','Ghadge','Ghorpade','Godbole','Gokhale','Gore',
          'Gupta','Hegde','Inamdar','Jadhav','Jagtap','Jain','Jog','Joshi','Kadam','Kale','Kamat','Kanade',
          'Karandikar','Karve','Kelkar','Khan','Khare','Kher','Kolhe','Kulkarni','Kumbhar','Lele','Limaye',
          'Mahajan','Mane','Marathe','Mehta','Mhatre','Mistry','Modak','More','Nadkarni','Naik','Nair','Natu',
          'Oak','Paranjpe','Patankar','Patil','Pawar','Phadke','Pingle','Pradhan','Purohit','Raut','Rane',
          'Ranade','Sabnis','Sahasrabudhe','Salunkhe','Sathe','Sawant','Shaikh','Shinde','Shirke','Sohoni',
          'Sule','Surve','Tambe','Thakur','Tilak','Tiwari','Upadhye','Vaidya','Vartak','Vaze','Wagh',
          'Walimbe','Yadav','Zende','Bose','Chatterjee','Iyengar','Menon','Pillai','Reddy','Saxena','Sinha',
          'Varma','Banerjee'] AS last;
CREATE FUNCTION pg_temp.person(k int) RETURNS text LANGUAGE sql AS $$
    SELECT first[1 + k % cardinality(first)] || ' '
           || last[1 + (k * 7 + (k / cardinality(first)) * 13) % cardinality(last)]
    FROM names
$$;

-- ── Students: 250 per program, admitted 2024, now in semester 5 ─────────────
CREATE TEMP TABLE seed_students AS
SELECT p.id AS program_id,
       upper(p.short) || '24' || lpad(n::text, 3, '0') AS roll_no,
       CASE WHEN p.id = 11 AND n = 1 THEN 'student.a@northgate.example'
            WHEN p.id = 11 AND n = 2 THEN 'student.b@northgate.example'
            ELSE p.short || '24' || lpad(n::text, 3, '0') || '@northgate.example' END AS email,
       CASE WHEN p.id = 11 AND n = 1 THEN 'Atharva Joshi'
            WHEN p.id = 11 AND n = 2 THEN 'Sanika Deshpande'
            ELSE pg_temp.person((p.id - 11) * 250 + n) END AS display_name,
       -- Probability of attending a session.
       CASE WHEN p.id = 11 AND n = 1 THEN 0.93
            WHEN p.id = 11 AND n = 2 THEN 0.66
            ELSE 0.62 + random() * 0.36 END AS presence
FROM (VALUES (11, 'ece'), (12, 'cse')) p(id, short), generate_series(1, 250) n;

INSERT INTO identity.users (institution_id, email, display_name, role)
SELECT 1, email, display_name, 'student' FROM seed_students;

INSERT INTO people.students (user_id, program_id, roll_no, batch_year, current_semester)
SELECT u.id, s.program_id, s.roll_no, 2024, 5
FROM seed_students s JOIN identity.users u USING (email);

-- ── Faculty: 6 per program; faculty n teaches subject n in each semester ─────
CREATE TEMP TABLE seed_faculty AS
SELECT p.id AS program_id, n,
       CASE WHEN p.id = 11 AND n = 1 THEN 'faculty.ece@northgate.example'
            ELSE p.short || '.faculty' || n || '@northgate.example' END AS email,
       CASE WHEN p.id = 11 AND n = 1 THEN 'Dr. Revati Gokhale'
            ELSE (ARRAY['Dr. ','Prof. '])[1 + n % 2] || pg_temp.person(500 + (p.id - 11) * 6 + n) END AS display_name
FROM (VALUES (11, 'ece'), (12, 'cse')) p(id, short), generate_series(1, 6) n;

INSERT INTO identity.users (institution_id, email, display_name, role)
SELECT 1, email, display_name, 'faculty' FROM seed_faculty;

INSERT INTO people.faculty (user_id, program_id, designation)
SELECT u.id, f.program_id, CASE WHEN f.n = 1 THEN 'Professor' ELSE 'Assistant Professor' END
FROM seed_faculty f JOIN identity.users u USING (email);

INSERT INTO identity.users (institution_id, email, display_name, role)
VALUES (1, 'examcell@northgate.example', 'Exam Cell Office', 'exam_cell'),
       (1, 'coe@northgate.example', 'Dr. Vikram Bhosale', 'exam_cell');

-- ── Subjects: semesters 1-2 (AY 2024), 3-4 (AY 2025), 5 (AY 2026) ────────────
CREATE TEMP TABLE seed_subjects (program_id smallint, code text, name text, credits smallint, semester smallint, n int);
INSERT INTO seed_subjects VALUES
    (11, 'EC101', 'Engineering Mathematics I',          4, 1, 1),
    (11, 'EC102', 'Engineering Physics',                4, 1, 2),
    (11, 'EC103', 'Basic Electrical Engineering',       4, 1, 3),
    (11, 'EC104', 'Programming in C',                   3, 1, 4),
    (11, 'EC105', 'Engineering Graphics',               3, 1, 5),
    (11, 'EC106', 'Physics Lab',                        1, 1, 6),
    (11, 'EC201', 'Engineering Mathematics II',         4, 2, 1),
    (11, 'EC202', 'Engineering Chemistry',              4, 2, 2),
    (11, 'EC203', 'Basic Electronics',                  4, 2, 3),
    (11, 'EC204', 'Data Structures in C',               3, 2, 4),
    (11, 'EC205', 'Environmental Studies',              3, 2, 5),
    (11, 'EC206', 'Electronics Workshop',               1, 2, 6),
    (11, 'EC401', 'Analog Circuits',                    4, 4, 1),
    (11, 'EC402', 'Analog Communication',               4, 4, 2),
    (11, 'EC403', 'Linear Integrated Circuits',         4, 4, 3),
    (11, 'EC404', 'Probability & Random Processes',     3, 4, 4),
    (11, 'EC405', 'Computer Architecture',              3, 4, 5),
    (11, 'EC406', 'Analog Circuits Lab',                1, 4, 6),
    (12, 'CS101', 'Engineering Mathematics I',          4, 1, 1),
    (12, 'CS102', 'Engineering Physics',                4, 1, 2),
    (12, 'CS103', 'Basic Electrical Engineering',       4, 1, 3),
    (12, 'CS104', 'Programming in C',                   3, 1, 4),
    (12, 'CS105', 'Engineering Graphics',               3, 1, 5),
    (12, 'CS106', 'Physics Lab',                        1, 1, 6),
    (12, 'CS201', 'Engineering Mathematics II',         4, 2, 1),
    (12, 'CS202', 'Engineering Chemistry',              4, 2, 2),
    (12, 'CS203', 'Digital Electronics',                4, 2, 3),
    (12, 'CS204', 'Python Programming',                 3, 2, 4),
    (12, 'CS205', 'Environmental Studies',              3, 2, 5),
    (12, 'CS206', 'Programming Lab',                    1, 2, 6),
    (12, 'CS401', 'Design & Analysis of Algorithms',    4, 4, 1),
    (12, 'CS402', 'Microprocessors',                    4, 4, 2),
    (12, 'CS403', 'Probability & Statistics',           4, 4, 3),
    (12, 'CS404', 'Web Technologies',                   3, 4, 4),
    (12, 'CS405', 'Formal Languages',                   3, 4, 5),
    (12, 'CS406', 'Algorithms Lab',                     1, 4, 6),
    (11, 'EC301', 'Electronic Devices & Circuits',      4, 3, 1),
    (11, 'EC302', 'Digital Logic Design',               4, 3, 2),
    (11, 'EC303', 'Signals & Systems',                  4, 3, 3),
    (11, 'EC304', 'Network Analysis',                   3, 3, 4),
    (11, 'EC305', 'Engineering Mathematics III',        3, 3, 5),
    (11, 'EC306', 'Electronic Devices Lab',             1, 3, 6),
    (11, 'EC501', 'Digital Communication',              4, 5, 1),
    (11, 'EC502', 'Microprocessors & Microcontrollers', 4, 5, 2),
    (11, 'EC503', 'Control Systems',                    4, 5, 3),
    (11, 'EC504', 'Electromagnetic Waves',              3, 5, 4),
    (11, 'EC505', 'VLSI Design',                        3, 5, 5),
    (11, 'EC506', 'Microprocessors Lab',                1, 5, 6),
    (12, 'CS301', 'Data Structures',                    4, 3, 1),
    (12, 'CS302', 'Discrete Mathematics',               4, 3, 2),
    (12, 'CS303', 'Computer Organization',              4, 3, 3),
    (12, 'CS304', 'Object-Oriented Programming',        3, 3, 4),
    (12, 'CS305', 'Engineering Mathematics III',        3, 3, 5),
    (12, 'CS306', 'Data Structures Lab',                1, 3, 6),
    (12, 'CS501', 'Operating Systems',                  4, 5, 1),
    (12, 'CS502', 'Database Management Systems',        4, 5, 2),
    (12, 'CS503', 'Computer Networks',                  4, 5, 3),
    (12, 'CS504', 'Theory of Computation',              3, 5, 4),
    (12, 'CS505', 'Software Engineering',               3, 5, 5),
    (12, 'CS506', 'Database Systems Lab',               1, 5, 6);

INSERT INTO academic.subjects (program_id, code, name, credits, semester)
SELECT program_id, code, name, credits, semester FROM seed_subjects;

INSERT INTO academic.offerings (subject_id, academic_year, faculty_id)
SELECT sub.id, 2024 + (ss.semester - 1) / 2, u.id
FROM seed_subjects ss
JOIN academic.subjects sub ON sub.code = ss.code
JOIN seed_faculty f ON f.program_id = ss.program_id AND f.n = ss.n
JOIN identity.users u ON u.email = f.email;

-- ── Attendance ───────────────────────────────────────────────────────────────
SELECT academic.ensure_attendance_partition(p, y)
FROM (VALUES (11), (12)) a(p), (VALUES (2025), (2026)) b(y);

-- Odd-numbered subjects meet Mon/Wed/Fri, even ones Tue/Thu/Sat, in slot n.
-- Only semesters 3 and 5: AY 2025's odd term is complete, AY 2026 runs to the
-- start of October. Earlier semesters have results but no attendance history.
INSERT INTO academic.attendance
    (program_id, academic_year, student_id, offering_id, session_date, slot, status, marked_by, marked_at)
SELECT program_id, academic_year, student_id, offering_id, day::date, slot,
       CASE WHEN r < presence - 0.03 THEN 'present'
            WHEN r < presence        THEN 'late'
            WHEN r < presence + 0.02 THEN 'excused'
            ELSE 'absent' END::academic.attendance_status,
       faculty_id, day + time '16:00'
FROM (
    SELECT st.program_id, o.academic_year, st.user_id AS student_id, o.id AS offering_id,
           day, ss.n AS slot, s.presence, o.faculty_id, random() AS r
    FROM academic.offerings o
    JOIN academic.subjects sub ON sub.id = o.subject_id
    JOIN seed_subjects ss ON ss.code = sub.code
    JOIN people.students st ON st.program_id = sub.program_id
    JOIN seed_students s ON s.roll_no = st.roll_no
    CROSS JOIN LATERAL generate_series(
        CASE o.academic_year WHEN 2025 THEN date '2025-07-14' ELSE date '2026-07-13' END,
        CASE o.academic_year WHEN 2025 THEN date '2025-11-07' ELSE date '2026-10-02' END,
        interval '1 day') day
    WHERE extract(isodow FROM day) = ANY (CASE WHEN ss.n % 2 = 1 THEN ARRAY[1, 3, 5] ELSE ARRAY[2, 4, 6] END)
      AND ss.semester IN (3, 5)
) sessions;

-- ── Results ──────────────────────────────────────────────────────────────────
-- A 10-point absolute scale; passing needs 40% of the external exam.
INSERT INTO exams.grade_scale VALUES
    (1, 90, 'O', 10), (1, 80, 'A+', 9), (1, 70, 'A', 8), (1, 60, 'B+', 7),
    (1, 50, 'B', 6), (1, 45, 'C', 5), (1, 40, 'P', 4), (1, 0, 'F', 0);
INSERT INTO exams.grading_policy VALUES (1, 40);

-- The exam cell office asks to publish; the Controller of Examinations approves.
INSERT INTO exams.result_publications (program_id, semester, academic_year, staged_at,
                                       requested_by, requested_at, published_at, published_by)
SELECT p.id, sem, 2024 + (sem - 1) / 2, pub - interval '5 days',
       CASE WHEN sem < 4 THEN u.id END, CASE WHEN sem < 4 THEN pub - interval '1 day' END,
       CASE WHEN sem < 4 THEN pub END, CASE WHEN sem < 4 THEN coe.id END
FROM org.programs p
CROSS JOIN (VALUES (1, timestamptz '2025-01-20 10:00+05:30'), (2, '2025-06-25 10:00+05:30'),
                   (3, '2026-01-22 10:00+05:30'), (4, now())) s(sem, pub)
CROSS JOIN identity.users u CROSS JOIN identity.users coe
WHERE u.email = 'examcell@northgate.example' AND coe.email = 'coe@northgate.example';

-- Theory subjects: two CCAs and a midterm. Labs: two LCAs and a viva. 40 each.
INSERT INTO exams.internal_components (offering_id, code, max_marks, position)
SELECT o.id, c.code, c.max_marks, c.position
FROM academic.offerings o
JOIN academic.subjects sub ON sub.id = o.subject_id
CROSS JOIN LATERAL (VALUES ('CCA1', 10, 1, true), ('CCA2', 10, 2, true), ('MID', 20, 3, true),
                           ('LCA1', 15, 1, false), ('LCA2', 15, 2, false), ('VIVA', 10, 3, false))
     c(code, max_marks, position, theory)
WHERE sub.semester <= 4 AND c.theory = (sub.credits > 1);

-- Each student has an ability tied to their attendance habit, plus per-subject
-- and per-component noise: students who attend more tend to score more.
CREATE TEMP TABLE seed_scores AS
SELECT st.user_id AS student_id, o.id AS offering_id, sub.semester,
       least(0.98, greatest(0.15, 0.15 + s.presence * 0.75 + (random() - 0.5) * 0.3)) AS ability
FROM people.students st
JOIN seed_students s ON s.roll_no = st.roll_no
JOIN academic.subjects sub ON sub.program_id = st.program_id AND sub.semester <= 4
JOIN academic.offerings o ON o.subject_id = sub.id;

INSERT INTO exams.internal_marks (student_id, component_id, marks)
SELECT sc.student_id, c.id,
       round(c.max_marks * least(1, greatest(0, sc.ability + (random() - 0.5) * 0.2)) * 2) / 2
FROM seed_scores sc
JOIN exams.internal_components c ON c.offering_id = sc.offering_id;

INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks)
SELECT sc.student_id, sc.offering_id, rp.id,
       round(60 * least(1, greatest(0, sc.ability + (random() - 0.5) * 0.25))), 60
FROM seed_scores sc
JOIN academic.offerings o ON o.id = sc.offering_id
JOIN academic.subjects sub ON sub.id = o.subject_id
JOIN exams.result_publications rp
  ON rp.program_id = sub.program_id AND rp.semester = sub.semester AND rp.academic_year = o.academic_year;

-- Sanika Deshpande failed Network Analysis's external exam: a backlog for the demo.
UPDATE exams.external_marks em SET marks = 19
FROM people.students st, academic.offerings o, academic.subjects sub
WHERE st.roll_no = 'ECE24002' AND em.student_id = st.user_id
  AND o.id = em.offering_id AND sub.id = o.subject_id AND sub.code = 'EC304';

-- ── Fees: AY 2026 charges for every student ──────────────────────────────────
INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, due_on)
SELECT st.user_id, 2026, 'charge', f.particulars, f.amount_paise, f.due_on
FROM people.students st
CROSS JOIN (VALUES ('Tuition fee — Semester 5',            6250000, date '2026-07-31'),
                   ('Library & laboratory fee — 2026-27',   450000, date '2026-07-31'),
                   ('Student activities fee — 2026-27',     150000, date '2026-08-31'),
                   ('Examination fee — Semester 5',         240000, date '2026-10-20'))
     f(particulars, amount_paise, due_on);

-- Merit scholarships: a quarter of tuition, for Sanika Deshpande and about one
-- student in twelve.
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, remarks)
SELECT c.student_id, 2026, 'concession', c.id, c.amount_paise / 4, 'Merit scholarship (25%)'
FROM fees.ledger c
JOIN people.students st ON st.user_id = c.student_id
WHERE c.particulars = 'Tuition fee — Semester 5'
  AND (st.roll_no = 'ECE24002' OR (st.roll_no <> 'ECE24001' AND random() < 0.08));

-- Atharva and Sanika paid their library and activities fees in July: one
-- payment each, receipts 1 and 2 of FY 2026-27.
CREATE TEMP TABLE seed_paid AS
SELECT st.user_id AS student_id, (row_number() OVER (ORDER BY st.roll_no))::int AS n,
       gen_random_uuid() AS attempt_id, gen_random_uuid() AS payment_id
FROM people.students st WHERE st.roll_no IN ('ECE24001', 'ECE24002');

INSERT INTO fees.payment_attempts (id, student_id, idempotency_key, amount_paise, gateway_order_id, status, created_at, updated_at)
SELECT attempt_id, student_id, 'seed-payment-' || n, 600000, 'order_seed' || n, 'captured',
       timestamptz '2026-07-20 11:00+05:30', timestamptz '2026-07-20 11:02+05:30'
FROM seed_paid;
INSERT INTO fees.payment_attempt_items (attempt_id, charge_id, amount_paise)
SELECT p.attempt_id, c.id, c.amount_paise
FROM seed_paid p
JOIN fees.ledger c ON c.student_id = p.student_id AND c.kind = 'charge'
 AND c.particulars IN ('Library & laboratory fee — 2026-27', 'Student activities fee — 2026-27');
INSERT INTO fees.payments (id, attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
SELECT payment_id, attempt_id, student_id, 'pay_seed' || n, 600000, 'upi', timestamptz '2026-07-20 11:02+05:30'
FROM seed_paid;
INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id, created_at)
SELECT p.student_id, 2026, 'payment', i.charge_id, i.amount_paise, p.payment_id, timestamptz '2026-07-20 11:02+05:30'
FROM seed_paid p JOIN fees.payment_attempt_items i ON i.attempt_id = p.attempt_id;
INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks, issued_at)
SELECT 'RCPT/2026-27/' || lpad(n::text, 6, '0'), 2026, n, payment_id, student_id, 600000,
       'Library & laboratory fee — 2026-27, Student activities fee — 2026-27', timestamptz '2026-07-20 11:02+05:30'
FROM seed_paid;
INSERT INTO fees.receipt_seq VALUES (2026, 2);
-- Their PDFs come from the worker, like any other receipt's.
INSERT INTO ops.outbox (topic, ordering_key, payload)
SELECT 'receipt.requested', student_id::text, jsonb_build_object('receipt_id', id) FROM fees.receipts;

-- ── Notices ─────────────────────────────────────────────────────────────────
-- Times are relative to seeding, so the board always looks current.
INSERT INTO comms.notices (institution_id, program_id, severity, title, body, published_at, expires_at, created_by)
SELECT 1, program_id, severity::comms.notice_severity, title, body, now() - age, expires, u.id
FROM (VALUES
    (NULL::smallint, 'important', 'Semester 5 examination form window',
     E'The examination form for Semester 5 opens on 12 October and closes on 20 October.\nForms submitted late attract a late fee.',
     interval '1 day', NULL::timestamptz),
    (NULL, 'general', 'Diwali break',
     E'The institute is closed from 18 to 22 October. Classes resume on 23 October.',
     interval '2 days', NULL),
    (11, 'critical', 'EC502 lab moved to Block C',
     E'From Monday, Microprocessors Lab sessions are held in Block C, room 204.\nBring your lab record.',
     interval '5 hours', NULL),
    (12, 'general', 'Guest lecture: CS503',
     E'A guest lecture on software-defined networking is scheduled for Thursday, 2 pm, in the seminar hall.',
     interval '3 days', NULL),
    (NULL, 'general', 'Library hours extended',
     E'The central library stays open until 10 pm on weekdays during the examination period.',
     interval '12 days', NULL),
    (NULL, 'general', 'Orientation week schedule',
     E'Orientation for the new batch ran from 14 to 18 July.',
     interval '80 days', now() - interval '60 days')
) n(program_id, severity, title, body, age, expires)
CROSS JOIN identity.users u WHERE u.email = 'examcell@northgate.example';

COMMIT;

ANALYZE;
