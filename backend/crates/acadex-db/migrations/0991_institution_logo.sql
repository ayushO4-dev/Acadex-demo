-- The university's logo, shown on the sign-in page: part of its profile, set
-- by the platform operator. Small (at most 1 MB), so it lives in the row.
-- Raster images only: an SVG could carry script.
ALTER TABLE org.institutions
    ADD COLUMN logo            bytea CHECK (octet_length(logo) <= 1048576),
    ADD COLUMN logo_type       text CHECK (logo_type IN ('image/png', 'image/jpeg', 'image/webp')),
    ADD COLUMN logo_updated_at timestamptz,
    ADD CONSTRAINT logo_whole CHECK ((logo IS NULL) = (logo_type IS NULL));

CREATE FUNCTION identity.platform_set_logo(p_logo bytea, p_type text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM identity.require_platform();
    UPDATE org.institutions
    SET logo = p_logo, logo_type = p_type, logo_updated_at = CASE WHEN p_logo IS NULL THEN NULL ELSE now() END
    WHERE id = (SELECT institution_id FROM identity.users WHERE id = identity.current_user_id());
END $$;
REVOKE EXECUTE ON FUNCTION identity.platform_set_logo(bytea, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.platform_set_logo(bytea, text) TO acadex_app;
