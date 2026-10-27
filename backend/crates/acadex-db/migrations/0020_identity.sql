CREATE TYPE identity.user_role AS ENUM ('student', 'faculty', 'exam_cell');

CREATE TABLE identity.users (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    institution_id smallint NOT NULL REFERENCES org.institutions,
    email          text NOT NULL UNIQUE CHECK (email = lower(email)),
    display_name   text NOT NULL,
    role           identity.user_role NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now()
);

-- Auth0 `sub` ↔ Acadex user, one login per user, created on first sign-in.
CREATE TABLE identity.user_identities (
    provider_subject text PRIMARY KEY,
    user_id          uuid NOT NULL UNIQUE REFERENCES identity.users ON DELETE CASCADE,
    linked_at        timestamptz NOT NULL DEFAULT now()
);

-- Maps an Auth0 subject to its Acadex user, linking it on first sign-in by email.
-- SECURITY DEFINER because it runs before the acting user is known, and the app
-- role has no direct access to user_identities or users.email.
--
-- Linking needs a verified email unless p_allow_unverified (local development,
-- with Auth0 sign-ups disabled). A user already linked to another subject is
-- never re-linked, so a second Auth0 account cannot take over a record.
CREATE FUNCTION identity.resolve_subject(
    p_subject          text,
    p_email            text,
    p_email_verified   boolean,
    p_allow_unverified boolean
) RETURNS TABLE (user_id uuid, role text, newly_linked boolean)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    RETURN QUERY
        SELECT u.id, u.role::text, false
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
        SELECT u.id, u.role::text, true
        FROM identity.user_identities i JOIN identity.users u ON u.id = i.user_id
        WHERE i.provider_subject = p_subject;
END $$;
