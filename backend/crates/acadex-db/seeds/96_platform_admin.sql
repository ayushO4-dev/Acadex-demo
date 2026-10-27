-- One administrator: the platform owner, operator@acadex.example. Databases
-- seeded before this had a demo administrator, Arjun Rao (admin@acadex.test);
-- whatever points at him (decisions, notices, verifications) now points at
-- the owner, and his account is removed. Fresh databases get the owner from
-- 10_roles.sql and this changes nothing.

BEGIN;

INSERT INTO identity.users (institution_id, email, display_name, role)
VALUES (1, 'operator@acadex.example', 'Acadex Operator', 'platform_admin')
ON CONFLICT (email) DO NOTHING;

DO $$
DECLARE
    v_old uuid := (SELECT id FROM identity.users WHERE email = 'admin@acadex.test');
    v_new uuid := (SELECT id FROM identity.users WHERE email = 'operator@acadex.example');
    fk    record;
BEGIN
    IF v_old IS NULL THEN
        RETURN;
    END IF;
    -- Every column that references a user, wherever it is.
    FOR fk IN
        SELECT c.conrelid::regclass AS tbl, a.attname AS col
        FROM pg_constraint c
        JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1]
        WHERE c.contype = 'f' AND c.confrelid = 'identity.users'::regclass AND array_length(c.conkey, 1) = 1
    LOOP
        EXECUTE format('UPDATE %s SET %I = $1 WHERE %I = $2', fk.tbl, fk.col, fk.col) USING v_new, v_old;
    END LOOP;
    -- The audit log names actors without a foreign key; its rows stay as
    -- written (they're hash-chained), and are his history, not the owner's.
    DELETE FROM identity.users WHERE id = v_old;
END $$;

COMMIT;
