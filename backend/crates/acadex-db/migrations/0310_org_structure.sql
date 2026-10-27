-- Departments own programs; an HOD's reach is their department's programs.
CREATE TABLE org.departments (
    id             smallint PRIMARY KEY,
    institution_id smallint NOT NULL REFERENCES org.institutions,
    code           text NOT NULL UNIQUE CHECK (code ~ '^[a-z0-9_]+$'),
    name           text NOT NULL,
    hod_id         uuid REFERENCES identity.users
);
ALTER TABLE org.programs ADD COLUMN department_id smallint REFERENCES org.departments;

-- Batch = program + admission year (already on students); section within it.
ALTER TABLE people.students
    ADD COLUMN section text NOT NULL DEFAULT 'A' CHECK (section ~ '^[A-Z]$');

-- User lifecycle: deactivated users keep their records but can't sign in.
ALTER TABLE identity.users
    ADD COLUMN active         boolean NOT NULL DEFAULT true,
    ADD COLUMN deactivated_at timestamptz,
    ADD CONSTRAINT users_deactivation_dated CHECK (active = (deactivated_at IS NULL));

-- Institution settings, editable by admin (architecture.md A4: configuration,
-- not code).
CREATE TABLE org.institution_settings (
    institution_id            smallint PRIMARY KEY REFERENCES org.institutions,
    attendance_threshold_pct  numeric(5, 2) NOT NULL DEFAULT 75
                              CHECK (attendance_threshold_pct BETWEEN 0 AND 100),
    -- The month an academic year starts; 7 = July, so October 2026 is AY 2026-27.
    academic_year_start_month smallint NOT NULL DEFAULT 7 CHECK (academic_year_start_month BETWEEN 1 AND 12),
    -- Days after publication in which re-evaluation can be requested.
    results_appeal_days       smallint NOT NULL DEFAULT 15 CHECK (results_appeal_days BETWEEN 0 AND 90),
    updated_at                timestamptz NOT NULL DEFAULT now(),
    updated_by                uuid REFERENCES identity.users
);

-- Sign-in now reports whether the account is active, so a deactivated user
-- gets "deactivated" rather than "no account". Same linking rules as before.
DROP FUNCTION identity.resolve_subject(text, text, boolean, boolean);
CREATE FUNCTION identity.resolve_subject(
    p_subject          text,
    p_email            text,
    p_email_verified   boolean,
    p_allow_unverified boolean
) RETURNS TABLE (user_id uuid, role text, active boolean, newly_linked boolean)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    RETURN QUERY
        SELECT u.id, u.role::text, u.active, false
        FROM identity.user_identities i JOIN identity.users u ON u.id = i.user_id
        WHERE i.provider_subject = p_subject;
    IF FOUND THEN
        RETURN;
    END IF;

    IF p_email IS NULL OR NOT (p_email_verified OR p_allow_unverified) THEN
        RETURN;
    END IF;

    INSERT INTO identity.user_identities (provider_subject, user_id)
        SELECT p_subject, u.id
        FROM identity.users u
        WHERE u.email = lower(p_email)
          AND NOT EXISTS (SELECT 1 FROM identity.user_identities i WHERE i.user_id = u.id)
    ON CONFLICT DO NOTHING;

    RETURN QUERY
        SELECT u.id, u.role::text, u.active, true
        FROM identity.user_identities i JOIN identity.users u ON u.id = i.user_id
        WHERE i.provider_subject = p_subject;
END $$;
REVOKE EXECUTE ON FUNCTION identity.resolve_subject(text, text, boolean, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.resolve_subject(text, text, boolean, boolean) TO acadex_app;
