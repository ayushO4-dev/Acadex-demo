-- Transactional outbox (architecture.md §11.1): a business write and the event
-- announcing it commit together; pending rows are published to Pub/Sub after
-- commit, with a sweeper for any a crash left behind. An event can't be lost,
-- or sent for a write that rolled back.
CREATE TABLE ops.outbox (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    topic        text NOT NULL,
    ordering_key text,
    payload      jsonb NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz
);
-- The sweeper's queue: only pending rows, oldest first.
CREATE INDEX outbox_pending ON ops.outbox (id) WHERE published_at IS NULL;
