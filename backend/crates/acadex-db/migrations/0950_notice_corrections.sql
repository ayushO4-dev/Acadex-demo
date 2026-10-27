-- Publishers can correct a notice, withdraw it, or delete one still
-- scheduled. A correction is marked "edited" (the audit log keeps the
-- earlier text); correcting a critical notice asks its readers to
-- acknowledge again. A withdrawn notice ends at once (it expires now), so
-- its audience and the public board stop showing it; publishers still see
-- it, marked withdrawn with the reason.

ALTER TABLE comms.notices
    ADD COLUMN edited_at       timestamptz,
    ADD COLUMN edited_by       uuid REFERENCES identity.users,
    ADD COLUMN withdrawn_at    timestamptz,
    ADD COLUMN withdrawn_by    uuid REFERENCES identity.users,
    ADD COLUMN withdraw_reason text CHECK (length(withdraw_reason) BETWEEN 3 AND 500);

CREATE FUNCTION comms.require_publisher() RETURNS void
LANGUAGE plpgsql STABLE
AS $$
BEGIN
    IF identity.current_app_role() IS DISTINCT FROM 'exam_cell' AND NOT identity.is_admin() THEN
        RAISE EXCEPTION 'publishers only' USING ERRCODE = 'insufficient_privilege';
    END IF;
END $$;

-- Returns the notice as it was, for the audit log; NULL if there's none to edit.
CREATE FUNCTION comms.edit_notice(p_id uuid, p_title text, p_body text, p_severity text, p_expires timestamptz)
RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_old comms.notices;
BEGIN
    PERFORM comms.require_publisher();
    SELECT * INTO v_old FROM comms.notices WHERE id = p_id AND withdrawn_at IS NULL FOR UPDATE;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    UPDATE comms.notices
    SET title = p_title, body = p_body, severity = p_severity::comms.notice_severity, expires_at = p_expires,
        edited_at = now(), edited_by = identity.current_user_id()
    WHERE id = p_id;
    -- What people acknowledged has changed: they acknowledge the new text.
    IF p_severity = 'critical' AND (p_title <> v_old.title OR p_body <> v_old.body OR v_old.severity <> 'critical') THEN
        UPDATE comms.notice_reads SET acknowledged_at = NULL WHERE notice_id = p_id;
    END IF;
    RETURN jsonb_build_object('title', v_old.title, 'body', v_old.body, 'severity', v_old.severity,
                              'expires_at', v_old.expires_at);
END $$;

-- Withdraws a published notice (false if there's none to withdraw).
CREATE FUNCTION comms.withdraw_notice(p_id uuid, p_reason text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM comms.require_publisher();
    UPDATE comms.notices
    SET withdrawn_at = now(), withdrawn_by = identity.current_user_id(), withdraw_reason = trim(p_reason),
        expires_at = greatest(now(), published_at + interval '1 second')
    WHERE id = p_id AND withdrawn_at IS NULL AND published_at <= now();
    RETURN FOUND;
END $$;

-- Deletes a notice nobody has seen yet: one still scheduled.
CREATE FUNCTION comms.delete_scheduled_notice(p_id uuid) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM comms.require_publisher();
    DELETE FROM comms.notices WHERE id = p_id AND published_at > now();
    RETURN FOUND;
END $$;

DO $$
DECLARE
    f text;
BEGIN
    FOREACH f IN ARRAY ARRAY[
        'comms.edit_notice(uuid, text, text, text, timestamptz)',
        'comms.withdraw_notice(uuid, text)',
        'comms.delete_scheduled_notice(uuid)']
    LOOP
        EXECUTE format('REVOKE EXECUTE ON FUNCTION %s FROM PUBLIC', f);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO acadex_app', f);
    END LOOP;
END $$;

-- The audience stops seeing a withdrawn notice at once, whatever its times.
DROP POLICY everyone_reads_current_general ON comms.notices;
CREATE POLICY everyone_reads_current_general ON comms.notices FOR SELECT TO acadex_app
    USING (program_id IS NULL AND batch_year IS NULL AND section IS NULL AND NOT individual
           AND withdrawn_at IS NULL
           AND published_at <= now() AND (expires_at IS NULL OR expires_at > now()));
DROP POLICY audience_reads_current ON comms.notices;
CREATE POLICY audience_reads_current ON comms.notices FOR SELECT TO acadex_app
    USING (withdrawn_at IS NULL AND published_at <= now() AND (expires_at IS NULL OR expires_at > now())
           AND comms.is_for(identity.current_user_id(), notices));
