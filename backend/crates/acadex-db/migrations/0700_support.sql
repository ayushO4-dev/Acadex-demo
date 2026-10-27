-- F5: support tickets (architecture.md §9.7). Anyone signed in raises a
-- ticket; support agents (and administrators) work them. Each priority has a
-- first-response and a resolution target; the `support.sla` job escalates
-- tickets that miss them. Internal notes are for agents only.

CREATE SCHEMA support;
GRANT USAGE ON SCHEMA support TO acadex_app;

CREATE TABLE support.sla_policy (
    priority               text PRIMARY KEY CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    first_response_minutes integer NOT NULL CHECK (first_response_minutes > 0),
    resolve_minutes        integer NOT NULL CHECK (resolve_minutes > 0)
);
INSERT INTO support.sla_policy VALUES
    ('urgent', 60, 8 * 60), ('high', 4 * 60, 24 * 60), ('normal', 8 * 60, 72 * 60), ('low', 24 * 60, 120 * 60);

CREATE TABLE support.tickets (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- What people quote: #4801, #4802, …
    number             bigint GENERATED ALWAYS AS IDENTITY (START WITH 4801) UNIQUE,
    requester_id       uuid NOT NULL REFERENCES identity.users,
    subject            text NOT NULL CHECK (length(subject) BETWEEN 3 AND 200),
    category           text NOT NULL CHECK (category IN ('academic', 'attendance', 'examination', 'fees', 'profile', 'technical', 'other')),
    priority           text NOT NULL DEFAULT 'normal' REFERENCES support.sla_policy,
    status             text NOT NULL DEFAULT 'open'
                       CHECK (status IN ('open', 'in_progress', 'waiting_on_requester', 'resolved', 'closed')),
    assignee_id        uuid REFERENCES identity.users,
    -- Optional pointer to what it's about, e.g. a receipt number or exam form.
    linked_resource    text CHECK (length(linked_resource) <= 100),
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    first_response_due timestamptz NOT NULL,
    resolve_due        timestamptz NOT NULL,
    first_responded_at timestamptz,
    resolved_at        timestamptz,
    closed_at          timestamptz,
    -- 0: on time; 1: first response overdue; 2: resolution overdue.
    escalation_level   smallint NOT NULL DEFAULT 0 CHECK (escalation_level BETWEEN 0 AND 2),
    escalated_at       timestamptz
);
CREATE INDEX ON support.tickets (requester_id, updated_at DESC);
CREATE INDEX ON support.tickets (status, resolve_due);
CREATE INDEX ON support.tickets (assignee_id, status);

CREATE TABLE support.messages (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    ticket_id  uuid NOT NULL REFERENCES support.tickets ON DELETE CASCADE,
    -- NULL for the system (escalations, status changes).
    author_id  uuid REFERENCES identity.users,
    body       text NOT NULL CHECK (length(body) BETWEEN 1 AND 5000),
    -- Agents only; the requester never sees these.
    internal   boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON support.messages (ticket_id, id);

CREATE TABLE support.attachments (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    ticket_id    uuid NOT NULL REFERENCES support.tickets ON DELETE CASCADE,
    message_id   bigint REFERENCES support.messages ON DELETE CASCADE,
    uploader_id  uuid NOT NULL REFERENCES identity.users,
    object       text NOT NULL UNIQUE,
    name         text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    content_type text NOT NULL CHECK (content_type IN ('application/pdf', 'image/png', 'image/jpeg')),
    size_bytes   integer NOT NULL CHECK (size_bytes > 0),
    uploaded_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON support.attachments (ticket_id);

-- When each person last read a ticket, for unread markers.
CREATE TABLE support.reads (
    ticket_id    uuid NOT NULL REFERENCES support.tickets ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES identity.users,
    last_read_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ticket_id, user_id)
);

-- Due times from the policy, set on insert and when the priority changes.
CREATE FUNCTION support.set_due() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    p support.sla_policy;
BEGIN
    IF TG_OP = 'INSERT' OR NEW.priority IS DISTINCT FROM OLD.priority THEN
        SELECT * INTO p FROM support.sla_policy WHERE priority = NEW.priority;
        NEW.first_response_due := NEW.created_at + make_interval(mins => p.first_response_minutes);
        NEW.resolve_due := NEW.created_at + make_interval(mins => p.resolve_minutes);
    END IF;
    -- Only agents (and the SLA job) change priority, assignment or escalation;
    -- a requester closes their ticket, reopens it, or answers a question.
    IF TG_OP = 'UPDATE' AND NOT (support.is_agent() OR identity.current_app_role() = 'worker') THEN
        IF (NEW.priority, NEW.assignee_id, NEW.escalation_level, NEW.first_responded_at)
           IS DISTINCT FROM (OLD.priority, OLD.assignee_id, OLD.escalation_level, OLD.first_responded_at)
           OR (NEW.status IS DISTINCT FROM OLD.status AND NEW.status NOT IN ('closed', 'open', 'in_progress')) THEN
            RAISE EXCEPTION 'only support agents change that' USING ERRCODE = 'insufficient_privilege';
        END IF;
    END IF;
    IF TG_OP = 'UPDATE' THEN
        NEW.updated_at := now();
        IF NEW.status = 'resolved' AND OLD.status <> 'resolved' THEN NEW.resolved_at := now(); END IF;
        IF NEW.status = 'closed' AND OLD.status <> 'closed' THEN NEW.closed_at := now(); END IF;
        IF NEW.status NOT IN ('resolved', 'closed') THEN NEW.resolved_at := NULL; NEW.closed_at := NULL; END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER set_due BEFORE INSERT OR UPDATE ON support.tickets
    FOR EACH ROW EXECUTE FUNCTION support.set_due();

-- ── Access ──────────────────────────────────────────────────────────────────
CREATE FUNCTION support.is_agent() RETURNS boolean
LANGUAGE sql STABLE
AS $$ SELECT identity.current_app_role() IN ('support_agent', 'admin', 'super_admin') $$;
GRANT EXECUTE ON FUNCTION support.is_agent() TO acadex_app;

GRANT SELECT ON support.sla_policy TO acadex_app;

GRANT SELECT, INSERT ON support.tickets TO acadex_app;
GRANT UPDATE (status, priority, assignee_id, first_responded_at, escalation_level, escalated_at) ON support.tickets TO acadex_app;
ALTER TABLE support.tickets ENABLE ROW LEVEL SECURITY;
CREATE POLICY requester_reads ON support.tickets FOR SELECT TO acadex_app
    USING (requester_id = identity.current_user_id());
CREATE POLICY agents_read ON support.tickets FOR SELECT TO acadex_app
    USING (support.is_agent() OR identity.current_app_role() = 'worker');
CREATE POLICY anyone_raises ON support.tickets FOR INSERT TO acadex_app
    WITH CHECK (requester_id = identity.current_user_id() AND status = 'open' AND assignee_id IS NULL);
-- The requester can close their own ticket or reopen a resolved one; agents
-- work any ticket; the SLA job escalates.
CREATE POLICY requester_updates ON support.tickets FOR UPDATE TO acadex_app
    USING (requester_id = identity.current_user_id())
    WITH CHECK (requester_id = identity.current_user_id() AND status IN ('open', 'closed', 'waiting_on_requester', 'in_progress', 'resolved'));
CREATE POLICY agents_update ON support.tickets FOR UPDATE TO acadex_app
    USING (support.is_agent() OR identity.current_app_role() = 'worker')
    WITH CHECK (support.is_agent() OR identity.current_app_role() = 'worker');

GRANT SELECT, INSERT ON support.messages TO acadex_app;
ALTER TABLE support.messages ENABLE ROW LEVEL SECURITY;
CREATE POLICY reads_with_ticket ON support.messages FOR SELECT TO acadex_app
    USING (ticket_id IN (SELECT id FROM support.tickets) AND (NOT internal OR support.is_agent()));
CREATE POLICY writes_on_visible_ticket ON support.messages FOR INSERT TO acadex_app
    WITH CHECK (ticket_id IN (SELECT id FROM support.tickets)
                AND ((author_id = identity.current_user_id() AND (NOT internal OR support.is_agent()))
                     OR (author_id IS NULL AND identity.current_app_role() = 'worker')));

GRANT SELECT, INSERT ON support.attachments TO acadex_app;
ALTER TABLE support.attachments ENABLE ROW LEVEL SECURITY;
CREATE POLICY reads_with_message ON support.attachments FOR SELECT TO acadex_app
    USING (ticket_id IN (SELECT id FROM support.tickets)
           AND (message_id IS NULL OR message_id IN (SELECT id FROM support.messages)));
CREATE POLICY uploads_on_visible_ticket ON support.attachments FOR INSERT TO acadex_app
    WITH CHECK (uploader_id = identity.current_user_id() AND ticket_id IN (SELECT id FROM support.tickets));

GRANT SELECT, INSERT, UPDATE ON support.reads TO acadex_app;
ALTER TABLE support.reads ENABLE ROW LEVEL SECURITY;
CREATE POLICY own_reads ON support.reads FOR ALL TO acadex_app
    USING (user_id = identity.current_user_id()) WITH CHECK (user_id = identity.current_user_id());
