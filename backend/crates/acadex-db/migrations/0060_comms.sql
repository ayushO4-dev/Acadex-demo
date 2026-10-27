CREATE TYPE comms.notice_severity AS ENUM ('general', 'important', 'critical');

CREATE TABLE comms.notices (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    institution_id smallint NOT NULL REFERENCES org.institutions,
    -- NULL: the whole institution, and shown on the public notice board.
    -- Otherwise only that program's students and faculty see it.
    program_id     smallint REFERENCES org.programs,
    severity       comms.notice_severity NOT NULL DEFAULT 'general',
    title          text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    -- Plain text, rendered as text: no HTML, so nothing to sanitise.
    body           text NOT NULL CHECK (length(body) BETWEEN 1 AND 10000),
    published_at   timestamptz NOT NULL DEFAULT now(),
    expires_at     timestamptz CHECK (expires_at > published_at),
    created_by     uuid REFERENCES identity.users
);
CREATE INDEX ON comms.notices (published_at DESC);

-- Per-user read state; a row means "read".
CREATE TABLE comms.notice_reads (
    user_id   uuid NOT NULL REFERENCES identity.users ON DELETE CASCADE,
    notice_id uuid NOT NULL REFERENCES comms.notices ON DELETE CASCADE,
    read_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, notice_id)
);
