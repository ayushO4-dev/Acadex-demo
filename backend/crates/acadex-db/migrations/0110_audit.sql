-- Append-only, hash-chained log of user actions — the Logs page (architecture.md §6.7).
-- The app role can neither write nor change rows directly: audit.record() is the
-- only way in, and it takes the actor from the transaction's RLS settings.
CREATE TABLE audit.action_log (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at   timestamptz NOT NULL,
    actor_id      uuid,
    actor_role    text,
    action        text NOT NULL,  -- 'auth.login', 'attendance.marked'
    resource_type text NOT NULL,
    resource_id   text,
    outcome       text NOT NULL CHECK (outcome IN ('success', 'denied', 'error')),
    ip            inet,
    user_agent    text,
    trace_id      text,
    metadata      jsonb NOT NULL DEFAULT '{}',
    prev_hash     bytea,
    row_hash      bytea NOT NULL
);
CREATE INDEX ON audit.action_log (actor_id, occurred_at DESC);

-- sha256(prev_hash || canonical row). The timestamp is hashed as epoch
-- microseconds so the result doesn't depend on the session's time zone.
CREATE FUNCTION audit.compute_hash(p_prev bytea, r audit.action_log) RETURNS bytea
LANGUAGE sql STABLE
AS $$
    SELECT pg_catalog.sha256(coalesce(p_prev, ''::bytea) || convert_to(jsonb_build_object(
        'occurred_at_us', (extract(epoch FROM r.occurred_at) * 1000000)::bigint,
        'actor_id',       r.actor_id,
        'actor_role',     r.actor_role,
        'action',         r.action,
        'resource_type',  r.resource_type,
        'resource_id',    r.resource_id,
        'outcome',        r.outcome,
        'ip',             r.ip,
        'user_agent',     r.user_agent,
        'trace_id',       r.trace_id,
        'metadata',       r.metadata
    )::text, 'UTF8'))
$$;

CREATE FUNCTION audit.record(
    p_action        text,
    p_resource_type text,
    p_resource_id   text,
    p_outcome       text,
    p_ip            inet DEFAULT NULL,
    p_user_agent    text DEFAULT NULL,
    p_trace_id      text DEFAULT NULL,
    p_metadata      jsonb DEFAULT '{}'
) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    r    audit.action_log;
    v_id bigint;
BEGIN
    -- One writer at a time keeps the chain linear. Fine at demo volume; production
    -- would chain per partition or batch writes through the outbox.
    PERFORM pg_advisory_xact_lock(hashtext('audit.action_log'));
    SELECT l.row_hash INTO r.prev_hash FROM audit.action_log l ORDER BY l.id DESC LIMIT 1;

    r.occurred_at   := clock_timestamp();
    r.actor_id      := identity.current_user_id();
    r.actor_role    := identity.current_app_role();
    r.action        := p_action;
    r.resource_type := p_resource_type;
    r.resource_id   := p_resource_id;
    r.outcome       := p_outcome;
    r.ip            := p_ip;
    r.user_agent    := p_user_agent;
    r.trace_id      := p_trace_id;
    r.metadata      := coalesce(p_metadata, '{}');
    r.row_hash      := audit.compute_hash(r.prev_hash, r);

    INSERT INTO audit.action_log (occurred_at, actor_id, actor_role, action, resource_type,
                                  resource_id, outcome, ip, user_agent, trace_id, metadata,
                                  prev_hash, row_hash)
    VALUES (r.occurred_at, r.actor_id, r.actor_role, r.action, r.resource_type,
            r.resource_id, r.outcome, r.ip, r.user_agent, r.trace_id, r.metadata,
            r.prev_hash, r.row_hash)
    RETURNING id INTO v_id;
    RETURN v_id;
END $$;

-- Walks the chain in order and reports the first row whose hash or back-link
-- doesn't match. Catches edits and deletions anywhere except the newest row,
-- which the nightly job covers by keeping the last verified head hash.
CREATE FUNCTION audit.verify_chain() RETURNS TABLE (rows_checked bigint, first_broken_id bigint)
LANGUAGE plpgsql STABLE
AS $$
DECLARE
    r      audit.action_log;
    v_prev bytea := NULL;
    v_n    bigint := 0;
BEGIN
    FOR r IN SELECT * FROM audit.action_log ORDER BY id LOOP
        v_n := v_n + 1;
        IF r.prev_hash IS DISTINCT FROM v_prev OR r.row_hash <> audit.compute_hash(v_prev, r) THEN
            rows_checked := v_n;
            first_broken_id := r.id;
            RETURN NEXT;
            RETURN;
        END IF;
        v_prev := r.row_hash;
    END LOOP;
    rows_checked := v_n;
    first_broken_id := NULL;
    RETURN NEXT;
END $$;
