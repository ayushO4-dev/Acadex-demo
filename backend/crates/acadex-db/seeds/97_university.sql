-- The first university: Northgate University, Pune. Acadex is sold to
-- universities, so the vendor's platform operator (operator@acadex.example)
-- and the university's own accounts are separate: the university gets its
-- own administrator, the registrar, and its accounts its own domain. Brings
-- databases seeded before this into line; fresh ones already are.

BEGIN;

UPDATE org.institutions
SET name = 'Northgate University', short_name = 'NGU', city = 'Pune', website = 'https://northgate.example',
    updated_at = now()
WHERE id = 1;

UPDATE identity.users
SET email = replace(replace(replace(email, '@students.acadex.test', '@northgate.example'),
                            '@staff.acadex.test', '@northgate.example'),
                    '@acadex.test', '@northgate.example')
WHERE role <> 'platform_admin' AND email LIKE '%acadex.test';

UPDATE identity.users SET role = 'platform_admin' WHERE email = 'operator@acadex.example';

INSERT INTO identity.users (institution_id, email, display_name, role)
VALUES (1, 'registrar@northgate.example', 'Dr. Anand Bapat', 'admin')
ON CONFLICT (email) DO NOTHING;

-- Decisions the owner took while acting as the university's administrator
-- (document verifications and the like: every "…_by" column) are the
-- registrar's from now on. The audit trail keeps who actually did them.
DO $$
DECLARE
    v_owner     uuid := (SELECT id FROM identity.users WHERE email = 'operator@acadex.example');
    v_registrar uuid := (SELECT id FROM identity.users WHERE email = 'registrar@northgate.example');
    fk          record;
BEGIN
    IF v_owner IS NULL THEN
        RETURN;
    END IF;
    FOR fk IN
        SELECT c.conrelid::regclass AS tbl, a.attname AS col
        FROM pg_constraint c
        JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1]
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE c.contype = 'f' AND c.confrelid = 'identity.users'::regclass AND array_length(c.conkey, 1) = 1
          AND a.attname LIKE '%\_by' AND n.nspname <> 'audit'
    LOOP
        EXECUTE format('UPDATE %s SET %I = $1 WHERE %I = $2', fk.tbl, fk.col, fk.col) USING v_registrar, v_owner;
    END LOOP;
    UPDATE org.departments SET hod_id = NULL WHERE hod_id = v_owner;
END $$;

COMMIT;
