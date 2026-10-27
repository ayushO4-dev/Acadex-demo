-- F5: notices, completed (architecture.md §9.6). Targeting by program,
-- batch, section or individual; scheduled publication; attachments; read
-- receipts for publishers; acknowledgement of critical notices; email
-- fan-out by the worker, recorded per recipient; live updates over
-- LISTEN/NOTIFY.

ALTER TABLE comms.notices
    -- Narrow a program's (or the institution's) audience to a batch and section of students.
    ADD COLUMN batch_year    smallint CHECK (batch_year BETWEEN 2000 AND 2100),
    ADD COLUMN section       text CHECK (section ~ '^[A-Z]$'),
    -- Addressed to named people (comms.notice_recipients) instead.
    ADD COLUMN individual    boolean NOT NULL DEFAULT false,
    -- When the worker sent its emails; NULL until then (or until its time comes).
    ADD COLUMN fanned_out_at timestamptz;
CREATE INDEX notices_awaiting_fanout ON comms.notices (published_at) WHERE fanned_out_at IS NULL;

CREATE TABLE comms.notice_recipients (
    notice_id uuid NOT NULL REFERENCES comms.notices ON DELETE CASCADE,
    user_id   uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
    PRIMARY KEY (notice_id, user_id)
);
CREATE INDEX ON comms.notice_recipients (user_id);

CREATE TABLE comms.notice_attachments (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    notice_id    uuid NOT NULL REFERENCES comms.notices ON DELETE CASCADE,
    object       text NOT NULL UNIQUE,
    name         text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    content_type text NOT NULL CHECK (content_type IN ('application/pdf', 'image/png', 'image/jpeg')),
    size_bytes   integer NOT NULL CHECK (size_bytes > 0),
    uploaded_at  timestamptz NOT NULL DEFAULT now()
);

-- Critical notices must be acknowledged, not just seen.
ALTER TABLE comms.notice_reads ADD COLUMN acknowledged_at timestamptz;

-- What was sent to whom, so a retried fan-out never sends twice.
CREATE TABLE comms.notice_deliveries (
    notice_id uuid NOT NULL REFERENCES comms.notices ON DELETE CASCADE,
    user_id   uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
    channel   text NOT NULL CHECK (channel IN ('email')),
    sent_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (notice_id, user_id, channel)
);

-- Which notices each person wants by email. Critical ones always go.
CREATE TABLE identity.notification_prefs (
    user_id     uuid PRIMARY KEY REFERENCES identity.users ON DELETE CASCADE,
    email_level text NOT NULL DEFAULT 'important' CHECK (email_level IN ('all', 'important', 'critical')),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- ── Who a notice is for ─────────────────────────────────────────────────────
-- Individual notices: their recipients. Otherwise students match on program,
-- batch and section (each optional); faculty (HODs included) get
-- institution-wide notices and their program's; other staff, institution-wide
-- ones. Nobody but students gets one narrowed to a batch or section.
CREATE FUNCTION comms.is_for(p_user uuid, n comms.notices) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT CASE
        WHEN n.individual THEN EXISTS (SELECT 1 FROM comms.notice_recipients r WHERE r.notice_id = n.id AND r.user_id = p_user)
        WHEN EXISTS (SELECT 1 FROM people.students s WHERE s.user_id = p_user) THEN EXISTS (
            SELECT 1 FROM people.students s
            WHERE s.user_id = p_user
              AND (n.program_id IS NULL OR s.program_id = n.program_id)
              AND (n.batch_year IS NULL OR s.batch_year = n.batch_year)
              AND (n.section IS NULL OR s.section = n.section))
        ELSE n.batch_year IS NULL AND n.section IS NULL
             AND (n.program_id IS NULL OR EXISTS (SELECT 1 FROM people.faculty f
                                                  WHERE f.user_id = p_user AND f.program_id = n.program_id))
    END
$$;
REVOKE EXECUTE ON FUNCTION comms.is_for(uuid, comms.notices) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION comms.is_for(uuid, comms.notices) TO acadex_app;

-- Everyone a notice reaches, for fan-out and read receipts.
CREATE FUNCTION comms.audience(p_notice uuid)
RETURNS TABLE (user_id uuid, email text, display_name text, email_level text)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT u.id, u.email, u.display_name, coalesce(p.email_level, 'important')
    FROM comms.notices n
    JOIN identity.users u ON u.active AND u.institution_id = n.institution_id
    LEFT JOIN identity.notification_prefs p ON p.user_id = u.id
    WHERE n.id = p_notice AND comms.is_for(u.id, n)
$$;
REVOKE EXECUTE ON FUNCTION comms.audience(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION comms.audience(uuid) TO acadex_app;

-- ── Visibility, rewritten around comms.is_for ───────────────────────────────
DROP POLICY everyone_reads_current_general ON comms.notices;
DROP POLICY members_read_current_program ON comms.notices;
-- The public board: institution-wide, untargeted, current. Needs no user.
CREATE POLICY everyone_reads_current_general ON comms.notices FOR SELECT TO acadex_app
    USING (program_id IS NULL AND batch_year IS NULL AND section IS NULL AND NOT individual
           AND published_at <= now() AND (expires_at IS NULL OR expires_at > now()));
CREATE POLICY audience_reads_current ON comms.notices FOR SELECT TO acadex_app
    USING (published_at <= now() AND (expires_at IS NULL OR expires_at > now())
           AND comms.is_for(identity.current_user_id(), notices));
-- Publishers (exam cell, admin) already read every notice; the worker too.
CREATE POLICY worker_reads ON comms.notices FOR SELECT TO acadex_app
    USING (identity.current_app_role() = 'worker');
GRANT UPDATE (fanned_out_at) ON comms.notices TO acadex_app;
CREATE POLICY worker_marks_fanned_out ON comms.notices FOR UPDATE TO acadex_app
    USING (identity.current_app_role() = 'worker') WITH CHECK (identity.current_app_role() = 'worker');

GRANT SELECT, INSERT ON comms.notice_recipients TO acadex_app;
ALTER TABLE comms.notice_recipients ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_or_publisher ON comms.notice_recipients FOR SELECT TO acadex_app
    USING (user_id = identity.current_user_id()
           OR identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin', 'worker'));
CREATE POLICY publisher_addresses ON comms.notice_recipients FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));

GRANT SELECT, INSERT ON comms.notice_attachments TO acadex_app;
ALTER TABLE comms.notice_attachments ENABLE ROW LEVEL SECURITY;
CREATE POLICY follows_notice ON comms.notice_attachments FOR SELECT TO acadex_app
    USING (notice_id IN (SELECT id FROM comms.notices));
CREATE POLICY publisher_attaches ON comms.notice_attachments FOR INSERT TO acadex_app
    WITH CHECK (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));

-- Read receipts: publishers see who read and acknowledged.
GRANT UPDATE (acknowledged_at) ON comms.notice_reads TO acadex_app;
CREATE POLICY publishers_see_receipts ON comms.notice_reads FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));

GRANT SELECT, INSERT ON comms.notice_deliveries TO acadex_app;
ALTER TABLE comms.notice_deliveries ENABLE ROW LEVEL SECURITY;
CREATE POLICY worker_delivers ON comms.notice_deliveries FOR ALL TO acadex_app
    USING (identity.current_app_role() = 'worker') WITH CHECK (identity.current_app_role() = 'worker');
CREATE POLICY publishers_see_deliveries ON comms.notice_deliveries FOR SELECT TO acadex_app
    USING (identity.current_app_role() IN ('exam_cell', 'admin', 'super_admin'));

GRANT SELECT, INSERT, UPDATE ON identity.notification_prefs TO acadex_app;
ALTER TABLE identity.notification_prefs ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_prefs ON identity.notification_prefs FOR ALL TO acadex_app
    USING (user_id = identity.current_user_id()) WITH CHECK (user_id = identity.current_user_id());

-- ── Live updates ────────────────────────────────────────────────────────────
-- A notice that's live now wakes every API instance's listener, which tells
-- the connected browsers it's for. Scheduled ones notify when the worker
-- releases them (comms.release).
CREATE FUNCTION comms.notify_live() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.published_at <= now() THEN
        PERFORM pg_notify('acadex_notices', NEW.id::text);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER notify_live AFTER INSERT ON comms.notices
    FOR EACH ROW EXECUTE FUNCTION comms.notify_live();

CREATE FUNCTION comms.announce(p_notice uuid) RETURNS void
LANGUAGE sql
AS $$ SELECT pg_notify('acadex_notices', p_notice::text) $$;
GRANT EXECUTE ON FUNCTION comms.announce(uuid) TO acadex_app;
