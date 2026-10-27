-- F1: departments, sections, settings, and a demo login for every role.
--
-- Demo logins added here — create them in Auth0 to sign in as them:
--   hod.ece@northgate.example     Dr. Prakash Chavan, HOD Electronics & Communication
--   hod.cse@northgate.example     Dr. Smita Phadke, HOD Computer Science
--   accounts@northgate.example    Shubhangi Pawar, accounts office
--   support@northgate.example     Rutuja Shinde, student support
--   registrar@northgate.example    Dr. Anand Bapat, Registrar: the university's administrator
--   operator@acadex.example     Acadex Operator, platform operator (the vendor, not the university)
--   superadmin@northgate.example  Break-glass account (every request audited)

BEGIN;

INSERT INTO org.departments (id, institution_id, code, name) VALUES
    (1, 1, 'ece', 'Electronics & Communication Engineering'),
    (2, 1, 'cse', 'Computer Science & Engineering');
UPDATE org.programs SET department_id = CASE id WHEN 11 THEN 1 WHEN 12 THEN 2 END;

-- Two sections per batch: the first 125 roll numbers in A, the rest in B.
UPDATE people.students SET section = CASE WHEN right(roll_no, 3)::int <= 125 THEN 'A' ELSE 'B' END;

INSERT INTO org.institution_settings (institution_id) VALUES (1);

INSERT INTO identity.users (institution_id, email, display_name, role) VALUES
    (1, 'hod.ece@northgate.example', 'Dr. Prakash Chavan', 'hod'),
    (1, 'hod.cse@northgate.example', 'Dr. Smita Phadke', 'hod'),
    (1, 'accounts@northgate.example', 'Shubhangi Pawar', 'accounts'),
    (1, 'support@northgate.example', 'Rutuja Shinde', 'support_agent'),
    (1, 'registrar@northgate.example', 'Dr. Anand Bapat', 'admin'),
    (1, 'operator@acadex.example', 'Acadex Operator', 'platform_admin'),
    (1, 'superadmin@northgate.example', 'Break-glass Administrator', 'super_admin');

-- HODs are faculty of their department too.
INSERT INTO people.faculty (user_id, program_id, designation)
SELECT id, CASE email WHEN 'hod.ece@northgate.example' THEN 11 ELSE 12 END, 'Professor & Head of Department'
FROM identity.users WHERE email IN ('hod.ece@northgate.example', 'hod.cse@northgate.example');
UPDATE org.departments d SET hod_id = u.id
FROM identity.users u
WHERE u.email = CASE d.code WHEN 'ece' THEN 'hod.ece@northgate.example' ELSE 'hod.cse@northgate.example' END;

COMMIT;
