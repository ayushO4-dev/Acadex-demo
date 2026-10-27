-- F4a: activities. About half the students have one to three approved
-- activities from the last year, each with its review history (forwarded by
-- a faculty member of their program, approved by their HOD) and a sample
-- certificate as evidence.
--
-- For the demo, every stage has something to show:
--   Atharva: a hackathon win and a workshop approved, a seminar waiting for
--          faculty (Dr. Revati Gokhale), an industrial visit still a draft.
--   Sanika:  a seminar waiting for the HOD (Dr. Prakash Chavan), and a sports
--          activity returned to her to attach the certificate.

BEGIN;
SELECT setseed(0.6006);

-- Seeds write final states directly; the transition check is for live moves.
ALTER TABLE activities.activities DISABLE TRIGGER check_transition;

CREATE TEMP TABLE seed_pool (kind text, n int, title text, a text, b text, c text, d text);
INSERT INTO seed_pool VALUES
    ('general', 1, 'Inter-college football tournament', 'sports', 'Sports Council', 'participant', 'state'),
    ('general', 2, 'Annual cultural fest: group dance', 'cultural', 'Cultural Committee', 'participant', 'college'),
    ('general', 3, 'Smart India Hackathon', 'technical', 'Ministry of Education', 'participant', 'national'),
    ('general', 4, 'Blood donation camp', 'social_service', 'NSS Unit', 'volunteer', 'college'),
    ('general', 5, 'Robotics challenge', 'technical', 'IIT Bombay Techfest', 'participant', 'national'),
    ('general', 6, 'State chess championship', 'sports', 'State Chess Association', 'participant', 'state'),
    ('seminar', 1, '5G network slicing', 'Dr. A. Menon', 'IEEE Communications Society', 'offline', '2'),
    ('seminar', 2, 'Careers in VLSI design', 'R. Kulkarni', 'Texas Instruments', 'online', '1.5'),
    ('seminar', 3, 'Introduction to quantum computing', 'Prof. S. Bose', 'IISc Bengaluru', 'online', '2'),
    ('seminar', 4, 'Cyber security awareness', 'Inspector P. Rao', 'Cyber Crime Cell', 'offline', '1'),
    ('seminar', 5, 'Renewable energy grids', 'Dr. N. Gupta', 'IEEE Power & Energy Society', 'offline', '3'),
    ('workshop', 1, 'PCB design with KiCad', 'IEEE Student Branch', 'PCB layout,KiCad,DFM', '16', NULL),
    ('workshop', 2, 'IoT with ESP32', 'Electronics Club', 'ESP32,MQTT,Sensors', '12', NULL),
    ('workshop', 3, 'Machine learning bootcamp', 'Google Developer Student Club', 'Python,scikit-learn', '24', NULL),
    ('workshop', 4, 'Linux and Git essentials', 'FOSS Club', 'Linux,Git,Shell', '8', NULL),
    ('industrial_visit', 1, 'Bharat Electronics Limited', 'Bengaluru', 'Defence electronics', NULL, NULL),
    ('industrial_visit', 2, 'ISRO Telemetry Centre', 'Bengaluru', 'Space', NULL, NULL),
    ('industrial_visit', 3, 'Tata Motors plant', 'Pune', 'Automotive', NULL, NULL),
    ('industrial_visit', 4, 'Infosys Mysuru campus', 'Mysuru', 'IT services', NULL, NULL);

-- Who reviews whose: a faculty member of the student's program, and the HOD.
CREATE TEMP TABLE seed_reviewers AS
SELECT p.id AS program_id,
       (SELECT f.user_id FROM people.faculty f JOIN identity.users u ON u.id = f.user_id
        WHERE f.program_id = p.id AND u.role = 'faculty' ORDER BY u.email = 'faculty.ece@northgate.example' DESC, u.email LIMIT 1) AS faculty_id,
       d.hod_id
FROM org.programs p JOIN org.departments d ON d.id = p.department_id;

CREATE TEMP TABLE seed_acts (
    id uuid, student_id uuid, program_id smallint, kind text, n int, occurred_on date,
    status text, note text, forwarded_at timestamptz, decided_at timestamptz
);

-- The cohort: about half have one to three approved activities. Choices
-- come from hashing the student and row, so each row gets its own and every
-- run the same.
INSERT INTO seed_acts
SELECT gen_random_uuid(), st.user_id, st.program_id, k.kind,
       1 + abs(hashtext(st.user_id::text || i || 'n')) % (SELECT count(*) FROM seed_pool p WHERE p.kind = k.kind)::int,
       d, 'approved', NULL, d + 10 + interval '10 hours', d + 14 + interval '11 hours'
FROM people.students st
CROSS JOIN LATERAL generate_series(1, 1 + abs(hashtext(st.user_id::text || 'count')) % 3) i
CROSS JOIN LATERAL (SELECT (ARRAY['general', 'seminar', 'workshop', 'industrial_visit'])[1 + abs(hashtext(st.user_id::text || i || 'kind')) % 4] AS kind) k
CROSS JOIN LATERAL (SELECT date '2025-08-01' + abs(hashtext(st.user_id::text || i || 'date')) % 380 AS d) dd
WHERE st.roll_no NOT IN ('ECE24001', 'ECE24002') AND abs(hashtext(st.user_id::text || 'some')) % 2 = 0;

-- Atharva and Sanika, by hand.
INSERT INTO seed_acts
SELECT gen_random_uuid(), st.user_id, st.program_id, v.kind, v.n, v.d, v.status, v.note,
       CASE WHEN v.status IN ('hod_review', 'approved') THEN v.d + 9 + interval '10 hours' END,
       CASE WHEN v.status = 'approved' THEN v.d + 12 + interval '11 hours' END
FROM (VALUES ('ECE24001', 'general', 3, date '2026-03-14', 'approved', NULL),
             ('ECE24001', 'workshop', 1, date '2026-07-20', 'approved', NULL),
             ('ECE24001', 'seminar', 1, date '2026-09-22', 'submitted', NULL),
             ('ECE24001', 'industrial_visit', 1, date '2026-09-26', 'draft', NULL),
             ('ECE24002', 'seminar', 2, date '2026-09-05', 'hod_review', NULL),
             ('ECE24002', 'general', 1, date '2026-08-30', 'draft', 'Please attach the participation certificate from the Sports Council.'))
     v(roll_no, kind, n, d, status, note)
JOIN people.students st ON st.roll_no = v.roll_no;

INSERT INTO activities.activities (id, student_id, kind, title, description, occurred_on, status, points, review_note,
                                   created_at, updated_at, submitted_at, decided_at)
SELECT a.id, a.student_id, a.kind,
       p.title,
       CASE a.kind WHEN 'general' THEN 'Represented the college.'
                   WHEN 'seminar' THEN 'Attended the talk and the Q&A that followed.'
                   WHEN 'workshop' THEN 'Hands-on sessions with a project at the end.'
                   ELSE 'Department visit with the class.' END,
       -- Approved needs points; the real ones once the type rows exist.
       a.occurred_on, a.status, CASE WHEN a.status = 'approved' THEN 0 END, a.note,
       a.occurred_on + interval '2 days 18 hours',
       coalesce(a.decided_at, a.forwarded_at, a.occurred_on + interval '3 days 9 hours'),
       CASE WHEN a.status <> 'draft' OR a.note IS NOT NULL THEN a.occurred_on + interval '3 days 9 hours' END,
       a.decided_at
FROM seed_acts a JOIN seed_pool p ON p.kind = a.kind AND p.n = a.n;

INSERT INTO activities.general
SELECT a.id, p.a, p.b, CASE WHEN p.n = 3 AND a.status = 'approved' AND a.occurred_on = date '2026-03-14' THEN 'winner' ELSE p.c END, p.d,
       CASE WHEN p.n = 3 AND a.occurred_on = date '2026-03-14' THEN 'First place, hardware track' END
FROM seed_acts a JOIN seed_pool p ON p.kind = a.kind AND p.n = a.n WHERE a.kind = 'general';
INSERT INTO activities.seminars
SELECT a.id, p.a, p.b, p.c, p.d::numeric FROM seed_acts a JOIN seed_pool p ON p.kind = a.kind AND p.n = a.n WHERE a.kind = 'seminar';
INSERT INTO activities.workshops
SELECT a.id, p.a, a.occurred_on + 1, p.c::numeric, string_to_array(p.b, ','), true
FROM seed_acts a JOIN seed_pool p ON p.kind = a.kind AND p.n = a.n WHERE a.kind = 'workshop';
INSERT INTO activities.industrial_visits
SELECT a.id, p.a, p.b, coalesce((SELECT u.display_name FROM seed_reviewers r JOIN identity.users u ON u.id = r.faculty_id
                                 WHERE r.program_id = a.program_id), 'Department faculty'),
       'We toured the production and testing floors and saw how quality checks are automated. The engineers '
       || 'explained the design-to-manufacture process and answered our questions about careers.'
FROM seed_acts a JOIN seed_pool p ON p.kind = a.kind AND p.n = a.n WHERE a.kind = 'industrial_visit';

UPDATE activities.activities a SET points = activities.suggested_points(a.id) WHERE a.status = 'approved';

-- Evidence: the sample certificate, on everything but Sanika's returned activity.
INSERT INTO activities.attachments (activity_id, object, name, content_type, size_bytes, uploaded_at)
SELECT a.id, 'seed/sample-certificate.pdf', 'certificate.pdf', 'application/pdf', 739, a.occurred_on + interval '2 days 19 hours'
FROM seed_acts a WHERE a.note IS NULL;

-- Review history, as the reviewers moved each one.
INSERT INTO activities.reviews (activity_id, actor_id, from_status, to_status, points, note, at)
SELECT a.id, a.student_id, 'draft', 'submitted', NULL::smallint, NULL, a.occurred_on + interval '3 days 9 hours'
FROM seed_acts a WHERE a.status <> 'draft' OR a.note IS NOT NULL
UNION ALL
SELECT a.id, r.faculty_id, 'submitted', 'hod_review', NULL, NULL, a.forwarded_at
FROM seed_acts a JOIN seed_reviewers r USING (program_id) WHERE a.forwarded_at IS NOT NULL
UNION ALL
SELECT a.id, r.hod_id, 'hod_review', 'approved', act.points, NULL, a.decided_at
FROM seed_acts a JOIN seed_reviewers r USING (program_id) JOIN activities.activities act ON act.id = a.id
WHERE a.status = 'approved'
UNION ALL
SELECT a.id, r.faculty_id, 'submitted', 'draft', NULL, a.note, a.occurred_on + interval '6 days 12 hours'
FROM seed_acts a JOIN seed_reviewers r USING (program_id) WHERE a.note IS NOT NULL;

ALTER TABLE activities.activities ENABLE TRIGGER check_transition;

COMMIT;
