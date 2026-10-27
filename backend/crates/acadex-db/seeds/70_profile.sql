-- F4b: profiles. Every student gets admission details, personal, parent,
-- contact and education information (choices hashed from the roll number, so
-- every run agrees). Aadhaar and bank numbers are sealed by the API's key, so
-- `just seed` runs `acadex-api seed-pii` after this file.
--
-- For the demo:
--   Atharva: class 10 and 12 marksheets verified; his Aadhaar card uploaded,
--          scanned clean and waiting for the registrar.
--   Sanika:  asked to correct her blood group (B+ to O+), with her transfer
--          certificate, which also waits for verification.

BEGIN;

CREATE FUNCTION pg_temp.pick(key text, options text[]) RETURNS text
LANGUAGE sql IMMUTABLE AS $$ SELECT options[1 + abs(hashtext(key)) % cardinality(options)] $$;
CREATE FUNCTION pg_temp.num(key text, lo int, hi int) RETURNS int
LANGUAGE sql IMMUTABLE AS $$ SELECT lo + abs(hashtext(key)) % (hi - lo + 1) $$;

INSERT INTO profile.admission (student_id, admission_no, admitted_on, quota)
SELECT st.user_id,
       'AIT/' || st.batch_year || '/' || CASE st.program_id WHEN 11 THEN 'ECE' ELSE 'CSE' END || '/' || right(st.roll_no, 3),
       make_date(st.batch_year, 7, 10 + pg_temp.num(st.roll_no || 'adm', 0, 15)),
       pg_temp.pick(st.roll_no || 'quota', ARRAY['merit', 'merit', 'merit', 'merit', 'management', 'nri'])
FROM people.students st
ON CONFLICT DO NOTHING;

INSERT INTO profile.personal_info (student_id, date_of_birth, gender, blood_group, category, nationality, mother_tongue)
SELECT st.user_id,
       make_date(st.batch_year - 18, 1 + pg_temp.num(st.roll_no || 'm', 0, 11), 1 + pg_temp.num(st.roll_no || 'd', 0, 27)),
       pg_temp.pick(st.roll_no || 'g', ARRAY['female', 'male']),
       pg_temp.pick(st.roll_no || 'b', ARRAY['O+', 'O+', 'B+', 'B+', 'A+', 'A+', 'AB+', 'O-', 'B-', 'A-']),
       pg_temp.pick(st.roll_no || 'c', ARRAY['general', 'general', 'obc', 'obc', 'sc', 'st', 'ews']),
       'Indian',
       pg_temp.pick(st.roll_no || 't', ARRAY['Hindi', 'Marathi', 'Tamil', 'Telugu', 'Kannada', 'Bengali', 'Gujarati', 'Malayalam'])
FROM people.students st
ON CONFLICT DO NOTHING;
-- The demo students, as their names and the earlier seeds have them.
UPDATE profile.personal_info p SET gender = 'male', blood_group = 'A+', date_of_birth = '2006-03-14', mother_tongue = 'Hindi'
FROM people.students st WHERE st.user_id = p.student_id AND st.roll_no = 'ECE24001';
UPDATE profile.personal_info p SET gender = 'female', blood_group = 'B+', date_of_birth = '2006-08-02', mother_tongue = 'Gujarati'
FROM people.students st WHERE st.user_id = p.student_id AND st.roll_no = 'ECE24002';

INSERT INTO profile.parent_info (student_id, father_name, father_occupation, father_phone, mother_name,
                                 mother_occupation, mother_phone, annual_income)
SELECT st.user_id,
       pg_temp.pick(st.roll_no || 'fn', ARRAY['Rajesh', 'Suresh', 'Anil', 'Vijay', 'Manoj', 'Ramesh', 'Sanjay']) || ' ' || split_part(u.display_name, ' ', 2),
       pg_temp.pick(st.roll_no || 'fo', ARRAY['Engineer', 'Teacher', 'Business owner', 'Farmer', 'Bank officer', 'Doctor', 'Government employee']),
       '9' || lpad(pg_temp.num(st.roll_no || 'fp', 0, 999999999)::text, 9, '0'),
       pg_temp.pick(st.roll_no || 'mn', ARRAY['Sunita', 'Anita', 'Kavita', 'Meena', 'Rekha', 'Asha', 'Neha']) || ' ' || split_part(u.display_name, ' ', 2),
       pg_temp.pick(st.roll_no || 'mo', ARRAY['Homemaker', 'Teacher', 'Nurse', 'Accountant', 'Business owner']),
       '8' || lpad(pg_temp.num(st.roll_no || 'mp', 0, 999999999)::text, 9, '0'),
       pg_temp.pick(st.roll_no || 'inc', ARRAY['below_2_5l', '2_5l_to_5l', '5l_to_10l', '5l_to_10l', 'above_10l'])
FROM people.students st JOIN identity.users u ON u.id = st.user_id
ON CONFLICT DO NOTHING;

INSERT INTO profile.contact_info (student_id, phone, correspondence_address, permanent_address,
                                  emergency_name, emergency_relation, emergency_phone)
SELECT st.user_id,
       '7' || lpad(pg_temp.num(st.roll_no || 'p', 0, 999999999)::text, 9, '0'),
       'Room ' || pg_temp.num(st.roll_no || 'room', 101, 450) || ', ' || pg_temp.pick(st.roll_no || 'h', ARRAY['Aryabhata', 'Raman', 'Kalpana', 'Sarabhai']) || ' Hostel, Northgate University',
       pg_temp.num(st.roll_no || 'hn', 1, 250) || ', ' || pg_temp.pick(st.roll_no || 'st', ARRAY['MG Road', 'Station Road', 'Gandhi Nagar', 'Civil Lines', 'Park Street'])
           || ', ' || pg_temp.pick(st.roll_no || 'city', ARRAY['Pune', 'Nagpur', 'Indore', 'Bhopal', 'Nashik', 'Jaipur', 'Surat']),
       p.mother_name, 'Mother', p.mother_phone
FROM people.students st JOIN profile.parent_info p ON p.student_id = st.user_id
ON CONFLICT DO NOTHING;

INSERT INTO profile.education (student_id, level, board, institution, year, score)
SELECT st.user_id, e.level, e.board,
       pg_temp.pick(st.roll_no || e.level, e.schools),
       st.batch_year - e.years_before,
       CASE e.level WHEN 'entrance' THEN 'Rank ' || pg_temp.num(st.roll_no || 'rank', 2000, 90000)
                    ELSE pg_temp.num(st.roll_no || e.level || 's', 72, 97) || '.' || pg_temp.num(st.roll_no || e.level || 'f', 0, 9) || '%' END
FROM people.students st
CROSS JOIN (VALUES
    ('class_10', 'CBSE', 2, ARRAY['Kendriya Vidyalaya', 'Delhi Public School', 'St. Xavier''s High School', 'Navodaya Vidyalaya']),
    ('class_12', 'CBSE', 0, ARRAY['Kendriya Vidyalaya', 'Delhi Public School', 'Ryan International School', 'DAV Public School']),
    ('entrance', 'JEE Main', 0, ARRAY['National Testing Agency'])) e(level, board, years_before, schools)
ON CONFLICT DO NOTHING;

-- Demo documents: copies of the sample certificate (see `just emulators`),
-- already scanned clean; seed files live outside documents/, so they aren't re-scanned.
INSERT INTO profile.documents (student_id, doc_type, object, name, content_type, size_bytes, scan_status, scan_detail,
                               scanned_at, verification, verified_by, verified_at, uploaded_at)
SELECT st.user_id, d.doc_type, 'seed/documents/' || st.roll_no || '-' || d.doc_type || '.pdf', d.name, 'application/pdf', 739,
       'clean', 'Type checked; ClamAV found nothing', d.at + interval '1 minute',
       d.verification, CASE WHEN d.verification = 'verified' THEN reg.id END,
       CASE WHEN d.verification = 'verified' THEN d.at + interval '2 days' END, d.at
FROM (VALUES ('ECE24001', 'class_10_marksheet', 'class10-marksheet.pdf', 'verified', timestamptz '2024-07-15 11:00+05:30'),
             ('ECE24001', 'class_12_marksheet', 'class12-marksheet.pdf', 'verified', timestamptz '2024-07-15 11:05+05:30'),
             ('ECE24001', 'aadhaar', 'aadhaar-card.pdf', 'pending', timestamptz '2026-09-28 18:20+05:30'),
             ('ECE24002', 'transfer_certificate', 'transfer-certificate.pdf', 'pending', timestamptz '2026-09-30 21:10+05:30'))
     d(roll_no, doc_type, name, verification, at)
JOIN people.students st ON st.roll_no = d.roll_no
CROSS JOIN identity.users reg
WHERE reg.email = 'registrar@northgate.example';

INSERT INTO profile.edit_requests (student_id, field, old_display, new_display, new_value, reason, document_id, created_at)
SELECT st.user_id, 'blood_group', 'B+', 'O+', 'O+',
       'My blood group was entered wrongly at admission; the lab report from the health camp shows O+.',
       (SELECT id FROM profile.documents d WHERE d.student_id = st.user_id AND d.doc_type = 'transfer_certificate'),
       timestamptz '2026-09-30 21:15+05:30'
FROM people.students st WHERE st.roll_no = 'ECE24002';

COMMIT;
