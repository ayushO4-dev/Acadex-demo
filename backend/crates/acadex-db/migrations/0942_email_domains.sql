-- The university's email domains, set by the platform operator when the
-- university is onboarded: its identity boundary. Every university account
-- must have an address on one of them, however it's created (added,
-- imported, corrected, or an administrator the platform adds), so a sign-in
-- can only ever be linked to one of the university's own addresses. Empty
-- means not yet set (no restriction). Platform accounts are the vendor's.

ALTER TABLE org.institutions
    ADD COLUMN email_domains text[] NOT NULL DEFAULT '{}'
        CHECK (array_position(email_domains, NULL) IS NULL);

CREATE FUNCTION identity.email_on_domain(p_institution smallint, p_email text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT cardinality(i.email_domains) = 0
        OR lower(split_part(p_email, '@', 2)) = ANY (i.email_domains)
    FROM org.institutions i WHERE i.id = p_institution
$$;

CREATE FUNCTION identity.guard_email_domain() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.role <> 'platform_admin'
       AND (TG_OP = 'INSERT' OR NEW.email IS DISTINCT FROM OLD.email)
       AND NOT identity.email_on_domain(NEW.institution_id, NEW.email) THEN
        RAISE EXCEPTION 'the email must be on one of the university''s domains (%)',
            (SELECT array_to_string(email_domains, ', ') FROM org.institutions WHERE id = NEW.institution_id)
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER guard_email_domain BEFORE INSERT OR UPDATE OF email ON identity.users
    FOR EACH ROW EXECUTE FUNCTION identity.guard_email_domain();

CREATE FUNCTION identity.platform_set_email_domains(p_domains text[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_domains text[] := ARRAY(SELECT DISTINCT lower(trim(d)) FROM unnest(p_domains) d WHERE trim(d) <> '' ORDER BY 1);
BEGIN
    PERFORM identity.require_platform();
    IF EXISTS (SELECT 1 FROM unnest(v_domains) d WHERE d !~ '^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$') THEN
        RAISE EXCEPTION 'not a domain name' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE org.institutions SET email_domains = v_domains, updated_at = now()
    WHERE id = (SELECT institution_id FROM identity.users WHERE id = identity.current_user_id());
END $$;
REVOKE EXECUTE ON FUNCTION identity.platform_set_email_domains(text[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.platform_set_email_domains(text[]) TO acadex_app;

-- University accounts already off the domains, for the platform's console.
CREATE FUNCTION identity.platform_off_domain_accounts() RETURNS bigint
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_platform();
    RETURN (SELECT count(*) FROM identity.users u
            WHERE u.role <> 'platform_admin' AND NOT identity.email_on_domain(u.institution_id, u.email));
END $$;
REVOKE EXECUTE ON FUNCTION identity.platform_off_domain_accounts() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.platform_off_domain_accounts() TO acadex_app;
